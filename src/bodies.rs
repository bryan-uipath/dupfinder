//! Group equal normalized bodies; scores remain evidence, never equivalence claims.
use crate::extract::{Extraction, FnRecord};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

pub struct BodyPair<'a> {
    pub a: &'a FnRecord,
    pub b: &'a FnRecord,
    pub tokens: usize,
}

pub fn pairs(
    ex: &Extraction,
    min_tokens: usize,
    include_tests: bool,
    limit: usize,
) -> (usize, Vec<BodyPair<'_>>) {
    let mut groups: HashMap<&[String], Vec<&FnRecord>> = HashMap::new();
    for record in &ex.fns {
        if !include_tests && record.is_testish() {
            continue;
        }
        if let Some(shape) = &record.shape {
            if shape.leaves >= min_tokens {
                groups.entry(&shape.tokens).or_default().push(record);
            }
        }
    }
    let mut pairs = BTreeMap::new();
    let mut total = 0;
    for records in groups.values() {
        for (i, &a) in records.iter().enumerate() {
            for &b in &records[i + 1..] {
                if a.file == b.file && a.start <= b.end && b.start <= a.end {
                    continue;
                }
                if let Some(shape) = &a.shape {
                    total += 1;
                    let key = (
                        Reverse(shape.leaves),
                        a.file.as_str(),
                        a.start,
                        b.file.as_str(),
                        b.start,
                    );
                    if limit > 0
                        && (pairs.len() < limit
                            || pairs.last_key_value().is_some_and(|(last, _)| &key < last))
                    {
                        pairs.insert(
                            key,
                            BodyPair {
                                a,
                                b,
                                tokens: shape.leaves,
                            },
                        );
                        if pairs.len() > limit {
                            pairs.pop_last();
                        }
                    }
                }
            }
        }
    }
    (total, pairs.into_values().collect())
}

pub fn location(record: &FnRecord) -> serde_json::Value {
    serde_json::json!({"file": record.file, "name": record.name, "start": record.start, "end": record.end})
}
