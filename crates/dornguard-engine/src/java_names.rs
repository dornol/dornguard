use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser, Tree};

use crate::jspecify::{children, text};

#[derive(Default)]
pub struct JavaNames {
    trees: BTreeMap<PathBuf, Option<Tree>>,
    parser: Parser,
}

impl JavaNames {
    pub fn starts_with_value(&mut self, path: &Path, source: &str, range: Range<usize>) -> bool {
        let tree = self.trees.entry(path.to_path_buf()).or_insert_with(|| {
            self.parser
                .set_language(&tree_sitter_java::LANGUAGE.into())
                .ok()?;
            self.parser.parse(source, None)
        });
        let Some(node) = tree.as_ref().and_then(|tree| {
            tree.root_node()
                .named_descendant_for_byte_range(range.start, range.end.saturating_sub(1))
        }) else {
            return false;
        };
        if !matches!(node.kind(), "field_access" | "method_invocation")
            || node.start_byte() != range.start
            || node.has_error()
        {
            return false;
        }
        let Some((head, _)) = source
            .get(range.clone())
            .and_then(|name| name.split_once('.'))
        else {
            return false;
        };
        let head = head.trim_end();
        std::iter::successors(node.parent(), Node::parent)
            .any(|scope| declares_value(scope, source, head, range.start))
    }
}

fn declares_value<'tree>(scope: Node<'tree>, source: &str, name: &str, position: usize) -> bool {
    let is_name = |node: Node<'tree>| text(node, source) == name;
    let declares_name = |node: Node<'tree>| node.child_by_field_name("name").is_some_and(is_name);
    let is_declarator =
        |node: Node<'tree>| node.kind() == "variable_declarator" && declares_name(node);
    let in_body = || {
        scope
            .child_by_field_name("body")
            .is_some_and(|body| body.start_byte() <= position && position < body.end_byte())
    };
    match scope.kind() {
        "block" | "constructor_body" | "switch_block_statement_group" => children(scope)
            .take_while(|node| node.start_byte() < position)
            .any(|node| {
                node.kind() == "local_variable_declaration"
                    && children(node).any(|declarator| {
                        is_declarator(declarator) && declarator.end_byte() <= position
                    })
            }),
        "for_statement" => children(scope).any(|node| {
            node.kind() == "local_variable_declaration"
                && node.end_byte() <= position
                && children(node).any(is_declarator)
        }),
        "enhanced_for_statement" => in_body() && declares_name(scope),
        "catch_clause" => children(scope)
            .any(|node| node.kind() == "catch_formal_parameter" && declares_name(node)),
        "try_with_resources_statement" => {
            // Resource bindings have no scope in catch and finally clauses.
            in_body()
                && children(scope)
                    .filter(|node| node.kind() == "resource_specification")
                    .flat_map(children)
                    .any(declares_name)
        }
        "method_declaration" | "constructor_declaration" | "lambda_expression" => {
            in_body()
                && scope
                    .child_by_field_name("parameters")
                    .is_some_and(|params| match params.kind() {
                        "identifier" => is_name(params),
                        "inferred_parameters" => children(params).any(is_name),
                        _ => children(params).any(|param| {
                            declares_name(param)
                                || (param.kind() == "spread_parameter"
                                    && children(param).any(is_declarator))
                        }),
                    })
        }
        "class_body" | "interface_body" => children(scope)
            .filter(|node| matches!(node.kind(), "field_declaration" | "constant_declaration"))
            .flat_map(children)
            .any(is_declarator),
        _ => false,
    }
}
