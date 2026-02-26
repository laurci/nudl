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
}
