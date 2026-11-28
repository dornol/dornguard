//! `kotlin.static-member-injection`: Spring's `@Value`, or `@Autowired` on a function,
//! inside a companion object or an `object` declaration. Their properties compile to static
//! fields and Spring does not inject static members, so the value stays unset.
//!
//! Semgrep's Kotlin support cannot restrict a match to a companion object, so the rule reads
//! the syntax tree. `@Autowired` properties are left to `kotlin.field-injection`.

use dornguard_config::Rule;
use dornguard_core::{Diagnostic, EngineName, Outcome};
use tree_sitter::{Node, Parser};

use crate::source::{Source, fragment_fingerprint};
use crate::{Request, display_path};

const PACKAGE: &str = "org.springframework.beans.factory.annotation";

pub(super) fn check(request: Request<'_>, rule: &Rule) -> Outcome {
    let mut outcome = Outcome::default();
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .is_err()
    {
        return outcome;
    }
    for file in request.files {
        let Ok(text) = std::fs::read_to_string(request.root.join(file)) else {
            continue;
        };
        let Some(tree) = parser.parse(&text, None) else {
            continue;
        };
        let source = Source::new(text);
        let imports = Imports::of(tree.root_node(), source.text());
        let mut found = Vec::new();
        static_injections(tree.root_node(), source.text(), &imports, &mut found);
        for (annotation, member) in found {
            let (Some((line, column)), end) = (
                source.position(annotation.start_byte()),
                source.position(annotation.end_byte()),
            ) else {
                continue;
            };
            let fragment = &source.text()[member.byte_range()];
            outcome.diagnostics.push(Diagnostic {
                path: Some(display_path(file)),
                module: None,
                class_name: None,
                line: Some(line),
                column: Some(column),
                end_line: end.map(|(line, _)| line),
                end_column: end.map(|(_, column)| column),
                subject: None,
                rule_id: rule.id.clone(),
                engine: EngineName::Builtin,
                severity: rule.severity,
                confidence: rule.confidence,
                message: rule.description.clone(),
                fix: rule.fix.clone(),
                edit: None,
                legacy_fingerprints: Vec::new(),
                group: None,
                fingerprint: fragment_fingerprint(fragment),
            });
        }
    }
    outcome
}

/// How the file can name Spring's `@Value` and `@Autowired`.
struct Imports {
    value: bool,
    autowired: bool,
}

impl Imports {
    fn of(root: Node<'_>, text: &str) -> Self {
        let mut imports = Imports {
            value: false,
            autowired: false,
        };
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            if child.kind() != "import" {
                continue;
            }
            let path = child
                .named_child(0)
                .map(|name| compact(&text[name.byte_range()]))
                .unwrap_or_default();
            let wildcard = text[child.byte_range()].trim_end().ends_with(".*");
            // An alias such as `import ...Value as Injected` renames the annotation.
            let aliased = child.named_child_count() > 1;
            if wildcard && path == PACKAGE {
                imports.value = true;
                imports.autowired = true;
            } else if !aliased && path == format!("{PACKAGE}.Value") {
                imports.value = true;
            } else if !aliased && path == format!("{PACKAGE}.Autowired") {
                imports.autowired = true;
            }
        }
        imports
    }

    fn names(&self, annotation: &str, simple: &str) -> bool {
        let imported = match simple {
            "Value" => self.value,
            _ => self.autowired,
        };
        annotation == format!("{PACKAGE}.{simple}") || (imported && annotation == simple)
    }
}

fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Collect (annotation, member) pairs for members of companion objects and object
/// declarations. Members of a class nested in such an object are instance members.
fn static_injections<'t>(
    node: Node<'t>,
    text: &str,
    imports: &Imports,
    found: &mut Vec<(Node<'t>, Node<'t>)>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(child.kind(), "companion_object" | "object_declaration") {
            let mut members = child.walk();
            for body in child
                .named_children(&mut members)
                .filter(|body| body.kind() == "class_body")
            {
                let mut items = body.walk();
                for member in body.named_children(&mut items) {
                    let property = match member.kind() {
                        "property_declaration" => true,
                        "function_declaration" => false,
                        _ => continue,
                    };
                    if let Some(annotation) = injection(member, property, text, imports) {
                        found.push((annotation, member));
                    }
                }
            }
        }
        static_injections(child, text, imports, found);
    }
}

/// The `@Value` annotation of a property or function, or the `@Autowired` annotation of a
/// function, among the member's own modifiers.
fn injection<'t>(
    member: Node<'t>,
    property: bool,
    text: &str,
    imports: &Imports,
) -> Option<Node<'t>> {
    let mut cursor = member.walk();
    let modifiers = member
        .named_children(&mut cursor)
        .find(|child| child.kind() == "modifiers")?;
    let mut annotations = modifiers.walk();
    modifiers
        .named_children(&mut annotations)
        .filter(|annotation| annotation.kind() == "annotation")
        .find(|annotation| {
            let Some(name) = annotation_name(*annotation, text) else {
                return false;
            };
            imports.names(&name, "Value") || (!property && imports.names(&name, "Autowired"))
        })
}

/// The annotation's type as written, such as `Value` or a fully qualified name.
fn annotation_name(annotation: Node<'_>, text: &str) -> Option<String> {
    let mut cursor = annotation.walk();
    let target = annotation
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "user_type" | "constructor_invocation"))?;
    let user_type = if target.kind() == "user_type" {
        target
    } else {
        let mut inner = target.walk();
        target
            .named_children(&mut inner)
            .find(|child| child.kind() == "user_type")?
    };
    Some(compact(&text[user_type.byte_range()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<usize> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        let imports = Imports::of(tree.root_node(), text);
        let mut found = Vec::new();
        static_injections(tree.root_node(), text, &imports, &mut found);
        found
            .iter()
            .map(|(annotation, _)| annotation.start_position().row + 1)
            .collect()
    }

    #[test]
    fn companion_and_object_members_are_reported() {
        let text = r#"import org.springframework.beans.factory.annotation.Autowired
import org.springframework.beans.factory.annotation.Value

class Settings {
    @Value("\${app.name}")
    lateinit var name: String

    companion object {
        @Value("\${app.limit}")
        var limit: Int = 0

        @JvmStatic
        @field:Value("\${app.url}")
        lateinit var url: String

        @Autowired
        lateinit var repository: Repository

        @Autowired
        fun init(repository: Repository) {}

        class Nested {
            @Value("\${app.nested}")
            lateinit var nested: String
        }
    }
}

object Holder {
    @Value("\${app.key}")
    lateinit var key: String
}
"#;
        assert_eq!(lines(text), [9, 13, 19, 30]);
    }

    #[test]
    fn other_annotations_named_value_are_not_reported() {
        let text = r#"import com.example.Value

object Holder {
    @Value("x")
    lateinit var key: String
}
"#;
        assert!(lines(text).is_empty());
        let qualified = r#"object Holder {
    @org.springframework.beans.factory.annotation.Value("x")
    lateinit var key: String
}
"#;
        assert_eq!(lines(qualified), [2]);
        let wildcard = r#"import org.springframework.beans.factory.annotation.*

object Holder {
    @Value("x")
    lateinit var key: String
}
"#;
        assert_eq!(lines(wildcard), [4]);
    }
}
