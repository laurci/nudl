#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

impl std::fmt::Debug for FileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FileId({})", self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file_id: FileId,
    pub start: u32,
    pub end: u32,
}

impl std::fmt::Debug for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

impl Span {
    pub fn new(file_id: FileId, start: u32, end: u32) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }

    pub fn dummy() -> Self {
        Self {
            file_id: FileId(0),
            start: 0,
            end: 0,
        }
    }

    pub fn merge(self, other: Span) -> Span {
        debug_assert_eq!(self.file_id, other.file_id);
        Span {
            file_id: self.file_id,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn len(self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_merge() {
        let a = Span::new(FileId(0), 5, 10);
        let b = Span::new(FileId(0), 8, 20);
        let merged = a.merge(b);
        assert_eq!(merged.start, 5);
        assert_eq!(merged.end, 20);
    }

    #[test]
    fn span_dummy() {
        let d = Span::dummy();
        assert_eq!(d.start, 0);
        assert_eq!(d.end, 0);
    }

    #[test]
    fn span_len() {
        let s = Span::new(FileId(0), 3, 10);
        assert_eq!(s.len(), 7);
        assert!(!s.is_empty());
        assert!(Span::dummy().is_empty());
    }
}
