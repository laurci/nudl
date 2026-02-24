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
    source: &'a str,
    file_id: FileId,
    pos: usize,
    diagnostics: DiagnosticBag,
    /// Stack of brace depths for nested template strings.
    /// When we enter a template string interpolation `{`, we push 1.
    /// Each nested `{` increments the top. Each `}` decrements.
    /// When top reaches 0, we're closing the interpolation and resume
    /// lexing the template string body.
    template_brace_depth: Vec<u32>,
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

    fn next_token(&mut self) -> Token {
        let start = self.pos;
        let ch = self.peek().unwrap();

        // If we're inside a template string interpolation and hit '}' that
        // closes it, resume lexing the template string body.
        if ch == '}' {
            if let Some(depth) = self.template_brace_depth.last_mut() {
                if *depth == 1 {
                    // This '}' closes the interpolation — resume template body
                    self.template_brace_depth.pop();
                    self.advance(); // skip }
                    return self.lex_template_string_continuation(start);
                } else {
                    *depth -= 1;
                }
            }
        }

        // Track brace depth for template string interpolation
        if ch == '{' {
            if let Some(depth) = self.template_brace_depth.last_mut() {
                *depth += 1;
            }
        }

        // Template string literal
        if ch == '`' {
            return self.lex_template_string_start(start);
        }

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

    /// Lex the body of a template string, consuming text and escape sequences
    /// until we hit an unescaped `{` (start of interpolation) or `` ` `` (end of
    /// template string). Returns the accumulated text.
    fn lex_template_string_body(&mut self) -> (String, TemplateEnd) {
        let mut value = String::new();
        loop {
            match self.peek() {
                None => {
                    return (value, TemplateEnd::Unterminated);
                }
                Some('`') => {
                    self.advance();
                    return (value, TemplateEnd::Backtick);
                }
                Some('{') => {
                    self.advance(); // skip {
                    self.template_brace_depth.push(1);
                    return (value, TemplateEnd::OpenBrace);
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
                        Some('r') => {
                            self.advance();
                            value.push('\r');
                        }
                        Some('\\') => {
                            self.advance();
                            value.push('\\');
                        }
                        Some('`') => {
                            self.advance();
                            value.push('`');
                        }
                        Some('{') => {
                            self.advance();
                            value.push('{');
                        }
                        Some('}') => {
                            self.advance();
                            value.push('}');
                        }
                        Some('0') => {
                            self.advance();
                            value.push('\0');
                        }
                        Some('"') => {
                            self.advance();
                            value.push('"');
                        }
                        Some('\'') => {
                            self.advance();
                            value.push('\'');
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
                            return (value, TemplateEnd::Unterminated);
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

    /// Called when we see the opening `` ` `` of a template string.
    fn lex_template_string_start(&mut self, start: usize) -> Token {
        self.advance(); // skip opening `
        let (value, end_reason) = self.lex_template_string_body();
        match end_reason {
            TemplateEnd::Backtick => {
                // No interpolation — just emit as a plain string literal
                Token::new(TokenKind::StringLiteral, self.span(start, self.pos), &value)
            }
            TemplateEnd::OpenBrace => Token::new(
                TokenKind::TemplateStringStart,
                self.span(start, self.pos),
                &value,
            ),
            TemplateEnd::Unterminated => {
                self.diagnostics
                    .add(&LexerDiagnostic::UnterminatedTemplateString {
                        span: self.span(start, self.pos),
                    });
                Token::new(TokenKind::Error, self.span(start, self.pos), &value)
            }
        }
    }

    /// Called after we close an interpolation `}` — lex the next segment
    /// of the template string.
    fn lex_template_string_continuation(&mut self, start: usize) -> Token {
        let (value, end_reason) = self.lex_template_string_body();
        match end_reason {
            TemplateEnd::Backtick => Token::new(
                TokenKind::TemplateStringEnd,
                self.span(start, self.pos),
                &value,
            ),
            TemplateEnd::OpenBrace => Token::new(
                TokenKind::TemplateStringPart,
                self.span(start, self.pos),
                &value,
            ),
            TemplateEnd::Unterminated => {
                self.diagnostics
                    .add(&LexerDiagnostic::UnterminatedTemplateString {
                        span: self.span(start, self.pos),
                    });
                Token::new(TokenKind::Error, self.span(start, self.pos), &value)
            }
        }
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

    #[test]
    fn template_string_no_interpolation() {
        let tokens = lex("`hello, world`");
        assert_eq!(tokens[0].kind, TokenKind::StringLiteral);
        assert_eq!(tokens[0].text, "hello, world");
    }

    #[test]
    fn template_string_single_interpolation() {
        let tokens = lex("`hello, {name}!`");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::TemplateStringStart,
                TokenKind::Ident,
                TokenKind::TemplateStringEnd,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[0].text, "hello, ");
        assert_eq!(tokens[1].text, "name");
        assert_eq!(tokens[2].text, "!");
    }

    #[test]
    fn template_string_multiple_interpolations() {
        let tokens = lex("`{a} + {b} = {c}`");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::TemplateStringStart,
                TokenKind::Ident, // a
                TokenKind::TemplateStringPart,
                TokenKind::Ident, // b
                TokenKind::TemplateStringPart,
                TokenKind::Ident, // c
                TokenKind::TemplateStringEnd,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[0].text, "");
        assert_eq!(tokens[2].text, " + ");
        assert_eq!(tokens[4].text, " = ");
        assert_eq!(tokens[6].text, "");
    }

    #[test]
    fn template_string_with_braces_in_expr() {
        // Expression contains braces (e.g., a block or struct literal)
        let tokens = lex("`result: {if true { 1 } else { 2 }}`");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::TemplateStringStart, // "result: "
                TokenKind::If,
                TokenKind::True,
                TokenKind::LBrace,
                TokenKind::IntLiteral, // 1
                TokenKind::RBrace,
                TokenKind::Else,
                TokenKind::LBrace,
                TokenKind::IntLiteral, // 2
                TokenKind::RBrace,
                TokenKind::TemplateStringEnd, // ""
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn template_string_escapes() {
        let tokens = lex(r"`\{not interpolated\}`");
        assert_eq!(tokens[0].kind, TokenKind::StringLiteral);
        assert_eq!(tokens[0].text, "{not interpolated}");
    }

    #[test]
    fn template_string_backtick_escape() {
        let tokens = lex(r"`contains a \` backtick`");
        assert_eq!(tokens[0].kind, TokenKind::StringLiteral);
        assert_eq!(tokens[0].text, "contains a ` backtick");
    }

    #[test]
    fn error_unterminated_template_string() {
        let (tokens, diags) = Lexer::new("`hello", FileId(0)).tokenize();
        assert!(diags.has_errors());
        assert_eq!(tokens[0].kind, TokenKind::Error);
    }
}
