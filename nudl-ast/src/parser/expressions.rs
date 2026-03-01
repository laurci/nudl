use super::binding_power::*;
use super::*;

impl Parser {
    pub(super) fn parse_expr(&mut self) -> Option<SpannedExpr> {
        self.parse_expr_bp(0)
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> Option<SpannedExpr> {
        // Parse prefix or primary
        let mut lhs = if let Some(right_bp) = prefix_binding_power(self.peek_kind()) {
            let op_tok = self.advance().clone();
            let op = match op_tok.kind {
                TokenKind::Minus => UnaryOp::Neg,
                TokenKind::Bang => UnaryOp::Not,
                TokenKind::Tilde => UnaryOp::BitNot,
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

            // Postfix: field access (dot) — handles named fields, tuple .0/.1, and method calls
            if kind == TokenKind::Dot {
                let dot_tok = self.advance().clone();
                // Tuple element access: .0, .1, etc.
                if self.peek_kind() == TokenKind::IntLiteral {
                    let idx_tok = self.advance().clone();
                    let span = lhs.span.merge(idx_tok.span);
                    lhs = Spanned::new(
                        Expr::FieldAccess {
                            object: Box::new(lhs),
                            field: idx_tok.text.clone(),
                        },
                        span,
                    );
                } else {
                    let field_tok = self.expect(TokenKind::Ident)?;
                    // Check for turbofish method call: `obj.method::<T>(args)`
                    let method_type_args = if self.peek_kind() == TokenKind::ColonColon
                        && self.peek_nth(1).kind == TokenKind::Lt
                    {
                        self.advance(); // consume '::'
                        self.parse_type_args()
                    } else {
                        Vec::new()
                    };
                    // Check for method call: `obj.method(args)` or `obj.method::<T>(args)`
                    if self.peek_kind() == TokenKind::LParen {
                        let start = lhs.span;
                        self.advance(); // consume '('
                        let mut args = self.parse_call_args();
                        let mut end = self.expect(TokenKind::RParen)?.span;
                        // Trailing lambda: obj.method(args) |params| body
                        if let Some(trailing) = self.try_parse_trailing_lambda() {
                            end = trailing.span;
                            args.push(CallArg {
                                name: None,
                                value: trailing,
                            });
                        }
                        lhs = Spanned::new(
                            Expr::MethodCall {
                                object: Box::new(lhs),
                                method: field_tok.text.clone(),
                                args,
                                type_args: method_type_args,
                            },
                            start.merge(end),
                        );
                    } else {
                        let span = lhs.span.merge(field_tok.span);
                        lhs = Spanned::new(
                            Expr::FieldAccess {
                                object: Box::new(lhs),
                                field: field_tok.text.clone(),
                            },
                            span,
                        );
                    }
                }
                let _ = dot_tok;
                continue;
            }

            // Postfix: index access (brackets)
            if kind == TokenKind::LBracket {
                let start = lhs.span;
                self.advance(); // consume [
                let index = self.parse_expr()?;
                let end = self.expect(TokenKind::RBracket)?.span;
                lhs = Spanned::new(
                    Expr::IndexAccess {
                        object: Box::new(lhs),
                        index: Box::new(index),
                    },
                    start.merge(end),
                );
                continue;
            }

            // Postfix: call expressions
            // Block-like expressions (for, while, loop, if, block) cannot be callees.
            // Without this check, `for i in 1..5 { body } (lo, hi)` would be
            // misparsed as Call(For(...), [lo, hi]).
            if kind == TokenKind::LParen {
                let is_block_like = matches!(
                    lhs.node,
                    Expr::For { .. }
                        | Expr::While { .. }
                        | Expr::Loop { .. }
                        | Expr::If { .. }
                        | Expr::Block(_)
                );
                if is_block_like {
                    break;
                }
                let start = lhs.span;
                self.advance(); // (
                let mut args = self.parse_call_args();
                let mut end = self.expect(TokenKind::RParen)?.span;
                // Trailing lambda: func(args) |params| body or func(args) { body }
                if let Some(trailing) = self.try_parse_trailing_lambda() {
                    end = trailing.span;
                    args.push(CallArg {
                        name: None,
                        value: trailing,
                    });
                }
                lhs = Spanned::new(
                    Expr::Call {
                        callee: Box::new(lhs),
                        args,
                        type_args: Vec::new(),
                    },
                    start.merge(end),
                );
                continue;
            }

            // Postfix: `as` type cast
            if kind == TokenKind::As {
                // `as` has very high precedence (between infix and unary)
                let as_bp: u8 = 24;
                if as_bp < min_bp {
                    break;
                }
                self.advance(); // consume `as`
                let target_type = self.parse_type()?;
                let span = lhs.span.merge(target_type.span);
                lhs = Spanned::new(
                    Expr::Cast {
                        expr: Box::new(lhs),
                        target_type,
                    },
                    span,
                );
                continue;
            }

            // Postfix: `?` error propagation operator
            if kind == TokenKind::Question {
                let q_bp: u8 = 24; // same precedence as `as`
                if q_bp < min_bp {
                    break;
                }
                let q_tok = self.advance().clone();
                let span = lhs.span.merge(q_tok.span);
                lhs = Spanned::new(Expr::QuestionMark(Box::new(lhs)), span);
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

                // Range operators: `a..b` or `a..=b`
                if op_tok.kind == TokenKind::DotDot || op_tok.kind == TokenKind::DotDotEq {
                    let inclusive = op_tok.kind == TokenKind::DotDotEq;
                    let rhs = self.parse_expr_bp(r_bp)?;
                    let span = lhs.span.merge(rhs.span);
                    lhs = Spanned::new(
                        Expr::Range {
                            start: Box::new(lhs),
                            end: Box::new(rhs),
                            inclusive,
                        },
                        span,
                    );
                    continue;
                }

                // Pipe operator desugaring: `x |> f` → `f(x)`, `x |> f(y)` → `f(x, y)`
                if op_tok.kind == TokenKind::PipeGt {
                    let rhs = self.parse_expr_bp(r_bp)?;
                    let span = lhs.span.merge(rhs.span);
                    lhs = match rhs.node {
                        // `x |> f(y, z)` → `f(x, y, z)` — prepend lhs as first arg
                        Expr::Call {
                            callee,
                            mut args,
                            type_args,
                        } => {
                            args.insert(
                                0,
                                CallArg {
                                    name: None,
                                    value: lhs,
                                },
                            );
                            Spanned::new(
                                Expr::Call {
                                    callee,
                                    args,
                                    type_args,
                                },
                                span,
                            )
                        }
                        // `x |> f` → `f(x)` — wrap in call
                        _ => Spanned::new(
                            Expr::Call {
                                callee: Box::new(rhs),
                                args: vec![CallArg {
                                    name: None,
                                    value: lhs,
                                }],
                                type_args: Vec::new(),
                            },
                            span,
                        ),
                    };
                    // Attach trailing closure: `x |> f |params| body` → `f(x, |params| body)`
                    if let Some(trailing) = self.try_parse_trailing_lambda() {
                        if let Expr::Call { ref mut args, .. } = lhs.node {
                            let end = trailing.span;
                            args.push(CallArg {
                                name: None,
                                value: trailing,
                            });
                            lhs.span = lhs.span.merge(end);
                        }
                    }
                    continue;
                }

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
            TokenKind::Self_ => {
                // `self` used as an expression (inside methods)
                let tok = self.advance().clone();
                Some(Spanned::new(Expr::Ident("self".into()), tok.span))
            }
            TokenKind::Ident => {
                // Lookahead: if followed by `{` and (`ident :` or `ident ,` or `ident }` or `}`), parse as struct literal
                // Skip when inhibit_trailing_lambda is set — we're inside a condition/iterator
                // of if/while/for where `{` starts a block body, not a struct literal.
                if !self.inhibit_trailing_lambda
                    && self.peek_nth(1).kind == TokenKind::LBrace
                    && (self.peek_nth(2).kind == TokenKind::RBrace
                        || (self.peek_nth(2).kind == TokenKind::Ident
                            && (self.peek_nth(3).kind == TokenKind::Colon
                                || self.peek_nth(3).kind == TokenKind::Comma
                                || self.peek_nth(3).kind == TokenKind::RBrace)))
                {
                    return self.parse_struct_literal();
                }
                // Path-based expressions: Type::member
                if self.peek_nth(1).kind == TokenKind::ColonColon
                    && self.peek_nth(2).kind == TokenKind::Ident
                {
                    let type_tok = self.advance().clone();
                    let start = type_tok.span;
                    self.advance(); // consume '::'
                    let member_tok = self.advance().clone();

                    // Check for turbofish: Type::method::<T>(args)
                    let static_type_args = if self.peek_kind() == TokenKind::ColonColon
                        && self.peek_nth(1).kind == TokenKind::Lt
                    {
                        self.advance(); // consume '::'
                        self.parse_type_args()
                    } else {
                        Vec::new()
                    };

                    if self.peek_kind() == TokenKind::LParen {
                        // Call: Type::method(args) or Enum::Variant(args)
                        self.advance(); // consume '('
                        let mut args = self.parse_call_args();
                        let mut end = self.expect(TokenKind::RParen)?.span;
                        // Trailing lambda: Type::method(args) |params| body
                        if let Some(trailing) = self.try_parse_trailing_lambda() {
                            end = trailing.span;
                            args.push(CallArg {
                                name: None,
                                value: trailing,
                            });
                        }
                        return Some(Spanned::new(
                            Expr::StaticCall {
                                type_name: type_tok.text.clone(),
                                method: member_tok.text.clone(),
                                args,
                                type_args: static_type_args,
                            },
                            start.merge(end),
                        ));
                    } else if self.peek_kind() == TokenKind::LBrace
                        && (self.peek_nth(1).kind == TokenKind::RBrace
                            || (self.peek_nth(1).kind == TokenKind::Ident
                                && (self.peek_nth(2).kind == TokenKind::Colon
                                    || self.peek_nth(2).kind == TokenKind::Comma
                                    || self.peek_nth(2).kind == TokenKind::RBrace)))
                    {
                        // Enum struct variant: Enum::Variant { field: val }
                        self.advance(); // consume '{'
                        let mut fields = Vec::new();
                        while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
                            let field_name_tok = self.expect(TokenKind::Ident)?;
                            let field_name = field_name_tok.text.clone();
                            if self.peek_kind() == TokenKind::Colon {
                                self.advance();
                                let value = self.parse_expr()?;
                                fields.push((field_name, value));
                            } else {
                                let value = Spanned::new(
                                    Expr::Ident(field_name.clone()),
                                    field_name_tok.span,
                                );
                                fields.push((field_name, value));
                            }
                            if !self.eat(TokenKind::Comma) {
                                break;
                            }
                        }
                        let end = self.expect(TokenKind::RBrace)?.span;
                        return Some(Spanned::new(
                            Expr::StructLiteral {
                                name: format!("{}::{}", type_tok.text, member_tok.text),
                                fields,
                            },
                            start.merge(end),
                        ));
                    } else {
                        // Unit variant or path access: Enum::Variant (no parens)
                        let end = member_tok.span;
                        return Some(Spanned::new(
                            Expr::EnumLiteral {
                                enum_name: type_tok.text.clone(),
                                variant: member_tok.text.clone(),
                                args: Vec::new(),
                            },
                            start.merge(end),
                        ));
                    }
                }
                // Turbofish on plain function call: func::<T>(args)
                if self.peek_nth(1).kind == TokenKind::ColonColon
                    && self.peek_nth(2).kind == TokenKind::Lt
                {
                    let tok = self.advance().clone();
                    let start = tok.span;
                    self.advance(); // consume '::'
                    let turbo_type_args = self.parse_type_args();
                    if self.peek_kind() == TokenKind::LParen {
                        self.advance(); // consume '('
                        let mut args = self.parse_call_args();
                        let mut end = self.expect(TokenKind::RParen)?.span;
                        if let Some(trailing) = self.try_parse_trailing_lambda() {
                            end = trailing.span;
                            args.push(CallArg {
                                name: None,
                                value: trailing,
                            });
                        }
                        return Some(Spanned::new(
                            Expr::Call {
                                callee: Box::new(Spanned::new(
                                    Expr::Ident(tok.text.clone()),
                                    tok.span,
                                )),
                                args,
                                type_args: turbo_type_args,
                            },
                            start.merge(end),
                        ));
                    }
                    // If no parens after turbofish, fall back to just ident
                    return Some(Spanned::new(Expr::Ident(tok.text.clone()), tok.span));
                }
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
            TokenKind::Match => self.parse_match_expr(),
            TokenKind::If => self.parse_if_expr(),
            TokenKind::While => self.parse_while_expr(None),
            TokenKind::Loop => self.parse_loop_expr(None),
            TokenKind::Label => {
                // Labeled loop: 'label: loop { ... } or 'label: while ... { ... }
                let label_tok = self.advance().clone();
                let label = label_tok.text.clone();
                self.expect(TokenKind::Colon)?;
                match self.peek_kind() {
                    TokenKind::Loop => self.parse_loop_expr(Some(label)),
                    TokenKind::While => self.parse_while_expr(Some(label)),
                    _ => {
                        self.diagnostics.add(&ParserDiagnostic::UnexpectedToken {
                            span: self.peek().span,
                            expected: "'loop' or 'while' after label".into(),
                            found: self.peek().text.clone(),
                        });
                        None
                    }
                }
            }
            TokenKind::Break => {
                let tok = self.advance().clone();
                // Optional label: break 'label
                let label = if self.peek_kind() == TokenKind::Label {
                    let label_tok = self.advance().clone();
                    Some(label_tok.text.clone())
                } else {
                    None
                };
                let value = if self.peek_kind() != TokenKind::Semi
                    && self.peek_kind() != TokenKind::RBrace
                    && !self.at_eof()
                {
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                };
                let end = value.as_ref().map(|v| v.span).unwrap_or(tok.span);
                Some(Spanned::new(
                    Expr::Break { label, value },
                    tok.span.merge(end),
                ))
            }
            TokenKind::Continue => {
                let tok = self.advance().clone();
                // Optional label: continue 'label
                let label = if self.peek_kind() == TokenKind::Label {
                    let label_tok = self.advance().clone();
                    Some(label_tok.text.clone())
                } else {
                    None
                };
                let end = tok.span;
                Some(Spanned::new(Expr::Continue { label }, tok.span.merge(end)))
            }
            TokenKind::LParen => {
                let start = self.advance().clone();
                // Unit literal: ()
                if self.peek_kind() == TokenKind::RParen {
                    let end = self.advance();
                    let span = start.span.merge(end.span);
                    return Some(Spanned::new(Expr::TupleLiteral(vec![]), span));
                }
                let first = self.parse_expr()?;
                // If followed by comma, this is a tuple literal
                if self.peek_kind() == TokenKind::Comma {
                    let mut elements = vec![first];
                    while self.eat(TokenKind::Comma) {
                        if self.peek_kind() == TokenKind::RParen {
                            break; // trailing comma
                        }
                        elements.push(self.parse_expr()?);
                    }
                    let end = self.expect(TokenKind::RParen)?.span;
                    let span = start.span.merge(end);
                    Some(Spanned::new(Expr::TupleLiteral(elements), span))
                } else {
                    // Grouped expression
                    let end = self.expect(TokenKind::RParen)?.span;
                    let span = start.span.merge(end);
                    Some(Spanned::new(Expr::Grouped(Box::new(first)), span))
                }
            }
            TokenKind::LBracket => {
                let start = self.advance().clone(); // consume [
                // Empty array: []
                if self.peek_kind() == TokenKind::RBracket {
                    let end = self.advance().span;
                    return Some(Spanned::new(
                        Expr::ArrayLiteral(vec![]),
                        start.span.merge(end),
                    ));
                }
                let first = self.parse_expr()?;
                // Array repeat: [expr; N]
                if self.peek_kind() == TokenKind::Semi {
                    self.advance(); // consume ;
                    let count_tok = self.expect(TokenKind::IntLiteral)?;
                    let count: usize = count_tok.text.parse().unwrap_or(0);
                    let end = self.expect(TokenKind::RBracket)?.span;
                    return Some(Spanned::new(
                        Expr::ArrayRepeat {
                            value: Box::new(first),
                            count,
                        },
                        start.span.merge(end),
                    ));
                }
                // Array literal: [expr, expr, ...]
                let mut elements = vec![first];
                while self.eat(TokenKind::Comma) {
                    if self.peek_kind() == TokenKind::RBracket {
                        break; // trailing comma
                    }
                    elements.push(self.parse_expr()?);
                }
                let end = self.expect(TokenKind::RBracket)?.span;
                Some(Spanned::new(
                    Expr::ArrayLiteral(elements),
                    start.span.merge(end),
                ))
            }
            TokenKind::For => self.parse_for_expr(),
            TokenKind::LBrace => {
                let block = self.parse_block()?;
                let span = block.span;
                Some(Spanned::new(Expr::Block(block.node), span))
            }
            // Closure: |params| body or || body
            TokenKind::Pipe => self.parse_closure(),
            TokenKind::PipePipe => {
                // || is a zero-parameter closure shorthand
                let tok = self.advance().clone();
                let start = tok.span;
                // Parse optional return type
                let return_type = if self.peek_kind() == TokenKind::Arrow {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                // Parse body: either block or single expression
                let body = if self.peek_kind() == TokenKind::LBrace {
                    let block = self.parse_block()?;
                    let span = block.span;
                    Box::new(Spanned::new(Expr::Block(block.node), span))
                } else {
                    Box::new(self.parse_expr()?)
                };
                let end = body.span;
                Some(Spanned::new(
                    Expr::Closure {
                        params: vec![],
                        return_type,
                        body,
                    },
                    start.merge(end),
                ))
            }
            _ => {
                self.diagnostics.add(&ParserDiagnostic::ExpectedExpression {
                    span: self.peek().span,
                });
                None
            }
        }
    }

    fn parse_struct_literal(&mut self) -> Option<SpannedExpr> {
        let name_tok = self.expect(TokenKind::Ident)?;
        let start = name_tok.span;
        let name = name_tok.text.clone();
        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
            let field_name_tok = self.expect(TokenKind::Ident)?;
            let field_name = field_name_tok.text.clone();
            // Support field shorthand: `Foo { x }` is equivalent to `Foo { x: x }`
            if self.peek_kind() == TokenKind::Colon {
                self.advance(); // consume ':'
                let value = self.parse_expr()?;
                fields.push((field_name, value));
            } else {
                // Shorthand: the field name is also used as a variable reference
                let value = Spanned::new(Expr::Ident(field_name.clone()), field_name_tok.span);
                fields.push((field_name, value));
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Some(Spanned::new(
            Expr::StructLiteral { name, fields },
            start.merge(end),
        ))
    }

    fn parse_match_expr(&mut self) -> Option<SpannedExpr> {
        let start = self.expect(TokenKind::Match)?.span;
        let expr = self.parse_expr()?;
        self.expect(TokenKind::LBrace)?;

        let mut arms = Vec::new();
        while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
            let pattern = self.parse_pattern()?;

            // Optional guard: `if condition`
            let guard = if self.eat(TokenKind::If) {
                Some(self.parse_expr()?)
            } else {
                None
            };

            self.expect(TokenKind::FatArrow)?;
            let body = self.parse_expr()?;

            arms.push(MatchArm {
                pattern,
                guard,
                body,
            });

            if !self.eat(TokenKind::Comma) {
                break;
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Some(Spanned::new(
            Expr::Match {
                expr: Box::new(expr),
                arms,
            },
            start.merge(end),
        ))
    }

    fn parse_if_expr(&mut self) -> Option<SpannedExpr> {
        let start = self.expect(TokenKind::If)?.span;

        // Check for `if let` pattern
        if self.peek_kind() == TokenKind::Let {
            self.advance(); // consume 'let'
            let pattern = self.parse_pattern()?;
            self.expect(TokenKind::Eq)?;
            let saved = self.inhibit_trailing_lambda;
            self.inhibit_trailing_lambda = true;
            let expr = self.parse_expr()?;
            self.inhibit_trailing_lambda = saved;
            let then_branch = self.parse_block()?;

            let else_branch = if self.eat(TokenKind::Else) {
                if self.peek_kind() == TokenKind::If {
                    let else_if = self.parse_if_expr()?;
                    Some(Box::new(else_if))
                } else {
                    let else_block = self.parse_block()?;
                    let span = else_block.span;
                    Some(Box::new(Spanned::new(Expr::Block(else_block.node), span)))
                }
            } else {
                None
            };

            let end = else_branch
                .as_ref()
                .map(|e| e.span)
                .unwrap_or(then_branch.span);

            return Some(Spanned::new(
                Expr::IfLet {
                    pattern,
                    expr: Box::new(expr),
                    then_branch: Box::new(then_branch),
                    else_branch,
                },
                start.merge(end),
            ));
        }

        let saved = self.inhibit_trailing_lambda;
        self.inhibit_trailing_lambda = true;
        let condition = self.parse_expr()?;
        self.inhibit_trailing_lambda = saved;
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
                Some(Box::new(Spanned::new(Expr::Block(else_block.node), span)))
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

    fn parse_while_expr(&mut self, label: Option<String>) -> Option<SpannedExpr> {
        let start = self.expect(TokenKind::While)?.span;
        let saved = self.inhibit_trailing_lambda;
        self.inhibit_trailing_lambda = true;
        let condition = self.parse_expr()?;
        self.inhibit_trailing_lambda = saved;
        let body = self.parse_block()?;
        let end = body.span;

        Some(Spanned::new(
            Expr::While {
                label,
                condition: Box::new(condition),
                body: Box::new(body),
            },
            start.merge(end),
        ))
    }

    fn parse_loop_expr(&mut self, label: Option<String>) -> Option<SpannedExpr> {
        let start = self.expect(TokenKind::Loop)?.span;
        let body = self.parse_block()?;
        let end = body.span;

        Some(Spanned::new(
            Expr::Loop {
                label,
                body: Box::new(body),
            },
            start.merge(end),
        ))
    }

    fn parse_for_expr(&mut self) -> Option<SpannedExpr> {
        let start = self.expect(TokenKind::For)?.span;
        let binding_tok = self.expect(TokenKind::Ident)?;
        let binding = Spanned::new(binding_tok.text.clone(), binding_tok.span);
        self.expect(TokenKind::In)?;
        let saved = self.inhibit_trailing_lambda;
        self.inhibit_trailing_lambda = true;
        let iter = self.parse_expr()?;
        self.inhibit_trailing_lambda = saved;
        let body = self.parse_block()?;
        let end = body.span;

        Some(Spanned::new(
            Expr::For {
                binding,
                iter: Box::new(iter),
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

    pub(super) fn parse_call_args(&mut self) -> Vec<CallArg> {
        let mut args = Vec::new();
        while self.peek_kind() != TokenKind::RParen && !self.at_eof() {
            // Check for named argument: `ident: expr`
            // We look ahead for Ident followed by Colon (but not ColonColon which is path separator)
            if self.peek_kind() == TokenKind::Ident && self.peek_nth(1).kind == TokenKind::Colon {
                let name_tok = self.advance().clone();
                self.advance(); // consume ':'
                if let Some(value) = self.parse_expr() {
                    args.push(CallArg {
                        name: Some(name_tok.text.clone()),
                        value,
                    });
                }
            } else if let Some(expr) = self.parse_expr() {
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

    /// Try to parse a trailing lambda after a call expression.
    /// Supports: `func(args) |params| body` and `func(args) { body }` (implicit `it` parameter).
    fn try_parse_trailing_lambda(&mut self) -> Option<SpannedExpr> {
        if self.inhibit_trailing_lambda {
            return None;
        }
        match self.peek_kind() {
            TokenKind::Pipe => self.parse_closure(),
            TokenKind::PipePipe => {
                // || body — zero-parameter trailing closure
                let tok = self.advance().clone();
                let start = tok.span;
                let return_type = if self.peek_kind() == TokenKind::Arrow {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                let body = if self.peek_kind() == TokenKind::LBrace {
                    let block = self.parse_block()?;
                    let span = block.span;
                    Box::new(Spanned::new(Expr::Block(block.node), span))
                } else {
                    Box::new(self.parse_expr()?)
                };
                let end = body.span;
                Some(Spanned::new(
                    Expr::Closure {
                        params: vec![],
                        return_type,
                        body,
                    },
                    start.merge(end),
                ))
            }
            TokenKind::LBrace => {
                // Implicit `it` parameter: func(args) { body }
                let block = self.parse_block()?;
                let span = block.span;
                Some(Spanned::new(
                    Expr::Closure {
                        params: vec![ClosureParam {
                            name: "it".to_string(),
                            ty: None,
                            span,
                        }],
                        return_type: None,
                        body: Box::new(Spanned::new(Expr::Block(block.node), span)),
                    },
                    span,
                ))
            }
            _ => None,
        }
    }

    /// Parse a closure: |params| body or |params| -> Type { body }
    fn parse_closure(&mut self) -> Option<SpannedExpr> {
        let start = self.expect(TokenKind::Pipe)?.span;

        let mut params = Vec::new();
        while self.peek_kind() != TokenKind::Pipe && !self.at_eof() {
            let param_span = self.peek().span;
            let name = self.expect(TokenKind::Ident)?.text.clone();
            let ty = if self.eat(TokenKind::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };
            params.push(ClosureParam {
                name,
                ty,
                span: param_span,
            });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::Pipe)?;

        // Optional return type: -> Type
        let return_type = if self.peek_kind() == TokenKind::Arrow {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        // Parse body: block or single expression
        let body = if self.peek_kind() == TokenKind::LBrace {
            let block = self.parse_block()?;
            let span = block.span;
            Box::new(Spanned::new(Expr::Block(block.node), span))
        } else {
            Box::new(self.parse_expr()?)
        };

        let end = body.span;
        Some(Spanned::new(
            Expr::Closure {
                params,
                return_type,
                body,
            },
            start.merge(end),
        ))
    }
}
