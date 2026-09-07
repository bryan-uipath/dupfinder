//! Changed-line ranges for the working tree vs a base ref, via `git diff -U0`.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

pub type ChangedRanges = BTreeMap<String, Vec<(u32, u32)>>;

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .context("run git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn resolve_base(root: &Path, explicit: Option<String>) -> Result<String> {
    if let Some(b) = explicit {
        return Ok(b);
    }
    for candidate in ["origin/main", "origin/master", "main", "master"] {
        if git(root, &["rev-parse", "--verify", "--quiet", candidate]).is_ok() {
            return Ok(candidate.to_string());
        }
    }
    bail!("no base ref found (tried origin/main, origin/master, main, master) — pass --base");
}

/// Ranges of NEW/CHANGED lines per file: merge-base(base, HEAD) -> working tree,
/// plus whole-file ranges for untracked files.
pub fn changed_ranges(root: &Path, base: &str) -> Result<ChangedRanges> {
    // `--merge-base` (git >= 2.30) diffs merge-base(base, HEAD) against the
    // WORKING TREE, so committed and uncommitted edits are both in scope and
    // line numbers match the files extract_dir reads. (The tempting
    // `git diff base...` form stops at HEAD — uncommitted edits vanish and
    // ranges go stale against the worktree.)
    let diff = git(root, &["diff", "--relative", "-U0", "--no-color", "--merge-base", base])?;
    let mut ranges: ChangedRanges = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            current = Some(path.to_string());
        } else if line.starts_with("+++ ") {
            current = None; // deleted file (+++ /dev/null)
        } else if let (Some(file), true) = (&current, line.starts_with("@@")) {
            // @@ -a[,b] +c[,d] @@ — take the +c,d (new-file) side.
            if let Some(plus) = line.split_whitespace().find(|t| t.starts_with('+')) {
                let mut it = plus[1..].splitn(2, ',');
                let start: u32 = it.next().unwrap_or("0").parse().unwrap_or(0);
                let len: u32 = it.next().map_or(1, |s| s.parse().unwrap_or(1));
                if len > 0 && start > 0 {
                    ranges
                        .entry(file.clone())
                        .or_default()
                        .push((start, start + len - 1));
                }
            }
        }
    }
    let untracked = git(root, &["ls-files", "--others", "--exclude-standard"])?;
    for f in untracked.lines().filter(|l| !l.is_empty()) {
        ranges.entry(f.to_string()).or_default().push((1, u32::MAX));
    }
    Ok(ranges)
}

pub fn overlaps(ranges: &[(u32, u32)], start: u32, end: u32) -> bool {
    ranges.iter().any(|&(a, b)| !(b < start || a > end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdirectory_ranges_match_scan_relative_paths() {
        let root = std::env::temp_dir().join(format!("dupfinder-diff-{}", std::process::id()));
        std::fs::create_dir_all(root.join("app/src")).unwrap();
        git(&root, &["init", "-b", "main"]).unwrap();
        std::fs::write(root.join("app/src/a.ts"), "const value = 1;\n").unwrap();
        std::fs::write(root.join("outside.ts"), "const value = 1;\n").unwrap();
        git(&root, &["add", "."]).unwrap();
        git(&root, &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "fixture"]).unwrap();
        std::fs::write(root.join("app/src/a.ts"), "const value = 2;\n").unwrap();
        std::fs::write(root.join("outside.ts"), "const value = 2;\n").unwrap();
        std::fs::write(root.join("app/new.ts"), "const value = 3;\n").unwrap();
        let ranges = changed_ranges(&root.join("app"), "main").unwrap();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges["src/a.ts"], vec![(1, 1)]);
        assert_eq!(ranges["new.ts"], vec![(1, u32::MAX)]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
