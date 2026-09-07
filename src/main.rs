//! Duplication detection and reuse discovery through API indexing, names, and token clones.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod clones;
mod normalized;
mod bodies;
mod blocks;
mod audit;
mod extract;
mod gitdiff;
mod names;
mod skill;

#[derive(Parser)]
#[command(name = "dupfinder", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Emit a consolidated API index (functions + types) as markdown
    Index {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Include non-public/non-exported items
        #[arg(long)]
        private: bool,
        /// Write to a file instead of stdout
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Token-level copy-paste clones via jscpd (or npx fallback)
    Clones {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Match normalized TypeScript/JavaScript bodies despite renamed local bindings
    Bodies {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value_t = 30)]
        min_tokens: usize,
        #[arg(long, default_value_t = 40)]
        top: usize,
        #[arg(long)]
        include_tests: bool,
        #[arg(long = "exclude")]
        excludes: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Match repeated windows of three adjacent TypeScript/JavaScript statements
    Blocks {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value_t = 20)]
        min_tokens: usize,
        #[arg(long, default_value_t = 40)]
        top: usize,
        #[arg(long)]
        include_tests: bool,
        #[arg(long = "exclude")]
        excludes: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Group evidence: names >= 0.5, bodies >= 30 tokens, blocks >= 20, and configured clones
    Audit {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Maximum cleanup groups to display
        #[arg(long, default_value_t = 40)]
        top: usize,
        #[arg(long)]
        include_tests: bool,
        #[arg(long = "exclude")]
        excludes: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Lexical prior-art search: rank existing fns/types by identifier-token
    /// (Jaccard) similarity to what a change adds. No model required.
    Names {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Base ref (default: origin/main, origin/master, main, or master)
        #[arg(long)]
        base: Option<String>,
        /// Score a bare identifier instead of the git diff (repeatable).
        /// Use before writing the function, when there is nothing to diff yet.
        #[arg(long = "name")]
        names: Vec<String>,
        /// Candidates to list per query
        #[arg(long, default_value_t = 5)]
        top: usize,
        /// Hide candidates scoring below this (0.0-1.0)
        #[arg(long, default_value_t = 0.3)]
        min_score: f32,
        /// Include test functions as candidates
        #[arg(long)]
        include_tests: bool,
        /// Audit the WHOLE repo instead of a diff: rank every pair of existing
        /// fns/types, once each, damped by how distinctive their shared words are
        #[arg(long)]
        all: bool,
        /// Skip files matching this glob, e.g. 'examples/**' (repeatable).
        /// Self-contained examples legitimately repeat helpers; excluding them
        /// is usually the difference between a readable audit and noise.
        #[arg(long = "exclude")]
        excludes: Vec<String>,
    },
    /// Duplication review of the current change vs a base ref:
    /// token clones touching changed lines + lexical prior art
    Review {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Base ref (default: origin/main, origin/master, main, or master)
        #[arg(long)]
        base: Option<String>,
        /// Lexical candidates to list per changed item
        #[arg(long, default_value_t = 3)]
        top: usize,
        /// Ignore changed items smaller than this
        #[arg(long, default_value_t = 5)]
        min_lines: u32,
    },
    /// Install the bundled Claude Code skills (review-for-duplicates, audit-duplicates)
    InstallSkill {
        /// Install into ./.claude/skills instead of ~/.claude/skills
        #[arg(long)]
        project: bool,
        /// Explicit skills directory (overrides the default location)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Index { path, private, out } => cmd_index(&path, private, out),
        Cmd::Clones { path } => cmd_clones(&path),
        Cmd::Bodies { path, min_tokens, top, include_tests, excludes, json } => {
            let mut ex = extract::extract_structural(&path, true, false)?;
            let globs = build_globs(&excludes)?;
            ex.fns.retain(|r| !globs.is_match(&r.file));
            let (total, pairs) = bodies::pairs(&ex, min_tokens, include_tests, top);
            let shaped = ex.fns.iter().filter(|r| r.shape.as_ref().is_some_and(|shape| shape.leaves >= min_tokens) && (include_tests || !r.is_testish())).count();
            if json {
                let output: Vec<_> = pairs.iter().take(top).map(|p| serde_json::json!({
                    "a": bodies::location(p.a), "b": bodies::location(p.b), "tokens": p.tokens,
                    "evidence": ["body"]
                })).collect();
                println!("{}", serde_json::json!({"eligible_functions": shaped, "total_pairs": total, "pairs": output}));
            } else {
                println!("# normalized bodies ({} eligible functions, {} pairs)\n", shaped, total);
                for pair in pairs.iter().take(top) {
                    println!("{}:{}-{} <-> {}:{}-{} ({} tokens)", pair.a.file, pair.a.start, pair.a.end,
                             pair.b.file, pair.b.start, pair.b.end, pair.tokens);
                }
            }
            Ok(())
        }
        Cmd::Blocks { path, min_tokens, top, include_tests, excludes, json } => {
            let mut ex = extract::extract_structural(&path, false, true)?;
            let globs = build_globs(&excludes)?;
            ex.blocks.retain(|r| !globs.is_match(&r.file));
            let (total, pairs) = blocks::pairs(&ex.blocks, min_tokens, include_tests, top);
            if json {
                let output: Vec<_> = pairs.iter().take(top).map(|p| serde_json::json!({
                    "a": blocks::location(p.a), "b": blocks::location(p.b),
                    "tokens": p.a.shape.leaves, "evidence": ["block"]
                })).collect();
                println!("{}", serde_json::json!({"total_pairs": total, "pairs": output}));
            } else {
                println!("# statement blocks ({} pairs)\n", total);
                for p in pairs.iter().take(top) {
                    println!("{}:{}-{} <-> {}:{}-{} ({} tokens)", p.a.file, p.a.start, p.a.end,
                             p.b.file, p.b.start, p.b.end, p.a.shape.leaves);
                }
            }
            Ok(())
        }
        Cmd::Audit { path, top, include_tests, excludes, json } =>
            audit::run(&path, top, include_tests, &excludes, json),
        Cmd::Names {
            path,
            base,
            names,
            top,
            min_score,
            include_tests,
            all,
            excludes,
        } => cmd_names(
            &path,
            base,
            &names,
            top,
            min_score,
            include_tests,
            all,
            &excludes,
            1,
        ),
        Cmd::Review {
            path,
            base,
            top,
            min_lines,
        } => cmd_review(&path, base, top, min_lines),
        Cmd::InstallSkill { project, dir } => skill::install(project, dir),
    }
}

// ---------------------------------------------------------------- index

fn cmd_index(root: &Path, include_private: bool, out: Option<PathBuf>) -> Result<()> {
    let ex = extract::extract_dir(root)?;
    let mut md = String::from("# API index\n\nGenerated by dupfinder. Grep here for an existing helper before writing a new one.\n");
    let mut files: BTreeSet<&str> = BTreeSet::new();
    for r in &ex.fns {
        files.insert(&r.file);
    }
    for t in &ex.types {
        files.insert(&t.file);
    }
    let mut n = 0usize;
    for file in files {
        let mut section = String::new();
        for t in ex.types.iter().filter(|t| t.file == file) {
            if !(t.public || include_private) {
                continue;
            }
            section.push_str(&format!("- {} `{}`", t.kind, t.name));
            if !t.doc.is_empty() {
                section.push_str(&format!(" — {}", first_sentence(&t.doc)));
            }
            section.push_str(&format!("  ({}:{})\n", t.file, t.start));
            n += 1;
        }
        for r in ex.fns.iter().filter(|r| r.file == file) {
            if !(r.public || include_private) || r.is_testish() {
                continue;
            }
            let sig = r.sig.replace("pub fn ", "fn ").replace("pub(crate) fn ", "fn ");
            section.push_str(&format!("- `{}`", clip(&sig, 150)));
            if !r.doc.is_empty() {
                section.push_str(&format!(" — {}", first_sentence(&r.doc)));
            }
            section.push_str(&format!("  ({}:{})\n", r.file, r.start));
            n += 1;
        }
        if !section.is_empty() {
            md.push_str(&format!("\n## {file}\n{section}"));
        }
    }
    eprintln!("[dupfinder] indexed {n} items");
    match out {
        Some(p) => std::fs::write(p, md)?,
        None => print!("{md}"),
    }
    Ok(())
}

fn first_sentence(doc: &str) -> String {
    let s = doc.split(". ").next().unwrap_or(doc);
    clip(s, 110)
}

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

// --------------------------------------------------------------- clones

fn cmd_clones(root: &Path) -> Result<()> {
    match clones::run_jscpd(root)? {
        None => {}
        Some(list) => {
            println!("# token clones (jscpd): {}\n", list.len());
            for c in list {
                println!(
                    "{}:{}-{} <-> {}:{}-{}  ({} lines)",
                    c.file_a, c.a.0, c.a.1, c.file_b, c.b.0, c.b.1, c.lines
                );
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- names

#[allow(clippy::too_many_arguments)]
fn cmd_names(
    root: &Path,
    base: Option<String>,
    query_names: &[String],
    top: usize,
    min_score: f32,
    include_tests: bool,
    audit: bool,
    excludes: &[String],
    min_lines: u32,
) -> Result<()> {
    let ex = extract::extract_dir(root)?;
    let mut all = names::candidates(&ex);
    if !excludes.is_empty() {
        let before = all.len();
        let set = build_globs(excludes)?;
        all.retain(|c| !set.is_match(&c.file));
        println!("Excluded {} item(s) matching {}\n", before - all.len(), excludes.join(", "));
    }
    if audit {
        return audit_repo(&all, top, min_score, include_tests);
    }

    // Query set: explicit --name identifiers, else whatever the diff touches.
    let queries: Vec<names::Candidate> = if !query_names.is_empty() {
        query_names.iter().map(|n| names::query_from_name(n)).collect()
    } else {
        let base = gitdiff::resolve_base(root, base)?;
        let changed = gitdiff::changed_ranges(root, &base)?;
        println!("Base: `{base}` — {} changed file(s)\n", changed.len());
        all.iter()
            .filter(|c| {
                // Test code is not prior art worth deduping against, and a test
                // is not worth asking about either.
                (include_tests || !c.testish)
                    && c.end - c.start + 1 >= min_lines
                    && changed
                        .get(&c.file)
                        .is_some_and(|r| gitdiff::overlaps(r, c.start, c.end))
            })
            .cloned()
            .collect()
    };

    if queries.is_empty() {
        println!("Nothing to check (no changed fns/types, and no --name given).");
        return Ok(());
    }

    println!(
        "# lexical prior art ({} quer{}, {} candidates, min-score {min_score})\n",
        queries.len(),
        if queries.len() == 1 { "y" } else { "ies" },
        all.len()
    );
    println!("Token-overlap only — a rewrite sharing no words scores 0 here. Read the candidate before calling it a duplicate.\n");

    let mut any = false;
    for q in &queries {
        let mut hits: Vec<(f32, f32, f32, &names::Candidate)> = all
            .iter()
            .filter(|c| {
                // Skip the query itself and anything overlapping it.
                (include_tests || !c.testish)
                    && !(c.file == q.file && c.start <= q.end && q.start <= c.end)
            })
            .map(|c| {
                let (s, n, t) = names::score(q, c);
                (s, n, t, c)
            })
            .filter(|(s, ..)| *s >= min_score)
            .collect();
        hits.sort_by(|a, b| b.0.total_cmp(&a.0));
        hits.truncate(top);
        if hits.is_empty() {
            continue;
        }
        any = true;
        if q.kind == "query" {
            println!("### `{}`", q.name);
        } else {
            println!("### `{}`  ({} {})", q.name, q.kind, q.location());
        }
        for (s, n, t, c) in hits {
            let doc = if c.doc.is_empty() {
                String::new()
            } else {
                format!(" — {}", first_sentence(&c.doc))
            };
            println!("- {s:.2}  [name {n:.2} / types {t:.2}]  {} `{}`  {}{}", c.kind, c.name, c.location(), doc);
        }
        println!();
    }
    if !any {
        println!("No lexically similar prior art above {min_score}.");
    }
    Ok(())
}


/// Compile --exclude globs. Bare `dir/**` also excludes `dir` itself, which is
/// what people mean by it.
fn build_globs(patterns: &[String]) -> Result<globset::GlobSet> {
    let mut b = globset::GlobSetBuilder::new();
    for p in patterns {
        b.add(globset::Glob::new(p).with_context(|| format!("bad --exclude glob: {p}"))?);
        if let Some(stem) = p.strip_suffix("/**") {
            b.add(globset::Glob::new(stem)?);
        }
    }
    Ok(b.build()?)
}

/// Whole-repo audit: every unordered pair once, ranked by IDF-damped score.
fn audit_repo(all: &[names::Candidate], top: usize, min_score: f32, include_tests: bool) -> Result<()> {
    let pool = all.iter().filter(|c| (include_tests || !c.testish) && !c.delegating).count();
    let pairs = names::audit_pairs(all, min_score, include_tests);

    println!("# lexical duplication audit ({} candidates, {} pair(s) over {min_score})\n", pool, pairs.len());
    println!("Score = name/type Jaccard damped by how distinctive the shared words are, so `new` vs `new` sinks and `get_half_pixel` vs `get_half_pixel` floats. Trait impls that share a method name, or just forward to another method, are excluded. Read both sides before calling anything a duplicate.\n");
    for (s, n, t, a, b) in pairs.into_iter().take(top) {
        println!("{s:.2}  [name {n:.2} / types {t:.2}]");
        println!("      {} `{}`  {}", a.kind, a.name, a.location());
        println!("      {} `{}`  {}\n", b.kind, b.name, b.location());
    }
    Ok(())
}

// --------------------------------------------------------------- review

fn cmd_review(root: &Path, base: Option<String>, top: usize, min_lines: u32) -> Result<()> {
    let base = gitdiff::resolve_base(root, base)?;
    let changed = gitdiff::changed_ranges(root, &base)?;
    println!("# dupfinder review\n\nBase: `{base}` — {} changed file(s)\n", changed.len());
    if changed.is_empty() {
        println!("No changes to review.");
        return Ok(());
    }

    // --- token clones overlapping changed lines
    println!("## Token clones touching this change (jscpd)\n");
    match clones::run_jscpd(root)? {
        None => println!("(jscpd unavailable — skipped)\n"),
        Some(list) => {
            let hits: Vec<_> = list.iter().filter(|c| c.touches_change(&changed)).collect();
            if hits.is_empty() {
                println!("None.\n");
            } else {
                for c in hits {
                    println!(
                        "- `{}:{}-{}` <-> `{}:{}-{}` ({} lines)",
                        c.file_a, c.a.0, c.a.1, c.file_b, c.b.0, c.b.1, c.lines
                    );
                }
                println!();
            }
        }
    }

    println!("## Lexical prior art for changed items\n");
    cmd_names(
        root,
        Some(base),
        &[],
        top,
        0.3,
        false,
        false,
        &[],
        min_lines,
    )
}

#[cfg(test)]
mod glob_tests {
    use super::build_globs;

    #[test]
    fn dir_glob_also_matches_the_dir_itself() {
        let set = build_globs(&["examples/**".to_string()]).unwrap();
        assert!(set.is_match("examples/breakout/game.fun"));
        // `examples/**` alone does not match a file directly in `examples/`.
        assert!(set.is_match("examples"));
        assert!(!set.is_match("functor-lang/src/parser.rs"));
    }

    #[test]
    fn multiple_patterns_union() {
        let set = build_globs(&["site/demos/**".to_string(), "**/*.test.ts".to_string()]).unwrap();
        assert!(set.is_match("site/demos/mcp-drive.mjs"));
        assert!(set.is_match("tools/sdk/test/world-aim.test.ts"));
        assert!(!set.is_match("src/main.rs"));
    }

    #[test]
    fn bad_glob_is_an_error_not_a_panic() {
        assert!(build_globs(&["[unclosed".to_string()]).is_err());
    }
}
