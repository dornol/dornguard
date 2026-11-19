use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dornguard_core::fingerprint;

#[derive(Default)]
pub struct Sources(BTreeMap<PathBuf, Option<Source>>);

pub struct Source {
    text: String,
    lines: Vec<usize>,
}

impl Sources {
    pub fn offsets(
        &mut self,
        root: &Path,
        path: &Path,
        start: usize,
        end: usize,
    ) -> Option<String> {
        let source = self.get(root, path)?;
        fragment_fingerprint(source.fragment(start, end)?)
    }

    pub fn span(
        &mut self,
        root: &Path,
        path: &Path,
        start: (u32, u32),
        end: (u32, u32),
    ) -> Option<String> {
        let source = self.get(root, path)?;
        let start = source.offset(start)?;
        let end = source.offset(end)?;
        fragment_fingerprint(source.fragment(start, end)?)
    }

    pub fn get(&mut self, root: &Path, path: &Path) -> Option<&Source> {
        self.0
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                std::fs::read_to_string(root.join(path))
                    .ok()
                    .map(Source::new)
            })
            .as_ref()
    }
}

impl Source {
    pub(super) fn new(text: String) -> Self {
        let mut lines = vec![0];
        lines.extend(text.match_indices('\n').map(|(offset, _)| offset + 1));
        Self { text, lines }
    }

    pub fn fragment(&self, start: usize, end: usize) -> Option<&str> {
        self.text.get(start..end)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn pmd_fingerprints(&self, start: usize, end: usize) -> (Option<String>, Vec<String>) {
        let Some(fragment) = self.fragment(start, end) else {
            return (None, Vec::new());
        };
        let current = fragment_fingerprint(fragment);
        let following = self.text[end..]
            .split(['\r', '\n'])
            .next()
            .unwrap_or_default();

        // PMD reports an exclusive end column. The legacy implementation
        // included the next character, which may have been adjacent punctuation.
        let mut legacy = Vec::new();
        if let Some(next) = following.chars().next()
            && let Some(hash) = fragment_fingerprint(&self.text[start..end + next.len_utf8()])
            && Some(&hash) != current.as_ref()
        {
            legacy.push(hash);
        }
        if following.starts_with([' ', '\t'])
            && let Some(delimiter) = following.trim_start_matches([' ', '\t']).chars().next()
            && "=;,.)]{}".contains(delimiter)
            && let Some(hash) = fragment_fingerprint(&format!("{fragment}{delimiter}"))
            && Some(&hash) != current.as_ref()
            && !legacy.contains(&hash)
        {
            legacy.push(hash);
        }
        (current, legacy)
    }

    pub fn position(&self, offset: usize) -> Option<(u32, u32)> {
        let line = self.lines.partition_point(|start| *start <= offset);
        let start = self.lines[line - 1];
        let prefix = self.text.get(start..offset)?;
        (!prefix.ends_with('\r')).then(|| (line as u32, prefix.encode_utf16().count() as u32 + 1))
    }

    pub fn offset(&self, (line, column): (u32, u32)) -> Option<usize> {
        let row = line.checked_sub(1)? as usize;
        let start = *self.lines.get(row)?;
        let end = self.lines.get(row + 1).copied().unwrap_or(self.text.len());
        let line = self.text[start..end].trim_end_matches(['\r', '\n']);
        let mut target = column.checked_sub(1)? as usize;
        for (index, character) in line.char_indices() {
            if target < character.len_utf16() {
                return (target == 0).then_some(start + index);
            }
            target -= character.len_utf16();
        }
        (target == 0).then_some(start + line.len())
    }
}

pub(super) fn fragment_fingerprint(fragment: &str) -> Option<String> {
    let normalized = fragment
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!normalized.is_empty()).then(|| fingerprint(&normalized))
}
