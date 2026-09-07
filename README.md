# dupfinder

Find existing helpers and copied code in Rust, TypeScript/JavaScript, and Functor Lang.
Results are evidence for human or agent review: read both sides before recommending a refactor.

| Command | Purpose |
| --- | --- |
| `index` | Greppable public API summary with signatures and locations |
| `names --name IDENT` | Find prior art before writing a helper |
| `names --base REF` | Find lexical neighbors of changed functions/types |
| `names --all` | Rank name/type overlap across a repository |
| `clones` | Find copied token sequences using jscpd |
| `review --base REF` | Combine changed-line token clones and lexical prior art |

## Install

```sh
cargo install --path .
```

No model, embedding index, or model download. The clone pass uses `jscpd` on PATH,
falling back to `npx --yes jscpd`. If neither is available, lexical review still runs.

## Usage

```sh
dupfinder index [DIR] [--private] [--out api-index.md]
dupfinder names [DIR] --name parseManifest --name loadManifest
dupfinder names [DIR] --base origin/main --top 5 --min-score 0.3
dupfinder names [DIR] --all --top 40 --min-score 0.5 --exclude 'examples/**'
dupfinder clones [DIR]
dupfinder review [DIR] --base origin/main --top 3 --min-lines 5
dupfinder install-skill [--project] [--dir DIR]
```

`names` accepts repeatable `--exclude` globs and `--include-tests`. Test functions
and test-file types are excluded by default. `review --min-lines` filters changed
items, not their neighbors. Base detection tries origin/main, origin/master, main,
then master; pass `--base develop` or a stack parent explicitly when appropriate.
Diffs cover merge-base through the working tree, including untracked files.

`clones` honors `.jscpd.json`; without one it uses a 70-token minimum and skips
build outputs, dependencies, JSON, and Markdown. It scans the entire repository;
`review` reports only clones overlapping changed lines.

## Interpreting names

Identifiers are split at snake/camel/Pascal boundaries, stripped of stopwords,
and normalized with a small synonym map (`fetch` ~ `get`, `build` ~ `create`).
Scores combine 75% name overlap and 25% signature-token overlap. Signature tokens
include parameter names as well as types; they are supporting evidence only.
Audits additionally damp common vocabulary using inverse document frequency.

Names find shared vocabulary; clones find copied tokens. Neither proves semantic
equivalence, and rewrites with different vocabulary can be missed. Discard API
families, inverse operations, required interface methods, and deliberate fixtures.
Prefer findings with shared behavior, a feasible common owner, and meaningful
maintenance savings. Agreement between the two detectors strengthens a candidate.

## Languages and scope

- Rust: tree-sitter functions and types, including impl/trait/module context.
- TypeScript/JavaScript/TSX/JSX: functions, methods, arrow bindings, and types.
- Functor (`.fun`): top-level `let` and `type` bindings.

The extractor respects gitignore and skips `.d.ts`, hidden files, dependencies,
and build directories. Exclude self-contained examples and corpora explicitly
when auditing. Clone language support and exclusions come from jscpd.

## Bundled agent skills

`install-skill` installs `review-for-duplicates` and `audit-duplicates` into
`~/.claude/skills`, or `./.claude/skills` with `--project`. `--dir` selects another
skills directory. Sources live in `skills/` and ship inside the binary.

## Migration from the embedding version

`embed` and `similar` have been removed; `review` now uses names and clones.
Existing `.dupfinder/` and `.fastembed_cache/` directories are unused and may be
deleted manually.

## Evaluate detector changes

Use the [labeled benchmark](benchmarks/README.md) to compare coverage, false-positive
candidates, and runtime on a fixed corpus before tuning or adding detectors.
