use super::*;

impl Parser {
    pub(super) fn parse_block(&mut self) -> Option<Spanned<Block>> {
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
                TokenKind::Const => {
                    if let Some(stmt) = self.parse_const_stmt() {
                        stmts.push(stmt);
                    } else {
                        self.advance();
                    }
                }
                TokenKind::Defer => {
                    if let Some(stmt) = self.parse_defer_stmt() {
                        stmts.push(stmt);
                    } else {
                        self.advance();
                    }
                }
                TokenKind::Fn | TokenKind::Struct | TokenKind::Pub | TokenKind::Extern => {
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

        Some(Spanned::new(Block { stmts, tail_expr }, start.merge(end)))
    }

    fn parse_let_stmt(&mut self) -> Option<SpannedStmt> {
        let start = self.expect(TokenKind::Let)?.span;
        let is_mut = self.eat(TokenKind::Mut);

        // Check for pattern-based destructuring: let (a, b) = ... or let Foo { x, y } = ...
        if self.peek_kind() == TokenKind::LParen {
            // Tuple destructuring: let (a, b, c) = expr;
            let pattern = self.parse_pattern()?;
            let ty = if self.eat(TokenKind::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };
            self.expect(TokenKind::Eq)?;
            let value = self.parse_expr()?;
            let end = value.span;
            self.eat(TokenKind::Semi);
            return Some(Spanned::new(
                Stmt::LetPattern {
                    pattern,
                    ty,
                    value,
                    is_mut,
                },
                start.merge(end),
            ));
        }

        // Check for struct destructuring: let Foo { x, y } = expr;
        // This is: Ident followed by LBrace, and the ident is a known struct name
        // We'll use a heuristic: if Ident is followed by { and (Ident : or Ident , or Ident } or })
        if self.peek_kind() == TokenKind::Ident
            && self.peek_nth(1).kind == TokenKind::LBrace
            && (self.peek_nth(2).kind == TokenKind::RBrace
                || (self.peek_nth(2).kind == TokenKind::Ident
                    && (self.peek_nth(3).kind == TokenKind::Colon
                        || self.peek_nth(3).kind == TokenKind::Comma
                        || self.peek_nth(3).kind == TokenKind::RBrace)))
        {
            // Check if the name is uppercase (struct name heuristic)
            let name = &self.peek().text;
            if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                let pattern = self.parse_struct_pattern()?;
                let ty = if self.eat(TokenKind::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(TokenKind::Eq)?;
                let value = self.parse_expr()?;
                let end = value.span;
                self.eat(TokenKind::Semi);
                return Some(Spanned::new(
                    Stmt::LetPattern {
                        pattern,
                        ty,
                        value,
                        is_mut,
                    },
                    start.merge(end),
                ));
            }
        }

        // Check for wildcard: let _ = expr;
        if self.peek_kind() == TokenKind::Underscore {
            let pattern = self.parse_pattern()?;
            let ty = if self.eat(TokenKind::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };
            self.expect(TokenKind::Eq)?;
            let value = self.parse_expr()?;
            let end = value.span;
            self.eat(TokenKind::Semi);
            return Some(Spanned::new(
                Stmt::LetPattern {
                    pattern,
                    ty,
                    value,
                    is_mut,
                },
                start.merge(end),
            ));
        }

        // Simple let: let name = expr;
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

    fn parse_const_stmt(&mut self) -> Option<SpannedStmt> {
        let start = self.expect(TokenKind::Const)?.span;
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
            Stmt::Const { name, ty, value },
            start.merge(end),
        ))
    }

    fn parse_defer_stmt(&mut self) -> Option<SpannedStmt> {
        let start = self.expect(TokenKind::Defer)?.span;
        let body = self.parse_block()?;
        let end = body.span;
        Some(Spanned::new(Stmt::Defer { body }, start.merge(end)))
    }
}
