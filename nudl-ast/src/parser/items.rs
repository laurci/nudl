use super::*;

impl Parser {
    pub(super) fn parse_item(&mut self) -> Option<SpannedItem> {
        let is_pub = self.eat(TokenKind::Pub);

        match self.peek_kind() {
            TokenKind::Fn => self.parse_fn_def(is_pub),
            TokenKind::Struct => self.parse_struct_def(is_pub),
            TokenKind::Impl => self.parse_impl_block(),
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

    fn parse_struct_def(&mut self, is_pub: bool) -> Option<SpannedItem> {
        let start = self.expect(TokenKind::Struct)?.span;
        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.text.clone();
        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
            let field_name_tok = self.expect(TokenKind::Ident)?;
            let field_start = field_name_tok.span;
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            let field_end = ty.span;
            fields.push(StructField {
                name: field_name_tok.text.clone(),
                ty,
                span: field_start.merge(field_end),
            });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Some(Spanned::new(
            Item::StructDef {
                name,
                fields,
                is_pub,
            },
            start.merge(end),
        ))
    }

    fn parse_impl_block(&mut self) -> Option<SpannedItem> {
        let start = self.expect(TokenKind::Impl)?.span;
        let type_name_tok = self.expect(TokenKind::Ident)?;
        let type_name = type_name_tok.text.clone();
        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
            if let Some(item) = self.parse_fn_def(false) {
                methods.push(item);
            } else {
                self.advance();
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Some(Spanned::new(
            Item::ImplBlock { type_name, methods },
            start.merge(end),
        ))
    }

    pub(super) fn parse_extern_block(&mut self) -> Option<SpannedItem> {
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

    pub(super) fn parse_param_list(&mut self) -> Vec<Param> {
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
        let is_mut = self.eat(TokenKind::Mut);

        // Handle `self` and `mut self` parameters
        if self.peek_kind() == TokenKind::Self_ {
            let self_tok = self.advance().clone();
            return Some(Param {
                name: "self".into(),
                // Placeholder type — the type checker will replace this with the actual struct type
                ty: Spanned::new(TypeExpr::Named("Self".into()), self_tok.span),
                is_mut,
                is_self: true,
                default_value: None,
                span: self_tok.span,
            });
        }

        let name_tok = self.expect(TokenKind::Ident)?;
        let start = name_tok.span;
        self.expect(TokenKind::Colon)?;
        let ty = self.parse_type()?;
        let mut end = ty.span;

        // Parse optional default value: `= expr`
        let default_value = if self.eat(TokenKind::Eq) {
            let val = self.parse_expr()?;
            end = val.span;
            Some(Box::new(val))
        } else {
            None
        };

        Some(Param {
            name: name_tok.text.clone(),
            ty,
            is_mut,
            is_self: false,
            default_value,
            span: start.merge(end),
        })
    }

    pub(super) fn parse_type(&mut self) -> Option<Spanned<TypeExpr>> {
        if self.peek_kind() == TokenKind::LParen {
            let start = self.advance().span;
            // Unit type: ()
            if self.peek_kind() == TokenKind::RParen {
                let end = self.advance().span;
                return Some(Spanned::new(TypeExpr::Unit, start.merge(end)));
            }
            // Tuple type: (T1, T2, ...)
            let mut elements = Vec::new();
            elements.push(self.parse_type()?);
            while self.eat(TokenKind::Comma) {
                if self.peek_kind() == TokenKind::RParen {
                    break; // trailing comma
                }
                elements.push(self.parse_type()?);
            }
            let end = self.expect(TokenKind::RParen)?.span;
            return Some(Spanned::new(TypeExpr::Tuple(elements), start.merge(end)));
        }

        // Fixed array type: [T; N]
        if self.peek_kind() == TokenKind::LBracket {
            let start = self.advance().span;
            let element = self.parse_type()?;
            self.expect(TokenKind::Semi)?;
            let len_tok = self.expect(TokenKind::IntLiteral)?;
            let length: usize = len_tok.text.parse().unwrap_or(0);
            let end = self.expect(TokenKind::RBracket)?.span;
            return Some(Spanned::new(
                TypeExpr::FixedArray {
                    element: Box::new(element),
                    length,
                },
                start.merge(end),
            ));
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
}
