use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use dornguard_config::Ignore;
use dornguard_core::{EngineName, Outcome, SourceLanguage};

use html5ever::tokenizer::states::RawKind::{Rawtext, Rcdata, ScriptData};
use html5ever::tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer};

use crate::baseline::Baseline;

pub fn apply(root: &Path, config: &[Ignore], outcome: &mut Outcome) {
    outcome.diagnostics.sort();
    outcome.diagnostics.dedup();
    let mut files = BTreeMap::new();
    outcome.diagnostics.retain(|diagnostic| {
        let Some(path) = diagnostic.path.as_deref().map(Path::new) else {
            return true;
        };
        if config
            .iter()
            .any(|entry| entry.matches_path(path, &diagnostic.rule_id))
        {
            outcome.suppressed_count += 1;
            return false;
        }
        let Some(line) = diagnostic.line else {
            return true;
        };
        let language = SourceLanguage::of(path);
        if !matches!(
            diagnostic.engine,
            EngineName::Semgrep | EngineName::Pmd | EngineName::Builtin
        ) || !matches!(
            language,
            Some(SourceLanguage::Java | SourceLanguage::Html | SourceLanguage::Template)
        ) {
            return true;
        }
        let directives = files
            .entry(path.to_path_buf())
            .or_insert_with(|| read_directives(&root.join(path)));
        let line = line as usize;
        if directives.get(&line) != Some(&diagnostic.rule_id) {
            return true;
        }
        directives.remove(&line);
        outcome.suppressed_count += 1;
        false
    });
    Baseline::from_config(config).apply(outcome);
}

fn read_directives(path: &Path) -> BTreeMap<usize, String> {
    let Ok(source) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let mut directives = BTreeMap::new();
    if !source.contains("dornguard-ignore") {
        return directives;
    }
    if matches!(
        SourceLanguage::of(path),
        Some(SourceLanguage::Html | SourceLanguage::Template)
    ) {
        return html_directives(&source);
    }
    let mut context = JavaContext::Code;
    for (index, line) in source.lines().enumerate() {
        if matches!(context, JavaContext::Code)
            && let Some(rule) = line
                .trim_start()
                .strip_prefix("//")
                .and_then(directive_rule)
        {
            directives.insert(index + 2, rule.to_owned());
        }
        context = context.after(line.as_bytes());
    }
    directives
}

fn directive_rule(comment: &str) -> Option<&str> {
    let comment = comment.trim_start();
    let (directive, rest) = comment.split_once(char::is_whitespace)?;
    if directive != "dornguard-ignore" {
        return None;
    }
    let (rule, rest) = rest.trim_start().split_once(char::is_whitespace)?;
    let (separator, reason) = rest.trim_start().split_once(char::is_whitespace)?;
    (matches!(separator, "--" | "—") && !reason.trim().is_empty()).then_some(rule)
}

fn html_directives(source: &str) -> BTreeMap<usize, String> {
    struct DirectiveSink<'a> {
        lines: Vec<&'a str>,
        directives: RefCell<BTreeMap<usize, String>>,
    }

    impl TokenSink for DirectiveSink<'_> {
        type Handle = ();

        fn process_token(&self, token: Token, line: u64) -> TokenSinkResult<()> {
            match token {
                Token::CommentToken(comment) => {
                    let line = line as usize;
                    if let Some(raw) = self.lines.get(line.saturating_sub(1))
                        && raw
                            .trim()
                            .strip_prefix("<!--")
                            .and_then(|text| text.strip_suffix("-->"))
                            == Some(comment.as_ref())
                        && let Some(rule) = directive_rule(&comment)
                    {
                        self.directives
                            .borrow_mut()
                            .insert(line + 1, rule.to_owned());
                    }
                }
                Token::TagToken(tag) if tag.kind == TagKind::StartTag => {
                    return match tag.name.as_ref() {
                        "script" => TokenSinkResult::RawData(ScriptData),
                        "style" | "iframe" | "xmp" | "noembed" | "noframes" => {
                            TokenSinkResult::RawData(Rawtext)
                        }
                        "textarea" | "title" => TokenSinkResult::RawData(Rcdata),
                        "plaintext" => TokenSinkResult::Plaintext,
                        _ => TokenSinkResult::Continue,
                    };
                }
                _ => {}
            }
            TokenSinkResult::Continue
        }
    }

    let tokenizer = Tokenizer::new(
        DirectiveSink {
            lines: source.lines().collect(),
            directives: RefCell::default(),
        },
        Default::default(),
    );
    let input = BufferQueue::default();
    input.push_back(source.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.directives.into_inner()
}

#[derive(Clone, Copy)]
enum JavaContext {
    Code,
    BlockComment,
    TextBlock,
    Quoted(u8),
}

impl JavaContext {
    fn after(mut self, mut line: &[u8]) -> Self {
        while !line.is_empty() {
            (self, line) = match (self, line) {
                (Self::Code, [b'/', b'/', ..]) => return Self::Code,
                (Self::Code, [b'/', b'*', rest @ ..]) => (Self::BlockComment, rest),
                (Self::Code, [b'"', b'"', b'"', rest @ ..]) => (Self::TextBlock, rest),
                (Self::Code, [quote @ (b'"' | b'\''), rest @ ..]) => (Self::Quoted(*quote), rest),
                (Self::BlockComment, [b'*', b'/', rest @ ..]) => (Self::Code, rest),
                (Self::TextBlock, [b'"', b'"', b'"', rest @ ..]) => (Self::Code, rest),
                (Self::TextBlock | Self::Quoted(_), [b'\\', _, rest @ ..]) => (self, rest),
                (Self::Quoted(quote), [byte, rest @ ..]) if *byte == quote => (Self::Code, rest),
                (_, [_, rest @ ..]) => (self, rest),
                (_, []) => break,
            };
        }
        match self {
            Self::Quoted(_) => Self::Code,
            _ => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_standalone_java_comments_with_a_reason_produce_directives() {
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join("Example.java");
        fs::write(
            &path,
            r#"/*
 // dornguard-ignore java.stdout-usage -- Block comment
*/
String text = """
 // dornguard-ignore java.stdout-usage -- Text block
""";
String escaped = "\"/*";
print(); // dornguard-ignore java.stdout-usage -- Trailing comment
// dornguard-ignore java.stdout-usage --
// dornguard-ignore java.stdout-usage — Actual directive -- with detail
System.out.println(result);
"#,
        )
        .unwrap();
        assert_eq!(
            read_directives(&path),
            BTreeMap::from([(11, "java.stdout-usage".to_owned())])
        );
        assert_eq!(
            directive_rule(" dornguard-ignore java.stdout-usage --   "),
            None
        );
    }
}
