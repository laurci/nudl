use crate::span::{FileId, Span};
use std::path::PathBuf;

#[derive(Debug)]
pub struct SourceFile {
    pub id: FileId,
    pub path: PathBuf,
    pub content: String,
    line_offsets: Vec<u32>,
}

impl SourceFile {
    fn compute_line_offsets(content: &str) -> Vec<u32> {
        let mut offsets = vec![0];
        for (i, ch) in content.char_indices() {
            if ch == '\n' {
                offsets.push((i + 1) as u32);
            }
        }
        offsets
    }

    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let line = match self.line_offsets.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let col = offset - self.line_offsets[line];
        (line as u32 + 1, col + 1)
    }
}

#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn add_file(&mut self, path: PathBuf, content: String) -> FileId {
        let id = FileId(self.files.len() as u32);
        let line_offsets = SourceFile::compute_line_offsets(&content);
        self.files.push(SourceFile {
            id,
            path,
            content,
            line_offsets,
        });
        id
    }

    pub fn get_file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub fn span_to_location(&self, span: Span) -> (&SourceFile, u32, u32) {
        let file = self.get_file(span.file_id);
        let (line, col) = file.line_col(span.start);
        (file, line, col)
    }

    pub fn span_text(&self, span: Span) -> &str {
        let file = self.get_file(span.file_id);
        &file.content[span.start as usize..span.end as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_map_line_col() {
        let mut sm = SourceMap::new();
        let fid = sm.add_file(
            "test.nudl".into(),
            "fn main() {\n    println(\"hi\");\n}\n".into(),
        );
        // 'f' is at offset 0 → line 1, col 1
        let (_, line, col) = sm.span_to_location(Span::new(fid, 0, 1));
        assert_eq!((line, col), (1, 1));
        // 'p' is at offset 16 → line 2, col 5
        let (_, line, col) = sm.span_to_location(Span::new(fid, 16, 17));
        assert_eq!((line, col), (2, 5));
    }

    #[test]
    fn span_text() {
        let mut sm = SourceMap::new();
        let fid = sm.add_file("test.nudl".into(), "hello world".into());
        assert_eq!(sm.span_text(Span::new(fid, 0, 5)), "hello");
        assert_eq!(sm.span_text(Span::new(fid, 6, 11)), "world");
    }
}
