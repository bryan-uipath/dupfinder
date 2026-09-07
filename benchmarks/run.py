#!/usr/bin/env python3
"""Capture detector output and evaluate fixed, labeled pairs without model calls."""
import argparse
import fnmatch
import hashlib
import os
import json
from pathlib import Path
import re
import shutil
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', type=Path)
    parser.add_argument('--labels', type=Path)
    parser.add_argument('--engine', action='append', choices=['names', 'clones', 'bodies', 'blocks', 'audit'])
    parser.add_argument('--exclude', action='append', default=[])
    parser.add_argument('--previous', type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    root = (args.root or Path(__file__).parent / 'corpus').resolve()
    label_path = args.labels or (Path(__file__).parent / 'labels.json' if args.root is None else None)
    labels = json.loads(label_path.read_text()) if label_path else []
    labels = [label for label in labels if not any(
        fnmatch.fnmatchcase(label[side]['file'], pattern)
        for side in ['a', 'b'] for pattern in args.exclude)]
    binary = args.binary.resolve()
    if args.out.resolve().is_relative_to(root):
        parser.error('--out must be outside the scanned root to avoid changing the corpus')
    selected_engines = list(dict.fromkeys(args.engine or ['names', 'clones']))
    clone_version = None
    if 'clones' in selected_engines or 'audit' in selected_engines:
        for command in [['jscpd', '--version'], ['npx', '--yes', 'jscpd', '--version']]:
            if shutil.which(command[0]):
                probe = subprocess.run(command, text=True, capture_output=True, timeout=120)
                if probe.returncode == 0:
                    clone_version = probe.stdout.strip()
                    break
    digest = hashlib.sha256()
    for directory, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in ['.git', 'node_modules', 'target', 'dist', 'build', 'vendor'])
        for name in sorted(files):
            path = Path(directory) / name
            relative = path.relative_to(root).as_posix()
            if path.name not in ['.jscpd.json', '.gitignore', '.ignore'] and any(
                    fnmatch.fnmatchcase(relative, pattern) for pattern in args.exclude):
                continue
            digest.update(relative.encode() + b'\0' + path.read_bytes() + b'\0')
    local_ignore = root / '.git' / 'info' / 'exclude'
    if local_ignore.is_file():
        digest.update(b'.git/info/exclude\0' + local_ignore.read_bytes())
    fingerprint = digest.hexdigest()
    labels_fingerprint = hashlib.sha256(json.dumps(labels, sort_keys=True).encode()).hexdigest()
    previous = json.loads(args.previous.read_text()) if args.previous else None
    if previous and (previous['fingerprint'] != fingerprint or previous['excludes'] != args.exclude
                     or previous['labels_fingerprint'] != labels_fingerprint):
        parser.error('previous run has a different corpus, labels, or exclusion scope')
    if previous and previous.get('clone_version') and clone_version != previous['clone_version']:
        parser.error('clone backend changed; capture a new baseline')
    records = []
    engines = {}
    for engine in selected_engines:
        command = [str(binary), engine, str(root)]
        if engine == 'names':
            command += ['--all', '--min-score', '0.5', '--top', '1000000']
        elif engine != 'clones':
            command += ['--json', '--top', '1000000']
        if engine != 'clones':
            for pattern in args.exclude:
                command += ['--exclude', pattern]
        started = time.perf_counter()
        result = subprocess.run(command, text=True, capture_output=True, check=True)
        elapsed = time.perf_counter() - started
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.with_suffix(f'.{engine}.txt').write_text(result.stdout + result.stderr)
        if engine == 'clones' and '# token clones' not in result.stdout:
            engines[engine] = {'status': 'unavailable', 'seconds': round(elapsed, 3)}
            continue
        pairs = parse_pairs(engine, result.stdout)
        pairs = [pair for pair in pairs if not any(
            fnmatch.fnmatchcase(side['file'], pattern)
            for side in [pair['a'], pair['b']] for pattern in args.exclude)]
        for pair in pairs:
            pair['engine'] = engine
        reviewed = [next((label['actionable'] for label in labels if matches(pair, label)), None)
                    for pair in pairs[:20]]
        engines[engine] = {'status': 'ok', 'seconds': round(elapsed, 3), 'pairs': len(pairs),
                          'top20': {'positive': reviewed.count(True), 'negative': reviewed.count(False),
                                    'unlabeled': reviewed.count(None)}}
        records.extend(pairs)
    found = {label['id']: any(matches(pair, label) for pair in records) for label in labels}
    metrics = {}
    for split in sorted({label['split'] for label in labels}):
        group = [label for label in labels if label['split'] == split]
        positives = [label for label in group if label['actionable']]
        negatives = [label for label in group if not label['actionable']]
        metrics[split] = {
            'positive_hits': sum(found[label['id']] for label in positives),
            'positive_total': len(positives),
            'negative_hits': sum(found[label['id']] for label in negatives),
            'negative_total': len(negatives),
        }
    keys = {pair_key(pair) for pair in records}
    old_keys = {pair_key(pair) for pair in previous['pairs']} if previous else set()
    report = {'root': str(root), 'binary': str(binary), 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'selected_engines': selected_engines, 'clone_version': clone_version,
              'comparison': ('same-engines' if selected_engines == previous['selected_engines'] else 'changed-engines') if previous else None,
              'fingerprint': fingerprint, 'labels_fingerprint': labels_fingerprint, 'excludes': args.exclude,
              'engines': engines, 'labeled_metrics': metrics, 'label_hits': found,
              'unique_pairs': len(keys),
              'additional_pairs': len(keys - old_keys) if previous else None,
              'additional_positive_labels': [label['id'] for label in labels
                  if previous and label['actionable'] and found[label['id']]
                  and not previous['label_hits'].get(label['id'], False)],
              'pairs': records}
    args.out.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: value for key, value in report.items() if key != 'pairs'}, indent=2))


def parse_pairs(engine, output):
    if engine not in ['names', 'clones']:
        return json.loads(output)['pairs']
    pairs = []
    if engine == 'names':
        pending = []
        score = 0.0
        for line in output.splitlines():
            score_match = re.match(r'^(\d+\.\d+)  \[name ', line)
            if score_match:
                score = float(score_match[1])
            match = re.match(r'^\s+(?:fn|type|struct|enum|trait) `(.+)`  (.+):(\d+)$', line)
            if match:
                name, file, start = match.groups()
                pending.append({'file': file, 'start': int(start), 'end': int(start), 'name': name})
                if len(pending) == 2:
                    pairs.append({'a': pending[0], 'b': pending[1], 'score': score})
                    pending = []
    else:
        for line in output.splitlines():
            match = re.match(r'^(.+):(\d+)-(\d+) <-> (.+):(\d+)-(\d+)  \((\d+) lines\)$', line)
            if match:
                a, start_a, end_a, b, start_b, end_b, length = match.groups()
                pairs.append({'a': {'file': a, 'start': int(start_a), 'end': int(end_a)},
                              'b': {'file': b, 'start': int(start_b), 'end': int(end_b)},
                              'lines': int(length)})
    return pairs


def matches(pair, label):
    def overlaps(a, b):
        return a['file'] == b['file'] and a['start'] <= b['end'] and b['start'] <= a['end']
    return (overlaps(pair['a'], label['a']) and overlaps(pair['b'], label['b'])) or (
        overlaps(pair['a'], label['b']) and overlaps(pair['b'], label['a']))


def pair_key(pair):
    return tuple(sorted((side['file'], side['start'], side['end']) for side in [pair['a'], pair['b']]))


if __name__ == '__main__':
    main()
