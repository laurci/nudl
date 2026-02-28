use super::*;

impl Parser {
    pub(super) fn parse_item(&mut self) -> Option<SpannedItem> {
        // Handle import at the top level (before pub)
        if self.peek_kind() == TokenKind::Import {
            return self.parse_import();
        }

        let is_pub = self.eat(TokenKind::Pub);

        match self.peek_kind() {
            TokenKind::Fn => self.parse_fn_def(is_pub),
            TokenKind::Struct => self.parse_struct_def(is_pub, false),
            TokenKind::Enum => self.parse_enum_def(is_pub),
            TokenKind::Interface => self.parse_interface_def(is_pub),
            TokenKind::Impl => self.parse_impl_block(),
            TokenKind::Extern => {
                // Check if `extern struct` (extern value-type struct)
                if self.peek_nth(1).kind == TokenKind::Struct {
                    self.advance(); // consume `extern`
                    self.parse_struct_def(is_pub, true)
                } else {
                    self.parse_extern_block()
                }
            }
            TokenKind::Type => self.parse_type_alias(is_pub),
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

        // Optional type parameters: fn foo<T, U>
        let type_params = self.parse_optional_type_params();

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
                type_params,
                params,
                return_type,
                body,
                is_pub,
            },
            start.merge(end),
        ))
    }

    fn parse_struct_def(&mut self, is_pub: bool, is_extern: bool) -> Option<SpannedItem> {
        let start = self.expect(TokenKind::Struct)?.span;
        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.text.clone();

        let type_params = self.parse_optional_type_params();

        // Extern structs must not have type params
        if is_extern && !type_params.is_empty() {
            self.diagnostics.add(&ParserDiagnostic::UnexpectedToken {
                span: start,
                expected: "extern struct cannot have type parameters".into(),
                found: name.clone(),
            });
        }

        if self.peek_kind() == TokenKind::Semi {
            // Unit struct: `struct Foo;`
            let end = self.advance().span;
            return Some(Spanned::new(
                Item::StructDef {
                    name,
                    type_params,
                    fields: Vec::new(),
                    is_pub,
                    is_extern,
                },
                start.merge(end),
            ));
        }

        if self.peek_kind() == TokenKind::LParen {
            // Tuple struct: `struct Foo(T1, T2);`
            self.advance(); // consume '('
            let mut fields = Vec::new();
            let mut idx = 0;
            while self.peek_kind() != TokenKind::RParen && !self.at_eof() {
                let ty = self.parse_type()?;
                let span = ty.span;
                fields.push(StructField {
                    name: format!("{}", idx),
                    ty,
                    span,
                    is_pub: false,
                });
                idx += 1;
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            let mut end = self.expect(TokenKind::RParen)?.span;
            // Optional semicolon
            if self.peek_kind() == TokenKind::Semi {
                end = self.advance().span;
            }
            return Some(Spanned::new(
                Item::StructDef {
                    name,
                    type_params,
                    fields,
                    is_pub,
                    is_extern,
                },
                start.merge(end),
            ));
        }

        // Named struct: `struct Foo { field: T, ... }`
        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
            let field_is_pub = self.eat(TokenKind::Pub);
            let field_name_tok = self.expect(TokenKind::Ident)?;
            let field_start = field_name_tok.span;
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            let field_end = ty.span;
            fields.push(StructField {
                name: field_name_tok.text.clone(),
                ty,
                span: field_start.merge(field_end),
                is_pub: field_is_pub,
            });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Some(Spanned::new(
            Item::StructDef {
                name,
                type_params,
                fields,
                is_pub,
                is_extern,
            },
            start.merge(end),
        ))
    }

    fn parse_enum_def(&mut self, is_pub: bool) -> Option<SpannedItem> {
        let start = self.expect(TokenKind::Enum)?.span;
        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.text.clone();

        let type_params = self.parse_optional_type_params();

        self.expect(TokenKind::LBrace)?;

        let mut variants = Vec::new();
        while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
            let var_tok = self.expect(TokenKind::Ident)?;
            let var_start = var_tok.span;
            let var_name = var_tok.text.clone();

            let kind = if self.peek_kind() == TokenKind::LParen {
                // Tuple variant: Variant(T1, T2, ...)
                self.advance();
                let mut types = Vec::new();
                while self.peek_kind() != TokenKind::RParen && !self.at_eof() {
                    types.push(self.parse_type()?);
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RParen)?;
                VariantKind::Tuple(types)
            } else if self.peek_kind() == TokenKind::LBrace {
                // Struct variant: Variant { field: T, ... }
                self.advance();
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
                        is_pub: false,
                    });
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RBrace)?;
                VariantKind::Struct(fields)
            } else {
                VariantKind::Unit
            };

            let var_end = self.prev_span();
            variants.push(EnumVariantDef {
                name: var_name,
                kind,
                span: var_start.merge(var_end),
            });

            if !self.eat(TokenKind::Comma) {
                break;
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Some(Spanned::new(
            Item::EnumDef {
                name,
                type_params,
                variants,
                is_pub,
            },
            start.merge(end),
        ))
    }

    fn parse_interface_def(&mut self, is_pub: bool) -> Option<SpannedItem> {
        let start = self.expect(TokenKind::Interface)?.span;
        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.text.clone();

        let type_params = self.parse_optional_type_params();

        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
            let method_start = self.expect(TokenKind::Fn)?.span;
            let method_name = self.expect(TokenKind::Ident)?.text.clone();
            self.expect(TokenKind::LParen)?;
            let params = self.parse_param_list();
            self.expect(TokenKind::RParen)?;

            let return_type = if self.eat(TokenKind::Arrow) {
                Some(self.parse_type()?)
            } else {
                None
            };

            let method_end = self.prev_span();
            methods.push(InterfaceMethodDef {
                name: method_name,
                params,
                return_type,
                span: method_start.merge(method_end),
            });

            self.eat(TokenKind::Semi);
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Some(Spanned::new(
            Item::InterfaceDef {
                name,
                type_params,
                methods,
                is_pub,
            },
            start.merge(end),
        ))
    }

    fn parse_impl_block(&mut self) -> Option<SpannedItem> {
        let start = self.expect(TokenKind::Impl)?.span;

        // Parse the first identifier (could be interface name or type name)
        let first_name = self.expect(TokenKind::Ident)?.text.clone();

        // Check for `impl Interface for Type`
        let (interface_name, type_name) = if self.peek_kind() == TokenKind::For {
            self.advance();
            let tn = self.expect(TokenKind::Ident)?.text.clone();
            (Some(first_name), tn)
        } else {
            (None, first_name)
        };

        // Parse optional type arguments on the type: `impl Foo<i32>`
        let type_args = if self.peek_kind() == TokenKind::Lt {
            self.parse_type_args()
        } else {
            Vec::new()
        };

        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
            let method_is_pub = self.eat(TokenKind::Pub);
            if let Some(item) = self.parse_fn_def(method_is_pub) {
                methods.push(item);
            } else {
                self.advance();
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Some(Spanned::new(
            Item::ImplBlock {
                type_name,
                type_args,
                interface_name,
                methods,
            },
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

    /// Parse optional type parameters: `<T, U: Display>`
    fn parse_optional_type_params(&mut self) -> Vec<TypeParam> {
        if self.peek_kind() != TokenKind::Lt {
            return Vec::new();
        }
        self.advance(); // consume '<'

        let mut params = Vec::new();
        while self.peek_kind() != TokenKind::Gt && !self.at_eof() {
            let name_tok = self.expect(TokenKind::Ident);
            let name_tok = match name_tok {
                Some(t) => t,
                None => break,
            };
            let start = name_tok.span;
            let name = name_tok.text.clone();

            // Optional bounds: T: Display + Debug
            let mut bounds = Vec::new();
            if self.eat(TokenKind::Colon) {
                let bound_tok = self.expect(TokenKind::Ident);
                if let Some(bt) = bound_tok {
                    bounds.push(bt.text.clone());
                }
                while self.eat(TokenKind::Plus) {
                    if let Some(bt) = self.expect(TokenKind::Ident) {
                        bounds.push(bt.text.clone());
                    }
                }
            }

            let end = self.prev_span();
            params.push(TypeParam {
                name,
                bounds,
                span: start.merge(end),
            });

            if !self.eat(TokenKind::Comma) {
                break;
            }
        }

        self.expect(TokenKind::Gt);
        params
    }

    /// Parse type arguments: `<i32, string>`
    fn parse_type_args(&mut self) -> Vec<Spanned<TypeExpr>> {
        if self.peek_kind() != TokenKind::Lt {
            return Vec::new();
        }
        self.advance(); // consume '<'

        let mut args = Vec::new();
        while self.peek_kind() != TokenKind::Gt && !self.at_eof() {
            if let Some(ty) = self.parse_type() {
                args.push(ty);
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }

        self.expect(TokenKind::Gt);
        args
    }

    pub(super) fn parse_type(&mut self) -> Option<Spanned<TypeExpr>> {
        // Function/closure type: |T1, T2| -> R or || -> R
        if self.peek_kind() == TokenKind::Pipe || self.peek_kind() == TokenKind::PipePipe {
            let start = self.peek().span;
            let params = if self.peek_kind() == TokenKind::PipePipe {
                self.advance(); // consume '||'
                Vec::new()
            } else {
                self.advance(); // consume '|'
                let mut params = Vec::new();
                while self.peek_kind() != TokenKind::Pipe && !self.at_eof() {
                    params.push(self.parse_type()?);
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::Pipe)?;
                params
            };
            self.expect(TokenKind::Arrow)?;
            let return_type = self.parse_type()?;
            let end = return_type.span;
            return Some(Spanned::new(
                TypeExpr::FnType {
                    params,
                    return_type: Box::new(return_type),
                },
                start.merge(end),
            ));
        }

        // dyn Interface
        if self.peek_kind() == TokenKind::Dyn {
            let start = self.advance().span;
            let name_tok = self.expect(TokenKind::Ident)?;
            let end = name_tok.span;
            return Some(Spanned::new(
                TypeExpr::DynInterface {
                    name: name_tok.text.clone(),
                },
                start.merge(end),
            ));
        }

        let base_type = if self.peek_kind() == TokenKind::LParen {
            let start = self.advance().span;
            // Unit type: ()
            if self.peek_kind() == TokenKind::RParen {
                let end = self.advance().span;
                Some(Spanned::new(TypeExpr::Unit, start.merge(end)))
            } else {
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
                Some(Spanned::new(TypeExpr::Tuple(elements), start.merge(end)))
            }
        } else if self.peek_kind() == TokenKind::LBracket {
            // Fixed array type: [T; N]
            let start = self.advance().span;
            let element = self.parse_type()?;
            self.expect(TokenKind::Semi)?;
            let len_tok = self.expect(TokenKind::IntLiteral)?;
            let length: usize = len_tok.text.parse().unwrap_or(0);
            let end = self.expect(TokenKind::RBracket)?.span;
            Some(Spanned::new(
                TypeExpr::FixedArray {
                    element: Box::new(element),
                    length,
                },
                start.merge(end),
            ))
        } else if self.peek_kind() == TokenKind::SelfType {
            // Self type keyword
            let tok = self.advance();
            Some(Spanned::new(TypeExpr::Named("Self".into()), tok.span))
        } else if self.peek_kind() == TokenKind::Ident {
            let tok = self.advance().clone();
            let name = tok.text.clone();

            // Check for generic type args: Name<T, U>
            if self.peek_kind() == TokenKind::Lt {
                // Save position for backtrack
                let saved_pos = self.pos;
                self.advance(); // consume '<'

                let mut args = Vec::new();
                let mut success = true;
                loop {
                    if self.peek_kind() == TokenKind::Gt || self.at_eof() {
                        break;
                    }
                    match self.parse_type() {
                        Some(ty) => args.push(ty),
                        None => {
                            success = false;
                            break;
                        }
                    }
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }

                if success && self.peek_kind() == TokenKind::Gt {
                    let end = self.advance().span; // consume '>'
                    Some(Spanned::new(
                        TypeExpr::Generic { name, args },
                        tok.span.merge(end),
                    ))
                } else {
                    // Backtrack - this wasn't a generic type
                    self.pos = saved_pos;
                    Some(Spanned::new(TypeExpr::Named(name), tok.span))
                }
            } else {
                Some(Spanned::new(TypeExpr::Named(name), tok.span))
            }
        } else {
            self.diagnostics.add(&ParserDiagnostic::ExpectedType {
                span: self.peek().span,
            });
            None
        };

        // Check for dynamic array suffix: T[]
        let base = base_type?;
        if self.peek_kind() == TokenKind::LBracket {
            let saved_pos = self.pos;
            self.advance(); // consume '['
            if self.peek_kind() == TokenKind::RBracket {
                let end = self.advance().span; // consume ']'
                let span = base.span.merge(end);
                return Some(Spanned::new(
                    TypeExpr::DynamicArray {
                        element: Box::new(base),
                    },
                    span,
                ));
            } else {
                // Backtrack
                self.pos = saved_pos;
            }
        }

        Some(base)
    }

    /// Parse a pattern for match arms and if-let
    pub(super) fn parse_pattern(&mut self) -> Option<Spanned<Pattern>> {
        let start = self.peek().span;

        // Wildcard: _
        if self.peek_kind() == TokenKind::Underscore {
            let tok = self.advance().clone();
            return Some(Spanned::new(Pattern::Wildcard, tok.span));
        }

        // Literal patterns
        match self.peek_kind() {
            TokenKind::IntLiteral => {
                let tok = self.advance().clone();
                return Some(Spanned::new(
                    Pattern::Literal(Literal::Int(tok.text.clone(), None)),
                    tok.span,
                ));
            }
            TokenKind::BoolLiteral | TokenKind::True | TokenKind::False => {
                let tok = self.advance().clone();
                let val = tok.text == "true";
                return Some(Spanned::new(Pattern::Literal(Literal::Bool(val)), tok.span));
            }
            TokenKind::StringLiteral => {
                let tok = self.advance().clone();
                return Some(Spanned::new(
                    Pattern::Literal(Literal::String(tok.text.clone())),
                    tok.span,
                ));
            }
            _ => {}
        }

        // Array pattern: [a, b], [a, b, ..], [.., a, b], [a, .., b]
        if self.peek_kind() == TokenKind::LBracket {
            self.advance(); // consume '['
            let mut prefix = Vec::new();
            let mut suffix = Vec::new();
            let mut has_rest = false;

            // Check for leading `..`
            if self.peek_kind() == TokenKind::DotDot {
                self.advance();
                has_rest = true;
                self.eat(TokenKind::Comma);
                // Parse suffix elements
                while self.peek_kind() != TokenKind::RBracket && !self.at_eof() {
                    suffix.push(self.parse_pattern()?);
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
            } else {
                // Parse prefix elements until ], .., or EOF
                while self.peek_kind() != TokenKind::RBracket
                    && self.peek_kind() != TokenKind::DotDot
                    && !self.at_eof()
                {
                    prefix.push(self.parse_pattern()?);
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                // Check for `..`
                if self.peek_kind() == TokenKind::DotDot {
                    self.advance();
                    has_rest = true;
                    self.eat(TokenKind::Comma);
                    // Parse suffix elements after ..
                    while self.peek_kind() != TokenKind::RBracket && !self.at_eof() {
                        suffix.push(self.parse_pattern()?);
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                }
            }

            let end = self.expect(TokenKind::RBracket)?.span;
            return Some(Spanned::new(
                Pattern::Array {
                    prefix,
                    suffix,
                    has_rest,
                },
                start.merge(end),
            ));
        }

        // Tuple pattern: (p1, p2)
        if self.peek_kind() == TokenKind::LParen {
            self.advance();
            let mut elements = Vec::new();
            while self.peek_kind() != TokenKind::RParen && !self.at_eof() {
                elements.push(self.parse_pattern()?);
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            let end = self.expect(TokenKind::RParen)?.span;
            return Some(Spanned::new(Pattern::Tuple(elements), start.merge(end)));
        }

        // Identifier-based patterns
        if self.peek_kind() == TokenKind::Ident {
            // Check for struct pattern: Name { field, ... } (uppercase first char)
            if self.peek_nth(1).kind == TokenKind::LBrace {
                let first_char = self.peek().text.chars().next().unwrap_or('a');
                if first_char.is_uppercase() {
                    return self.parse_struct_pattern();
                }
            }

            let tok = self.advance().clone();
            let name = tok.text.clone();

            // Check for enum variant pattern: Enum::Variant or Enum::Variant(...)
            if self.peek_kind() == TokenKind::ColonColon {
                self.advance(); // consume '::'
                let variant_tok = self.expect(TokenKind::Ident)?;
                let variant = variant_tok.text.clone();

                // Check for fields: Variant(p1, p2) or Variant { p1, p2 }
                let fields = if self.peek_kind() == TokenKind::LParen {
                    self.advance();
                    let mut pats = Vec::new();
                    while self.peek_kind() != TokenKind::RParen && !self.at_eof() {
                        pats.push(self.parse_pattern()?);
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    pats
                } else {
                    Vec::new()
                };

                let end = self.prev_span();
                return Some(Spanned::new(
                    Pattern::Enum {
                        enum_name: Some(name),
                        variant,
                        fields,
                    },
                    start.merge(end),
                ));
            }

            // Check for unqualified enum variant pattern: Variant(p1, p2)
            // (uppercase first char followed by '(' indicates enum variant)
            if self.peek_kind() == TokenKind::LParen {
                let first_char = name.chars().next().unwrap_or('a');
                if first_char.is_uppercase() {
                    self.advance(); // consume '('
                    let mut pats = Vec::new();
                    while self.peek_kind() != TokenKind::RParen && !self.at_eof() {
                        pats.push(self.parse_pattern()?);
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    let end = self.prev_span();
                    return Some(Spanned::new(
                        Pattern::Enum {
                            enum_name: None,
                            variant: name,
                            fields: pats,
                        },
                        start.merge(end),
                    ));
                }
            }

            // Simple binding pattern
            return Some(Spanned::new(Pattern::Binding(name), tok.span));
        }

        self.diagnostics.add(&ParserDiagnostic::UnexpectedToken {
            span: self.peek().span,
            expected: "pattern".into(),
            found: self.peek().text.clone(),
        });
        None
    }

    /// Parse a struct pattern: `Foo { x, y: renamed, .. }`
    pub(super) fn parse_struct_pattern(&mut self) -> Option<Spanned<Pattern>> {
        let start = self.peek().span;
        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.text.clone();
        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        let mut has_rest = false;

        while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
            // Check for `..` rest pattern
            if self.peek_kind() == TokenKind::DotDot {
                self.advance();
                has_rest = true;
                // After `..`, expect closing brace (possibly after comma)
                self.eat(TokenKind::Comma);
                break;
            }

            let field_name_tok = self.expect(TokenKind::Ident)?;
            let field_name = field_name_tok.text.clone();

            // Check for `field: pattern` or shorthand `field`
            let pattern = if self.eat(TokenKind::Colon) {
                self.parse_pattern()?
            } else {
                // Shorthand: `x` is equivalent to `x: x`
                Spanned::new(Pattern::Binding(field_name.clone()), field_name_tok.span)
            };

            fields.push((field_name, pattern));

            if !self.eat(TokenKind::Comma) {
                break;
            }
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Some(Spanned::new(
            Pattern::Struct {
                name,
                fields,
                has_rest,
            },
            start.merge(end),
        ))
    }

    /// Parse an import statement: `import std::io;` or `import std::io::{print, println};`
    fn parse_import(&mut self) -> Option<SpannedItem> {
        let start = self.expect(TokenKind::Import)?.span;

        // Parse module path: std::io::...
        let mut path = Vec::new();
        let first = self.expect(TokenKind::Ident)?;
        path.push(first.text.clone());

        while self.eat(TokenKind::ColonColon) {
            if self.peek_kind() == TokenKind::LBrace {
                // Grouped import: import std::io::{print, println}
                self.advance(); // consume {
                let mut items = Vec::new();
                while self.peek_kind() != TokenKind::RBrace && !self.at_eof() {
                    let item_tok = self.expect(TokenKind::Ident)?;
                    items.push(item_tok.text.clone());
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                let end = self.expect(TokenKind::RBrace)?.span;
                self.eat(TokenKind::Semi);
                return Some(Spanned::new(
                    Item::Import {
                        path,
                        items: Some(items),
                        alias: None,
                    },
                    start.merge(end),
                ));
            }

            if self.peek_kind() == TokenKind::Star {
                // Glob import: import std::io::*
                self.advance();
                let end = self.prev_span();
                self.eat(TokenKind::Semi);
                return Some(Spanned::new(
                    Item::Import {
                        path,
                        items: None, // glob import
                        alias: None,
                    },
                    start.merge(end),
                ));
            }

            let next = self.expect(TokenKind::Ident)?;
            path.push(next.text.clone());
        }

        // Check for alias: `as name`
        let alias = if self.eat(TokenKind::As) {
            let alias_tok = self.expect(TokenKind::Ident)?;
            Some(alias_tok.text.clone())
        } else {
            None
        };

        let end = self.prev_span();
        self.eat(TokenKind::Semi);

        Some(Spanned::new(
            Item::Import {
                path,
                items: None,
                alias,
            },
            start.merge(end),
        ))
    }

    fn parse_type_alias(&mut self, is_pub: bool) -> Option<SpannedItem> {
        let start = self.expect(TokenKind::Type)?.span;
        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.text.clone();
        self.expect(TokenKind::Eq)?;
        let ty = self.parse_type()?;
        let end = ty.span;
        // Optional semicolon
        self.eat(TokenKind::Semi);
        Some(Spanned::new(
            Item::TypeAlias { name, ty, is_pub },
            start.merge(end),
        ))
    }
}
