//! Token-level copy-paste clones via jscpd, with an npx fallback.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub struct TokenClone {
    pub file_a: String,
    pub a: (u32, u32),
    pub file_b: String,
    pub b: (u32, u32),
    pub lines: u32,
}

impl TokenClone {
    pub fn touches_change(&self, changed: &crate::gitdiff::ChangedRanges) -> bool {
        [(&self.file_a, self.a), (&self.file_b, self.b)]
            .iter()
            .any(|(file, (start, end))| {
                changed
                    .get(file.as_str())
                    .is_some_and(|ranges| crate::gitdiff::overlaps(ranges, *start, *end))
            })
    }
}

pub fn run_jscpd(root: &Path) -> Result<Option<Vec<TokenClone>>> {
    let root = root.canonicalize().context("resolve scan root")?;
    let mut cmd = if Command::new("jscpd")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        Command::new("jscpd")
    } else if Command::new("npx")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        let mut cmd = Command::new("npx");
        cmd.args(["--yes", "jscpd"]);
        cmd
    } else {
        eprintln!("[dupfinder] jscpd and npx unavailable — skipping token-clone pass");
        return Ok(None);
    };
    let out_dir = std::env::temp_dir().join(format!("dupfinder-jscpd-{}", std::process::id()));
    cmd.current_dir(&root)
        .args(["--reporters", "json", "--silent", "--output"])
        .arg(&out_dir);
    // Honor the repo's own jscpd config when present; otherwise sane defaults.
    if !root.join(".jscpd.json").exists() {
        // jscpd >=5 respects .gitignore by default (it only offers --no-gitignore),
        // and rejects the old --gitignore flag outright.
        cmd.args(["--min-tokens", "70", "--ignore"]).arg(
            "**/node_modules/**,**/target/**,**/dist/**,**/build/**,**/.git/**,**/*.min.js,**/*.json,**/*.md",
        );
        cmd.arg(".");
    }
    let output = cmd.output().context("run jscpd")?;
    let report_path = out_dir.join("jscpd-report.json");
    if !report_path.exists() {
        eprintln!(
            "[dupfinder] jscpd produced no report — skipping token-clone pass\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(None);
    }
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path)?).context("parse jscpd report")?;
    let _ = std::fs::remove_dir_all(&out_dir);

    let mut clones = Vec::new();
    for d in report["duplicates"].as_array().into_iter().flatten() {
        let side = |k: &str| -> Option<(String, (u32, u32))> {
            let f = &d[k];
            let path = Path::new(f["name"].as_str()?);
            let path = path.strip_prefix(&root).unwrap_or(path);
            let path = path.strip_prefix(".").unwrap_or(path);
            Some((
                path.to_string_lossy().replace('\\', "/"),
                (f["start"].as_u64()? as u32, f["end"].as_u64()? as u32),
            ))
        };
        if let (Some((file_a, a)), Some((file_b, b))) = (side("firstFile"), side("secondFile")) {
            clones.push(TokenClone {
                file_a,
                a,
                file_b,
                b,
                lines: d["lines"].as_u64().unwrap_or(0) as u32,
            });
        }
    }
    Ok(Some(clones))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_must_overlap_changed_lines_in_the_same_file() {
        let clone = TokenClone {
            file_a: "src/a.ts".into(),
            a: (10, 20),
            file_b: "src/b.ts".into(),
            b: (30, 40),
            lines: 10,
        };
        for (file, range, expected) in [
            ("src/a.ts", (1, 9), false),
            ("src/a.ts", (20, 20), true),
            ("src/b.ts", (35, 36), true),
            ("other/src/a.ts", (10, 20), false),
        ] {
            let changed = [(file.to_string(), vec![range])].into_iter().collect();
            assert_eq!(clone.touches_change(&changed), expected);
        }
    }
}
