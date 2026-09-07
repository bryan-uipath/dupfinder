//! Group equal normalized bodies; scores remain evidence, never equivalence claims.
use crate::extract::{Extraction, FnRecord};
use std::collections::HashMap;

pub struct BodyPair<'a> {
    pub a: &'a FnRecord,
    pub b: &'a FnRecord,
    pub tokens: usize,
}

pub fn pairs(ex: &Extraction, min_tokens: usize, include_tests: bool) -> Vec<BodyPair<'_>> {
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
    let mut pairs = Vec::new();
    for records in groups.values() {
        for (i, &a) in records.iter().enumerate() {
            for &b in &records[i + 1..] {
                if a.file == b.file && a.start <= b.end && b.start <= a.end {
                    continue;
                }
                if let Some(shape) = &a.shape {
                    pairs.push(BodyPair {
                        a,
                        b,
                        tokens: shape.leaves,
                    });
                }
            }
        }
    }
    pairs.sort_by(|a, b| {
        b.tokens.cmp(&a.tokens).then_with(|| {
            (&a.a.file, a.a.start, &a.b.file, a.b.start)
                .cmp(&(&b.a.file, b.a.start, &b.b.file, b.b.start))
        })
    });
    pairs
}

pub fn location(record: &FnRecord) -> serde_json::Value {
    serde_json::json!({"file": record.file, "name": record.name, "start": record.start, "end": record.end})
}
