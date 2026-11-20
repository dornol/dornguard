use std::path::Path;

use dornguard_core::SourceLanguage;
use serde::Deserialize;

use crate::PathGlob;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FormatConfig {
    pub java_indent: u8,
    pub kotlin_indent: u8,
    pub javascript_indent: u8,
    pub line_ending: LineEnding,
    pub max_blank_lines: u8,
    pub indent_style: IndentStyle,
    pub control_spacing: ControlSpacing,
    include_minified: bool,
    exclude: Vec<PathGlob>,
    pub(crate) overrides: Vec<FormatOverride>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FormatOverride {
    pub(crate) files: Vec<PathGlob>,
    pub(crate) indent_size: Option<u8>,
    pub(crate) indent_style: Option<IndentStyle>,
    pub(crate) control_spacing: Option<ControlSpacing>,
    pub(crate) line_ending: Option<LineEnding>,
    pub(crate) max_blank_lines: Option<u8>,
}

#[derive(Clone, Copy)]
pub struct FileFormat {
    pub indent_size: u8,
    pub indent_style: IndentStyle,
    pub control_spacing: ControlSpacing,
    pub line_ending: LineEnding,
    pub max_blank_lines: u8,
}

impl FormatConfig {
    pub fn excludes_file(&self, file: &Path) -> bool {
        self.exclude.iter().any(|glob| glob.0.is_match(file))
            || !self.include_minified
                && SourceLanguage::of(file) == Some(SourceLanguage::Javascript)
                && file
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".min"))
    }

    pub fn for_file(&self, file: &Path, language: SourceLanguage) -> FileFormat {
        let mut settings = FileFormat {
            indent_size: match language {
                SourceLanguage::Java => self.java_indent,
                SourceLanguage::Kotlin => self.kotlin_indent,
                _ => self.javascript_indent,
            },
            line_ending: self.line_ending,
            max_blank_lines: self.max_blank_lines,
            indent_style: self.indent_style,
            control_spacing: self.control_spacing,
        };
        for entry in &self.overrides {
            if entry.files.iter().any(|glob| glob.0.is_match(file)) {
                settings.indent_size = entry.indent_size.unwrap_or(settings.indent_size);
                settings.indent_style = entry.indent_style.unwrap_or(settings.indent_style);
                settings.control_spacing =
                    entry.control_spacing.unwrap_or(settings.control_spacing);
                settings.line_ending = entry.line_ending.unwrap_or(settings.line_ending);
                settings.max_blank_lines =
                    entry.max_blank_lines.unwrap_or(settings.max_blank_lines);
            }
        }
        settings
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndentStyle {
    #[default]
    Spaces,
    Tabs,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlSpacing {
    #[default]
    Space,
    None,
}

#[derive(Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartialParsing {
    #[default]
    Error,
    Warning,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            java_indent: 4,
            kotlin_indent: 4,
            javascript_indent: 2,
            line_ending: LineEnding::Lf,
            max_blank_lines: 1,
            indent_style: IndentStyle::Spaces,
            control_spacing: ControlSpacing::Space,
            include_minified: false,
            exclude: Vec::new(),
            overrides: Vec::new(),
        }
    }
}
