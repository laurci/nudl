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
        let mut tail_expr = None;

        while !self.at_eof() && self.peek_kind() != TokenKind::RBrace {
            // Try to parse a statement
            match self.peek_kind() {
                TokenKind::Let => {
                    if let Some(stmt) = self.parse_let_stmt() {
                        stmts.push(stmt);
                    } else {
                        self.advance();
                    }
                }
                TokenKind::Fn | TokenKind::Pub | TokenKind::Extern => {
                    if let Some(item) = self.parse_item() {
                        let span = item.span;
                        stmts.push(Spanned::new(Stmt::Item(item), span));
                    } else {
                        self.advance();
                    }
                }
                _ => {
                    if let Some(expr) = self.parse_expr() {
                        // Check if this expression is followed by ';' or '}'
                        if self.peek_kind() == TokenKind::Semi {
                            // Expression statement
                            let span = expr.span;
                            self.advance(); // consume ;
                            stmts.push(Spanned::new(Stmt::Expr(expr), span));
                        } else if self.peek_kind() == TokenKind::RBrace {
                            // Tail expression
                            tail_expr = Some(Box::new(expr));
                        } else {
                            // Expression statement without semicolon (e.g., if/while/loop)
                            let span = expr.span;
                            stmts.push(Spanned::new(Stmt::Expr(expr), span));
                        }
                    } else {
                        self.advance();
                    }
                }
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;

        Some(Spanned::new(
            Block { stmts, tail_expr },
            start.merge(end),
        ))
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
            Stmt::Let {
                name,
                ty,
                value,
                is_mut,
            },
            start.merge(end),
        ))
    }

    fn parse_expr(&mut self) -> Option<SpannedExpr> {
        self.parse_expr_bp(0)
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> Option<SpannedExpr> {
        // Parse prefix or primary
        let mut lhs = if let Some(right_bp) = prefix_binding_power(self.peek_kind()) {
            let op_tok = self.advance().clone();
            let op = match op_tok.kind {
                TokenKind::Minus => UnaryOp::Neg,
                TokenKind::Bang => UnaryOp::Not,
                _ => unreachable!(),
            };
            let operand = self.parse_expr_bp(right_bp)?;
            let span = op_tok.span.merge(operand.span);
            Spanned::new(
                Expr::Unary {
                    op,
                    operand: Box::new(operand),
                },
                span,
            )
        } else {
            self.parse_primary()?
        };

        loop {
            let kind = self.peek_kind();

            // Postfix: call expressions
            if kind == TokenKind::LParen {
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
                continue;
            }

            // Assignment operators (right-associative)
            if let Some(assign_bp) = assign_binding_power(kind) {
                if assign_bp < min_bp {
                    break;
                }
                let op_tok = self.advance().clone();
                let rhs = self.parse_expr_bp(assign_bp)?; // right-associative: same bp
                let span = lhs.span.merge(rhs.span);
                lhs = match op_tok.kind {
                    TokenKind::Eq => Spanned::new(
                        Expr::Assign {
                            target: Box::new(lhs),
                            value: Box::new(rhs),
                        },
                        span,
                    ),
                    _ => {
                        let op = compound_assign_op(op_tok.kind);
                        Spanned::new(
                            Expr::CompoundAssign {
                                op,
                                target: Box::new(lhs),
                                value: Box::new(rhs),
                            },
                            span,
                        )
                    }
                };
                continue;
            }

            // Infix operators
            if let Some((l_bp, r_bp)) = infix_binding_power(kind) {
                if l_bp < min_bp {
                    break;
                }
                let op_tok = self.advance().clone();
                let op = token_to_binop(op_tok.kind);
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                lhs = Spanned::new(
                    Expr::Binary {
                        op,
                        left: Box::new(lhs),
                        right: Box::new(rhs),
                    },
                    span,
                );
                continue;
            }

            break;
        }

        Some(lhs)
    }

    fn parse_primary(&mut self) -> Option<SpannedExpr> {
        match self.peek_kind() {
            TokenKind::IntLiteral => {
                let tok = self.advance().clone();
                let (value, suffix) = parse_int_suffix(&tok.text);
                Some(Spanned::new(
                    Expr::Literal(Literal::Int(value, suffix)),
                    tok.span,
                ))
            }
            TokenKind::FloatLiteral => {
                let tok = self.advance().clone();
                Some(Spanned::new(
                    Expr::Literal(Literal::Float(tok.text.clone())),
                    tok.span,
                ))
            }
            TokenKind::StringLiteral => {
                let tok = self.advance().clone();
                Some(Spanned::new(
                    Expr::Literal(Literal::String(tok.text.clone())),
                    tok.span,
                ))
            }
            TokenKind::TemplateStringStart => self.parse_template_string(),
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
            TokenKind::If => self.parse_if_expr(),
            TokenKind::While => self.parse_while_expr(),
            TokenKind::Loop => self.parse_loop_expr(),
            TokenKind::Break => {
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
                Some(Spanned::new(Expr::Break(value), tok.span.merge(end)))
            }
            TokenKind::Continue => {
                let tok = self.advance().clone();
                Some(Spanned::new(Expr::Continue, tok.span))
            }
            TokenKind::LParen => {
                let start = self.advance().clone();
                // Check for unit literal ()
                if self.peek_kind() == TokenKind::RParen {
                    let end = self.advance();
                    let span = start.span.merge(end.span);
                    return Some(Spanned::new(
                        Expr::Literal(Literal::Bool(false)), // unit as expression — use Block
                        span,
                    ));
                }
                let inner = self.parse_expr()?;
                let end = self.expect(TokenKind::RParen)?.span;
                let span = start.span.merge(end);
                Some(Spanned::new(Expr::Grouped(Box::new(inner)), span))
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

    fn parse_if_expr(&mut self) -> Option<SpannedExpr> {
        let start = self.expect(TokenKind::If)?.span;
        let condition = self.parse_expr()?;
        let then_branch = self.parse_block()?;

        let else_branch = if self.eat(TokenKind::Else) {
            if self.peek_kind() == TokenKind::If {
                // else if ...
                let else_if = self.parse_if_expr()?;
                Some(Box::new(else_if))
            } else {
                // else { ... }
                let else_block = self.parse_block()?;
                let span = else_block.span;
                Some(Box::new(Spanned::new(
                    Expr::Block(else_block.node),
                    span,
                )))
            }
        } else {
            None
        };

        let end = else_branch
            .as_ref()
            .map(|e| e.span)
            .unwrap_or(then_branch.span);

        Some(Spanned::new(
            Expr::If {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch,
            },
            start.merge(end),
        ))
    }

    fn parse_while_expr(&mut self) -> Option<SpannedExpr> {
        let start = self.expect(TokenKind::While)?.span;
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;
        let end = body.span;

        Some(Spanned::new(
            Expr::While {
                condition: Box::new(condition),
                body: Box::new(body),
            },
            start.merge(end),
        ))
    }

    fn parse_loop_expr(&mut self) -> Option<SpannedExpr> {
        let start = self.expect(TokenKind::Loop)?.span;
        let body = self.parse_block()?;
        let end = body.span;

        Some(Spanned::new(
            Expr::Loop {
                body: Box::new(body),
            },
            start.merge(end),
        ))
    }

    fn parse_template_string(&mut self) -> Option<SpannedExpr> {
        let start_tok = self.advance().clone(); // TemplateStringStart
        let start = start_tok.span;
        let mut parts = vec![start_tok.text.clone()];
        let mut exprs = Vec::new();

        loop {
            // Parse the interpolated expression
            if let Some(expr) = self.parse_expr() {
                exprs.push(expr);
            }

            match self.peek_kind() {
                TokenKind::TemplateStringEnd => {
                    let end_tok = self.advance().clone();
                    parts.push(end_tok.text.clone());
                    let end = end_tok.span;
                    return Some(Spanned::new(
                        Expr::Literal(Literal::TemplateString { parts, exprs }),
                        start.merge(end),
                    ));
                }
                TokenKind::TemplateStringPart => {
                    let mid_tok = self.advance().clone();
                    parts.push(mid_tok.text.clone());
                    // continue to next interpolation
                }
                _ => {
                    // Error recovery: unexpected token
                    self.diagnostics.add(&ParserDiagnostic::UnexpectedToken {
                        span: self.peek().span,
                        expected: "template string continuation".into(),
                        found: self.peek().text.clone(),
                    });
                    return None;
                }
            }
        }
    }

    fn parse_call_args(&mut self) -> Vec<CallArg> {
        let mut args = Vec::new();
        while self.peek_kind() != TokenKind::RParen && !self.at_eof() {
            if let Some(expr) = self.parse_expr() {
                args.push(CallArg {
                    name: None,
                    value: expr,
                });
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        args
    }
}

/// Returns (left_bp, right_bp) for infix operators.
fn infix_binding_power(kind: TokenKind) -> Option<(u8, u8)> {
    match kind {
        // Logical or: left-associative
        TokenKind::PipePipe => Some((5, 6)),
        // Logical and: left-associative
        TokenKind::AmpAmp => Some((7, 8)),
        // Shift: left-associative
        TokenKind::LtLt | TokenKind::GtGt => Some((9, 10)),
        // Comparison: non-associative (use same bp on both sides)
        TokenKind::EqEq | TokenKind::BangEq | TokenKind::Lt | TokenKind::Gt
        | TokenKind::LtEq | TokenKind::GtEq => Some((11, 12)),
        // Addition/subtraction: left-associative
        TokenKind::Plus | TokenKind::Minus => Some((13, 14)),
        // Multiplication/division/modulo: left-associative
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Some((15, 16)),
        _ => None,
    }
}

/// Returns the right_bp for prefix operators.
fn prefix_binding_power(kind: TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Minus | TokenKind::Bang => Some(17),
        _ => None,
    }
}

/// Returns the binding power for assignment operators (right-associative).
fn assign_binding_power(kind: TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Eq | TokenKind::PlusEq | TokenKind::MinusEq | TokenKind::StarEq
        | TokenKind::SlashEq | TokenKind::PercentEq | TokenKind::LtLtEq
        | TokenKind::GtGtEq => Some(1),
        _ => None,
    }
}

fn token_to_binop(kind: TokenKind) -> BinOp {
    match kind {
        TokenKind::Plus => BinOp::Add,
        TokenKind::Minus => BinOp::Sub,
        TokenKind::Star => BinOp::Mul,
        TokenKind::Slash => BinOp::Div,
        TokenKind::Percent => BinOp::Mod,
        TokenKind::LtLt => BinOp::Shl,
        TokenKind::GtGt => BinOp::Shr,
        TokenKind::EqEq => BinOp::Eq,
        TokenKind::BangEq => BinOp::Ne,
        TokenKind::Lt => BinOp::Lt,
        TokenKind::LtEq => BinOp::Le,
        TokenKind::Gt => BinOp::Gt,
        TokenKind::GtEq => BinOp::Ge,
        TokenKind::AmpAmp => BinOp::And,
        TokenKind::PipePipe => BinOp::Or,
        _ => unreachable!("not a binop token: {:?}", kind),
    }
}

fn parse_int_suffix(text: &str) -> (String, Option<IntSuffix>) {
    const SUFFIXES: &[(&str, IntSuffix)] = &[
        ("i16", IntSuffix::I16),
        ("i32", IntSuffix::I32),
        ("i64", IntSuffix::I64),
        ("i8", IntSuffix::I8),
        ("u16", IntSuffix::U16),
        ("u32", IntSuffix::U32),
        ("u64", IntSuffix::U64),
        ("u8", IntSuffix::U8),
    ];
    for &(s, suffix) in SUFFIXES {
        if let Some(value) = text.strip_suffix(s) {
            // Make sure what remains before the suffix is not empty and ends
            // with a digit or underscore (not another letter).
            if !value.is_empty() && value.as_bytes().last().is_some_and(|&b| b.is_ascii_digit() || b == b'_') {
                return (value.to_string(), Some(suffix));
            }
        }
    }
    (text.to_string(), None)
}

fn compound_assign_op(kind: TokenKind) -> BinOp {
    match kind {
        TokenKind::PlusEq => BinOp::Add,
        TokenKind::MinusEq => BinOp::Sub,
        TokenKind::StarEq => BinOp::Mul,
        TokenKind::SlashEq => BinOp::Div,
        TokenKind::PercentEq => BinOp::Mod,
        TokenKind::LtLtEq => BinOp::Shl,
        TokenKind::GtGtEq => BinOp::Shr,
        _ => unreachable!("not a compound assign token: {:?}", kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use nudl_core::span::FileId;

    fn parse(source: &str) -> Module {
        let (tokens, lex_diags) = Lexer::new(source, FileId(0)).tokenize();
        assert!(
            !lex_diags.has_errors(),
            "lex errors: {:?}",
            lex_diags.reports()
        );
        let (module, parse_diags) = Parser::new(tokens).parse_module();
        assert!(
            !parse_diags.has_errors(),
            "parse errors: {:?}",
            parse_diags.reports()
        );
        module
    }

    #[test]
    fn parse_hello_world() {
        let module = parse(
            r#"fn main() {
    println("Hello, world!");
}"#,
        );
        assert_eq!(module.items.len(), 1);
        match &module.items[0].node {
            Item::FnDef {
                name,
                params,
                return_type,
                ..
            } => {
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
            Item::FnDef {
                name,
                params,
                return_type,
                ..
            } => {
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
        let module = parse(
            r#"extern "C" {
    fn write(fd: i32, buf: RawPtr, len: u64) -> i64;
}"#,
        );
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

    #[test]
    fn parse_binary_precedence() {
        let module = parse("fn main() { let x = 1 + 2 * 3; }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                let stmt = &body.node.stmts[0].node;
                match stmt {
                    Stmt::Let { value, .. } => match &value.node {
                        Expr::Binary { op, right, .. } => {
                            assert_eq!(*op, BinOp::Add);
                            match &right.node {
                                Expr::Binary { op, .. } => assert_eq!(*op, BinOp::Mul),
                                _ => panic!("expected Binary(Mul)"),
                            }
                        }
                        _ => panic!("expected Binary"),
                    },
                    _ => panic!("expected Let"),
                }
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_if_else() {
        let module = parse(
            r#"fn main() {
    if x > 0 { 1 } else { 2 }
}"#,
        );
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                // The if/else is a tail expression or statement
                let has_if = body.node.tail_expr.is_some()
                    || body.node.stmts.iter().any(|s| {
                        matches!(
                            &s.node,
                            Stmt::Expr(e) if matches!(&e.node, Expr::If { .. })
                        )
                    });
                assert!(has_if, "expected If expression in body");
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_while_loop() {
        let module = parse("fn main() { while x < 10 { x = x + 1; } }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                let in_stmts = body.node.stmts.iter().any(|s| {
                    matches!(
                        &s.node,
                        Stmt::Expr(e) if matches!(&e.node, Expr::While { .. })
                    )
                });
                let in_tail = body
                    .node
                    .tail_expr
                    .as_ref()
                    .is_some_and(|e| matches!(&e.node, Expr::While { .. }));
                assert!(in_stmts || in_tail, "expected While expression");
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_loop_break() {
        let module = parse("fn main() { loop { break; } }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                let in_stmts = body.node.stmts.iter().any(|s| {
                    matches!(
                        &s.node,
                        Stmt::Expr(e) if matches!(&e.node, Expr::Loop { .. })
                    )
                });
                let in_tail = body
                    .node
                    .tail_expr
                    .as_ref()
                    .is_some_and(|e| matches!(&e.node, Expr::Loop { .. }));
                assert!(in_stmts || in_tail, "expected Loop expression");
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_tail_expression() {
        let module = parse("fn foo() -> i32 { 42 }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                assert!(
                    body.node.tail_expr.is_some(),
                    "expected tail expression"
                );
                match &body.node.tail_expr.as_ref().unwrap().node {
                    Expr::Literal(Literal::Int(s, None)) => assert_eq!(s, "42"),
                    _ => panic!("expected Int literal tail"),
                }
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_assignment() {
        let module = parse("fn main() { x = 42; }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                let has_assign = body.node.stmts.iter().any(|s| {
                    matches!(
                        &s.node,
                        Stmt::Expr(e) if matches!(&e.node, Expr::Assign { .. })
                    )
                });
                assert!(has_assign, "expected Assign expression");
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_compound_assign() {
        let module = parse("fn main() { x += 1; }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                let has_compound = body.node.stmts.iter().any(|s| {
                    matches!(
                        &s.node,
                        Stmt::Expr(e) if matches!(&e.node, Expr::CompoundAssign { .. })
                    )
                });
                assert!(has_compound, "expected CompoundAssign expression");
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_unary_negation() {
        let module = parse("fn main() { let x = -42; }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => match &body.node.stmts[0].node {
                Stmt::Let { value, .. } => {
                    assert!(
                        matches!(&value.node, Expr::Unary { op: UnaryOp::Neg, .. }),
                        "expected Unary Neg"
                    );
                }
                _ => panic!("expected Let"),
            },
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_grouped_expression() {
        let module = parse("fn main() { let x = (1 + 2) * 3; }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => match &body.node.stmts[0].node {
                Stmt::Let { value, .. } => {
                    // Should be Mul at top level with grouped Add inside
                    assert!(
                        matches!(&value.node, Expr::Binary { op: BinOp::Mul, .. }),
                        "expected Mul at top"
                    );
                }
                _ => panic!("expected Let"),
            },
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_add_function_with_tail() {
        let module = parse("fn add(a: i32, b: i32) -> i32 { a + b }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                assert!(body.node.tail_expr.is_some());
                match &body.node.tail_expr.as_ref().unwrap().node {
                    Expr::Binary { op: BinOp::Add, .. } => {}
                    _ => panic!("expected Binary Add tail"),
                }
            }
            _ => panic!("expected FnDef"),
        }
    }
}
