---
name: audit-duplicates
description: Audit a repository for actionable duplication using dupfinder name matching and token clones. Use for an explicit repo-wide duplication audit; use review-for-duplicates for a change or prior-art lookup.
---

# Audit duplicates

Run `dupfinder names <root> --all --min-score 0.5 --top 40` and
`dupfinder clones <root>`. Exclude generated code, fixtures, corpora, and deliberate
self-contained examples before judging results. `names` accepts repeatable
`--exclude` globs; clones honors the repository's `.jscpd.json`. Without a config,
filter clone results yourself. State each pass's scope and exclusions.

Read both sides of each candidate. Name scores rank vocabulary overlap, not
semantic equivalence. Agreement with token clones strengthens the evidence.
Discard required interface implementations, inverse operations, intentional API
families, and trivial test scaffolding. Diverged implementations of one behavior
are especially useful findings, but differing behavior alone does not prove a bug.

Rank confirmed findings by maintenance benefit and feasible reuse, not similarity
score. Identify the shared owner, locations, and any architectural constraint.
Use `merge`, `reuse`, `extract`, or `diverged` as concise finding labels. Estimate
removable lines only when supported by a concrete consolidation plan.

Report coverage, skipped passes, and remaining uncertainty. If nothing survives,
say "No actionable duplication." Apply fixes only when the user requested them.
