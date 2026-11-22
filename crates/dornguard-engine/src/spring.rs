use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use dornguard_config::{
    ASYNC_SELF_RULE, CACHE_SELF_RULE, JPA_RESPONSE_RULE, RETRY_SELF_RULE, Rule,
    TRANSACTIONAL_SELF_RULE,
};
use dornguard_core::{Diagnostic, EngineName, Outcome, SourceLanguage};
use ignore::WalkBuilder;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use tree_sitter::{Node, Parser};

use crate::jspecify::{children, package_name, qualified_name, text};
use crate::source::{Source, fragment_fingerprint};
use crate::{Request, diagnostic_message, display_path, fail};

const TRANSACTIONAL: &str = "org.springframework.transaction.annotation.Transactional";
const ASYNC: &str = "org.springframework.scheduling.annotation.Async";
const CACHING: [&str; 4] = [
    "org.springframework.cache.annotation.Cacheable",
    "org.springframework.cache.annotation.CacheEvict",
    "org.springframework.cache.annotation.CachePut",
    "org.springframework.cache.annotation.Caching",
];
const RETRY: &str = "org.springframework.retry.annotation.Retryable";
const WEB: &str = "org.springframework.web.bind.annotation";

pub(super) fn check(request: Request<'_>, required: bool) -> Outcome {
    let mut outcome = Outcome::default();
    if let Err(error) = scan(request, required, &mut outcome) {
        fail(&mut outcome, EngineName::Builtin, format!("{error:#}"));
    }
    request.mark_incomplete(&mut outcome);
    outcome
}

fn scan(request: Request<'_>, required: bool, outcome: &mut Outcome) -> Result<()> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .into_diagnostic()?;
    let entities = if request
        .rules
        .iter()
        .any(|rule| rule.id == JPA_RESPONSE_RULE)
    {
        entity_names(request.root, &mut parser, required)?
    } else {
        BTreeMap::new()
    };
    for file in request.files {
        let source = match std::fs::read_to_string(request.root.join(file)) {
            Ok(source) => Source::new(source),
            Err(error) => {
                fail(
                    outcome,
                    EngineName::Builtin,
                    format!("Could not read {}: {error}", file.display()),
                );
                continue;
            }
        };
        let Some(tree) = parser.parse(source.text(), None) else {
            fail(
                outcome,
                EngineName::Builtin,
                format!("Could not parse {}", file.display()),
            );
            continue;
        };
        let unit = tree.root_node();
        let imports = Imports::new(unit, source.text());
        for class in types(unit) {
            if class.has_error() {
                // Avoid suggesting changes when the containing declaration cannot be parsed.
                continue;
            }
            let Some(body) = class.child_by_field_name("body") else {
                continue;
            };
            let methods = children(body)
                .filter(|node| node.kind() == "method_declaration")
                .collect::<Vec<_>>();
            for rule in request.rules {
                match rule.id.as_str() {
                    TRANSACTIONAL_SELF_RULE
                    | ASYNC_SELF_RULE
                    | CACHE_SELF_RULE
                    | RETRY_SELF_RULE => proxy_calls(
                        class,
                        &methods,
                        &imports,
                        |node, name| {
                            emit(outcome, file, &source, rule, node, name);
                        },
                        match rule.id.as_str() {
                            TRANSACTIONAL_SELF_RULE => &[TRANSACTIONAL],
                            ASYNC_SELF_RULE => &[ASYNC],
                            CACHE_SELF_RULE => &CACHING,
                            _ => &[RETRY],
                        },
                    ),
                    JPA_RESPONSE_RULE => {
                        entity_responses(class, &methods, &imports, &entities, |node, name| {
                            emit(outcome, file, &source, rule, node, name);
                        })
                    }
                    _ => unreachable!("unexpected Spring rule"),
                }
            }
        }
        if unit.has_error() {
            fail(
                outcome,
                EngineName::Builtin,
                format!("Could not fully parse {}", file.display()),
            );
        } else {
            outcome.scanned_files.insert(file.clone());
        }
    }
    Ok(())
}

fn proxy_calls(
    class: Node<'_>,
    methods: &[Node<'_>],
    imports: &Imports<'_>,
    mut found: impl FnMut(Node<'_>, String),
    annotations: &[&str],
) {
    let advised = |method: Node<'_>| {
        annotations
            .iter()
            .any(|annotation| imports.annotated(method, annotation))
    };
    if !methods.iter().any(|method| advised(*method)) {
        return;
    }
    let inherited = class.child_by_field_name("superclass").is_some()
        || class.child_by_field_name("interfaces").is_some();
    for caller in methods {
        if has_modifier(*caller, "static") {
            continue;
        }
        let Some(body) = caller.child_by_field_name("body") else {
            continue;
        };
        let mut pending = vec![body];
        while let Some(node) = pending.pop() {
            if matches!(
                node.kind(),
                "class_body" | "interface_body" | "enum_body" | "annotation_type_body"
            ) {
                continue;
            }
            pending.extend(children(node));
            if node.kind() != "method_invocation"
                || node
                    .child_by_field_name("object")
                    .is_some_and(|receiver| receiver.kind() != "this")
            {
                continue;
            }
            let Some(name) = node
                .child_by_field_name("name")
                .map(|name| text(name, imports.source))
            else {
                continue;
            };
            let Some(args) = node.child_by_field_name("arguments") else {
                continue;
            };
            let arity = children(args).filter(|node| !node.is_extra()).count();
            if arity > 0 && (inherited || matches!(name, "equals" | "wait")) {
                continue;
            }
            let candidates = methods.iter().filter(|method| {
                method
                    .child_by_field_name("name")
                    .is_some_and(|candidate| text(candidate, imports.source) == name)
            });
            let mut target = None;
            for method in candidates {
                let Some(parameters) = method.child_by_field_name("parameters") else {
                    continue;
                };
                let count = children(parameters)
                    .filter(|param| !param.is_extra() && param.kind() != "receiver_parameter")
                    .count();
                if arity > 0 && children(parameters).any(|param| param.kind() == "spread_parameter")
                {
                    target = None;
                    break;
                }
                if count == arity && target.replace(*method).is_some() {
                    target = None;
                    break;
                }
            }
            if let Some(target) = target
                && target != *caller
                && !has_modifier(target, "static")
                && advised(target)
                && (annotations != [TRANSACTIONAL]
                    || !covered_by_transaction(*caller, target, node, imports))
            {
                found(node, name.to_owned());
            }
        }
    }
}

fn covered_by_transaction(
    caller: Node<'_>,
    target: Node<'_>,
    call: Node<'_>,
    imports: &Imports<'_>,
) -> bool {
    if !has_modifier(caller, "public") || has_modifier(caller, "final") {
        return false;
    }
    let mut ancestor = call.parent();
    while let Some(parent) = ancestor.filter(|parent| *parent != caller) {
        if matches!(parent.kind(), "lambda_expression" | "class_body")
            || matches!(
                parent.kind(),
                "try_statement" | "try_with_resources_statement"
            ) && children(parent).any(|child| child.kind() == "catch_clause")
        {
            return false;
        }
        ancestor = parent.parent();
    }
    transaction_contract(caller, imports)
        .zip(transaction_contract(target, imports))
        .is_some_and(|(outer, inner)| outer == inner)
}

fn transaction_contract<'a>(
    node: Node<'_>,
    imports: &Imports<'a>,
) -> Option<BTreeMap<&'a str, &'a str>> {
    let annotation = imports.annotation(node, TRANSACTIONAL)?;
    let mut settings = BTreeMap::new();
    if let Some(arguments) = annotation.child_by_field_name("arguments") {
        for argument in children(arguments).filter(|child| !child.is_extra()) {
            let (key, value) = if argument.kind() == "element_value_pair" {
                (
                    text(argument.child_by_field_name("key")?, imports.source),
                    argument.child_by_field_name("value")?,
                )
            } else {
                ("value", argument)
            };
            let value_text = text(value, imports.source);
            match key {
                "propagation" => {
                    let receiver = value.child_by_field_name("object")?;
                    let member = value.child_by_field_name("field")?;
                    if text(member, imports.source) != "REQUIRED"
                        || !imports.matches(
                            &qualified_name(receiver, imports.source),
                            "org.springframework.transaction.annotation.Propagation",
                        )
                    {
                        return None;
                    }
                }
                "readOnly" if value_text == "false" => {}
                "value" | "transactionManager" if value.kind() == "string_literal" => {
                    if value_text != "\"\"" {
                        settings.insert("transactionManager", value_text);
                    }
                }
                "readOnly"
                | "isolation"
                | "timeout"
                | "timeoutString"
                | "rollbackFor"
                | "rollbackForClassName"
                | "noRollbackFor"
                | "noRollbackForClassName"
                | "label" => {
                    settings.insert(key, value_text);
                }
                _ => return None,
            }
        }
    }
    Some(settings)
}

fn has_modifier(node: Node<'_>, name: &str) -> bool {
    let mut cursor = node.walk();
    if let Some(modifiers) = children(node).find(|child| child.kind() == "modifiers") {
        modifiers
            .children(&mut cursor)
            .any(|child| child.kind() == name)
    } else {
        false
    }
}

fn entity_names(
    root: &Path,
    parser: &mut Parser,
    required: bool,
) -> Result<BTreeMap<String, bool>> {
    let mut declarations = BTreeMap::new();
    let mut walker = WalkBuilder::new(root);
    walker
        .require_git(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .hidden(!required)
        .git_ignore(!required)
        .filter_entry(|entry| entry.file_name() != ".git");
    for entry in walker.build() {
        let entry = entry
            .into_diagnostic()
            .wrap_err("Could not enumerate Java entity declarations")?;
        if let Some(error) = entry.error() {
            bail!("Could not read project ignore rules: {error}");
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file())
            || SourceLanguage::of(entry.path()) != Some(SourceLanguage::Java)
        {
            continue;
        }
        let source = std::fs::read_to_string(entry.path())
            .into_diagnostic()
            .wrap_err_with(|| format!("Could not read {}", entry.path().display()))?;
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        let unit = tree.root_node();
        let imports = Imports::new(unit, &source);
        for node in children(unit).filter(|node| is_type_declaration(*node)) {
            if let Some(name) = node.child_by_field_name("name") {
                let entity = node.kind() == "class_declaration"
                    && !node.has_error()
                    && (imports.annotated(node, "jakarta.persistence.Entity")
                        || imports.annotated(node, "javax.persistence.Entity"));
                declarations
                    .entry(imports.local_name(text(name, &source)))
                    .and_modify(|known| *known &= entity)
                    .or_insert(entity);
            }
        }
    }
    Ok(declarations)
}

fn is_type_declaration(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "class_declaration"
            | "record_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "annotation_type_declaration"
    )
}

fn entity_responses(
    class: Node<'_>,
    methods: &[Node<'_>],
    imports: &Imports<'_>,
    entities: &BTreeMap<String, bool>,
    mut found: impl FnMut(Node<'_>, String),
) {
    let rest = imports.annotated(class, &format!("{WEB}.RestController"));
    let controller = rest || imports.annotated(class, "org.springframework.stereotype.Controller");
    if !controller {
        return;
    }
    let class_response = rest || imports.annotated(class, &format!("{WEB}.ResponseBody"));
    for method in methods {
        let Some(return_type) = method.child_by_field_name("type") else {
            continue;
        };
        let response_entity = return_type.kind() == "generic_type"
            && children(return_type).next().is_some_and(|name| {
                imports.matches(
                    &java_name(name, imports.source),
                    "org.springframework.http.ResponseEntity",
                )
            });
        if (!class_response
            && !response_entity
            && !imports.annotated(*method, &format!("{WEB}.ResponseBody")))
            || ![
                "RequestMapping",
                "GetMapping",
                "PostMapping",
                "PutMapping",
                "PatchMapping",
                "DeleteMapping",
            ]
            .iter()
            .any(|annotation| imports.annotated(*method, &format!("{WEB}.{annotation}")))
        {
            continue;
        }
        let shadowed = [class, *method]
            .into_iter()
            .filter_map(|node| node.child_by_field_name("type_parameters"))
            .flat_map(children)
            .filter_map(|param| children(param).find(|child| child.kind() == "type_identifier"))
            .chain(
                class
                    .child_by_field_name("body")
                    .into_iter()
                    .flat_map(children)
                    .filter(|node| is_type_declaration(*node))
                    .filter_map(|node| node.child_by_field_name("name")),
            )
            .map(|name| text(name, imports.source))
            .collect::<BTreeSet<_>>();
        response_entities(return_type, imports, entities, &shadowed, &mut found);
    }
}

fn response_entities(
    node: Node<'_>,
    imports: &Imports<'_>,
    entities: &BTreeMap<String, bool>,
    shadowed: &BTreeSet<&str>,
    found: &mut impl FnMut(Node<'_>, String),
) {
    match node.kind() {
        "type_identifier" | "scoped_type_identifier" => {
            let name = java_name(node, imports.source);
            if !shadowed.contains(name.as_str())
                && let Some(entity) = imports.resolve(&name, entities)
            {
                found(node, entity.to_owned());
            }
        }
        "array_type" => {
            if let Some(element) = node.child_by_field_name("element") {
                response_entities(element, imports, entities, shadowed, found);
            }
        }
        "wildcard" if !children(node).any(|child| child.kind() == "super") => {
            for child in children(node) {
                response_entities(child, imports, entities, shadowed, found);
            }
        }
        "generic_type" => {
            let mut parts = children(node);
            let Some(container) = parts.next() else {
                return;
            };
            let name = java_name(container, imports.source);
            if !shadowed.contains(name.as_str())
                && [
                    "org.springframework.http.ResponseEntity",
                    "org.springframework.data.domain.Page",
                    "org.springframework.data.domain.Slice",
                    "java.util.Collection",
                    "java.util.List",
                    "java.util.Set",
                    "java.util.Optional",
                    "java.lang.Iterable",
                    "java.util.Map",
                ]
                .iter()
                .any(|container| imports.matches(&name, container))
                && let Some(arguments) = parts.find(|node| node.kind() == "type_arguments")
            {
                for argument in children(arguments) {
                    response_entities(argument, imports, entities, shadowed, found);
                }
            }
        }
        _ => {}
    }
}

fn types(unit: Node<'_>) -> Vec<Node<'_>> {
    let mut result = Vec::new();
    let mut pending = vec![unit];
    while let Some(node) = pending.pop() {
        if matches!(node.kind(), "class_declaration" | "record_declaration") {
            result.push(node);
        }
        pending.extend(children(node));
    }
    result
}

struct Imports<'a> {
    source: &'a str,
    package: String,
    explicit: Vec<String>,
    wildcards: Vec<String>,
}

impl<'a> Imports<'a> {
    fn new(unit: Node<'_>, source: &'a str) -> Self {
        let mut imports = Self {
            source,
            package: package_name(unit, source),
            explicit: Vec::new(),
            wildcards: Vec::new(),
        };
        for import in children(unit).filter(|node| node.kind() == "import_declaration") {
            let mut cursor = import.walk();
            if import
                .children(&mut cursor)
                .any(|node| node.kind() == "static")
            {
                continue;
            }
            let Some(name) = children(import)
                .find(|node| matches!(node.kind(), "scoped_identifier" | "identifier"))
            else {
                continue;
            };
            let name = qualified_name(name, source);
            if children(import).any(|node| node.kind() == "asterisk") {
                imports.wildcards.push(name);
            } else {
                imports.explicit.push(name);
            }
        }
        imports
    }

    fn annotated(&self, node: Node<'_>, canonical: &str) -> bool {
        self.annotation(node, canonical).is_some()
    }

    fn annotation<'node>(&self, node: Node<'node>, canonical: &str) -> Option<Node<'node>> {
        let modifiers = children(node).find(|child| child.kind() == "modifiers")?;
        children(modifiers)
            .filter(|child| matches!(child.kind(), "marker_annotation" | "annotation"))
            .find(|annotation| {
                annotation
                    .child_by_field_name("name")
                    .is_some_and(|name| self.matches(&qualified_name(name, self.source), canonical))
            })
    }

    fn matches(&self, name: &str, canonical: &str) -> bool {
        if name.contains('.') {
            return name == canonical;
        }
        let Some((package, simple)) = canonical.rsplit_once('.') else {
            return name == canonical && self.package.is_empty();
        };
        if simple != name {
            return false;
        }
        if let Some(explicit) = self.explicit(name) {
            return explicit == canonical;
        }
        self.package == package
            || package == "java.lang"
            || self.wildcards.iter().any(|import| import == package)
    }

    fn explicit(&self, name: &str) -> Option<&str> {
        self.explicit
            .iter()
            .find(|import| {
                import
                    .rsplit_once('.')
                    .is_some_and(|(_, simple)| simple == name)
            })
            .map(String::as_str)
    }

    fn local_name(&self, name: &str) -> String {
        if self.package.is_empty() {
            name.to_owned()
        } else {
            format!("{}.{name}", self.package)
        }
    }

    fn resolve<'b>(&self, name: &str, entities: &'b BTreeMap<String, bool>) -> Option<&'b str> {
        let entity = |(name, is_entity): (&'b String, &bool)| is_entity.then_some(name.as_str());
        if name.contains('.') {
            return entities.get_key_value(name).and_then(entity);
        }
        if let Some(explicit) = self.explicit(name) {
            return entities.get_key_value(explicit).and_then(entity);
        }
        if let Some(local) = entities.get_key_value(&self.local_name(name)) {
            return entity(local);
        }
        let mut matches = self
            .wildcards
            .iter()
            .filter_map(|package| entities.get_key_value(&format!("{package}.{name}")));
        let candidate = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        entity(candidate)
    }
}

fn java_name(node: Node<'_>, source: &str) -> String {
    text(node, source).split_whitespace().collect()
}

fn emit(
    outcome: &mut Outcome,
    path: &Path,
    source: &Source,
    rule: &Rule,
    node: Node<'_>,
    subject: String,
) {
    let Some((line, column)) = source.position(node.start_byte()) else {
        return;
    };
    let end = source.position(node.end_byte());
    outcome.diagnostics.push(Diagnostic {
        path: Some(display_path(path)),
        module: None,
        class_name: None,
        line: Some(line),
        column: Some(column),
        end_line: end.map(|(line, _)| line),
        end_column: end.map(|(_, column)| column),
        message: diagnostic_message(&rule.description, Some(&subject)),
        subject: Some(subject),
        rule_id: rule.id.clone(),
        engine: EngineName::Builtin,
        severity: rule.severity,
        confidence: rule.confidence,
        fix: rule.fix.clone(),
        fingerprint: fragment_fingerprint(text(node, source.text())),
        edit: None,
        legacy_fingerprints: Vec::new(),
        group: None,
    });
}
