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
    bytes: Option<(usize, usize)>,
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
    let mut ex = extract::extract_structural(root, true, true)?;
    ex.fns.retain(|r| !globs.is_match(&r.file));
    ex.types.retain(|r| !globs.is_match(&r.file));
    ex.blocks.retain(|r| !globs.is_match(&r.file));
    let mut pairs = BTreeMap::new();
    let site = |file: &str, start, end| Site {
        file: file.into(),
        start,
        end,
        bytes: None,
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
        if !pair.fragments.contains(&fragment) {
            pair.fragments.push(fragment);
        }
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
    for family in bodies::families(&ex, 30, include_tests) {
        for (a, b) in links(&family, |a, b| bodies::overlaps(a, b)) {
            let fn_site = |r: &extract::FnRecord| Site {
                file: r.file.clone(),
                start: r.start,
                end: r.end,
                bytes: r.bytes.as_ref().map(|r| (r.start, r.end)),
            };
            add(fn_site(family[a]), fn_site(family[b]), "body", 0.0);
        }
    }
    for family in blocks::families(&ex.blocks, 20, include_tests) {
        for (a, b) in links(&family, |a, b| blocks::overlaps(a, b)) {
            let block_site = |b: &blocks::Block| Site {
                file: b.file.clone(),
                start: b.start,
                end: b.end,
                bytes: Some((b.bytes.start, b.bytes.end)),
            };
            add(block_site(family[a]), block_site(family[b]), "block", 0.0);
        }
    }
    let token_clones = clones::run_jscpd(root)?;
    let clone_status = if token_clones.is_some() {
        "ok"
    } else {
        "unavailable"
    };
    for p in token_clones.into_iter().flatten() {
        if allowed(&p.file_a)
            && allowed(&p.file_b)
            && (include_tests
                || (!test_region(&p.file_a, p.a, &ex) && !test_region(&p.file_b, p.b, &ex)))
        {
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

fn test_region(file: &str, range: (u32, u32), ex: &extract::Extraction) -> bool {
    let overlapping: Vec<_> = ex
        .fns
        .iter()
        .filter(|r| r.file == file && r.start <= range.1 && r.end >= range.0)
        .collect();
    !overlapping.is_empty() && overlapping.iter().all(|r| r.is_testish())
}

fn enclosing(site: Site, ex: &extract::Extraction) -> Site {
    let containing = ex
        .fns
        .iter()
        .filter(|r| {
            r.file == site.file
                && match (site.bytes, &r.bytes) {
                    (Some((start, end)), Some(bytes)) => bytes.start <= start && bytes.end >= end,
                    _ => r.start <= site.start && r.end >= site.end,
                }
        })
        .min_by_key(|r| {
            r.bytes
                .as_ref()
                .map(|b| b.end - b.start)
                .unwrap_or((r.end - r.start) as usize)
        });
    let Some(r) = containing else {
        return site;
    };
    Site {
        file: r.file.clone(),
        start: r.start,
        end: r.end,
        bytes: r.bytes.as_ref().map(|b| (b.start, b.end)),
    }
}

// Keep enough non-overlapping links to connect each family, without its quadratic edge list.
fn links<T>(records: &[T], overlaps: impl Fn(&T, &T) -> bool) -> Vec<(usize, usize)> {
    let mut parents: Vec<_> = (0..records.len()).collect();
    let mut links = Vec::new();
    for (i, a) in records.iter().enumerate() {
        for (j, b) in records.iter().enumerate().skip(i + 1) {
            let left = root(&mut parents, i);
            let right = root(&mut parents, j);
            if left != right && !overlaps(a, b) {
                parents[right] = left;
                links.push((i, j));
                if links.len() + 1 == records.len() {
                    return links;
                }
            }
        }
    }
    links
}

fn root(parents: &mut [usize], mut i: usize) -> usize {
    while parents[i] != i {
        parents[i] = parents[parents[i]];
        i = parents[i];
    }
    i
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
            .map(|&i| pairs[i].name_score.to_bits())
            .max()
            .unwrap_or(0);
        (structural.len(), lines, score)
    };
    groups.sort_by(|a, b| rank(b).cmp(&rank(a)).then(a[0].cmp(&b[0])));
    groups
}

fn location(site: &Site) -> serde_json::Value {
    let mut value = serde_json::json!({"file": site.file, "start": site.start, "end": site.end});
    if let Some((start, end)) = site.bytes {
        value["start_byte"] = start.into();
        value["end_byte"] = end.into();
    }
    value
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
                bytes: None,
            },
            b: Site {
                file: b.into(),
                start: 1,
                end: 10,
                bytes: None,
            },
            evidence: [evidence].into_iter().collect(),
            lines: 10,
            name_score: 0.5,
            fragments: vec![],
        }
    }

    #[test]
    fn compact_links_preserve_connectivity_through_overlapping_windows() {
        let records = [(0, 10), (1, 2), (3, 11), (12, 13)];
        let found = links(&records, |a, b| a.0 < b.1 && b.0 < a.1);
        assert_eq!(found.len(), 3);
        assert_eq!(links(&vec![0; 1000], |_, _| false).len(), 999);
    }

    #[test]
    fn byte_locations_remain_distinct_when_no_function_can_be_identified() {
        let a = Site {
            file: "one.ts".into(),
            start: 1,
            end: 1,
            bytes: Some((10, 30)),
        };
        let b = Site {
            bytes: Some((50, 70)),
            ..a.clone()
        };
        let ex = extract::Extraction {
            fns: vec![],
            types: vec![],
            blocks: vec![],
        };
        assert_ne!(location(&enclosing(a, &ex)), location(&enclosing(b, &ex)));
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
