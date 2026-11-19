use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dornguard_core::fingerprint;

#[derive(Default)]
pub struct Sources(BTreeMap<PathBuf, Option<String>>);

impl Sources {
    pub fn offsets(
        &mut self,
        root: &Path,
        path: &Path,
        start: usize,
        end: usize,
    ) -> Option<String> {
        let source = self.get(root, path)?;
        fragment_fingerprint(source.get(start..end)?)
    }

    pub fn lines(
        &mut self,
        root: &Path,
        path: &Path,
        start: (u32, u32),
        end: (u32, u32),
    ) -> Option<String> {
        self.span(root, path, start, end, true)
    }

    pub fn span(
        &mut self,
        root: &Path,
        path: &Path,
        start: (u32, u32),
        end: (u32, u32),
        inclusive: bool,
    ) -> Option<String> {
        let source = self.get(root, path)?;
        let start = byte_offset(source, start.0, start.1, false)?;
        let end = byte_offset(source, end.0, end.1, inclusive)?;
        fragment_fingerprint(source.get(start..end)?)
    }

    fn get(&mut self, root: &Path, path: &Path) -> Option<&str> {
        self.0
            .entry(path.to_path_buf())
            .or_insert_with(|| std::fs::read_to_string(root.join(path)).ok())
            .as_deref()
    }
}

fn fragment_fingerprint(fragment: &str) -> Option<String> {
    let normalized = fragment
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!normalized.is_empty()).then(|| fingerprint(&normalized))
}

fn byte_offset(source: &str, line: u32, column: u32, inclusive: bool) -> Option<usize> {
    let mut start = 0;
    let mut lines = source.split_inclusive('\n');
    for text in lines.by_ref().take(line.checked_sub(1)? as usize) {
        start += text.len();
    }
    let line = lines.next()?;
    let mut target = column.checked_sub(1)? as usize;
    let line = line.trim_end_matches(['\r', '\n']);
    for (index, character) in line.char_indices() {
        if target < character.len_utf16() {
            return Some(start + index + usize::from(inclusive) * character.len_utf8());
        }
        target -= character.len_utf16();
    }
    (!inclusive && target == 0).then_some(start + line.len())
}
