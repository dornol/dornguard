use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::Path;

use dornguard_config::{ControlSpacing, FileFormat, FormatConfig, IndentStyle, LineEnding, Rule};
use dornguard_core::{Diagnostic, EngineName, Outcome, SourceEdit, SourceLanguage, fingerprint};
use tree_sitter::{Node, Parser};

use crate::{Request, display_path, engine_failure, fail};

pub fn check(request: Request<'_>, settings: &FormatConfig) -> Outcome {
    let mut outcome = Outcome::default();
    let mut parser = Parser::new();
    let rules: BTreeMap<&str, &Rule> = request
        .rules
        .iter()
        .map(|rule| (rule.id.as_str(), *rule))
        .collect();
    let line_rules = rules
        .iter()
        .filter(|(id, _)| matches!(**id, "format.line-ending" | "format.final-newline"))
        .map(|(id, rule)| (*id, *rule))
        .collect::<BTreeMap<_, _>>();
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
        let root = tree.root_node();
        let partial = root.has_error() && line_rules.len() < rules.len();
        if partial {
            let mut failure = engine_failure(
                EngineName::Builtin,
                "Source syntax prevented a complete formatting check",
            );
            failure.path = Some(display_path(file));
            failure.error_type = Some("PartialParsing".to_owned());
            failure.file_count = Some(1);
            failure.set_rules(
                rules
                    .keys()
                    .copied()
                    .filter(|id| !line_rules.contains_key(id)),
            );
            outcome.failures.push(failure);
        } else {
            outcome.scanned_files.insert(file.clone());
        }
        check_layout(
            file,
            &source,
            root,
            settings.for_file(file, language),
            if partial { &line_rules } else { &rules },
            &mut outcome,
        );
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

/// Builds layout diagnostics for one file.
struct Emitter<'a> {
    file: &'a Path,
    source: &'a str,
    root: Node<'a>,
    lines: &'a [Line<'a>],
    settings: FileFormat,
    rules: &'a BTreeMap<&'a str, &'a Rule>,
    ending: &'static str,
    source_sha256: OnceCell<String>,
    outcome: &'a mut Outcome,
}

impl Emitter<'_> {
    fn emit(&mut self, id: &str, row: usize, column: usize, detail: Option<String>) {
        let Some(rule) = self.rules.get(id) else {
            return;
        };
        let settings = self.settings;
        let line = &self.lines[row];
        let edit = if self.root.has_error() {
            None
        } else {
            let replacement = match id {
                "format.trailing-whitespace" => Some(""),
                "format.final-newline" => Some(self.ending),
                _ => None,
            };
            replacement.map(|replacement| SourceEdit {
                start_byte: line.start + column,
                end_byte: line.start + line.text.len(),
                replacement: replacement.to_owned(),
                source_sha256: self
                    .source_sha256
                    .get_or_init(|| fingerprint(self.source))
                    .clone(),
            })
        };
        self.outcome.diagnostics.push(Diagnostic {
            path: Some(display_path(self.file)),
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
            confidence: rule.confidence,
            message: detail.unwrap_or_else(|| rule.description.clone()),
            fix: match id {
                "format.control-spacing"
                    if matches!(settings.control_spacing, ControlSpacing::None) =>
                {
                    "Put the opening parenthesis directly after the keyword.".to_owned()
                }
                "format.indentation" if matches!(settings.indent_style, IndentStyle::Tabs) => {
                    "Indent each block level by one tab.".to_owned()
                }
                _ => rule.fix.clone(),
            },
            edit,
            legacy_fingerprints: Vec::new(),
            group: None,
            fingerprint: Some(fingerprint(line.text.trim())),
        });
    }
}

/// Walk the syntax tree. Returns the expected indentation by row and the string literal ranges,
/// and reports control keywords whose spacing differs from the setting.
fn walk_syntax(
    emitter: &mut Emitter<'_>,
    width: usize,
) -> (BTreeMap<usize, usize>, Vec<Range<usize>>) {
    let (root, source, lines, settings) = (
        emitter.root,
        emitter.source,
        emitter.lines,
        emitter.settings,
    );
    let control_spacing = match settings.control_spacing {
        ControlSpacing::Space => " ",
        ControlSpacing::None => "",
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
            collect_indentation(node, root, source, lines, width, &mut expected);
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
            if rest.starts_with('(') && &tail[..tail.len() - rest.len()] != control_spacing {
                let row = line_index(lines, node.start_byte());
                emitter.emit(
                    "format.control-spacing",
                    row,
                    node.start_byte() - lines[row].start,
                    matches!(settings.control_spacing, ControlSpacing::None).then(|| {
                        "Unexpected whitespace before the opening parenthesis.".to_owned()
                    }),
                );
            }
        }
        let previous = pending.len();
        pending.extend(node.children(&mut cursor));
        pending[previous..].reverse();
    }
    (expected, literals)
}

/// Check line endings, trailing whitespace, blank lines, indentation and the final newline.
fn check_lines(
    emitter: &mut Emitter<'_>,
    expected: &BTreeMap<usize, usize>,
    literals: &[Range<usize>],
    indent: (u8, &str),
) {
    let (indent_byte, indent_unit) = indent;
    let (lines, settings, ending) = (emitter.lines, emitter.settings, emitter.ending);
    let in_literal = |offset: usize| {
        let index = literals.partition_point(|range: &Range<usize>| range.end <= offset);
        literals
            .get(index)
            .is_some_and(|range| range.contains(&offset))
    };
    let mut blank_lines = 0;
    let mut reported_ending = false;
    for (row, line) in lines.iter().enumerate() {
        let trimmed = line.text.trim_end_matches([' ', '\t']);
        if !reported_ending && !line.ending.is_empty() && line.ending != ending {
            emitter.emit("format.line-ending", row, line.text.len(), None);
            reported_ending = true;
        }
        if trimmed.len() < line.text.len() && !in_literal(line.start + trimmed.len()) {
            emitter.emit("format.trailing-whitespace", row, trimmed.len(), None);
        }
        if trimmed.is_empty() && !in_literal(line.start) {
            blank_lines += 1;
            if blank_lines == usize::from(settings.max_blank_lines) + 1 {
                emitter.emit("format.blank-lines", row, 0, None);
            }
        } else {
            blank_lines = 0;
        }
        if trimmed.is_empty() || in_literal(line.start) {
            continue;
        }
        let prefix = leading_indent(line.text);
        if prefix.bytes().any(|byte| byte != indent_byte) {
            emitter.emit(
                "format.indentation",
                row,
                0,
                Some(format!("Use {indent_unit} for indentation.")),
            );
        } else if let Some(&indent) = expected.get(&row)
            && prefix.len() != indent
        {
            emitter.emit(
                "format.indentation",
                row,
                prefix.len(),
                Some(format!(
                    "Expected {indent} {indent_unit} of indentation, found {}.",
                    prefix.len()
                )),
            );
        }
    }
    let row = lines.len() - 1;
    if lines[row].ending.is_empty() {
        emitter.emit("format.final-newline", row, lines[row].text.len(), None);
    }
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
    let (indent_byte, width, indent_unit) = match settings.indent_style {
        IndentStyle::Spaces => (b' ', usize::from(settings.indent_size), "spaces"),
        IndentStyle::Tabs => (b'\t', 1, "tabs"),
    };
    let mut emitter = Emitter {
        file,
        source,
        root,
        lines: &lines,
        settings,
        rules,
        ending: match settings.line_ending {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        },
        source_sha256: OnceCell::new(),
        outcome,
    };
    let (expected, literals) = walk_syntax(&mut emitter, width);
    check_lines(
        &mut emitter,
        &expected,
        &literals,
        (indent_byte, indent_unit),
    );
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
