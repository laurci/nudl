use super::*;

impl Checker {
    pub(super) fn check_fn_body(&mut self, fn_name: &str, params: &[Param], body: &Spanned<Block>) {
        let mut locals = ScopedLocals::<LocalInfo>::new();

        let sig = self.functions.get(fn_name).cloned();
        let ret_ty = if let Some(ref sig) = sig {
            for (i, (pname, pty)) in sig.params.iter().enumerate() {
                let is_mut = params.get(i).map_or(false, |p| p.is_mut);
                locals.insert(pname.clone(), LocalInfo { ty: *pty, is_mut });
            }
            sig.return_type
        } else {
            self.types.unit()
        };

        self.current_return_type = Some(ret_ty);
        let body_ty = self.check_block(&body.node, &mut locals);
        self.current_return_type = None;

        if body_ty != ret_ty && body_ty != self.types.error() && ret_ty != self.types.error() {
            self.diagnostics
                .add(&CheckerDiagnostic::ReturnTypeMismatch {
                    span: body.span,
                    expected: self.type_name(ret_ty),
                    found: self.type_name(body_ty),
                });
        }
    }

    pub(super) fn check_item(&mut self, item: &SpannedItem) {
        match &item.node {
            Item::StructDef { .. } => {}    // Already handled in pass 1
            Item::EnumDef { .. } => {}      // Already handled in pass 1
            Item::InterfaceDef { .. } => {} // Already handled in pass 1
            Item::FnDef {
                name, params, body, ..
            } => {
                self.check_fn_body(name, params, body);
            }
            Item::ImplBlock {
                type_name, methods, ..
            } => {
                for method_item in methods {
                    if let Item::FnDef {
                        name: method_name,
                        params,
                        body,
                        ..
                    } = &method_item.node
                    {
                        let mangled_name = format!("{}__{}", type_name, method_name);
                        self.check_fn_body(&mangled_name, params, body);
                    }
                }
            }
            Item::ExternBlock { .. } => {} // Already handled in pass 1
            Item::Import { .. } => {}      // Handled at pipeline level
            Item::TypeAlias { .. } => {}   // Already handled in collect pass
        }
    }

    pub(super) fn check_block(
        &mut self,
        block: &Block,
        locals: &mut ScopedLocals<LocalInfo>,
    ) -> TypeId {
        locals.push_scope();
        for stmt in &block.stmts {
            self.check_stmt(stmt, locals);
        }
        let result = if let Some(tail) = &block.tail_expr {
            self.check_expr(tail, locals)
        } else {
            self.types.unit()
        };
        locals.pop_scope();
        result
    }

    pub(super) fn check_stmt(&mut self, stmt: &SpannedStmt, locals: &mut ScopedLocals<LocalInfo>) {
        match &stmt.node {
            Stmt::Expr(expr) => {
                self.check_expr(expr, locals);
            }
            Stmt::Let {
                name,
                ty,
                value,
                is_mut,
            } => {
                let val_ty = self.check_expr(value, locals);

                // If explicit type annotation, check it matches
                if let Some(type_expr) = ty {
                    let declared_ty = self.resolve_type(type_expr);
                    // Allow unsuffixed integer literals to coerce to any integer type
                    let is_coercible = self.is_unsuffixed_int_literal(&value.node)
                        && self.is_integer_type(declared_ty);
                    if val_ty != declared_ty
                        && val_ty != self.types.error()
                        && declared_ty != self.types.error()
                        && !is_coercible
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: value.span,
                            expected: self.type_name(declared_ty),
                            found: self.type_name(val_ty),
                        });
                    }
                    locals.insert(
                        name.clone(),
                        LocalInfo {
                            ty: declared_ty,
                            is_mut: *is_mut,
                        },
                    );
                } else {
                    locals.insert(
                        name.clone(),
                        LocalInfo {
                            ty: val_ty,
                            is_mut: *is_mut,
                        },
                    );
                }
            }
            Stmt::LetPattern {
                pattern,
                ty: _,
                value,
                is_mut,
            } => {
                let val_ty = self.check_expr(value, locals);
                self.check_pattern_bindings(&pattern.node, val_ty, *is_mut, locals);
            }
            Stmt::Const { name, ty, value } => {
                let val_ty = self.check_expr(value, locals);
                if let Some(type_expr) = ty {
                    let declared_ty = self.resolve_type(type_expr);
                    let is_coercible = self.is_unsuffixed_int_literal(&value.node)
                        && self.is_integer_type(declared_ty);
                    if val_ty != declared_ty
                        && val_ty != self.types.error()
                        && declared_ty != self.types.error()
                        && !is_coercible
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: value.span,
                            expected: self.type_name(declared_ty),
                            found: self.type_name(val_ty),
                        });
                    }
                    locals.insert(
                        name.clone(),
                        LocalInfo {
                            ty: declared_ty,
                            is_mut: false,
                        },
                    );
                } else {
                    locals.insert(
                        name.clone(),
                        LocalInfo {
                            ty: val_ty,
                            is_mut: false,
                        },
                    );
                }
            }
            Stmt::Defer { body } => {
                // Type-check the defer body
                self.check_block(&body.node, locals);
            }
            Stmt::Item(item) => self.collect_item(item),
        }
    }

    /// Check pattern bindings and add them to locals
    fn check_pattern_bindings(
        &mut self,
        pattern: &Pattern,
        val_ty: TypeId,
        is_mut: bool,
        locals: &mut ScopedLocals<LocalInfo>,
    ) {
        match pattern {
            Pattern::Wildcard => {} // ignore
            Pattern::Binding(name) => {
                locals.insert(name.clone(), LocalInfo { ty: val_ty, is_mut });
            }
            Pattern::Tuple(elements) => {
                // Destructure tuple type
                if let TypeKind::Tuple(elem_types) = self.types.resolve(val_ty).clone() {
                    for (i, pat) in elements.iter().enumerate() {
                        let elem_ty = elem_types
                            .get(i)
                            .copied()
                            .unwrap_or_else(|| self.types.error());
                        self.check_pattern_bindings(&pat.node, elem_ty, is_mut, locals);
                    }
                } else {
                    // Not a tuple, all bindings get the val_ty
                    for pat in elements {
                        self.check_pattern_bindings(&pat.node, val_ty, is_mut, locals);
                    }
                }
            }
            Pattern::Struct { name, fields, .. } => {
                // Look up struct type
                let struct_ty = self
                    .structs
                    .get(name)
                    .copied()
                    .unwrap_or_else(|| self.types.error());
                if let TypeKind::Struct {
                    fields: struct_fields,
                    ..
                } = self.types.resolve(struct_ty).clone()
                {
                    for (field_name, pat) in fields {
                        let field_ty = struct_fields
                            .iter()
                            .find(|(n, _)| n == field_name)
                            .map(|(_, ty)| *ty)
                            .unwrap_or_else(|| self.types.error());
                        self.check_pattern_bindings(&pat.node, field_ty, is_mut, locals);
                    }
                }
            }
            Pattern::Enum { fields, .. } => {
                // For now, bind each sub-pattern with error type
                for field_pat in fields {
                    self.check_pattern_bindings(
                        &field_pat.node,
                        self.types.error(),
                        is_mut,
                        locals,
                    );
                }
            }
            Pattern::Literal(_) => {} // nothing to bind
        }
    }

    /// Check call arguments with support for named args and defaults.
    /// `skip_params` is the number of leading parameters to skip (e.g. 1 for self in methods).
    pub(super) fn check_call_args(
        &mut self,
        call_span: Span,
        sig: &FunctionSig,
        args: &[CallArg],
        locals: &mut ScopedLocals<LocalInfo>,
        skip_params: usize,
    ) -> TypeId {
        let callable_params = &sig.params[skip_params..];
        let callable_defaults = &sig.has_default[skip_params..];
        let required = sig.required_params.saturating_sub(skip_params);

        // Build a positional map: for each param, which arg index fills it (if any)
        let mut filled = vec![false; callable_params.len()];

        // Process positional args first (those without a name)
        let mut positional_idx = 0;
        for (arg_idx, arg) in args.iter().enumerate() {
            if arg.name.is_some() {
                break; // switch to named args
            }
            if positional_idx >= callable_params.len() {
                self.diagnostics
                    .add(&CheckerDiagnostic::ArgumentCountMismatch {
                        span: call_span,
                        expected: callable_params.len().to_string(),
                        found: args.len().to_string(),
                    });
                return sig.return_type;
            }
            let arg_ty = self.check_expr(&arg.value, locals);
            let param_ty = callable_params[positional_idx].1;
            let is_coercible =
                self.is_unsuffixed_int_literal(&arg.value.node) && self.is_integer_type(param_ty);
            if arg_ty != param_ty
                && arg_ty != self.types.error()
                && param_ty != self.types.error()
                && !is_coercible
            {
                self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                    span: arg.value.span,
                    expected: self.type_name(param_ty),
                    found: self.type_name(arg_ty),
                });
            }
            filled[positional_idx] = true;
            positional_idx += 1;
            let _ = arg_idx;
        }

        // Process named args
        for arg in args.iter().skip(positional_idx) {
            if let Some(arg_name) = &arg.name {
                // Find the parameter by name
                if let Some(pos) = callable_params.iter().position(|(n, _)| n == arg_name) {
                    let arg_ty = self.check_expr(&arg.value, locals);
                    let param_ty = callable_params[pos].1;
                    let is_coercible = self.is_unsuffixed_int_literal(&arg.value.node)
                        && self.is_integer_type(param_ty);
                    if arg_ty != param_ty
                        && arg_ty != self.types.error()
                        && param_ty != self.types.error()
                        && !is_coercible
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: arg.value.span,
                            expected: self.type_name(param_ty),
                            found: self.type_name(arg_ty),
                        });
                    }
                    filled[pos] = true;
                } else {
                    self.check_expr(&arg.value, locals);
                    self.diagnostics
                        .add(&CheckerDiagnostic::UnknownParameterName {
                            span: arg.value.span,
                            name: arg_name.clone(),
                        });
                }
            } else {
                // Positional arg that came after named — still process by position
                if positional_idx < callable_params.len() {
                    let arg_ty = self.check_expr(&arg.value, locals);
                    let param_ty = callable_params[positional_idx].1;
                    let is_coercible = self.is_unsuffixed_int_literal(&arg.value.node)
                        && self.is_integer_type(param_ty);
                    if arg_ty != param_ty
                        && arg_ty != self.types.error()
                        && param_ty != self.types.error()
                        && !is_coercible
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: arg.value.span,
                            expected: self.type_name(param_ty),
                            found: self.type_name(arg_ty),
                        });
                    }
                    filled[positional_idx] = true;
                    positional_idx += 1;
                } else {
                    self.check_expr(&arg.value, locals);
                    self.diagnostics
                        .add(&CheckerDiagnostic::ArgumentCountMismatch {
                            span: call_span,
                            expected: callable_params.len().to_string(),
                            found: args.len().to_string(),
                        });
                }
            }
        }

        // Check that all required params are filled
        for (i, is_filled) in filled.iter().enumerate() {
            if !is_filled && !callable_defaults[i] {
                self.diagnostics
                    .add(&CheckerDiagnostic::MissingRequiredArgument {
                        span: call_span,
                        name: callable_params[i].0.clone(),
                    });
            }
        }

        sig.return_type
    }
}
