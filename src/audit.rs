//! Structural edges form review groups; name-only edges cannot join unrelated groups.
use crate::{blocks, bodies, clones, extract, names};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Site {
    file: String,
    start: u32,
    end: u32,
}

struct Pair {
    a: Site,
    b: Site,
    evidence: BTreeSet<&'static str>,
    lines: u32,
    name_score: f32,
    fragments: Vec<serde_json::Value>,
}

pub fn run(
    root: &Path,
    top: usize,
    include_tests: bool,
    excludes: &[String],
    json: bool,
) -> Result<()> {
    let globs = crate::build_globs(excludes)?;
    let allowed =
        |file: &str| !globs.is_match(file) && (include_tests || !extract::is_test_file(file));
    let mut ex = extract::extract_structural(root)?;
    ex.fns.retain(|r| !globs.is_match(&r.file));
    ex.types.retain(|r| !globs.is_match(&r.file));
    ex.blocks.retain(|r| !globs.is_match(&r.file));
    let mut pairs = BTreeMap::new();
    let site = |file: &str, start, end| Site {
        file: file.into(),
        start,
        end,
    };
    let mut add = |a: Site, b: Site, evidence, name_score| {
        let lines = (a.end - a.start + 1).min(b.end - b.start + 1);
        let fragment =
            serde_json::json!({"a": location(&a), "b": location(&b), "evidence": evidence});
        let a = enclosing(a, &ex);
        let b = enclosing(b, &ex);
        // Separate fragments inside one function still deserve review.
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let pair = pairs.entry((a.clone(), b.clone())).or_insert(Pair {
            a,
            b,
            evidence: BTreeSet::new(),
            lines: 0,
            name_score: 0.0,
            fragments: Vec::new(),
        });
        pair.evidence.insert(evidence);
        pair.fragments.push(fragment);
        if evidence != "name" {
            pair.lines = pair.lines.max(lines);
        }
        pair.name_score = pair.name_score.max(name_score);
    };
    let candidates = names::candidates(&ex);
    for (score, _, _, a, b) in names::audit_pairs(&candidates, 0.5, include_tests) {
        add(
            site(&a.file, a.start, a.end),
            site(&b.file, b.start, b.end),
            "name",
            score,
        );
    }
    for p in bodies::pairs(&ex, 30, include_tests, usize::MAX).1 {
        add(
            site(&p.a.file, p.a.start, p.a.end),
            site(&p.b.file, p.b.start, p.b.end),
            "body",
            0.0,
        );
    }
    for p in blocks::pairs(&ex.blocks, 20, include_tests) {
        add(
            site(&p.a.file, p.a.start, p.a.end),
            site(&p.b.file, p.b.start, p.b.end),
            "block",
            0.0,
        );
    }
    let token_clones = clones::run_jscpd(root)?;
    let clone_status = if token_clones.is_some() {
        "ok"
    } else {
        "unavailable"
    };
    for p in token_clones.into_iter().flatten() {
        if allowed(&p.file_a) && allowed(&p.file_b) {
            add(
                site(&p.file_a, p.a.0, p.a.1),
                site(&p.file_b, p.b.0, p.b.1),
                "clone",
                0.0,
            );
        }
    }
    let pairs: Vec<_> = pairs.into_values().collect();
    let groups = group(&pairs);
    let output: Vec<_> = groups.iter().take(top).map(|indices| {
        let sites: BTreeSet<_> = indices.iter().flat_map(|&i| [&pairs[i].a, &pairs[i].b]).collect();
        let evidence: BTreeSet<_> = indices.iter().flat_map(|&i| pairs[i].evidence.iter().copied()).collect();
        serde_json::json!({"locations": sites.into_iter().map(location).collect::<Vec<_>>(),
            "evidence": evidence, "pairs": indices.iter().map(|&i| pair_json(&pairs[i])).collect::<Vec<_>>()})
    }).collect();
    if json {
        let visible: Vec<_> = groups
            .iter()
            .take(top)
            .flatten()
            .map(|&i| pair_json(&pairs[i]))
            .collect();
        println!(
            "{}",
            serde_json::json!({"clone_status": clone_status, "total_groups": groups.len(),
            "total_pairs": pairs.len(), "groups": output, "pairs": visible})
        );
    } else {
        println!(
            "# grouped duplication audit ({} groups, {} pairs; clones: {})\n",
            groups.len(),
            pairs.len(),
            clone_status
        );
        for (i, g) in output.iter().enumerate() {
            println!("## {} — evidence {}", i + 1, g["evidence"]);
            if let Some(locations) = g["locations"].as_array() {
                for p in locations {
                    println!(
                        "- {}:{}-{}",
                        p["file"].as_str().unwrap_or(""),
                        p["start"],
                        p["end"]
                    );
                }
            }
            println!();
        }
    }
    Ok(())
}

fn enclosing(site: Site, ex: &extract::Extraction) -> Site {
    ex.fns
        .iter()
        .filter(|r| r.file == site.file && r.start <= site.start && r.end >= site.end)
        .min_by_key(|r| r.end - r.start)
        .map(|r| Site {
            file: r.file.clone(),
            start: r.start,
            end: r.end,
        })
        .unwrap_or(site)
}

fn group(pairs: &[Pair]) -> Vec<Vec<usize>> {
    let mut adjacency: BTreeMap<&Site, Vec<&Site>> = BTreeMap::new();
    for p in pairs
        .iter()
        .filter(|p| p.evidence.iter().any(|&e| e != "name"))
    {
        adjacency.entry(&p.a).or_default().push(&p.b);
        adjacency.entry(&p.b).or_default().push(&p.a);
    }
    let mut owners = BTreeMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for &start in adjacency.keys() {
        if owners.contains_key(start) {
            continue;
        }
        let id = groups.len();
        groups.push(Vec::new());
        let mut pending = vec![start];
        while let Some(site) = pending.pop() {
            if owners.contains_key(site) {
                continue;
            }
            owners.insert(site, id);
            pending.extend(adjacency.get(site).into_iter().flatten().copied());
        }
    }
    for (i, p) in pairs.iter().enumerate() {
        if let Some(&owner) = owners
            .get(&p.a)
            .filter(|&owner| Some(owner) == owners.get(&p.b))
        {
            groups[owner].push(i);
        } else {
            groups.push(vec![i]);
        }
    }
    let rank = |indices: &[usize]| {
        let structural: BTreeSet<_> = indices
            .iter()
            .flat_map(|&i| pairs[i].evidence.iter())
            .filter(|&&e| e != "name")
            .collect();
        let lines = indices.iter().map(|&i| pairs[i].lines).max().unwrap_or(0);
        let score = indices
            .iter()
            .map(|&i| (pairs[i].name_score * 1000.0) as u32)
            .max()
            .unwrap_or(0);
        (structural.len(), lines, score)
    };
    groups.sort_by(|a, b| rank(b).cmp(&rank(a)).then(a[0].cmp(&b[0])));
    groups
}

fn location(site: &Site) -> serde_json::Value {
    serde_json::json!({"file": site.file, "start": site.start, "end": site.end})
}

fn pair_json(pair: &Pair) -> serde_json::Value {
    serde_json::json!({"a": location(&pair.a), "b": location(&pair.b), "evidence": pair.evidence,
        "fragments": pair.fragments, "matched_lines": pair.lines, "name_score": pair.name_score})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pair(a: &str, b: &str, evidence: &'static str) -> Pair {
        Pair {
            a: Site {
                file: a.into(),
                start: 1,
                end: 10,
            },
            b: Site {
                file: b.into(),
                start: 1,
                end: 10,
            },
            evidence: [evidence].into_iter().collect(),
            lines: 10,
            name_score: 0.5,
            fragments: vec![],
        }
    }

    #[test]
    fn names_cannot_bridge_distinct_structural_groups() {
        let pairs = vec![
            pair("a", "b", "body"),
            pair("c", "d", "block"),
            pair("b", "c", "name"),
        ];
        let groups = group(&pairs);
        assert_eq!(groups, vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn related_structural_edges_group_once_and_rank_first() {
        let pairs = vec![
            pair("x", "y", "name"),
            pair("a", "b", "body"),
            pair("b", "c", "block"),
            pair("a", "c", "name"),
        ];
        assert_eq!(group(&pairs), vec![vec![1, 2, 3], vec![0]]);
        assert!(group(&[]).is_empty());
    }
}
