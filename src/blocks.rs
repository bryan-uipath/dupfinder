//! Fixed statement windows keep indexing linear in the number of statements.
use crate::{extract::is_test_file, normalized};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use tree_sitter::Node;

pub struct Block {
    pub file: String,
    pub start: u32,
    pub bytes: std::ops::Range<usize>,
    pub end: u32,
    pub shape: normalized::Shape,
}

pub struct BlockPair<'a> {
    pub a: &'a Block,
    pub b: &'a Block,
}

pub fn extract(node: Node, src: &str, file: &str, out: &mut Vec<Block>) {
    let mut cursor = node.walk();
    let statements: Vec<_> = node
        .named_children(&mut cursor)
        .filter(|n| n.kind() != "comment")
        .collect();
    for window in statements.windows(3) {
        if let Some(shape) = normalized::statements(window, src) {
            out.push(Block {
                file: file.to_string(),
                bytes: window[0].start_byte()..window[2].end_byte(),
                start: window[0].start_position().row as u32 + 1,
                end: window[2].end_position().row as u32 + 1,
                shape,
            });
        }
    }
}

pub fn pairs(
    blocks: &[Block],
    min_tokens: usize,
    include_tests: bool,
    limit: usize,
) -> (usize, Vec<BlockPair<'_>>) {
    let mut pairs = BTreeMap::new();
    let mut total = 0;
    for records in families(blocks, min_tokens, include_tests) {
        for (i, &a) in records.iter().enumerate() {
            for &b in &records[i + 1..] {
                if !overlaps(a, b) {
                    total += 1;
                    let key = (
                        Reverse(a.shape.leaves),
                        a.file.as_str(),
                        a.bytes.start,
                        b.file.as_str(),
                        b.bytes.start,
                    );
                    if limit > 0
                        && (pairs.len() < limit
                            || pairs.last_key_value().is_some_and(|(last, _)| &key < last))
                    {
                        pairs.insert(key, BlockPair { a, b });
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

pub fn families(blocks: &[Block], min_tokens: usize, include_tests: bool) -> Vec<Vec<&Block>> {
    let mut groups: HashMap<&[String], Vec<&Block>> = HashMap::new();
    for block in blocks {
        if block.shape.leaves >= min_tokens && (include_tests || !is_test_file(&block.file)) {
            groups.entry(&block.shape.tokens).or_default().push(block);
        }
    }
    let mut families: Vec<_> = groups.into_values().collect();
    families.sort_by_key(|g| (&g[0].file, g[0].bytes.start));
    families
}

pub fn overlaps(a: &Block, b: &Block) -> bool {
    a.file == b.file && a.bytes.start < b.bytes.end && b.bytes.start < a.bytes.end
}

pub fn location(block: &Block) -> serde_json::Value {
    serde_json::json!({"file": block.file, "start": block.start, "end": block.end, "start_byte": block.bytes.start, "end_byte": block.bytes.end})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(src: &str, file: &str) -> Vec<Block> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .unwrap();
        let tree = parser.parse(src, None).unwrap();
        let body = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        let mut blocks = Vec::new();
        extract(body, src, file, &mut blocks);
        blocks
    }

    #[test]
    fn detects_byte_disjoint_windows_on_one_line() {
        let src = "function a(){alpha(one,two,three);beta(four,five,six);gamma(seven,eight,nine);} function b(){alpha(one,two,three);beta(four,five,six);gamma(seven,eight,nine);}";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .unwrap();
        let tree = parser.parse(src, None).unwrap();
        let mut blocks = Vec::new();
        let mut cursor = tree.root_node().walk();
        for function in tree.root_node().named_children(&mut cursor) {
            extract(
                function.child_by_field_name("body").unwrap(),
                src,
                "a.ts",
                &mut blocks,
            );
        }
        assert_eq!(pairs(&blocks, 20, false, usize::MAX).1.len(), 1);
        let (_, found) = pairs(&blocks, 20, false, 1);
        assert_ne!(location(found[0].a), location(found[0].b));
        assert_eq!(pairs(&blocks, 20, false, 0).0, 1);
        assert!(pairs(&blocks, 20, false, 0).1.is_empty());
    }

    #[test]
    fn finds_internal_windows_despite_different_surrounding_statements() {
        let mut blocks = scan("function a() { before(); const x = input.trim(); const y = codec.encode(x); save(y); after(); }", "a.ts");
        blocks.extend(scan("function b() { other(); const clean = input.trim(); const result = codec.encode(clean); save(result); finish(); }", "b.ts"));
        let found = pairs(&blocks, 20, false, usize::MAX).1;
        assert_eq!(found.len(), 1);
        assert!(pairs(&blocks, 100, false, usize::MAX).1.is_empty());
    }

    #[test]
    fn preserves_external_names_and_excludes_test_files() {
        let mut blocks = scan(
            "function a() { const x = input.trim(); const y = codec.encode(x); save(y); }",
            "a.ts",
        );
        blocks.extend(scan(
            "function b() { const x = other.trim(); const y = codec.encode(x); save(y); }",
            "b.ts",
        ));
        blocks.extend(scan(
            "function c() { const x = input.trim(); const y = codec.encode(x); save(y); }",
            "a.test.ts",
        ));
        assert!(pairs(&blocks, 20, false, usize::MAX).1.is_empty());
        assert_eq!(pairs(&blocks, 20, true, usize::MAX).1.len(), 1);
    }
}
