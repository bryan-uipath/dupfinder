//! Conservative TypeScript/JavaScript body matching with lexical binding identities.
use std::ops::Range;
use tree_sitter::Node;

#[derive(Clone)]
pub struct Shape {
    pub tokens: Vec<String>,
    pub leaves: usize,
}

pub fn function(callable: Node, src: &str) -> Option<Shape> {
    if callable.has_error() {
        return None;
    }
    let body = callable.child_by_field_name("body")?;
    let mut bindings = Vec::new();
    if let Some(name) = callable.child_by_field_name("name") {
        if name.kind() == "identifier" {
            bind(name, callable.byte_range(), src, &mut bindings)?;
        }
    }
    let params = callable
        .child_by_field_name("parameters")
        .or_else(|| callable.child_by_field_name("parameter"));
    if let Some(params) = params {
        if params.kind() == "identifier" {
            bind(params, callable.byte_range(), src, &mut bindings)?;
        } else {
            let mut cursor = params.walk();
            for param in params
                .named_children(&mut cursor)
                .filter(|n| n.kind() != "comment")
            {
                let pattern = param
                    .child_by_field_name("pattern")
                    .or_else(|| param.child_by_field_name("name"))?;
                bind(pattern, callable.byte_range(), src, &mut bindings)?;
            }
        }
    }
    declarations(body, src, &mut bindings)?;
    let mut shape = Shape {
        tokens: vec![callable.kind().to_string()],
        leaves: 0,
    };
    let mut cursor = callable.walk();
    if callable.children(&mut cursor).any(|n| n.kind() == "async") {
        shape.tokens.push("async".into());
    }
    for part in [
        params,
        callable.child_by_field_name("return_type"),
        Some(body),
    ]
    .into_iter()
    .flatten()
    {
        emit(part, src, &bindings, &mut shape);
    }
    Some(shape)
}

struct Binding<'a> {
    name: &'a str,
    scope: Range<usize>,
}

fn bind<'a>(
    node: Node,
    scope: Range<usize>,
    src: &'a str,
    bindings: &mut Vec<Binding<'a>>,
) -> Option<()> {
    // Destructuring and shadowing need fuller binding analysis; abstain for now.
    if node.kind() != "identifier" {
        return None;
    }
    let name = &src[node.byte_range()];
    if bindings.iter().any(|binding| binding.name == name) {
        return None;
    }
    bindings.push(Binding { name, scope });
    Some(())
}

fn declarations<'a>(node: Node, src: &'a str, bindings: &mut Vec<Binding<'a>>) -> Option<()> {
    match node.kind() {
        "arrow_function"
        | "function_expression"
        | "function_declaration"
        | "generator_function"
        | "generator_function_declaration"
        | "class"
        | "class_declaration"
        | "method_definition"
        | "variable_declaration" | "with_statement" => return None,
        "variable_declarator" => {
            let name = node.child_by_field_name("name")?;
            let mut scope = node.parent()?;
            while !matches!(
                scope.kind(),
                "statement_block" | "for_statement" | "for_in_statement"
            ) {
                scope = scope.parent()?;
            }
            bind(name, scope.byte_range(), src, bindings)?;
        }
        "for_in_statement" => {
            if let Some(kind) = node.child_by_field_name("kind") {
                if kind.kind() == "var" {
                    return None;
                }
                bind(
                    node.child_by_field_name("left")?,
                    node.byte_range(),
                    src,
                    bindings,
                )?;
            }
        }
        "catch_clause" => {
            if let Some(param) = node.child_by_field_name("parameter") {
                bind(param, node.byte_range(), src, bindings)?;
            }
        }
        // Direct eval can refer to bindings through string literals.
        "call_expression"
            if node
                .child_by_field_name("function")
                .is_some_and(|f| &src[f.byte_range()] == "eval") =>
        {
            return None
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        declarations(child, src, bindings)?;
    }
    Some(())
}

fn emit(node: Node, src: &str, bindings: &[Binding], shape: &mut Shape) {
    if node.kind() == "comment" || node.kind() == ";" {
        return;
    }
    shape.tokens.push(format!("({}", node.kind()));
    if node.child_count() == 0 {
        let text = &src[node.byte_range()];
        let binding = bindings
            .iter()
            .position(|binding| binding.name == text && binding.scope.contains(&node.start_byte()));
        match (node.kind(), binding) {
            ("identifier", Some(index)) => shape.tokens.push(format!("binding:{index}")),
            ("shorthand_property_identifier", Some(index)) => {
                shape
                    .tokens
                    .push(format!("property:{text}:binding:{index}"));
            }
            _ => shape.tokens.push(text.to_string()),
        }
        shape.leaves += 1;
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            emit(child, src, bindings, shape);
        }
    }
    shape.tokens.push(")".into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn shape(src: &str) -> Option<Shape> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .unwrap();
        let tree = parser.parse(src, None).unwrap();
        function(tree.root_node().named_child(0).unwrap(), src)
    }

    #[test]
    fn matches_renamed_bindings_and_comments() {
        let a = shape("function sum(items: number[]) { let total = 0; for (const item of items) { total += item; } return total; }").unwrap();
        let b = shape("function count(values: number[]) { /* comment */ let result = 0; for (const value of values) { result += value; } return result; }").unwrap();
        assert_eq!(a.tokens, b.tokens);
    }

    #[test]
    fn preserves_semantic_tokens() {
        let original =
            "function f(x: number) { const y = encrypt(x + 1); return store.active + y; }";
        let a = shape(original).unwrap();
        for changed in [
            original.replace("+ 1", "- 1"),
            original.replace("+ 1", "+ 2"),
            original.replace("active", "archived"),
            original.replace("encrypt", "decrypt"),
        ] {
            assert_ne!(a.tokens, shape(&changed).unwrap().tokens);
        }
        assert_ne!(
            shape("function a(x) { return {x}; }").unwrap().tokens,
            shape("function b(y) { return {y}; }").unwrap().tokens
        );
    }

    #[test]
    fn does_not_leak_block_bindings_or_guess_unsupported_scopes() {
        assert_ne!(
            shape("function a() { { const x = 1; } return x; }")
                .unwrap()
                .tokens,
            shape("function b() { { const y = 1; } return y; }")
                .unwrap()
                .tokens
        );
        for src in [
            "function f(x) { { let x = 2; } return x; }",
            "function f({x}) { return x; }",
            "function f(x) { return () => x; }",
            "function f(x) { var y = x; return y; }",
            "function f(x) { return eval('x'); }",
        ] {
            assert!(shape(src).is_none(), "{src}");
        }
    }
}
