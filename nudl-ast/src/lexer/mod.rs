mod tokens;

#[cfg(test)]
mod tests;

use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::span::{FileId, Span};

use crate::lexer_diagnostic::LexerDiagnostic;
use crate::token::{Token, TokenKind, keyword_from_str};

/// How a template string body segment ended.
enum TemplateEnd {
    /// Hit closing `` ` ``
    Backtick,
    /// Hit unescaped `{` (start of interpolation)
    OpenBrace,
    /// Hit EOF without closing
    Unterminated,
}

pub struct Lexer<'a> {
    pub(super) source: &'a str,
    pub(super) file_id: FileId,
    pub(super) pos: usize,
    pub(super) diagnostics: DiagnosticBag,
    /// Stack of brace depths for nested template strings.
    /// When we enter a template string interpolation `{`, we push 1.
    /// Each nested `{` increments the top. Each `}` decrements.
    /// When top reaches 0, we're closing the interpolation and resume
    /// lexing the template string body.
    pub(super) template_brace_depth: Vec<u32>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str, file_id: FileId) -> Self {
        Self {
            source,
            file_id,
            pos: 0,
            diagnostics: DiagnosticBag::new(),
            template_brace_depth: Vec::new(),
        }
    }

    pub fn tokenize(mut self) -> (Vec<Token>, DiagnosticBag) {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.source.len() {
                tokens.push(Token::new(
                    TokenKind::Eof,
                    self.span(self.pos, self.pos),
                    "",
                ));
                break;
            }
            tokens.push(self.next_token());
        }
        (tokens, self.diagnostics)
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file_id, start as u32, end as u32)
    }

    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.source[self.pos + offset..].chars().next()
    }

    fn advance(&mut self) -> char {
        let ch = self.source[self.pos..].chars().next().unwrap();
        self.pos += ch.len_utf8();
        ch
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace
            while self.pos < self.source.len() {
                let ch = self.source.as_bytes()[self.pos];
                if ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r' {
                    self.pos += 1;
                } else {
                    break;
                }
            }

            // Skip line comments
            if self.pos + 1 < self.source.len() && &self.source[self.pos..self.pos + 2] == "//" {
                while self.pos < self.source.len() && self.source.as_bytes()[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }

            // Skip block comments (nested)
            if self.pos + 1 < self.source.len() && &self.source[self.pos..self.pos + 2] == "/*" {
                let start = self.pos;
                self.pos += 2;
                let mut depth = 1;
                while self.pos + 1 < self.source.len() && depth > 0 {
                    if &self.source[self.pos..self.pos + 2] == "/*" {
                        depth += 1;
                        self.pos += 2;
                    } else if &self.source[self.pos..self.pos + 2] == "*/" {
                        depth -= 1;
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                    }
                }
                if depth > 0 {
                    // Handle remaining single character at end of source
                    if self.pos < self.source.len() {
                        self.pos += 1;
                    }
                    self.diagnostics
                        .add(&LexerDiagnostic::UnterminatedBlockComment {
                            span: self.span(start, self.pos),
                        });
                }
                continue;
            }

            break;
        }
    }
}
