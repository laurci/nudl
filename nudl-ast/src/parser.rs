use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::span::Spanned;

use crate::ast::*;
use crate::parser_diagnostic::ParserDiagnostic;
use crate::token::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: DiagnosticBag,
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

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_kind(&self) -> TokenKind {
        self.tokens[self.pos].kind
    }

    fn at_eof(&self) -> bool {
        self.peek_kind() == TokenKind::Eof
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, kind: TokenKind) -> Option<Token> {
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

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.peek_kind() == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn parse_item(&mut self) -> Option<SpannedItem> {
        let is_pub = self.eat(TokenKind::Pub);

        match self.peek_kind() {
            TokenKind::Fn => self.parse_fn_def(is_pub),
            TokenKind::Extern => self.parse_extern_block(),
            _ => {
                if is_pub {
                    self.diagnostics.add(&ParserDiagnostic::UnexpectedToken {
                        span: self.peek().span,
                        expected: "item after 'pub'".into(),
                        found: self.peek().text.clone(),
                    });
                }
                None
            }
        }
    }

    fn parse_fn_def(&mut self, is_pub: bool) -> Option<SpannedItem> {
        let fn_tok = self.expect(TokenKind::Fn)?;
        let start = fn_tok.span;

        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.text.clone();

        self.expect(TokenKind::LParen)?;
        let params = self.parse_param_list();
        self.expect(TokenKind::RParen)?;

        let return_type = if self.eat(TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;
        let end = body.span;

        Some(Spanned::new(
            Item::FnDef {
                name,
                params,
                return_type,
                body,
                is_pub,
            },
            start.merge(end),
        ))
    }

    fn parse_extern_block(&mut self) -> Option<SpannedItem> {
        let start = self.expect(TokenKind::Extern)?.span;

        // Optional library string: extern "C" { ... }
        let library = if self.peek_kind() == TokenKind::StringLiteral {
            let tok = self.advance().clone();
            Some(tok.text.clone())
        } else {
            None
        };

        self.expect(TokenKind::LBrace)?;

        let mut items = Vec::new();
        while !self.at_eof() && self.peek_kind() != TokenKind::RBrace {
            if let Some(decl) = self.parse_extern_fn_decl() {
                items.push(decl);
            } else {
                self.advance();
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;

        Some(Spanned::new(
            Item::ExternBlock { library, items },
            start.merge(end),
        ))
    }

    fn parse_extern_fn_decl(&mut self) -> Option<Spanned<ExternFnDecl>> {
        let start = self.expect(TokenKind::Fn)?.span;
        let name = self.expect(TokenKind::Ident)?.text.clone();
        self.expect(TokenKind::LParen)?;
        let params = self.parse_param_list();
        let end = self.expect(TokenKind::RParen)?.span;

        let (return_type, end) = if self.eat(TokenKind::Arrow) {
            let ty = self.parse_type()?;
            let end = ty.span;
            (Some(ty), end)
        } else {
            (None, end)
        };

        self.eat(TokenKind::Semi);

        Some(Spanned::new(
            ExternFnDecl {
                name,
                params,
                return_type,
            },
            start.merge(end),
        ))
    }

    fn parse_param_list(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        while self.peek_kind() != TokenKind::RParen && !self.at_eof() {
            if let Some(param) = self.parse_param() {
                params.push(param);
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        params
    }

    fn parse_param(&mut self) -> Option<Param> {
        let name_tok = self.expect(TokenKind::Ident)?;
        let start = name_tok.span;
        self.expect(TokenKind::Colon)?;
        let ty = self.parse_type()?;
        let end = ty.span;
        Some(Param {
            name: name_tok.text.clone(),
            ty,
            span: start.merge(end),
        })
    }

    fn parse_type(&mut self) -> Option<Spanned<TypeExpr>> {
        if self.peek_kind() == TokenKind::LParen {
            let start = self.advance().span;
            let end = self.expect(TokenKind::RParen)?.span;
            return Some(Spanned::new(TypeExpr::Unit, start.merge(end)));
        }

        if self.peek_kind() == TokenKind::Ident {
            let tok = self.advance().clone();
            return Some(Spanned::new(TypeExpr::Named(tok.text.clone()), tok.span));
        }

        self.diagnostics.add(&ParserDiagnostic::ExpectedType {
            span: self.peek().span,
        });
        None
    }

    fn parse_block(&mut self) -> Option<Spanned<Block>> {
        let start = self.expect(TokenKind::LBrace)?.span;
        let mut stmts = Vec::new();

        while !self.at_eof() && self.peek_kind() != TokenKind::RBrace {
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            } else {
                self.advance();
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;

        // For simplicity in POC: no tail expression handling
        Some(Spanned::new(Block { stmts, tail_expr: None }, start.merge(end)))
    }

    fn parse_stmt(&mut self) -> Option<SpannedStmt> {
        match self.peek_kind() {
            TokenKind::Let => self.parse_let_stmt(),
            TokenKind::Fn | TokenKind::Pub | TokenKind::Extern => {
                let item = self.parse_item()?;
                let span = item.span;
                Some(Spanned::new(Stmt::Item(item), span))
            }
            _ => {
                let expr = self.parse_expr()?;
                let span = expr.span;
                self.eat(TokenKind::Semi);
                Some(Spanned::new(Stmt::Expr(expr), span))
            }
        }
    }

    fn parse_let_stmt(&mut self) -> Option<SpannedStmt> {
        let start = self.expect(TokenKind::Let)?.span;
        let is_mut = self.eat(TokenKind::Mut);
        let name = self.expect(TokenKind::Ident)?.text.clone();

        let ty = if self.eat(TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(TokenKind::Eq)?;
        let value = self.parse_expr()?;
        let end = value.span;
        self.eat(TokenKind::Semi);

        Some(Spanned::new(
            Stmt::Let { name, ty, value, is_mut },
            start.merge(end),
        ))
    }

    fn parse_expr(&mut self) -> Option<SpannedExpr> {
        self.parse_expr_bp(0)
    }

    fn parse_expr_bp(&mut self, _min_bp: u8) -> Option<SpannedExpr> {
        // For the POC, we only need primary + call expressions
        let mut lhs = self.parse_primary()?;

        // Handle postfix: call expressions
        loop {
            if self.peek_kind() == TokenKind::LParen {
                let start = lhs.span;
                self.advance(); // (
                let args = self.parse_call_args();
                let end = self.expect(TokenKind::RParen)?.span;
                lhs = Spanned::new(
                    Expr::Call {
                        callee: Box::new(lhs),
                        args,
                    },
                    start.merge(end),
                );
            } else {
                break;
            }
        }

        Some(lhs)
    }

    fn parse_primary(&mut self) -> Option<SpannedExpr> {
        match self.peek_kind() {
            TokenKind::IntLiteral => {
                let tok = self.advance().clone();
                Some(Spanned::new(Expr::Literal(Literal::Int(tok.text.clone())), tok.span))
            }
            TokenKind::FloatLiteral => {
                let tok = self.advance().clone();
                Some(Spanned::new(Expr::Literal(Literal::Float(tok.text.clone())), tok.span))
            }
            TokenKind::StringLiteral => {
                let tok = self.advance().clone();
                Some(Spanned::new(Expr::Literal(Literal::String(tok.text.clone())), tok.span))
            }
            TokenKind::CharLiteral => {
                let tok = self.advance().clone();
                let ch = tok.text.chars().next().unwrap_or('\0');
                Some(Spanned::new(Expr::Literal(Literal::Char(ch)), tok.span))
            }
            TokenKind::True => {
                let tok = self.advance().clone();
                Some(Spanned::new(Expr::Literal(Literal::Bool(true)), tok.span))
            }
            TokenKind::False => {
                let tok = self.advance().clone();
                Some(Spanned::new(Expr::Literal(Literal::Bool(false)), tok.span))
            }
            TokenKind::Ident => {
                let tok = self.advance().clone();
                Some(Spanned::new(Expr::Ident(tok.text.clone()), tok.span))
            }
            TokenKind::Return => {
                let tok = self.advance().clone();
                let value = if self.peek_kind() != TokenKind::Semi
                    && self.peek_kind() != TokenKind::RBrace
                    && !self.at_eof()
                {
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                };
                let end = value.as_ref().map(|v| v.span).unwrap_or(tok.span);
                Some(Spanned::new(Expr::Return(value), tok.span.merge(end)))
            }
            TokenKind::LBrace => {
                let block = self.parse_block()?;
                let span = block.span;
                Some(Spanned::new(Expr::Block(block.node), span))
            }
            _ => {
                self.diagnostics.add(&ParserDiagnostic::ExpectedExpression {
                    span: self.peek().span,
                });
                None
            }
        }
    }

    fn parse_call_args(&mut self) -> Vec<CallArg> {
        let mut args = Vec::new();
        while self.peek_kind() != TokenKind::RParen && !self.at_eof() {
            if let Some(expr) = self.parse_expr() {
                args.push(CallArg { name: None, value: expr });
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use nudl_core::span::FileId;

    fn parse(source: &str) -> Module {
        let (tokens, lex_diags) = Lexer::new(source, FileId(0)).tokenize();
        assert!(!lex_diags.has_errors(), "lex errors: {:?}", lex_diags.reports());
        let (module, parse_diags) = Parser::new(tokens).parse_module();
        assert!(!parse_diags.has_errors(), "parse errors: {:?}", parse_diags.reports());
        module
    }

    #[test]
    fn parse_hello_world() {
        let module = parse(r#"fn main() {
    println("Hello, world!");
}"#);
        assert_eq!(module.items.len(), 1);
        match &module.items[0].node {
            Item::FnDef { name, params, return_type, .. } => {
                assert_eq!(name, "main");
                assert!(params.is_empty());
                assert!(return_type.is_none());
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_fn_with_params_and_return() {
        let module = parse("fn add(a: i32, b: i32) -> i32 { a }");
        match &module.items[0].node {
            Item::FnDef { name, params, return_type, .. } => {
                assert_eq!(name, "add");
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "a");
                assert_eq!(params[1].name, "b");
                assert!(return_type.is_some());
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_extern_block() {
        let module = parse(r#"extern "C" {
    fn write(fd: i32, buf: RawPtr, len: u64) -> i64;
}"#);
        match &module.items[0].node {
            Item::ExternBlock { library, items } => {
                assert_eq!(library.as_deref(), Some("C"));
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].node.name, "write");
            }
            _ => panic!("expected ExternBlock"),
        }
    }

    #[test]
    fn parse_error_missing_brace() {
        let (tokens, _) = Lexer::new("fn main() {", FileId(0)).tokenize();
        let (_, diags) = Parser::new(tokens).parse_module();
        assert!(diags.has_errors());
    }
}
