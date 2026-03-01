use super::*;

impl<'a> Lexer<'a> {
    pub(super) fn next_token(&mut self) -> Token {
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

        // Char literal or label ('ident)
        if ch == '\'' {
            // Distinguish labels from char literals:
            // - 'a' (char followed by closing quote) → char literal
            // - '\n' (escape sequence) → char literal
            // - 'abc (identifier, no closing quote after first char) → label
            if let Some(next) = self.peek_at(1) {
                if next.is_ascii_alphabetic() || next == '_' {
                    // Check if it's 'x' (char literal) or 'ident (label)
                    let after_next = self.peek_at(1 + next.len_utf8());
                    if after_next != Some('\'') {
                        // It's a label: consume ' and then the identifier
                        self.advance(); // skip '
                        let label_start = self.pos;
                        while self
                            .peek()
                            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                        {
                            self.advance();
                        }
                        let text = &self.source[label_start..self.pos];
                        return Token::new(TokenKind::Label, self.span(start, self.pos), text);
                    }
                }
            }
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
                        self.try_consume_int_suffix();
                        let text = &self.source[start..self.pos];
                        return Token::new(TokenKind::IntLiteral, self.span(start, self.pos), text);
                    }
                    'o' | 'O' => {
                        self.advance();
                        self.advance();
                        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                            self.advance();
                        }
                        self.try_consume_int_suffix();
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
                        self.try_consume_int_suffix();
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

        if !is_float {
            self.try_consume_int_suffix();
        }
        let text = &self.source[start..self.pos];
        let kind = if is_float {
            TokenKind::FloatLiteral
        } else {
            TokenKind::IntLiteral
        };
        Token::new(kind, self.span(start, self.pos), text)
    }

    /// Try to consume an integer type suffix (i8, i16, i32, i64, u8, u16, u32, u64)
    /// at the current position. Only consumes if the suffix is followed by a
    /// non-identifier character (or EOF).
    fn try_consume_int_suffix(&mut self) {
        let remaining = &self.source[self.pos..];
        const SUFFIXES: &[&str] = &["i16", "i32", "i64", "i8", "u16", "u32", "u64", "u8"];
        for &suffix in SUFFIXES {
            if remaining.starts_with(suffix) {
                let after = self.pos + suffix.len();
                let next_ch = self.source[after..].chars().next();
                if next_ch.is_none()
                    || !next_ch.unwrap().is_ascii_alphanumeric() && next_ch.unwrap() != '_'
                {
                    self.pos = after;
                    return;
                }
            }
        }
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
            ('|', Some('>')) => {
                self.advance();
                (TokenKind::PipeGt, "|>")
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
