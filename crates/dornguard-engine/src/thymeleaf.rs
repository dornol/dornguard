use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser, Tree};

use crate::jspecify::{children, text};

pub(super) struct SanitizedOutput<'a> {
    methods: &'a [String],
    parser: Option<Parser>,
    trees: BTreeMap<PathBuf, Option<Tree>>,
}

impl<'a> SanitizedOutput<'a> {
    pub(super) fn new(methods: &'a [String]) -> Self {
        let parser = (!methods.is_empty())
            .then(|| {
                let mut parser = Parser::new();
                parser
                    .set_language(&tree_sitter_html::LANGUAGE.into())
                    .ok()?;
                Some(parser)
            })
            .flatten();
        Self {
            methods,
            parser,
            trees: BTreeMap::new(),
        }
    }

    pub(super) fn contains(&mut self, path: &Path, source: &str, range: Range<usize>) -> bool {
        let Some(parser) = &mut self.parser else {
            return false;
        };
        let tree = self.trees.entry(path.to_path_buf()).or_insert_with(|| {
            parser
                .parse(source, None)
                .filter(|tree| !tree.root_node().has_error())
        });
        tree.as_ref().is_some_and(|tree| {
            trusted_output(tree.root_node(), source, range, self.methods).unwrap_or(false)
        })
    }
}

fn trusted_output(
    root: Node<'_>,
    source: &str,
    range: Range<usize>,
    methods: &[String],
) -> Option<bool> {
    let mut node = root.descendant_for_byte_range(range.start, range.end)?;
    while node.kind() != "attribute" {
        node = node.parent()?;
    }
    let (name, output) = attribute(node, source)?;
    if name != "utext" {
        return None;
    }
    let expression = variable_expression(output)?;
    if trusted_call(expression, methods) {
        return Some(true);
    }
    if !identifier(expression) {
        return None;
    }

    // Stay inside the template's lexical scope. Any processor that can
    // introduce variables, replace content or change scope blocks inference.
    let mut tag = node.parent()?;
    loop {
        let mut binding = None;
        for attr in children(tag).filter(|node| node.kind() == "attribute") {
            if let Some((name, value)) = attribute(attr, source) {
                match name.as_str() {
                    "with" if binding.is_some() => return None,
                    "with" => binding = Some(value),
                    "each" | "fragment" | "object" | "insert" | "replace" | "include" => {
                        return None;
                    }
                    _ => {}
                }
            } else if children(attr).next().is_some_and(|name| {
                text(name, source).contains(':') && !text(name, source).starts_with("th:")
            }) {
                return None;
            }
        }
        if let Some(binding) = binding {
            // A single whole-value assignment proves scope and rules out
            // reassignments and concatenation in the same th:with.
            let (name, value) = binding.split_once('=')?;
            return Some(
                name.trim() == expression && trusted_call(variable_expression(value)?, methods),
            );
        }
        let element = tag.parent()?;
        let parent = element.parent()?;
        tag = children(parent).find(|node| node.kind() == "start_tag")?;
    }
}

fn attribute<'a>(node: Node<'_>, source: &'a str) -> Option<(String, &'a str)> {
    let mut parts = children(node);
    let name = text(parts.next()?, source).to_ascii_lowercase();
    let name = name
        .strip_prefix("th:")
        .or_else(|| name.strip_prefix("data-th-"))?;
    let value = parts.next()?;
    let value = if value.kind() == "quoted_attribute_value" {
        children(value).next()?
    } else {
        value
    };
    Some((name.to_owned(), text(value, source)))
}

fn variable_expression(value: &str) -> Option<&str> {
    Some(value.trim().strip_prefix("${")?.strip_suffix('}')?.trim())
}

fn trusted_call(expression: &str, methods: &[String]) -> bool {
    let Some((method, argument)) = expression
        .strip_suffix(')')
        .and_then(|call| call.split_once('('))
    else {
        return false;
    };
    methods.iter().any(|allowed| allowed == method.trim())
        && argument.trim().split('.').all(identifier)
}

fn identifier(name: &str) -> bool {
    let mut chars = name.bytes();
    chars
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && chars.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
