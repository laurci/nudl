use super::*;

impl Checker {
    pub(super) fn check_fn_body(&mut self, fn_name: &str, params: &[Param], body: &Spanned<Block>) {
        let old_fn_name = std::mem::replace(&mut self.current_fn_name, fn_name.to_string());
        let mut locals = ScopedLocals::<LocalInfo>::new();

        let sig = self.functions.get(fn_name).cloned();
        let ret_ty = if let Some(ref sig) = sig {
            for (i, (pname, pty)) in sig.params.iter().enumerate() {
                let is_mut = params.get(i).map_or(false, |p| p.is_mut);
                let def_span = params.get(i).map_or(Span::dummy(), |p| p.ty.span);
                locals.insert(
                    pname.clone(),
                    LocalInfo {
                        ty: *pty,
                        is_mut,
                        def_span,
                    },
                );
            }
            sig.return_type
        } else {
            self.types.unit()
        };

        self.current_return_type = Some(ret_ty);
        let body_ty = self.check_block(&body.node, &mut locals);
        self.current_return_type = None;

        if body_ty != ret_ty
            && body_ty != self.types.error()
            && ret_ty != self.types.error()
            && body_ty != self.types.never()
        {
            self.diagnostics
                .add(&CheckerDiagnostic::ReturnTypeMismatch {
                    span: body.span,
                    expected: self.type_name(ret_ty),
                    found: self.type_name(body_ty),
                });
        }
        self.current_fn_name = old_fn_name;
    }

    pub(super) fn check_item(&mut self, item: &SpannedItem) {
        match &item.node {
            Item::StructDef { .. } => {}    // Already handled in pass 1
            Item::EnumDef { .. } => {}      // Already handled in pass 1
            Item::InterfaceDef { .. } => {} // Already handled in pass 1
            Item::FnDef {
                name,
                type_params,
                params,
                return_type,
                body,
                ..
            } => {
                // Shallow-check generic functions for obvious errors (symbol resolution, etc.)
                if !type_params.is_empty() {
                    self.check_generic_fn_body(type_params, params, return_type.as_ref(), body);
                    return;
                }
                self.check_fn_body(name, params, body);
            }
            Item::ImplBlock {
                type_name, methods, ..
            } => {
                // Shallow-check impl blocks for generic types
                if self.generic_structs.contains_key(type_name)
                    || self.generic_enums.contains_key(type_name)
                {
                    self.check_generic_impl_block(type_name, methods);
                    return;
                }
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
        } else if block.stmts.last().is_some_and(|s| self.stmt_diverges(s)) {
            self.types.never()
        } else {
            self.types.unit()
        };
        locals.pop_scope();
        result
    }

    fn stmt_diverges(&self, stmt: &SpannedStmt) -> bool {
        match &stmt.node {
            Stmt::Expr(expr) => self.expr_diverges(expr),
            _ => false,
        }
    }

    fn expr_diverges(&self, expr: &SpannedExpr) -> bool {
        match &expr.node {
            Expr::Return(_) => true,
            Expr::Break { .. } => true,
            Expr::Continue { .. } => true,
            Expr::Block(block) => block.stmts.last().is_some_and(|s| self.stmt_diverges(s)),
            _ => false,
        }
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
                let final_ty;
                if let Some(type_expr) = ty {
                    let declared_ty = self.resolve_type(type_expr);
                    // Allow unsuffixed integer literals to coerce to any integer type
                    let is_coercible = self.is_unsuffixed_int_literal(&value.node)
                        && self.is_integer_type(declared_ty);
                    // Allow fixed array literals to coerce to dynamic array type
                    // e.g., let mut x: i32[] = [1, 2, 3];
                    let is_array_coercible = matches!(
                        (self.types.resolve(declared_ty), self.types.resolve(val_ty)),
                        (
                            TypeKind::DynamicArray { element: dyn_elem },
                            TypeKind::FixedArray { element: fix_elem, .. }
                        ) if dyn_elem == fix_elem
                    );
                    // Allow Map::new() (returns Map<i64, i64> placeholder) to coerce
                    // to any declared Map<K, V> type
                    let is_map_coercible = matches!(
                        (self.types.resolve(declared_ty), self.types.resolve(val_ty)),
                        (
                            TypeKind::Map { .. },
                            TypeKind::Map { key, value }
                        ) if *key == self.types.i64() && *value == self.types.i64()
                    );
                    if val_ty != declared_ty
                        && val_ty != self.types.error()
                        && declared_ty != self.types.error()
                        && !is_coercible
                        && !is_array_coercible
                        && !is_map_coercible
                        && !self
                            .typevar_compatible(val_ty, declared_ty)
                            .unwrap_or(false)
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: value.span,
                            expected: self.type_name(declared_ty),
                            found: self.type_name(val_ty),
                        });
                    }
                    final_ty = declared_ty;
                    locals.insert(
                        name.node.clone(),
                        LocalInfo {
                            ty: declared_ty,
                            is_mut: *is_mut,
                            def_span: name.span,
                        },
                    );
                } else {
                    // Empty array literal without type annotation is ambiguous
                    if matches!(&value.node, Expr::ArrayLiteral(elems) if elems.is_empty()) {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: value.span,
                            expected: "type annotation".to_string(),
                            found: "empty array literal `[]` (ambiguous without type annotation)"
                                .to_string(),
                        });
                    }
                    final_ty = val_ty;
                    locals.insert(
                        name.node.clone(),
                        LocalInfo {
                            ty: val_ty,
                            is_mut: *is_mut,
                            def_span: name.span,
                        },
                    );
                }
                // Record the binding in the symbol table for hover/go-to-def
                self.symbol_table.record_definition(
                    name.span,
                    DefinitionInfo {
                        name: name.node.clone(),
                        kind: SymbolKind::LocalVariable,
                        def_span: name.span,
                        type_id: Some(final_ty),
                    },
                );
                self.symbol_table.record_expr_type(name.span, final_ty);
            }
            Stmt::LetPattern {
                pattern,
                ty: _,
                value,
                is_mut,
            } => {
                let val_ty = self.check_expr(value, locals);
                self.check_pattern_bindings(pattern, val_ty, *is_mut, locals);
            }
            Stmt::Const { name, ty, value } => {
                let val_ty = self.check_expr(value, locals);
                let final_ty;
                if let Some(type_expr) = ty {
                    let declared_ty = self.resolve_type(type_expr);
                    let is_coercible = self.is_unsuffixed_int_literal(&value.node)
                        && self.is_integer_type(declared_ty);
                    if val_ty != declared_ty
                        && val_ty != self.types.error()
                        && declared_ty != self.types.error()
                        && !is_coercible
                        && !self
                            .typevar_compatible(val_ty, declared_ty)
                            .unwrap_or(false)
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: value.span,
                            expected: self.type_name(declared_ty),
                            found: self.type_name(val_ty),
                        });
                    }
                    final_ty = declared_ty;
                    locals.insert(
                        name.node.clone(),
                        LocalInfo {
                            ty: declared_ty,
                            is_mut: false,
                            def_span: name.span,
                        },
                    );
                } else {
                    final_ty = val_ty;
                    locals.insert(
                        name.node.clone(),
                        LocalInfo {
                            ty: val_ty,
                            is_mut: false,
                            def_span: name.span,
                        },
                    );
                }
                // Record the binding in the symbol table for hover/go-to-def
                self.symbol_table.record_definition(
                    name.span,
                    DefinitionInfo {
                        name: name.node.clone(),
                        kind: SymbolKind::LocalVariable,
                        def_span: name.span,
                        type_id: Some(final_ty),
                    },
                );
                self.symbol_table.record_expr_type(name.span, final_ty);
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
        pattern: &Spanned<Pattern>,
        val_ty: TypeId,
        is_mut: bool,
        locals: &mut ScopedLocals<LocalInfo>,
    ) {
        match &pattern.node {
            Pattern::Wildcard => {} // ignore
            Pattern::Binding(name) => {
                locals.insert(
                    name.clone(),
                    LocalInfo {
                        ty: val_ty,
                        is_mut,
                        def_span: pattern.span,
                    },
                );
                // Record the binding in the symbol table for hover/go-to-def
                self.symbol_table.record_definition(
                    pattern.span,
                    DefinitionInfo {
                        name: name.clone(),
                        kind: SymbolKind::LocalVariable,
                        def_span: pattern.span,
                        type_id: Some(val_ty),
                    },
                );
                self.symbol_table.record_expr_type(pattern.span, val_ty);
            }
            Pattern::Tuple(elements) => {
                // Destructure tuple type
                if let TypeKind::Tuple(elem_types) = self.types.resolve(val_ty).clone() {
                    for (i, pat) in elements.iter().enumerate() {
                        let elem_ty = elem_types
                            .get(i)
                            .copied()
                            .unwrap_or_else(|| self.types.error());
                        self.check_pattern_bindings(pat, elem_ty, is_mut, locals);
                    }
                } else {
                    // Not a tuple, all bindings get the val_ty
                    for pat in elements {
                        self.check_pattern_bindings(pat, val_ty, is_mut, locals);
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
                        self.check_pattern_bindings(pat, field_ty, is_mut, locals);
                    }
                }
            }
            Pattern::Enum { fields, .. } => {
                // For now, bind each sub-pattern with error type
                let error_ty = self.types.error();
                for field_pat in fields {
                    self.check_pattern_bindings(field_pat, error_ty, is_mut, locals);
                }
            }
            Pattern::Literal(_) => {} // nothing to bind
            Pattern::Array { prefix, suffix, .. } => {
                // Extract element type from the array type
                let elem_ty = match self.types.resolve(val_ty).clone() {
                    TypeKind::DynamicArray { element } => element,
                    TypeKind::FixedArray { element, .. } => element,
                    _ => self.types.error(),
                };
                for pat in prefix.iter().chain(suffix.iter()) {
                    self.check_pattern_bindings(pat, elem_ty, is_mut, locals);
                }
            }
            Pattern::Or(alternatives) => {
                for alt in alternatives {
                    self.check_pattern_bindings(alt, val_ty, is_mut, locals);
                }
            }
            Pattern::Range { .. } => {
                // Range patterns don't bind any variables
            }
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
        let _required = sig.required_params.saturating_sub(skip_params);

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
            let param_ty = callable_params[positional_idx].1;
            self.set_closure_hint_if_fn(param_ty);
            let arg_ty = self.check_expr(&arg.value, locals);
            let is_coercible =
                self.is_unsuffixed_int_literal(&arg.value.node) && self.is_integer_type(param_ty);
            if arg_ty != param_ty
                && arg_ty != self.types.error()
                && param_ty != self.types.error()
                && !is_coercible
                && !self.typevar_compatible(arg_ty, param_ty).unwrap_or(false)
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
                    let param_ty = callable_params[pos].1;
                    self.set_closure_hint_if_fn(param_ty);
                    let arg_ty = self.check_expr(&arg.value, locals);
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
                // Positional arg that came after named — skip already-filled slots
                while positional_idx < callable_params.len() && filled[positional_idx] {
                    positional_idx += 1;
                }
                if positional_idx < callable_params.len() {
                    let param_ty = callable_params[positional_idx].1;
                    self.set_closure_hint_if_fn(param_ty);
                    let arg_ty = self.check_expr(&arg.value, locals);
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

    /// Shallow-check a generic function body using TypeVar placeholders.
    /// Catches obvious errors (undefined variables/functions, return type mismatches)
    /// without requiring concrete type arguments.
    fn check_generic_fn_body(
        &mut self,
        type_params: &[TypeParam],
        params: &[Param],
        return_type: Option<&Spanned<TypeExpr>>,
        body: &Spanned<Block>,
    ) {
        let old_scope = std::mem::take(&mut self.type_param_scope);

        // Create TypeVar TypeIds for each type parameter
        for tp in type_params {
            let type_var = self.types.intern(TypeKind::TypeVar {
                name: tp.name.clone(),
                bounds: tp.bounds.clone(),
            });
            self.type_param_scope.insert(tp.name.clone(), type_var);
        }

        // Resolve params using type_param_scope (TypeVars for generic params)
        let mut locals = ScopedLocals::<LocalInfo>::new();
        for p in params {
            let ty = self.resolve_type(&p.ty);
            locals.insert(
                p.name.clone(),
                LocalInfo {
                    ty,
                    is_mut: p.is_mut,
                    def_span: p.ty.span,
                },
            );
        }

        // Resolve return type
        let ret_ty = return_type
            .map(|t| self.resolve_type(t))
            .unwrap_or_else(|| self.types.unit());

        self.current_return_type = Some(ret_ty);
        let body_ty = self.check_block(&body.node, &mut locals);
        self.current_return_type = None;

        // Check return type fitting, allowing TypeVar == TypeVar
        if body_ty != ret_ty
            && body_ty != self.types.error()
            && ret_ty != self.types.error()
            && body_ty != self.types.never()
        {
            self.diagnostics
                .add(&CheckerDiagnostic::ReturnTypeMismatch {
                    span: body.span,
                    expected: self.type_name(ret_ty),
                    found: self.type_name(body_ty),
                });
        }

        self.type_param_scope = old_scope;
    }

    /// Shallow-check methods in a generic impl block.
    fn check_generic_impl_block(&mut self, type_name: &str, methods: &[SpannedItem]) {
        let old_scope = std::mem::take(&mut self.type_param_scope);

        // Get type params from the generic struct/enum definition
        let type_params = if let Some(def) = self.generic_structs.get(type_name) {
            def.type_params.clone()
        } else if let Some(def) = self.generic_enums.get(type_name) {
            def.type_params.clone()
        } else {
            self.type_param_scope = old_scope;
            return;
        };

        // Create TypeVar TypeIds for each type parameter
        for tp in &type_params {
            let type_var = self.types.intern(TypeKind::TypeVar {
                name: tp.name.clone(),
                bounds: tp.bounds.clone(),
            });
            self.type_param_scope.insert(tp.name.clone(), type_var);
        }

        // Set Self to error type (can't know the concrete Self type)
        let self_ty = self.types.error();
        self.type_param_scope.insert("Self".into(), self_ty);

        for method_item in methods {
            if let Item::FnDef {
                type_params: method_type_params,
                params,
                return_type,
                body,
                ..
            } = &method_item.node
            {
                // Also add method-level type params if any
                for tp in method_type_params {
                    let type_var = self.types.intern(TypeKind::TypeVar {
                        name: tp.name.clone(),
                        bounds: tp.bounds.clone(),
                    });
                    self.type_param_scope.insert(tp.name.clone(), type_var);
                }

                // Resolve params
                let mut locals = ScopedLocals::<LocalInfo>::new();
                for p in params {
                    let ty = if p.is_self {
                        self_ty
                    } else {
                        self.resolve_type(&p.ty)
                    };
                    locals.insert(
                        p.name.clone(),
                        LocalInfo {
                            ty,
                            is_mut: p.is_mut,
                            def_span: p.ty.span,
                        },
                    );
                }

                let ret_ty = return_type
                    .as_ref()
                    .map(|t| self.resolve_type(t))
                    .unwrap_or_else(|| self.types.unit());

                self.current_return_type = Some(ret_ty);
                let body_ty = self.check_block(&body.node, &mut locals);
                self.current_return_type = None;

                if body_ty != ret_ty
                    && body_ty != self.types.error()
                    && ret_ty != self.types.error()
                    && body_ty != self.types.never()
                {
                    self.diagnostics
                        .add(&CheckerDiagnostic::ReturnTypeMismatch {
                            span: body.span,
                            expected: self.type_name(ret_ty),
                            found: self.type_name(body_ty),
                        });
                }

                // Remove method-level type params
                for tp in method_type_params {
                    self.type_param_scope.remove(&tp.name);
                }
            }
        }

        self.type_param_scope = old_scope;
    }
}
