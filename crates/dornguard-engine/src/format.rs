use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::Path;

use dornguard_config::{FileFormat, FormatConfig, LineEnding, Rule};
use dornguard_core::{Diagnostic, EngineName, Outcome, SourceLanguage, fingerprint};
use tree_sitter::{Node, Parser};

use crate::{Request, display_path, fail};

pub fn check(request: Request<'_>, settings: &FormatConfig) -> Outcome {
    let mut outcome = Outcome::default();
    let mut parser = Parser::new();
    let rules: BTreeMap<&str, &Rule> = request
        .rules
        .iter()
        .map(|rule| (rule.id.as_str(), *rule))
        .collect();
    for file in request.files {
        let Some(language) = SourceLanguage::of(file) else {
            continue;
        };
        let grammar = match language {
            SourceLanguage::Java => tree_sitter_java::LANGUAGE,
            SourceLanguage::Kotlin => tree_sitter_kotlin_ng::LANGUAGE,
            SourceLanguage::Javascript => tree_sitter_javascript::LANGUAGE,
            _ => continue,
        };
        let source = match fs::read_to_string(request.root.join(file)) {
            Ok(source) => source,
            Err(error) => {
                fail(
                    &mut outcome,
                    EngineName::Builtin,
                    format!("Could not read {}: {error}", file.display()),
                );
                continue;
            }
        };
        if let Err(error) = parser.set_language(&grammar.into()) {
            fail(&mut outcome, EngineName::Builtin, error.to_string());
            continue;
        }
        let Some(tree) = parser.parse(&source, None) else {
            fail(
                &mut outcome,
                EngineName::Builtin,
                format!("Could not parse {}", file.display()),
            );
            continue;
        };
        check_layout(
            file,
            &source,
            tree.root_node(),
            settings.for_file(file, language),
            &rules,
            &mut outcome,
        );
        outcome.scanned_files.insert(file.clone());
    }
    outcome
}

struct Line<'a> {
    start: usize,
    text: &'a str,
    ending: &'a str,
}

fn lines(mut source: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    while !source.is_empty() {
        let length = source.find(['\r', '\n']).unwrap_or(source.len());
        let (text, rest) = source.split_at(length);
        let ending_length = if rest.starts_with("\r\n") {
            2
        } else {
            usize::from(!rest.is_empty())
        };
        let (ending, rest) = rest.split_at(ending_length);
        lines.push(Line {
            start,
            text,
            ending,
        });
        start += length + ending_length;
        source = rest;
    }
    lines
}

fn line_index(lines: &[Line<'_>], offset: usize) -> usize {
    lines.partition_point(|line| line.start <= offset) - 1
}

fn check_layout(
    file: &Path,
    source: &str,
    root: Node<'_>,
    settings: FileFormat,
    rules: &BTreeMap<&str, &Rule>,
    outcome: &mut Outcome,
) {
    let lines = lines(source);
    if lines.is_empty() {
        return;
    }
    let mut emit = |id, row: usize, column: usize, detail: Option<String>| {
        if let Some(rule) = rules.get(id) {
            let line = &lines[row];
            outcome.diagnostics.push(Diagnostic {
                path: Some(display_path(file)),
                module: None,
                class_name: None,
                line: Some(row as u32 + 1),
                column: Some(line.text[..column].encode_utf16().count() as u32 + 1),
                end_line: None,
                end_column: None,
                subject: None,
                rule_id: rule.id.clone(),
                engine: EngineName::Builtin,
                severity: rule.severity,
                message: detail.unwrap_or_else(|| rule.description.clone()),
                fix: rule.fix.clone(),
                legacy_fingerprints: Vec::new(),
                fingerprint: Some(fingerprint(line.text.trim())),
            });
        }
    };

    let mut expected = BTreeMap::new();
    let mut literals = Vec::new();
    let mut cursor = root.walk();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if matches!(
            node.kind(),
            "string_literal" | "multiline_string_literal" | "string" | "template_string"
        ) {
            literals.push(node.byte_range());
            continue;
        }
        if node == root || !node.has_error() && is_block(node.kind()) {
            collect_indentation(
                node,
                root,
                source,
                &lines,
                usize::from(settings.indent_size),
                &mut expected,
            );
        }
        if !node.is_named()
            && matches!(
                node.kind(),
                "if" | "for"
                    | "while"
                    | "switch"
                    | "catch"
                    | "with"
                    | "when"
                    | "synchronized"
                    | "try"
            )
        {
            let tail = &source[node.end_byte()..];
            let rest = tail.trim_start_matches([' ', '\t', '\r', '\n']);
            if rest.starts_with('(') && &tail[..tail.len() - rest.len()] != " " {
                let row = line_index(&lines, node.start_byte());
                emit(
                    "format.control-spacing",
                    row,
                    node.start_byte() - lines[row].start,
                    None,
                );
            }
        }
        let previous = pending.len();
        pending.extend(node.children(&mut cursor));
        pending[previous..].reverse();
    }
    let in_literal = |offset: usize| {
        let index = literals.partition_point(|range: &Range<usize>| range.end <= offset);
        literals
            .get(index)
            .is_some_and(|range| range.contains(&offset))
    };
    let ending = match settings.line_ending {
        LineEnding::Lf => "\n",
        LineEnding::Crlf => "\r\n",
    };
    let mut blank_lines = 0;
    let mut reported_ending = false;
    for (row, line) in lines.iter().enumerate() {
        let trimmed = line.text.trim_end_matches([' ', '\t']);
        if !reported_ending && !line.ending.is_empty() && line.ending != ending {
            emit("format.line-ending", row, line.text.len(), None);
            reported_ending = true;
        }
        if trimmed.len() < line.text.len() && !in_literal(line.start + trimmed.len()) {
            emit("format.trailing-whitespace", row, trimmed.len(), None);
        }
        if trimmed.is_empty() && !in_literal(line.start) {
            blank_lines += 1;
            if blank_lines == usize::from(settings.max_blank_lines) + 1 {
                emit("format.blank-lines", row, 0, None);
            }
        } else {
            blank_lines = 0;
        }
        if trimmed.is_empty() || in_literal(line.start) {
            continue;
        }
        let prefix = leading_indent(line.text);
        if prefix.contains('\t') {
            emit(
                "format.indentation",
                row,
                0,
                Some("Use spaces for indentation.".into()),
            );
        } else if let Some(&indent) = expected.get(&row)
            && prefix.len() != indent
        {
            emit(
                "format.indentation",
                row,
                prefix.len(),
                Some(format!(
                    "Expected {indent} spaces of indentation, found {}.",
                    prefix.len()
                )),
            );
        }
    }
    let row = lines.len() - 1;
    if lines[row].ending.is_empty() {
        emit("format.final-newline", row, lines[row].text.len(), None);
    }
}

fn is_block(kind: &str) -> bool {
    matches!(
        kind,
        "block"
            | "statement_block"
            | "class_body"
            | "interface_body"
            | "annotation_type_body"
            | "constructor_body"
            | "enum_body"
            | "enum_class_body"
            | "module_body"
            | "object"
            | "lambda_literal"
            | "static_initializer"
            | "switch_block"
            | "switch_body"
            | "when_expression"
    )
}

fn collect_indentation(
    node: Node<'_>,
    root: Node<'_>,
    source: &str,
    lines: &[Line<'_>],
    width: usize,
    expected: &mut BTreeMap<usize, usize>,
) {
    let mut cursor = node.walk();
    let mut children = node.children(&mut cursor);
    if node == root {
        for child in children.filter(|child| child.is_named() && !child.is_error()) {
            expect_indent(child, 0, source, lines, expected);
        }
        return;
    }
    let Some(open) = children.find(|child| child.kind() == "{") else {
        return;
    };
    let anchor = if node.kind() == "when_expression" {
        node.start_byte()
    } else if node.kind() == "object"
        || node.kind() == "block" && node.parent().is_some_and(|parent| is_block(parent.kind()))
    {
        open.start_byte()
    } else {
        node.parent().unwrap_or(node).start_byte()
    };
    let row = line_index(lines, anchor);
    let base = expected.get(&row).copied().unwrap_or_else(|| {
        leading_indent(lines[row].text)
            .bytes()
            .map(|byte| if byte == b'\t' { width } else { 1 })
            .sum()
    });
    for child in children {
        if child.kind() == "}" {
            expect_indent(child, base, source, lines, expected);
        } else if child.is_named() {
            expect_indent(child, base + width, source, lines, expected);
        }
    }
}

fn expect_indent(
    node: Node<'_>,
    indent: usize,
    source: &str,
    lines: &[Line<'_>],
    expected: &mut BTreeMap<usize, usize>,
) {
    let offset = node.start_byte();
    let row = line_index(lines, offset);
    if source[lines[row].start..offset]
        .bytes()
        .all(|b| matches!(b, b' ' | b'\t'))
    {
        expected.entry(row).or_insert(indent);
    }
}

fn leading_indent(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}
