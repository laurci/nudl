mod binding_power;
mod expressions;
mod items;
mod statements;

#[cfg(test)]
mod tests;

use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::span::Spanned;

use crate::ast::*;
use crate::parser_diagnostic::ParserDiagnostic;
use crate::token::{Token, TokenKind};

pub use binding_power::{compound_assign_op, parse_int_suffix};

pub struct Parser {
    pub(super) tokens: Vec<Token>,
    pub(super) pos: usize,
    pub(super) diagnostics: DiagnosticBag,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics: DiagnosticBag::new(),
        }
    }

    pub fn parse_module(mut self) -> (Module, DiagnosticBag) {
        let mut items = Vec::new();
        while !self.at_eof() {
            if let Some(item) = self.parse_item() {
                items.push(item);
            } else {
                // Skip token to avoid infinite loop on error
                self.advance();
            }
        }
        (Module { items }, self.diagnostics)
    }

    pub(super) fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    pub(super) fn peek_kind(&self) -> TokenKind {
        self.tokens[self.pos].kind
    }

    pub(super) fn peek_nth(&self, n: usize) -> &Token {
        let idx = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[idx]
    }

    pub(super) fn at_eof(&self) -> bool {
        self.peek_kind() == TokenKind::Eof
    }

    pub(super) fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    pub(super) fn expect(&mut self, kind: TokenKind) -> Option<Token> {
        if self.peek_kind() == kind {
            Some(self.advance().clone())
        } else if self.at_eof() {
            self.diagnostics.add(&ParserDiagnostic::UnexpectedEof {
                span: self.peek().span,
                expected: format!("{:?}", kind),
            });
            None
        } else {
            self.diagnostics.add(&ParserDiagnostic::UnexpectedToken {
                span: self.peek().span,
                expected: format!("{:?}", kind),
                found: self.peek().text.clone(),
            });
            None
        }
    }

    pub(super) fn eat(&mut self, kind: TokenKind) -> bool {
        if self.peek_kind() == kind {
            self.advance();
            true
        } else {
            false
        }
    }
}
