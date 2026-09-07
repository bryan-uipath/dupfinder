# Duplication benchmark

Run the same binary against a fixed corpus before and after a change:

```sh
cargo build --release
python3 benchmarks/run.py --binary target/release/dupfinder --out /tmp/before.json
python3 benchmarks/run.py --binary target/release/dupfinder --out /tmp/after.json --previous /tmp/before.json
```

The bundled corpus contains 12 labeled pairs: eight opportunities for shared
behavior and four deliberate semantic differences. `dev` cases guide development;
`holdout` cases are reserved for evaluation, not threshold tuning. These are
synthetic detection examples, not proof of production usefulness.

Use `--root` and `--labels` for another corpus. Omitting `--labels` for a custom
root produces candidate counts only. Freeze the checkout before comparing runs.
The runner hashes file paths/content and records the executable SHA-256 and clone backend version and refuses comparisons with a different
corpus, labels, or exclusion list. Logs and full pair evidence are saved beside the report; place outputs outside the scanned root.

`--engine` is repeatable. Default: `names`, `clones`. Later detector commands can
be evaluated using `--engine bodies`, `--engine blocks`, or `--engine audit`.
Unavailable clone tooling is reported explicitly. Other command failures fail the run.
`--exclude` is repeatable; it is passed to names/structural commands and applied to
clone results. Clone discovery itself still uses jscpd's config/default scope.

Read the metrics separately:

- **Positive hits:** labeled opportunities detected by any selected engine.
- **Negative hits:** labeled semantic differences surfaced as candidates.
- **Top 20:** known positives, known negatives, and unlabeled results, per engine.
  Unlabeled results are not counted as correct or incorrect. Do not call this
  precision until every result in the budget has been judged.
- **Additional pairs:** new location/range pairs, not newly confirmed cleanups.
  Different-sized fragments of the same duplication can count separately.
- **Additional positive labels:** known opportunities newly detected since the
  previous run. Report separately from new discoveries in a real repository.
- **Runtime:** one wall-clock sample per engine, excluding build time. Repeated
  controlled runs are needed for performance claims.

Compare identical engine sets for implementation changes; adding an engine
measures cumulative coverage and is explicitly marked `changed-engines`. Keep corpus and labels unchanged across the stack.
Private source and detailed internal findings belong outside public PRs.
