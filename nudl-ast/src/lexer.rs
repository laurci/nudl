use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::span::{FileId, Span};

use crate::lexer_diagnostic::LexerDiagnostic;
use crate::token::{Token, TokenKind, keyword_from_str};

pub struct Lexer<'a> {
    source: &'a str,
    file_id: FileId,
    pos: usize,
    diagnostics: DiagnosticBag,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str, file_id: FileId) -> Self {
        Self {
            source,
            file_id,
            pos: 0,
            diagnostics: DiagnosticBag::new(),
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

    fn next_token(&mut self) -> Token {
        let start = self.pos;
        let ch = self.peek().unwrap();

        // String literal
        if ch == '"' {
            return self.lex_string(start);
        }

        // Char literal
        if ch == '\'' {
            return self.lex_char(start);
        }

        // Number
        if ch.is_ascii_digit() {
            return self.lex_number(start);
        }

        // Identifier/keyword
        if ch.is_ascii_alphabetic() || ch == '_' {
            return self.lex_ident(start);
        }

        // Operators and punctuation
        self.lex_operator(start)
    }

    fn lex_string(&mut self, start: usize) -> Token {
        self.advance(); // skip opening "
        let mut value = String::new();
        loop {
            match self.peek() {
                None => {
                    self.diagnostics.add(&LexerDiagnostic::UnterminatedString {
                        span: self.span(start, self.pos),
                    });
                    return Token::new(TokenKind::Error, self.span(start, self.pos), &value);
                }
                Some('"') => {
                    self.advance();
                    return Token::new(
                        TokenKind::StringLiteral,
                        self.span(start, self.pos),
                        &value,
                    );
                }
                Some('\\') => {
                    self.advance();
                    match self.peek() {
                        Some('n') => {
                            self.advance();
                            value.push('\n');
                        }
                        Some('t') => {
                            self.advance();
                            value.push('\t');
                        }
                        Some('\\') => {
                            self.advance();
                            value.push('\\');
                        }
                        Some('"') => {
                            self.advance();
                            value.push('"');
                        }
                        Some('0') => {
                            self.advance();
                            value.push('\0');
                        }
                        Some('r') => {
                            self.advance();
                            value.push('\r');
                        }
                        Some(ch) => {
                            let esc_start = self.pos - 1;
                            self.advance();
                            self.diagnostics.add(&LexerDiagnostic::InvalidEscape {
                                span: self.span(esc_start, self.pos),
                                ch,
                            });
                            value.push(ch);
                        }
                        None => {
                            self.diagnostics.add(&LexerDiagnostic::UnterminatedString {
                                span: self.span(start, self.pos),
                            });
                            return Token::new(
                                TokenKind::Error,
                                self.span(start, self.pos),
                                &value,
                            );
                        }
                    }
                }
                Some(ch) => {
                    self.advance();
                    value.push(ch);
                }
            }
        }
    }

    fn lex_char(&mut self, start: usize) -> Token {
        self.advance(); // skip opening '
        let ch = match self.peek() {
            Some('\\') => {
                self.advance();
                match self.peek() {
                    Some('n') => {
                        self.advance();
                        '\n'
                    }
                    Some('t') => {
                        self.advance();
                        '\t'
                    }
                    Some('\\') => {
                        self.advance();
                        '\\'
                    }
                    Some('\'') => {
                        self.advance();
                        '\''
                    }
                    Some('0') => {
                        self.advance();
                        '\0'
                    }
                    Some(c) => {
                        self.advance();
                        self.diagnostics.add(&LexerDiagnostic::InvalidEscape {
                            span: self.span(start, self.pos),
                            ch: c,
                        });
                        c
                    }
                    None => {
                        self.diagnostics.add(&LexerDiagnostic::UnterminatedString {
                            span: self.span(start, self.pos),
                        });
                        return Token::new(TokenKind::Error, self.span(start, self.pos), "");
                    }
                }
            }
            Some(c) => {
                self.advance();
                c
            }
            None => {
                self.diagnostics.add(&LexerDiagnostic::UnterminatedString {
                    span: self.span(start, self.pos),
                });
                return Token::new(TokenKind::Error, self.span(start, self.pos), "");
            }
        };
        if self.peek() == Some('\'') {
            self.advance();
        }
        Token::new(
            TokenKind::CharLiteral,
            self.span(start, self.pos),
            ch.to_string(),
        )
    }

    fn lex_number(&mut self, start: usize) -> Token {
        // Check for hex/octal/binary prefixes
        if self.peek() == Some('0') {
            if let Some(next) = self.peek_at(1) {
                match next {
                    'x' | 'X' => {
                        self.advance();
                        self.advance();
                        while self
                            .peek()
                            .is_some_and(|c| c.is_ascii_hexdigit() || c == '_')
                        {
                            self.advance();
                        }
                        let text = &self.source[start..self.pos];
                        return Token::new(TokenKind::IntLiteral, self.span(start, self.pos), text);
                    }
                    'o' | 'O' => {
                        self.advance();
                        self.advance();
                        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                            self.advance();
                        }
                        let text = &self.source[start..self.pos];
                        return Token::new(TokenKind::IntLiteral, self.span(start, self.pos), text);
                    }
                    'b' | 'B' => {
                        self.advance();
                        self.advance();
                        while self
                            .peek()
                            .is_some_and(|c| c == '0' || c == '1' || c == '_')
                        {
                            self.advance();
                        }
                        let text = &self.source[start..self.pos];
                        return Token::new(TokenKind::IntLiteral, self.span(start, self.pos), text);
                    }
                    _ => {}
                }
            }
        }

        // Decimal digits
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
            self.advance();
        }

        // Check for float
        let mut is_float = false;
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            self.advance(); // skip .
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                self.advance();
            }
        }

        // Exponent
        if self.peek().is_some_and(|c| c == 'e' || c == 'E') {
            is_float = true;
            self.advance();
            if self.peek().is_some_and(|c| c == '+' || c == '-') {
                self.advance();
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                self.advance();
            }
        }

        let text = &self.source[start..self.pos];
        let kind = if is_float {
            TokenKind::FloatLiteral
        } else {
            TokenKind::IntLiteral
        };
        Token::new(kind, self.span(start, self.pos), text)
    }

    fn lex_ident(&mut self, start: usize) -> Token {
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.advance();
        }
        let text = &self.source[start..self.pos];

        // Check for bool literals first
        if text == "true" || text == "false" {
            return Token::new(
                if text == "true" {
                    TokenKind::True
                } else {
                    TokenKind::False
                },
                self.span(start, self.pos),
                text,
            );
        }

        let kind = keyword_from_str(text).unwrap_or(TokenKind::Ident);
        Token::new(kind, self.span(start, self.pos), text)
    }

    fn lex_operator(&mut self, start: usize) -> Token {
        let ch = self.advance();
        let next = self.peek();

        let (kind, text) = match (ch, next) {
            // Two-char operators (check three-char first)
            ('.', Some('.')) => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    (TokenKind::DotDotEq, "..=")
                } else {
                    (TokenKind::DotDot, "..")
                }
            }
            ('<', Some('<')) => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    (TokenKind::LtLtEq, "<<=")
                } else {
                    (TokenKind::LtLt, "<<")
                }
            }
            ('>', Some('>')) => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    (TokenKind::GtGtEq, ">>=")
                } else {
                    (TokenKind::GtGt, ">>")
                }
            }
            ('=', Some('=')) => {
                self.advance();
                (TokenKind::EqEq, "==")
            }
            ('!', Some('=')) => {
                self.advance();
                (TokenKind::BangEq, "!=")
            }
            ('<', Some('=')) => {
                self.advance();
                (TokenKind::LtEq, "<=")
            }
            ('>', Some('=')) => {
                self.advance();
                (TokenKind::GtEq, ">=")
            }
            ('&', Some('&')) => {
                self.advance();
                (TokenKind::AmpAmp, "&&")
            }
            ('|', Some('|')) => {
                self.advance();
                (TokenKind::PipePipe, "||")
            }
            ('+', Some('=')) => {
                self.advance();
                (TokenKind::PlusEq, "+=")
            }
            ('-', Some('=')) => {
                self.advance();
                (TokenKind::MinusEq, "-=")
            }
            ('*', Some('=')) => {
                self.advance();
                (TokenKind::StarEq, "*=")
            }
            ('/', Some('=')) => {
                self.advance();
                (TokenKind::SlashEq, "/=")
            }
            ('%', Some('=')) => {
                self.advance();
                (TokenKind::PercentEq, "%=")
            }
            ('&', Some('=')) => {
                self.advance();
                (TokenKind::AmpEq, "&=")
            }
            ('|', Some('=')) => {
                self.advance();
                (TokenKind::PipeEq, "|=")
            }
            ('^', Some('=')) => {
                self.advance();
                (TokenKind::CaretEq, "^=")
            }
            ('-', Some('>')) => {
                self.advance();
                (TokenKind::Arrow, "->")
            }
            ('=', Some('>')) => {
                self.advance();
                (TokenKind::FatArrow, "=>")
            }
            (':', Some(':')) => {
                self.advance();
                (TokenKind::ColonColon, "::")
            }

            // Single-char
            ('(', _) => (TokenKind::LParen, "("),
            (')', _) => (TokenKind::RParen, ")"),
            ('{', _) => (TokenKind::LBrace, "{"),
            ('}', _) => (TokenKind::RBrace, "}"),
            ('[', _) => (TokenKind::LBracket, "["),
            (']', _) => (TokenKind::RBracket, "]"),
            ('+', _) => (TokenKind::Plus, "+"),
            ('-', _) => (TokenKind::Minus, "-"),
            ('*', _) => (TokenKind::Star, "*"),
            ('/', _) => (TokenKind::Slash, "/"),
            ('%', _) => (TokenKind::Percent, "%"),
            ('&', _) => (TokenKind::Amp, "&"),
            ('|', _) => (TokenKind::Pipe, "|"),
            ('^', _) => (TokenKind::Caret, "^"),
            ('~', _) => (TokenKind::Tilde, "~"),
            ('!', _) => (TokenKind::Bang, "!"),
            ('<', _) => (TokenKind::Lt, "<"),
            ('>', _) => (TokenKind::Gt, ">"),
            ('=', _) => (TokenKind::Eq, "="),
            ('.', _) => (TokenKind::Dot, "."),
            (',', _) => (TokenKind::Comma, ","),
            (':', _) => (TokenKind::Colon, ":"),
            (';', _) => (TokenKind::Semi, ";"),
            ('#', _) => (TokenKind::Hash, "#"),
            ('@', _) => (TokenKind::At, "@"),
            ('?', _) => (TokenKind::Question, "?"),
            ('_', _) => (TokenKind::Underscore, "_"),
            _ => {
                self.diagnostics.add(&LexerDiagnostic::UnexpectedChar {
                    span: self.span(start, self.pos),
                    ch,
                });
                (TokenKind::Error, "")
            }
        };

        Token::new(kind, self.span(start, self.pos), text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(source: &str) -> Vec<Token> {
        let (tokens, diags) = Lexer::new(source, FileId(0)).tokenize();
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );
        tokens
    }

    fn lex_kinds(source: &str) -> Vec<TokenKind> {
        lex(source).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn hello_world() {
        let tokens = lex(r#"fn main() {
    println("Hello, world!");
}"#);
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Fn,
                TokenKind::Ident, // main
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::LBrace,
                TokenKind::Ident, // println
                TokenKind::LParen,
                TokenKind::StringLiteral,
                TokenKind::RParen,
                TokenKind::Semi,
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
        // Verify string literal value
        let str_tok = tokens
            .iter()
            .find(|t| t.kind == TokenKind::StringLiteral)
            .unwrap();
        assert_eq!(str_tok.text, "Hello, world!");
    }

    #[test]
    fn string_escapes() {
        let tokens = lex(r#""hello\nworld\t\\""#);
        let str_tok = &tokens[0];
        assert_eq!(str_tok.kind, TokenKind::StringLiteral);
        assert_eq!(str_tok.text, "hello\nworld\t\\");
    }

    #[test]
    fn nested_comments() {
        let kinds = lex_kinds("/* outer /* inner */ */ 42");
        assert_eq!(kinds, vec![TokenKind::IntLiteral, TokenKind::Eof]);
    }

    #[test]
    fn operators() {
        let kinds = lex_kinds("-> => == != <= >= && || << >> :: ..");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Arrow,
                TokenKind::FatArrow,
                TokenKind::EqEq,
                TokenKind::BangEq,
                TokenKind::LtEq,
                TokenKind::GtEq,
                TokenKind::AmpAmp,
                TokenKind::PipePipe,
                TokenKind::LtLt,
                TokenKind::GtGt,
                TokenKind::ColonColon,
                TokenKind::DotDot,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn numbers() {
        let tokens = lex("42 0xff 0b1010 3.14 1e10");
        assert_eq!(tokens[0].kind, TokenKind::IntLiteral);
        assert_eq!(tokens[0].text, "42");
        assert_eq!(tokens[1].kind, TokenKind::IntLiteral);
        assert_eq!(tokens[1].text, "0xff");
        assert_eq!(tokens[2].kind, TokenKind::IntLiteral);
        assert_eq!(tokens[2].text, "0b1010");
        assert_eq!(tokens[3].kind, TokenKind::FloatLiteral);
        assert_eq!(tokens[3].text, "3.14");
        assert_eq!(tokens[4].kind, TokenKind::FloatLiteral);
        assert_eq!(tokens[4].text, "1e10");
    }

    #[test]
    fn error_unterminated_string() {
        let (tokens, diags) = Lexer::new(r#""hello"#, FileId(0)).tokenize();
        assert!(diags.has_errors());
        assert_eq!(tokens[0].kind, TokenKind::Error);
    }

    #[test]
    fn error_unexpected_char() {
        let (_, diags) = Lexer::new("§", FileId(0)).tokenize();
        assert!(diags.has_errors());
    }
}
