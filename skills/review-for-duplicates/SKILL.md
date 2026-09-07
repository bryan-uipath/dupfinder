---
name: review-for-duplicates
description: Find reusable helpers before coding and review a change for duplication using dupfinder names and token clones. Use audit-duplicates for an explicit whole-repository audit.
---

# review-for-duplicates

Evidence-driven duplication review of **a change**. dupfinder supplies
deterministic evidence; your job is judgment — which findings warrant reuse or
extraction, and which duplication is fine.

**Scope: duplication only.** Correctness, security, performance, and style are
out of scope here; other reviewers cover those.

Use `dupfinder review <root> --base <ref>` for combined changed-line clone and
lexical evidence. Use `audit-duplicates` for an explicit whole-repository audit.

This skill ships with dupfinder itself. Install/update it with
`dupfinder install-skill` (writes to `~/.claude/skills/`) or
`dupfinder install-skill --project` (writes to `./.claude/skills/`).

## Prevention — before writing new code

The highest-leverage move: stop the duplicate from being written at all.

```sh
# ranked prior art for the helper you are about to write
dupfinder names <repo-root> --name parse_manifest_header

# or the full greppable API list, when you'd rather scan by domain term
dupfinder index <repo-root> --out /tmp/api-index.md
grep -i -E 'atomic|temp.?file|rename' /tmp/api-index.md
```

`names --name` splits the identifier into tokens, collapses synonym stems
(`closest`~`nearest`, `fetch`~`get`, `build`~`create`), and ranks existing
functions/types by overlap — so `find_nearest_cell` surfaces
`find_closest_reachable_cell` even though they share only one literal word. Each
hit is `score [name/types] kind name file:line — doc`.

If something already does the job, use or extend it, and say so.

## Step 0 — Locate dupfinder

```sh
DF=$(command -v dupfinder)
[ -n "$DF" ] || echo "dupfinder not on PATH — install with: cargo install --path <dupfinder-checkout>"
```

If it isn't installed and can't be, fall back to `npx jscpd --min-tokens 70` for
the token pass, grep for prior art by hand, and say which passes were skipped.

## Step 1 — Gather evidence

```sh
BASE=<the change's stack parent: the PR base ref if one exists, else origin/main>

# lexical prior art and clones overlapping changed lines
"$DF" review <repo-root> --base "$BASE" --min-lines 1 --top 5
```

The diff covers merge-base through the working tree, including uncommitted edits.
Pass the stack parent explicitly; omitted bases try origin/main, origin/master,
main, and master. `--min-lines 1` retains small helpers and types in this review.

## Step 2 — Judge the evidence

Open both `file:line`s before calling anything a duplicate. Scores order your
reading; they never conclude for you.

- **Genuine duplication** — the changed code re-implements existing code.
  Recommend reusing/extending it (name it and its location) or extracting a
  shared helper. Severity by blast radius: logic that must stay in sync across
  seams (platform targets, crates) is High; a local convenience copy is Low.
- **Acceptable near-duplication** — intentional API families (`cube`/`sphere`/
  `quad`), inverse pairs (`vec3_to_point3`/`point3_to_vec3`), platform twins that
  must differ, language-forced boilerplate. Say why; no finding.
- **Structurally forced** — trait/interface impls and overrides share names
  because the trait dictates them. `names --all` drops same-name trait impls
  and pure forwarding methods; `clones` does not.
- **Diverged duplicates** are the strongest finding: two implementations of one
  idea whose behavior differs (e.g. one handles Unicode, the other only ASCII).
  That is a live bug, not just redundancy.
- **Blind spot to cover yourself:** this is all token overlap. A re-implementation
  sharing no vocabulary scores 0.00 and will not appear. Skim the change for
  logic you recognize from elsewhere.

## Step 3 — Report

One line per finding, strongest first:

```
<tag> <what to cut>. <what to use instead>. [path:line <-> path:line]
```

Tags: `merge` (two impls of one idea), `reuse` (call the existing helper),
`extract` (both move to a shared home), `diverged` (behavior differs — probable
bug).

State what ran, so coverage is legible: the base ref, the number of queries and
candidates, and whether the clone pass was available.

If nothing survives judgment, say exactly that — **"No actionable duplication."**
with the scan sizes. A clean result is a real result; never pad it.
