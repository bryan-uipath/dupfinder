//! Fixed statement windows keep indexing linear in the number of statements.
use crate::{extract::is_test_file, normalized};
use std::collections::HashMap;
use tree_sitter::Node;

pub struct Block {
    pub file: String,
    pub start: u32,
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
                start: window[0].start_position().row as u32 + 1,
                end: window[2].end_position().row as u32 + 1,
                shape,
            });
        }
    }
}

pub fn pairs(blocks: &[Block], min_tokens: usize, include_tests: bool) -> Vec<BlockPair<'_>> {
    let mut groups: HashMap<&[String], Vec<&Block>> = HashMap::new();
    for block in blocks {
        if block.shape.leaves >= min_tokens && (include_tests || !is_test_file(&block.file)) {
            groups.entry(&block.shape.tokens).or_default().push(block);
        }
    }
    let mut pairs = Vec::new();
    for records in groups.values() {
        for (i, &a) in records.iter().enumerate() {
            for &b in &records[i + 1..] {
                if a.file != b.file || a.end < b.start || b.end < a.start {
                    pairs.push(BlockPair { a, b });
                }
            }
        }
    }
    pairs.sort_by(|a, b| {
        b.a.shape.leaves.cmp(&a.a.shape.leaves).then_with(|| {
            (&a.a.file, a.a.start, &a.b.file, a.b.start)
                .cmp(&(&b.a.file, b.a.start, &b.b.file, b.b.start))
        })
    });
    pairs
}

pub fn location(block: &Block) -> serde_json::Value {
    serde_json::json!({"file": block.file, "start": block.start, "end": block.end})
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
    fn finds_internal_windows_despite_different_surrounding_statements() {
        let mut blocks = scan("function a() { before(); const x = input.trim(); const y = codec.encode(x); save(y); after(); }", "a.ts");
        blocks.extend(scan("function b() { other(); const clean = input.trim(); const result = codec.encode(clean); save(result); finish(); }", "b.ts"));
        let found = pairs(&blocks, 20, false);
        assert_eq!(found.len(), 1);
        assert!(pairs(&blocks, 100, false).is_empty());
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
        assert!(pairs(&blocks, 20, false).is_empty());
        assert_eq!(pairs(&blocks, 20, true).len(), 1);
    }
}
