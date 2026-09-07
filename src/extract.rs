//! Multi-language function/type extraction: Rust and TypeScript via
//! tree-sitter, Functor Lang (.fun) via a line-based parser (top-level
//! bindings only, which is Functor's whole reuse surface: file = module).

use anyhow::{Context, Result};
use std::path::Path;
use tree_sitter::{Node, Parser};

#[derive(Clone)]
pub struct FnRecord {
    pub name: String,
    /// Enclosing impl/trait/mod/class chain, outermost first.
    pub context: String,
    pub sig: String,
    pub doc: String,
    pub body: String,
    pub shape: Option<crate::normalized::Shape>,
    /// Root-relative path, '/'-separated.
    pub file: String,
    /// 1-based lines.
    pub start: u32,
    pub end: u32,
    pub public: bool,
}

#[derive(Clone)]
pub struct TypeRecord {
    pub name: String,
    pub kind: String,
    pub doc: String,
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub public: bool,
}

pub struct Extraction {
    pub fns: Vec<FnRecord>,
    pub types: Vec<TypeRecord>,
}

impl FnRecord {
    pub fn is_testish(&self) -> bool {
        self.context.contains("mod tests") || is_test_file(&self.file)
    }
}

/// Shared by function and type candidates so test declarations stay out of audits.
pub fn is_test_file(file: &str) -> bool {
    file.split('/')
        .any(|part| matches!(part, "tests" | "test" | "__tests__" | "__mocks__"))
        || ["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"]
            .iter()
            .any(|ext| {
                file.ends_with(&format!(".test.{ext}")) || file.ends_with(&format!(".spec.{ext}"))
            })
}

fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn clip_doc(s: String) -> String {
    truncate_chars(&s, 300).to_string()
}

const SKIP_DIRS: &[&str] = &["node_modules", "target", "dist", "build", ".git", "vendor"];

pub fn extract_dir(root: &Path) -> Result<Extraction> {
    let mut files: Vec<std::path::PathBuf> = ignore::WalkBuilder::new(root)
        .hidden(true)
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .filter(|p| {
            !p.components().any(|c| {
                c.as_os_str()
                    .to_str()
                    .is_some_and(|s| SKIP_DIRS.contains(&s))
            })
        })
        .collect();
    files.sort();

    let mut ex = Extraction {
        fns: Vec::new(),
        types: Vec::new(),
    };

    let mut rust_parser = Parser::new();
    rust_parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .context("load rust grammar")?;
    let mut ts_parser = Parser::new();
    ts_parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .context("load ts grammar")?;
    let mut tsx_parser = Parser::new();
    tsx_parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .context("load tsx grammar")?;

    for path in files {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".d.ts") {
            continue;
        }
        let parser: Option<&mut Parser> = match ext {
            "rs" => Some(&mut rust_parser),
            "ts" | "mts" | "cts" => Some(&mut ts_parser),
            "tsx" | "jsx" | "js" | "mjs" | "cjs" => Some(&mut tsx_parser),
            "fun" => None,
            _ => continue,
        };
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        match parser {
            Some(p) => {
                let lang = if ext == "rs" { "rust" } else { "typescript" };
                let Some(tree) = p.parse(&src, None) else {
                    continue;
                };
                if lang == "rust" {
                    walk_rust(tree.root_node(), &src, &rel, &mut ex);
                } else {
                    walk_ts(tree.root_node(), &src, &rel, &mut ex);
                }
            }
            None => extract_functor(&src, &rel, &mut ex),
        }
    }
    Ok(ex)
}

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------- Rust

fn rust_doc_before(node: Node, src: &str) -> String {
    let mut parts = Vec::new();
    let mut sib = node.prev_named_sibling();
    while let Some(s) = sib {
        match s.kind() {
            "line_comment" => {
                let t = text(s, src);
                if let Some(stripped) = t.strip_prefix("///") {
                    parts.push(stripped.trim().to_string());
                } else {
                    break;
                }
            }
            // Keep walking past #[derive]/#[cfg]/#[test] to the docs above them.
            "attribute_item" => {}
            _ => break,
        }
        sib = s.prev_named_sibling();
    }
    parts.reverse();
    clip_doc(parts.join(" "))
}

fn rust_context(node: Node, src: &str) -> String {
    let mut parts = Vec::new();
    let mut cur = node.parent();
    while let Some(n) = cur {
        match n.kind() {
            "impl_item" => {
                let ty = n
                    .child_by_field_name("type")
                    .map(|t| collapse_ws(text(t, src)))
                    .unwrap_or_else(|| "?".into());
                match n.child_by_field_name("trait") {
                    Some(tr) => parts.push(format!("impl {} for {}", collapse_ws(text(tr, src)), ty)),
                    None => parts.push(format!("impl {ty}")),
                }
            }
            "trait_item" => {
                if let Some(name) = n.child_by_field_name("name") {
                    parts.push(format!("trait {}", text(name, src)));
                }
            }
            "mod_item" => {
                if let Some(name) = n.child_by_field_name("name") {
                    parts.push(format!("mod {}", text(name, src)));
                }
            }
            _ => {}
        }
        cur = n.parent();
    }
    parts.reverse();
    parts.join(" / ")
}

fn walk_rust(node: Node, src: &str, file: &str, ex: &mut Extraction) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let body = text(child, src);
                    let sig_end = child
                        .child_by_field_name("body")
                        .map(|b| b.start_byte() - child.start_byte())
                        .unwrap_or(body.len());
                    let sig = collapse_ws(&body[..sig_end]);
                    let start = child.start_position().row as u32 + 1;
                    let end = child.end_position().row as u32 + 1;
                    ex.fns.push(FnRecord {
                        name: text(name_node, src).to_string(),
                        context: rust_context(child, src),
                        doc: rust_doc_before(child, src),
                        public: sig.starts_with("pub"),
                        sig,
                        body: body.to_string(),
                        shape: None,
                        file: file.to_string(),
                        start,
                        end,
                    });
                }
            }
            "struct_item" | "enum_item" | "trait_item" | "type_item" | "union_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let head = text(child, src);
                    ex.types.push(TypeRecord {
                        name: text(name_node, src).to_string(),
                        kind: child.kind().trim_end_matches("_item").to_string(),
                        doc: rust_doc_before(child, src),
                        file: file.to_string(),
                        start: child.start_position().row as u32 + 1,
                        end: child.end_position().row as u32 + 1,

                        public: head.starts_with("pub"),
                    });
                }
            }
            _ => {}
        }
        walk_rust(child, src, file, ex);
    }
}

// ---------------------------------------------------------- TypeScript

fn ts_doc_before(node: Node, src: &str) -> String {
    let mut sib = node.prev_named_sibling();
    // Hop over `export` wrappers handled by callers passing the outermost node.
    while let Some(s) = sib {
        if s.kind() == "comment" {
            let t = text(s, src);
            if t.starts_with("/**") || t.starts_with("//") {
                let cleaned: String = t
                    .lines()
                    .map(|l| {
                        l.trim()
                            .trim_start_matches("/**")
                            .trim_start_matches("*/")
                            .trim_start_matches("//")
                            .trim_start_matches('*')
                            .trim()
                    })
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                return clip_doc(cleaned);
            }
            return String::new();
        }
        if s.kind() == "decorator" {
            sib = s.prev_named_sibling();
            continue;
        }
        return String::new();
    }
    String::new()
}

fn ts_context(node: Node, src: &str) -> String {
    let mut parts = Vec::new();
    let mut cur = node.parent();
    while let Some(n) = cur {
        match n.kind() {
            "class_declaration" | "abstract_class_declaration" => {
                if let Some(name) = n.child_by_field_name("name") {
                    parts.push(format!("class {}", text(name, src)));
                }
            }
            "internal_module" | "module" => {
                if let Some(name) = n.child_by_field_name("name") {
                    parts.push(format!("namespace {}", text(name, src)));
                }
            }
            _ => {}
        }
        cur = n.parent();
    }
    parts.reverse();
    parts.join(" / ")
}

fn ts_is_exported(node: Node, src: &str) -> bool {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if n.kind() == "export_statement" {
            return true;
        }
        if matches!(n.kind(), "program" | "function_declaration" | "function_expression" | "arrow_function"
            | "generator_function_declaration" | "method_definition" | "call_expression" | "class_static_block")
            || (n.kind() == "public_field_definition" && ts_is_private(n, src)) {
            return false;
        }
        cur = n.parent();
    }
    false
}

fn ts_is_private(node: Node, src: &str) -> bool {
    let mut cursor = node.walk();
    let private = node.children(&mut cursor).any(|n| {
        (n.kind() == "accessibility_modifier" && matches!(text(n, src), "private" | "protected"))
            || n.kind() == "private_property_identifier"
    });
    private
}

/// The node whose prev-sibling holds the doc comment: the export_statement
/// wrapper when present, else the declaration (or lexical_declaration) itself.
fn ts_doc_anchor(node: Node) -> Node {
    let mut anchor = node;
    while let Some(p) = anchor.parent() {
        match p.kind() {
            "export_statement" | "lexical_declaration" | "variable_declaration" => anchor = p,
            _ => break,
        }
    }
    anchor
}

fn push_ts_fn(ex: &mut Extraction, node: Node, name_node: Node, body_node: Option<Node>, sig_from: Node, src: &str, file: &str, public: bool) {
    let full = text(sig_from, src);
    let sig_end = body_node
        .map(|b| b.start_byte().saturating_sub(sig_from.start_byte()))
        .unwrap_or(full.len())
        .min(full.len());
    let start = sig_from.start_position().row as u32 + 1;
    let end = sig_from.end_position().row as u32 + 1;
    let raw_name = text(name_node, src);
    let name = if name_node.kind() == "string" { &raw_name[1..raw_name.len() - 1] } else { raw_name };
    ex.fns.push(FnRecord {
        name: name.to_string(),
        context: ts_context(node, src),
        sig: collapse_ws(&full[..sig_end]),
        doc: ts_doc_before(ts_doc_anchor(node), src),
        body: full.to_string(),
        shape: body_node.and_then(|body| body.parent()).and_then(|callable| crate::normalized::function(callable, src)),
        file: file.to_string(),
        start,
        end,

        public,
    });
}

fn walk_ts(node: Node, src: &str, file: &str, ex: &mut Extraction) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let body = child.child_by_field_name("body");
                    push_ts_fn(ex, child, name, body, child, src, file, ts_is_exported(child, src));
                }
            }
            "method_definition" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let body = child.child_by_field_name("body");
                    let private = ts_is_private(child, src);
                    push_ts_fn(ex, child, name, body, child, src, file, !private);
                }
            }
            "variable_declarator" | "pair" | "public_field_definition" => {
                let name_field = if child.kind() == "pair" { "key" } else { "name" };
                if let (Some(name), Some(value)) =
                    (child.child_by_field_name(name_field), child.child_by_field_name("value"))
                {
                    if matches!(name.kind(), "identifier" | "property_identifier" | "private_property_identifier" | "string") {
                        if let Some(callable) = ts_callable(value, src) {
                            let body = callable.child_by_field_name("body");
                            let private = child.kind() == "public_field_definition" && ts_is_private(child, src);
                            push_ts_fn(ex, child, name, body, child, src, file, !private && ts_is_exported(child, src));
                        }
                    }
                }
            }
            "interface_declaration" | "type_alias_declaration" | "enum_declaration"
            | "class_declaration" | "abstract_class_declaration" => {
                if let Some(name) = child.child_by_field_name("name") {
                    ex.types.push(TypeRecord {
                        name: text(name, src).to_string(),
                        kind: child.kind().trim_end_matches("_declaration").replace("_", " "),
                        doc: ts_doc_before(ts_doc_anchor(child), src),
                        file: file.to_string(),
                        start: child.start_position().row as u32 + 1,
                        end: child.end_position().row as u32 + 1,

                        public: ts_is_exported(child, src),
                    });
                }
            }
            _ => {}
        }
        walk_ts(child, src, file, ex);
    }
}

/// Only unwrap APIs whose first argument is the callable being named.
fn ts_callable<'a>(mut value: Node<'a>, src: &str) -> Option<Node<'a>> {
    loop {
        match value.kind() {
            "arrow_function" | "function_expression" => return Some(value),
            "call_expression" => {
                let callee = value.child_by_field_name("function")?;
                if !ts_react_wrapper(callee, src) {
                    return None;
                }
                let args = value.child_by_field_name("arguments")?;
                let mut cursor = args.walk();
                value = args.named_children(&mut cursor).find(|n| n.kind() != "comment")?;
            }
            _ => return None,
        }
    }
}

/// Match imported React wrappers, including named and namespace aliases.
fn ts_react_wrapper(callee: Node, src: &str) -> bool {
    let (binding, member) = if callee.kind() == "identifier" {
        (text(callee, src), None)
    } else if let (Some(object), Some(property)) = (callee.child_by_field_name("object"), callee.child_by_field_name("property")) {
        if object.kind() != "identifier" { return false; }
        (text(object, src), Some(text(property, src)))
    } else { return false; };
    let allowed = |name: &str| matches!(name, "useCallback" | "memo" | "forwardRef");
    let mut root = callee;
    while let Some(parent) = root.parent() { root = parent; }
    // A local declaration can hide the import. Abstain conservatively on collisions.
    let mut nodes = vec![root];
    while let Some(node) = nodes.pop() {
        if node.kind() == "import_statement" { continue; }
        let declared = match node.kind() {
            "variable_declarator" | "function_declaration" | "class_declaration" => node.child_by_field_name("name"),
            "required_parameter" | "optional_parameter" => node.child_by_field_name("pattern"),
            "catch_clause" => node.child_by_field_name("parameter"),
            "arrow_function" => node.child_by_field_name("parameter"),
            "pair_pattern" => node.child_by_field_name("value"),
            "assignment_pattern" => node.child_by_field_name("left"),
            "shorthand_property_identifier_pattern" => Some(node),
            _ => None,
        };
        if declared.is_some_and(|n| text(n, src) == binding) { return false; }
        let mut c = node.walk();
        nodes.extend(node.named_children(&mut c));
    }
    let mut cursor = root.walk();
    for import in root.named_children(&mut cursor).filter(|n| n.kind() == "import_statement") {
        if !import.child_by_field_name("source").is_some_and(|n| matches!(text(n, src), "'react'" | "\"react\"")) { continue; }
        let mut pending = vec![import];
        while let Some(node) = pending.pop() {
            if node.kind() == "import_specifier" && member.is_none() {
                if let Some(name) = node.child_by_field_name("name") {
                    let alias = node.child_by_field_name("alias").unwrap_or(name);
                    if text(alias, src) == binding && allowed(text(name, src)) { return true; }
                }
            }
            if matches!(node.kind(), "import_clause" | "namespace_import") && member.is_some_and(allowed) {
                let mut c = node.walk();
                if node.named_children(&mut c).any(|n| n.kind() == "identifier" && text(n, src) == binding) { return true; }
            }
            let mut c = node.walk();
            pending.extend(node.named_children(&mut c));
        }
    }
    false
}

// -------------------------------------------------------- Functor Lang

/// Line-based: a top-level binding is `let name = ...` at column 0; its body
/// runs to the next top-level construct. Doc = contiguous `//` lines above.
fn extract_functor(src: &str, file: &str, ex: &mut Extraction) {
    let lines: Vec<&str> = src.lines().collect();
    let is_top_level =
        |l: &str| !l.starts_with(' ') && !l.starts_with('\t') && !l.trim().is_empty();

    let mut starts: Vec<(usize, String, bool)> = Vec::new(); // (line idx, name, is_type)
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("let ") {
            if let Some(name) = ident_prefix(rest) {
                starts.push((i, name, false));
            }
        } else if let Some(rest) = line.strip_prefix("type ") {
            if let Some(name) = ident_prefix(rest) {
                starts.push((i, name, true));
            }
        }
    }

    for (k, (i, name, is_type)) in starts.iter().enumerate() {
        // Body: to the line before the next top-level construct (not just the
        // next binding — any non-indented, non-comment line ends the body).
        let mut end = lines.len();
        for (j, l) in lines.iter().enumerate().skip(i + 1) {
            if is_top_level(l) && !l.trim_start().starts_with("//") {
                end = j;
                break;
            }
        }
        // Trim trailing blank/comment-only lines off the body.
        while end > i + 1
            && (lines[end - 1].trim().is_empty() || lines[end - 1].trim().starts_with("//"))
        {
            end -= 1;
        }
        let mut doc_parts = Vec::new();
        let mut j = *i;
        while j > 0 && lines[j - 1].trim_start().starts_with("//") {
            doc_parts.push(lines[j - 1].trim_start().trim_start_matches("//").trim());
            j -= 1;
        }
        doc_parts.reverse();

        if *is_type {
            ex.types.push(TypeRecord {
                name: name.clone(),
                kind: "type".into(),
                doc: doc_parts.join(" "),
                file: file.to_string(),
                start: *i as u32 + 1,
                end: end as u32,

                public: true,
            });
            continue;
        }
        let body = lines[*i..end].join("\n");
        let _ = k;
        ex.fns.push(FnRecord {
            name: name.clone(),
            context: String::new(),
            sig: lines[*i].trim().to_string(),
            doc: doc_parts.join(" "),
            body,
            shape: None,
            file: file.to_string(),
            start: *i as u32 + 1,
            end: end as u32,

            public: true,
        });
    }
}

fn ident_prefix(s: &str) -> Option<String> {
    let name: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let rest = s[name.len()..].trim_start();
    if !name.is_empty() && (rest.starts_with('=') || rest.starts_with(':')) {
        Some(name)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_wrappers_properties_and_fields_once_with_ranges() {
        let src = "import React, {memo, forwardRef} from 'react'; export const save = React.useCallback((value: string) => {\n  return value.trim();\n}, []);\nexport const api = { clean: (x: string) => x.trim() };\nexport class Store {\n  encode = (x: string) => x.toLowerCase();\n  private hide = () => 1;\n  #secret = () => 2;\n}\nexport const View = memo(forwardRef((props, ref) => props.title));\nconst result = useMemo(() => 42, []);\nconst mapped = items.map(x => x.id);\n";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_typescript::LANGUAGE_TSX.into()).unwrap();
        let tree = parser.parse(src, None).unwrap();
        assert!(!tree.root_node().has_error());
        let mut ex = Extraction { fns: vec![], types: vec![] };
        walk_ts(tree.root_node(), src, "src/example.tsx", &mut ex);
        assert_eq!(ex.fns.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
                   vec!["save", "clean", "encode", "hide", "#secret", "View"]);
        assert_eq!((ex.fns[0].start, ex.fns[0].end), (1, 3));
        assert!(ex.fns[0].sig.contains("value: string"));
        assert!(!ex.fns[3].public);
        assert!(!ex.fns[4].public);
    }

    #[test]
    fn ignores_computed_names_and_non_callable_initializers() {
        let src = "const api = { [key]: () => 1, value: makeThing(() => 2) }; class A { data = 1; }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()).unwrap();
        let tree = parser.parse(src, None).unwrap();
        let mut ex = Extraction { fns: vec![], types: vec![] };
        walk_ts(tree.root_node(), src, "src/example.ts", &mut ex);
        assert!(ex.fns.is_empty());
    }
}

#[cfg(test)]
mod visibility_tests {
    use super::*;

    #[test]
    fn local_and_restricted_members_are_not_public_but_private_named_bindings_are() {
        let src = "import {useCallback} from 'react'; export function outer(){ const local={ hidden:()=>1 }; const cb=useCallback(()=>2,[]); } export const privateCallback=()=>1; export const api={ privateValue:()=>2, 'quoted':()=>3 }; export class A { protected hide=()=>1; private secret=()=>2; privateThing=()=>3; }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()).unwrap();
        let tree = parser.parse(src, None).unwrap();
        let mut ex = Extraction { fns: vec![], types: vec![] };
        walk_ts(tree.root_node(), src, "a.ts", &mut ex);
        let public: Vec<_> = ex.fns.iter().filter(|f| f.public).map(|f| f.name.as_str()).collect();
        assert_eq!(public, vec!["outer", "privateCallback", "privateValue", "quoted", "privateThing"]);
        assert!(ex.fns.iter().any(|f| f.name == "cb" && !f.public));
    }

    #[test]
    fn distinguishes_react_imports_from_other_forward_ref_apis() {
        let src = "import {forwardRef} from '@angular/core'; import {useCallback as stable} from 'react'; const token=forwardRef(()=>Thing); const fn=stable(()=>1,[]);";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()).unwrap();
        let tree = parser.parse(src, None).unwrap();
        let mut ex = Extraction { fns: vec![], types: vec![] };
        walk_ts(tree.root_node(), src, "a.ts", &mut ex);
        assert_eq!(ex.fns.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["fn"]);
    }
}
