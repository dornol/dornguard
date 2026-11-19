use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use dornguard_core::{Diagnostic, EngineName, Outcome, fingerprint};
use tree_sitter::{Node, Parser, Tree};

use crate::{Engine, Request, display_path, fail};

const ANNOTATIONS: &str = "org.jspecify.annotations";

pub struct JSpecify;

impl Engine for JSpecify {
    async fn check(&self, request: Request<'_>) -> Outcome {
        let mut outcome = Outcome::default();
        let mut parser = Parser::new();
        if let Err(error) = parser.set_language(&tree_sitter_java::LANGUAGE.into()) {
            fail(&mut outcome, EngineName::Builtin, error.to_string());
            request.mark_incomplete(&mut outcome);
            return outcome;
        }
        let mut descriptors = BTreeMap::new();
        for file in request.files {
            match check_file(&mut parser, &mut descriptors, &request, file, &mut outcome) {
                Ok(()) => {
                    outcome.scanned_files.insert(file.clone());
                }
                Err(error) => fail(&mut outcome, EngineName::Builtin, error),
            }
        }
        request.mark_incomplete(&mut outcome);
        outcome
    }
}

#[derive(Default)]
struct Descriptor {
    package: String,
    marked: Option<bool>,
}

type Descriptors = BTreeMap<PathBuf, Result<Descriptor, String>>;

fn check_file(
    parser: &mut Parser,
    descriptors: &mut Descriptors,
    request: &Request<'_>,
    file: &Path,
    outcome: &mut Outcome,
) -> Result<(), String> {
    if matches!(
        file.file_name().and_then(|name| name.to_str()),
        Some("package-info.java" | "module-info.java")
    ) {
        descriptor(parser, descriptors, request.root, file)?;
        return Ok(());
    }
    let (source, tree) = parse(parser, request.root, file)?;
    let unit = tree.root_node();
    let names = Names::new(unit, &source);
    let directory = file.parent().expect("source paths are project-relative");
    let package = descriptor(
        parser,
        descriptors,
        request.root,
        &directory.join("package-info.java"),
    )?;
    let package_scope = (package.package == names.package)
        .then_some(package.marked)
        .flatten();
    let mut marked = package_scope.unwrap_or(false);
    if package_scope.is_none()
        && let Some(root) = source_root(directory, &names.package)
    {
        marked = descriptor(
            parser,
            descriptors,
            request.root,
            &root.join("module-info.java"),
        )?
        .marked
        .unwrap_or(false);
    }

    let mut pending = vec![(unit, marked)];
    while let Some((node, mut marked)) = pending.pop() {
        let is_type = matches!(
            node.kind(),
            "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration"
                | "annotation_type_declaration"
        );
        if is_type
            || matches!(
                node.kind(),
                "method_declaration"
                    | "constructor_declaration"
                    | "compact_constructor_declaration"
            )
        {
            marked = child(node, "modifiers")
                .and_then(|modifiers| names.marked(modifiers))
                .unwrap_or(marked);
        }
        if is_type
            && !marked
            && let Some(name) = node.child_by_field_name("name")
        {
            let position = name.start_position();
            let line_start = name.start_byte() - position.column;
            let column = source[line_start..name.start_byte()].encode_utf16().count();
            let subject = text(name, &source);
            for rule in request.rules {
                outcome.diagnostics.push(Diagnostic {
                    path: Some(display_path(file)),
                    module: None,
                    class_name: None,
                    line: Some(position.row as u32 + 1),
                    column: Some(column as u32 + 1),
                    end_line: Some(position.row as u32 + 1),
                    end_column: Some((column + subject.encode_utf16().count()) as u32 + 1),
                    subject: Some(subject.to_owned()),
                    rule_id: rule.id.clone(),
                    engine: EngineName::Builtin,
                    severity: rule.severity,
                    message: rule.description.clone(),
                    fix: rule.fix.clone(),
                    fingerprint: Some(fingerprint(&declaration_identity(node, &source))),
                });
            }
        }
        pending.extend(children(node).map(|child| (child, marked)));
    }
    Ok(())
}

fn declaration_identity(node: Node<'_>, source: &str) -> String {
    let mut names = Vec::new();
    let mut current = Some(node);
    while let Some(node) = current {
        if let Some(name) = node.child_by_field_name("name") {
            names.push(text(name, source));
        }
        current = node.parent();
    }
    names.reverse();
    names.join(".")
}

fn source_root<'a>(mut directory: &'a Path, package: &str) -> Option<&'a Path> {
    if package.is_empty() {
        return None;
    }
    for part in package.rsplit('.') {
        if directory.file_name()?.to_str()? != part {
            return None;
        }
        directory = directory.parent()?;
    }
    Some(directory)
}

fn descriptor<'a>(
    parser: &mut Parser,
    descriptors: &'a mut Descriptors,
    root: &Path,
    file: &Path,
) -> Result<&'a Descriptor, String> {
    descriptors
        .entry(file.to_path_buf())
        .or_insert_with(|| {
            match fs::symlink_metadata(root.join(file)) {
                Ok(metadata) if metadata.is_file() => {}
                Ok(_) => return Ok(Descriptor::default()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(Descriptor::default());
                }
                Err(error) => return Err(format!("Could not access {}: {error}", file.display())),
            }
            let (source, tree) = parse(parser, root, file)?;
            let unit = tree.root_node();
            let names = Names::new(unit, &source);
            let kind = if file.ends_with("module-info.java") {
                "module_declaration"
            } else {
                "package_declaration"
            };
            Ok(Descriptor {
                marked: child(unit, kind).and_then(|node| names.marked(node)),
                package: names.package,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn parse(parser: &mut Parser, root: &Path, file: &Path) -> Result<(String, Tree), String> {
    let source = fs::read_to_string(root.join(file))
        .map_err(|error| format!("Could not read {}: {error}", file.display()))?;
    let tree = parser
        .parse(&source, None)
        .filter(|tree| !tree.root_node().has_error())
        .ok_or_else(|| format!("Could not parse Java declarations in {}", file.display()))?;
    Ok((source, tree))
}

struct Names<'a> {
    source: &'a str,
    package: String,
    imports: Vec<String>,
    wildcard: bool,
}

impl<'a> Names<'a> {
    fn new(unit: Node<'_>, source: &'a str) -> Self {
        let package = package_name(unit, source);
        let mut imports = Vec::new();
        let mut wildcard = false;
        for import in children(unit).filter(|node| node.kind() == "import_declaration") {
            if let Some(name) = declared_name(import) {
                let name = qualified_name(name, source);
                if child(import, "asterisk").is_some() {
                    wildcard |= name == ANNOTATIONS;
                } else {
                    imports.push(name);
                }
            }
        }
        Self {
            source,
            package,
            imports,
            wildcard,
        }
    }

    fn marked(&self, declaration: Node<'_>) -> Option<bool> {
        let mut marked = false;
        let mut unmarked = false;
        for annotation in children(declaration)
            .filter(|node| matches!(node.kind(), "annotation" | "marker_annotation"))
        {
            if let Some(name) = annotation.child_by_field_name("name")
                && let Some(jspecify_name) = self.annotation(name)
            {
                marked |= jspecify_name == "NullMarked";
                unmarked |= jspecify_name == "NullUnmarked";
            }
        }
        (marked != unmarked).then_some(marked)
    }

    fn annotation(&self, name: Node<'_>) -> Option<&'a str> {
        if name.kind() == "scoped_identifier" {
            let scope = name.child_by_field_name("scope")?;
            let simple = name.child_by_field_name("name")?;
            return (qualified_name(scope, self.source) == ANNOTATIONS)
                .then(|| text(simple, self.source));
        }
        let simple = text(name, self.source);
        if !matches!(simple, "NullMarked" | "NullUnmarked") {
            return None;
        }
        if let Some((package, _)) = self
            .imports
            .iter()
            .filter_map(|import| import.rsplit_once('.'))
            .find(|(_, imported)| *imported == simple)
        {
            return (package == ANNOTATIONS).then_some(simple);
        }
        (self.package == ANNOTATIONS || self.wildcard).then_some(simple)
    }
}

fn package_name(unit: Node<'_>, source: &str) -> String {
    child(unit, "package_declaration")
        .and_then(declared_name)
        .map_or_else(String::new, |name| qualified_name(name, source))
}

pub(super) fn children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    (0..node.named_child_count()).filter_map(move |index| node.named_child(index))
}

fn child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    children(node).find(|child| child.kind() == kind)
}

fn declared_name(node: Node<'_>) -> Option<Node<'_>> {
    children(node).find(|node| matches!(node.kind(), "identifier" | "scoped_identifier"))
}

fn qualified_name(node: Node<'_>, source: &str) -> String {
    let mut parts = Vec::new();
    let mut current = node;
    while let (Some(scope), Some(name)) = (
        current.child_by_field_name("scope"),
        current.child_by_field_name("name"),
    ) {
        parts.push(text(name, source));
        current = scope;
    }
    parts.push(text(current, source));
    parts.reverse();
    parts.join(".")
}

pub(super) fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
