use super::*;

impl Checker {
    pub(super) fn check_expr(
        &mut self,
        expr: &SpannedExpr,
        locals: &mut ScopedLocals<LocalInfo>,
    ) -> TypeId {
        match &expr.node {
            Expr::Literal(Literal::String(_)) => self.types.string(),
            Expr::Literal(Literal::TemplateString { exprs, .. }) => {
                for e in exprs {
                    self.check_expr(e, locals);
                }
                self.types.string()
            }
            Expr::Literal(Literal::Int(_, Some(suffix))) => match suffix {
                IntSuffix::I8 => self.types.i8(),
                IntSuffix::I16 => self.types.i16(),
                IntSuffix::I32 => self.types.i32(),
                IntSuffix::I64 => self.types.i64(),
                IntSuffix::U8 => self.types.u8(),
                IntSuffix::U16 => self.types.u16(),
                IntSuffix::U32 => self.types.u32(),
                IntSuffix::U64 => self.types.u64(),
            },
            Expr::Literal(Literal::Int(_, None)) => self.types.i32(),
            Expr::Literal(Literal::Float(_)) => self.types.f64(),
            Expr::Literal(Literal::Bool(_)) => self.types.bool(),
            Expr::Literal(Literal::Char(_)) => self.types.char_type(),

            Expr::Ident(name) => {
                if let Some(info) = locals.get(name) {
                    info.ty
                } else if let Some(&struct_ty) = self.structs.get(name.as_str()) {
                    // Unit struct constructor: just the name creates an instance
                    struct_ty
                } else {
                    self.diagnostics.add(&CheckerDiagnostic::UndefinedVariable {
                        span: expr.span,
                        name: name.clone(),
                    });
                    self.types.error()
                }
            }

            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = &callee.node {
                    let sig = self.functions.get(name).cloned();
                    if let Some(sig) = sig {
                        // Check if this is a generic function that needs monomorphization
                        if let Some(generic_def) = sig.generic_def.clone() {
                            // Infer type arguments from call arguments
                            let type_args = self.infer_type_args(&generic_def, args, locals);
                            if let Some(type_args) = type_args {
                                // Check if any inferred type arg is a TypeVar (generic-to-generic call)
                                let has_typevar_args = !self.type_param_scope.is_empty()
                                    && type_args.iter().any(|&ty| self.is_type_var(ty));

                                if has_typevar_args {
                                    // Don't monomorphize — validate TypeVar bounds and compute return type via substitution
                                    for (i, tp) in generic_def.type_params.iter().enumerate() {
                                        if self.is_type_var(type_args[i]) {
                                            for bound in &tp.bounds {
                                                if !self.type_var_has_bound(type_args[i], bound) {
                                                    self.diagnostics.add(
                                                        &CheckerDiagnostic::BoundNotSatisfied {
                                                            span: expr.span,
                                                            type_param: self
                                                                .type_name(type_args[i]),
                                                            bound: bound.clone(),
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    // Check arguments
                                    for arg in args {
                                        self.check_expr(&arg.value, locals);
                                    }
                                    // Build substitution map and compute return type
                                    if let Some(ref ast_ret) = generic_def.ast_return_type {
                                        let subst: std::collections::HashMap<String, TypeId> =
                                            generic_def
                                                .type_params
                                                .iter()
                                                .enumerate()
                                                .map(|(i, tp)| (tp.name.clone(), type_args[i]))
                                                .collect();
                                        return self.resolve_type_with_subst(ast_ret, &subst);
                                    }
                                    return self.types.unit();
                                }

                                // Concrete type args — validate bounds and monomorphize
                                for (i, tp) in generic_def.type_params.iter().enumerate() {
                                    for bound in &tp.bounds {
                                        let ty_name = self.type_name(type_args[i]);
                                        let bound_satisfied = self
                                            .interface_impls
                                            .get(bound)
                                            .map_or(false, |impls| impls.contains(&ty_name));
                                        if !bound_satisfied {
                                            self.diagnostics.add(
                                                &CheckerDiagnostic::BoundCheckFailed {
                                                    span: expr.span,
                                                    ty: ty_name,
                                                    interface: bound.clone(),
                                                },
                                            );
                                        }
                                    }
                                }
                                // Monomorphize
                                if let Some(mangled) = self.monomorphize_function(
                                    name,
                                    &generic_def,
                                    &type_args,
                                    expr.span,
                                ) {
                                    self.call_resolutions.insert(expr.span, mangled.clone());
                                    let mono_sig = self.functions.get(&mangled).cloned().unwrap();
                                    // Validate argument types against the concrete monomorphized signature
                                    self.check_call_args(expr.span, &mono_sig, args, locals, 0);
                                    return mono_sig.return_type;
                                }
                            } else {
                                // Could not infer — report error (but suppress during
                                // generic body shallow-checking since TypeVars prevent inference)
                                if self.type_param_scope.is_empty() {
                                    for tp in &generic_def.type_params {
                                        self.diagnostics.add(
                                            &CheckerDiagnostic::CannotInferTypeParam {
                                                span: expr.span,
                                                name: tp.name.clone(),
                                            },
                                        );
                                    }
                                }
                                for arg in args {
                                    self.check_expr(&arg.value, locals);
                                }
                            }
                            return self.types.error();
                        }
                        self.check_call_args(expr.span, &sig, args, locals, 0)
                    } else {
                        // Check if it's a local variable with a Function type (closure call)
                        if let Some(info) = locals.get(name) {
                            if let TypeKind::Function { params, ret } =
                                self.types.resolve(info.ty).clone()
                            {
                                // Type-check arguments against closure param types
                                for (i, arg) in args.iter().enumerate() {
                                    let arg_ty = self.check_expr(&arg.value, locals);
                                    if let Some(&expected) = params.get(i) {
                                        if arg_ty != expected
                                            && arg_ty != self.types.error()
                                            && expected != self.types.error()
                                            && !self
                                                .typevar_compatible(arg_ty, expected)
                                                .unwrap_or(false)
                                        {
                                            self.diagnostics.add(
                                                &CheckerDiagnostic::TypeMismatch {
                                                    span: arg.value.span,
                                                    expected: self.type_name(expected),
                                                    found: self.type_name(arg_ty),
                                                },
                                            );
                                        }
                                    }
                                }
                                return ret;
                            }
                        }
                        // Check if it's a tuple struct constructor: Foo(val1, val2)
                        if let Some(&struct_ty) = self.structs.get(name.as_str()) {
                            if let TypeKind::Struct { fields, .. } =
                                self.types.resolve(struct_ty).clone()
                            {
                                for (i, arg) in args.iter().enumerate() {
                                    let arg_ty = self.check_expr(&arg.value, locals);
                                    if let Some((_, expected_ty)) = fields.get(i) {
                                        if arg_ty != *expected_ty
                                            && arg_ty != self.types.error()
                                            && *expected_ty != self.types.error()
                                        {
                                            self.diagnostics.add(
                                                &CheckerDiagnostic::TypeMismatch {
                                                    span: arg.value.span,
                                                    expected: self.type_name(*expected_ty),
                                                    found: self.type_name(arg_ty),
                                                },
                                            );
                                        }
                                    }
                                }
                                return struct_ty;
                            }
                        }
                        self.diagnostics.add(&CheckerDiagnostic::UndefinedFunction {
                            span: callee.span,
                            name: name.clone(),
                        });
                        for arg in args {
                            self.check_expr(&arg.value, locals);
                        }
                        self.types.error()
                    }
                } else {
                    // Callee could be an expression that evaluates to a closure
                    let callee_ty = self.check_expr(callee, locals);
                    for arg in args {
                        self.check_expr(&arg.value, locals);
                    }
                    if let TypeKind::Function { ret, .. } = self.types.resolve(callee_ty).clone() {
                        ret
                    } else {
                        self.types.error()
                    }
                }
            }

            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                let obj_ty = self.check_expr(object, locals);
                if obj_ty == self.types.error() {
                    for arg in args {
                        self.check_expr(&arg.value, locals);
                    }
                    return self.types.error();
                }

                // TypeVar: can't resolve methods on unconstrained type params — just check args
                if self.is_type_var(obj_ty) {
                    for arg in args {
                        self.check_expr(&arg.value, locals);
                    }
                    return self.types.error();
                }

                let type_name = self.type_name(obj_ty);
                let mangled_name = format!("{}__{}", type_name, method);
                let sig = self.functions.get(&mangled_name).cloned();

                if let Some(sig) = sig {
                    // Check mutability for `mut self` methods
                    if sig.is_mut_method {
                        // Check if the object is a mutable binding
                        if let Expr::Ident(var_name) = &object.node {
                            if let Some(info) = locals.get(var_name) {
                                if !info.is_mut {
                                    self.diagnostics.add(
                                        &CheckerDiagnostic::MutatingMethodOnImmutable {
                                            span: expr.span,
                                            method: method.clone(),
                                        },
                                    );
                                }
                            }
                        }
                    }
                    // skip_params=1 to skip the self parameter
                    self.check_call_args(expr.span, &sig, args, locals, 1)
                } else {
                    // Check built-in methods for dynamic arrays and maps
                    let resolved = self.types.resolve(obj_ty).clone();
                    match &resolved {
                        TypeKind::DynamicArray { element } => {
                            let elem = *element;
                            match method.as_str() {
                                "push" => {
                                    if let Some(arg) = args.first() {
                                        let arg_ty = self.check_expr(&arg.value, locals);
                                        if arg_ty != elem
                                            && arg_ty != self.types.error()
                                            && elem != self.types.error()
                                        {
                                            self.diagnostics.add(
                                                &CheckerDiagnostic::TypeMismatch {
                                                    span: arg.value.span,
                                                    expected: self.type_name(elem),
                                                    found: self.type_name(arg_ty),
                                                },
                                            );
                                        }
                                    }
                                    return self.types.unit();
                                }
                                "pop" => {
                                    return elem;
                                }
                                "len" => {
                                    return self.types.i64();
                                }
                                _ => {}
                            }
                        }
                        TypeKind::Map { key, value } => {
                            let k = *key;
                            let v = *value;
                            match method.as_str() {
                                "insert" => {
                                    for arg in args {
                                        self.check_expr(&arg.value, locals);
                                    }
                                    return self.types.unit();
                                }
                                "get" => {
                                    for arg in args {
                                        self.check_expr(&arg.value, locals);
                                    }
                                    // Return Option<V> instead of raw V
                                    return self.monomorphize_enum_concrete(
                                        "Option",
                                        &[v],
                                        1,
                                        expr.span,
                                    );
                                }
                                "contains_key" => {
                                    for arg in args {
                                        self.check_expr(&arg.value, locals);
                                    }
                                    return self.types.bool();
                                }
                                "remove" => {
                                    for arg in args {
                                        self.check_expr(&arg.value, locals);
                                    }
                                    return self.types.bool();
                                }
                                "len" => {
                                    return self.types.i64();
                                }
                                _ => {
                                    let _ = (k, v);
                                }
                            }
                        }
                        _ => {}
                    }
                    self.diagnostics.add(&CheckerDiagnostic::UndefinedMethod {
                        span: expr.span,
                        ty: type_name,
                        method: method.clone(),
                    });
                    for arg in args {
                        self.check_expr(&arg.value, locals);
                    }
                    self.types.error()
                }
            }

            Expr::StaticCall {
                type_name,
                method,
                args,
            } => {
                // Check if this is an enum tuple variant constructor
                if let Some(&enum_ty) = self.enums.get(type_name.as_str()) {
                    let variants = match self.types.resolve(enum_ty).clone() {
                        TypeKind::Enum { variants, .. } => variants,
                        _ => Vec::new(),
                    };
                    if let Some(var) = variants.iter().find(|v| v.name == *method) {
                        if args.len() != var.fields.len() {
                            self.diagnostics
                                .add(&CheckerDiagnostic::ArgumentCountMismatch {
                                    span: expr.span,
                                    expected: var.fields.len().to_string(),
                                    found: args.len().to_string(),
                                });
                        }
                        for (i, arg) in args.iter().enumerate() {
                            let arg_ty = self.check_expr(&arg.value, locals);
                            if let Some((_, expected_ty)) = var.fields.get(i) {
                                if arg_ty != *expected_ty
                                    && arg_ty != self.types.error()
                                    && *expected_ty != self.types.error()
                                {
                                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                        span: arg.value.span,
                                        expected: self.type_name(*expected_ty),
                                        found: self.type_name(arg_ty),
                                    });
                                }
                            }
                        }
                        return enum_ty;
                    }
                }

                // Check if this is a generic enum variant constructor (e.g., Option::Some(42))
                if self.generic_enums.contains_key(type_name.as_str()) {
                    let generic_def = self.generic_enums.get(type_name).cloned().unwrap();

                    // Find the variant
                    if let Some(variant_def) =
                        generic_def.variants.iter().find(|v| v.name == *method)
                    {
                        // Type-check arguments to infer type parameters
                        let mut inferred: HashMap<String, TypeId> = HashMap::new();
                        let arg_types: Vec<TypeId> = args
                            .iter()
                            .map(|arg| self.check_expr(&arg.value, locals))
                            .collect();

                        match &variant_def.kind {
                            VariantKind::Tuple(types) => {
                                for (i, ty_expr) in types.iter().enumerate() {
                                    if let Some(&arg_ty) = arg_types.get(i) {
                                        if arg_ty != self.types.error() {
                                            self.unify_type_expr_with_concrete_pub(
                                                &ty_expr.node,
                                                arg_ty,
                                                &mut inferred,
                                            );
                                        }
                                    }
                                }
                            }
                            VariantKind::Unit => {}
                            VariantKind::Struct(fields) => {
                                for (i, f) in fields.iter().enumerate() {
                                    if let Some(&arg_ty) = arg_types.get(i) {
                                        if arg_ty != self.types.error() {
                                            self.unify_type_expr_with_concrete_pub(
                                                &f.ty.node,
                                                arg_ty,
                                                &mut inferred,
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Try to infer remaining type params from return type context
                        if let Some(ret_ty) = self.current_return_type {
                            if let TypeKind::Enum {
                                name: ename,
                                variants: ret_variants,
                            } = self.types.resolve(ret_ty).clone()
                            {
                                if ename == *type_name
                                    || ename.starts_with(&format!("{}$", type_name))
                                {
                                    for (gv, cv) in
                                        generic_def.variants.iter().zip(ret_variants.iter())
                                    {
                                        if let VariantKind::Tuple(types) = &gv.kind {
                                            for (j, ty_expr) in types.iter().enumerate() {
                                                if let TypeExpr::Named(n) = &ty_expr.node {
                                                    if !inferred.contains_key(n.as_str()) {
                                                        if let Some((_, field_ty)) =
                                                            cv.fields.get(j)
                                                        {
                                                            inferred.insert(n.clone(), *field_ty);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Build type args
                        let mut type_args = Vec::new();
                        let mut all_inferred = true;
                        for tp in &generic_def.type_params {
                            if let Some(&ty) = inferred.get(&tp.name) {
                                type_args.push(ty);
                            } else {
                                all_inferred = false;
                                self.diagnostics
                                    .add(&CheckerDiagnostic::CannotInferTypeParam {
                                        span: expr.span,
                                        name: tp.name.clone(),
                                    });
                            }
                        }

                        if all_inferred {
                            let type_arg_exprs: Vec<Spanned<TypeExpr>> = type_args
                                .iter()
                                .map(|&ty| self.type_id_to_type_expr(ty, expr.span))
                                .collect();

                            let enum_ty =
                                self.monomorphize_enum(type_name, &type_arg_exprs, expr.span);
                            if enum_ty != self.types.error() {
                                let mangled = self.mangle_name(type_name, &type_args);
                                self.enum_resolutions.insert(expr.span, mangled);
                            }
                            return enum_ty;
                        }
                        return self.types.error();
                    }
                }

                // Check if this is a static method on a generic struct/enum
                if self.generic_impl_methods.contains_key(type_name.as_str()) {
                    let methods = self.generic_impl_methods.get(type_name).cloned().unwrap();
                    if let Some(gm) = methods.iter().find(|m| m.method_name == *method) {
                        let gm = gm.clone();
                        // This is a static method on a generic type — we need to infer type args
                        // from the method's parameter types and the call arguments

                        // Type-check all arguments first
                        let arg_types: Vec<TypeId> = args
                            .iter()
                            .map(|arg| self.check_expr(&arg.value, locals))
                            .collect();

                        // Infer type args from method params (skip self if present)
                        let mut inferred: HashMap<String, TypeId> = HashMap::new();
                        let non_self_params: Vec<&Param> =
                            gm.ast_params.iter().filter(|p| !p.is_self).collect();
                        for (i, param) in non_self_params.iter().enumerate() {
                            if let Some(&arg_ty) = arg_types.get(i) {
                                if arg_ty != self.types.error() {
                                    self.unify_type_expr_with_concrete_pub(
                                        &param.ty.node,
                                        arg_ty,
                                        &mut inferred,
                                    );
                                }
                            }
                        }

                        // Get the type params from the struct/enum definition
                        let type_params: Vec<TypeParam> =
                            if let Some(sdef) = self.generic_structs.get(type_name) {
                                sdef.type_params.clone()
                            } else if let Some(edef) = self.generic_enums.get(type_name) {
                                edef.type_params.clone()
                            } else {
                                gm.type_params.clone()
                            };

                        let mut type_args = Vec::new();
                        let mut all_inferred = true;
                        for tp in type_params {
                            if let Some(&ty) = inferred.get(&tp.name) {
                                type_args.push(ty);
                            } else {
                                all_inferred = false;
                                self.diagnostics
                                    .add(&CheckerDiagnostic::CannotInferTypeParam {
                                        span: expr.span,
                                        name: tp.name.clone(),
                                    });
                            }
                        }

                        if all_inferred {
                            // Monomorphize the struct/enum
                            let type_arg_exprs: Vec<Spanned<TypeExpr>> = type_args
                                .iter()
                                .map(|&ty| self.type_id_to_type_expr(ty, expr.span))
                                .collect();

                            if self.generic_structs.contains_key(type_name.as_str()) {
                                let struct_ty =
                                    self.monomorphize_struct(type_name, &type_arg_exprs, expr.span);
                                if struct_ty != self.types.error() {
                                    let mangled = self.mangle_name(type_name, &type_args);
                                    let mangled_method = format!("{}__{}", mangled, method);
                                    if let Some(sig) = self.functions.get(&mangled_method).cloned()
                                    {
                                        self.call_resolutions.insert(expr.span, mangled_method);
                                        return self
                                            .check_call_args(expr.span, &sig, args, locals, 0);
                                    }
                                }
                            } else if self.generic_enums.contains_key(type_name.as_str()) {
                                let enum_ty =
                                    self.monomorphize_enum(type_name, &type_arg_exprs, expr.span);
                                if enum_ty != self.types.error() {
                                    let mangled = self.mangle_name(type_name, &type_args);
                                    let mangled_method = format!("{}__{}", mangled, method);
                                    if let Some(sig) = self.functions.get(&mangled_method).cloned()
                                    {
                                        self.call_resolutions.insert(expr.span, mangled_method);
                                        return self
                                            .check_call_args(expr.span, &sig, args, locals, 0);
                                    }
                                }
                            }
                        }
                        return self.types.error();
                    }
                }

                // Handle Map::new()
                if type_name == "Map" && method == "new" && args.is_empty() {
                    // Default to Map<i64, i64> — actual types inferred from usage
                    let key_ty = self.types.i64();
                    let val_ty = self.types.i64();
                    return self.types.intern(TypeKind::Map {
                        key: key_ty,
                        value: val_ty,
                    });
                }

                let mangled_name = format!("{}__{}", type_name, method);
                let sig = self.functions.get(&mangled_name).cloned();

                if let Some(sig) = sig {
                    self.check_call_args(expr.span, &sig, args, locals, 0)
                } else {
                    self.diagnostics.add(&CheckerDiagnostic::UndefinedFunction {
                        span: expr.span,
                        name: mangled_name,
                    });
                    for arg in args {
                        self.check_expr(&arg.value, locals);
                    }
                    self.types.error()
                }
            }

            Expr::Block(block) => self.check_block(block, locals),

            Expr::Return(Some(inner)) => {
                let val_ty = self.check_expr(inner, locals);
                if let Some(ret_ty) = self.current_return_type {
                    if val_ty != ret_ty
                        && val_ty != self.types.error()
                        && ret_ty != self.types.error()
                        && !self.typevar_compatible(val_ty, ret_ty).unwrap_or(false)
                    {
                        self.diagnostics
                            .add(&CheckerDiagnostic::ReturnTypeMismatch {
                                span: inner.span,
                                expected: self.type_name(ret_ty),
                                found: self.type_name(val_ty),
                            });
                    }
                } else {
                    // Inside a closure without explicit return type — record the
                    // return expression type so it can be used for inference.
                    self.current_return_type = Some(val_ty);
                }
                self.types.never()
            }
            Expr::Return(None) => {
                if self.current_return_type.is_none() {
                    self.current_return_type = Some(self.types.unit());
                }
                self.types.never()
            }

            Expr::Binary { op, left, right } => {
                let left_ty = self.check_expr(left, locals);
                let right_ty = self.check_expr(right, locals);

                if left_ty == self.types.error() || right_ty == self.types.error() {
                    return self.types.error();
                }

                // If either operand is a TypeVar, check bounds and return appropriate type
                if self.is_type_var(left_ty) || self.is_type_var(right_ty) {
                    // Both TypeVars but different → type mismatch (e.g. T + U)
                    if self.is_type_var(left_ty)
                        && self.is_type_var(right_ty)
                        && left_ty != right_ty
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: expr.span,
                            expected: self.type_name(left_ty),
                            found: self.type_name(right_ty),
                        });
                        return self.types.error();
                    }

                    let required_bound = match op {
                        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                            Some("Add")
                        }
                        BinOp::Eq | BinOp::Ne => Some("Eq"),
                        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Some("Ord"),
                        _ => None,
                    };
                    // Check bounds on both operands (skip duplicate when same TypeVar)
                    if let Some(bound) = required_bound {
                        if self.is_type_var(left_ty) && !self.type_var_has_bound(left_ty, bound) {
                            self.diagnostics.add(&CheckerDiagnostic::BoundNotSatisfied {
                                span: expr.span,
                                type_param: self.type_name(left_ty),
                                bound: bound.to_string(),
                            });
                        }
                        if self.is_type_var(right_ty)
                            && left_ty != right_ty
                            && !self.type_var_has_bound(right_ty, bound)
                        {
                            self.diagnostics.add(&CheckerDiagnostic::BoundNotSatisfied {
                                span: expr.span,
                                type_param: self.type_name(right_ty),
                                bound: bound.to_string(),
                            });
                        }
                    }
                    let typevar_ty = if self.is_type_var(left_ty) {
                        left_ty
                    } else {
                        right_ty
                    };
                    // Comparison ops return bool, arithmetic returns the TypeVar
                    return match op {
                        BinOp::Eq
                        | BinOp::Ne
                        | BinOp::Lt
                        | BinOp::Le
                        | BinOp::Gt
                        | BinOp::Ge
                        | BinOp::And
                        | BinOp::Or => self.types.bool(),
                        _ => typevar_ty,
                    };
                }

                // Check for operator overloading on user-defined types
                let op_method = match op {
                    BinOp::Add => Some("add"),
                    BinOp::Sub => Some("sub"),
                    BinOp::Mul => Some("mul"),
                    BinOp::Div => Some("div"),
                    BinOp::Mod => Some("rem"),
                    BinOp::Eq => Some("eq"),
                    BinOp::Ne => Some("ne"),
                    BinOp::Lt => Some("lt"),
                    BinOp::Le => Some("le"),
                    BinOp::Gt => Some("gt"),
                    BinOp::Ge => Some("ge"),
                    _ => None,
                };

                if let Some(method_name) = op_method {
                    let type_name = self.type_name(left_ty);
                    let mangled = format!("{}__{}", type_name, method_name);
                    if self.functions.contains_key(&mangled) {
                        // Operator overloading via method: use the method's return type
                        let sig = self.functions.get(&mangled).unwrap();
                        return sig.return_type;
                    }
                }

                // Both sides must be same type for primitive ops
                if left_ty != right_ty {
                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                        span: right.span,
                        expected: self.type_name(left_ty),
                        found: self.type_name(right_ty),
                    });
                    return self.types.error();
                }

                match op {
                    BinOp::Add
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::Div
                    | BinOp::Mod
                    | BinOp::Shl
                    | BinOp::Shr => {
                        if !self.is_numeric(left_ty) {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: format!("{:?}", op).to_lowercase(),
                                    ty: self.type_name(left_ty),
                                });
                            return self.types.error();
                        }
                        left_ty
                    }
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                        if !self.is_integer_type(left_ty) {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: format!("{:?}", op).to_lowercase(),
                                    ty: self.type_name(left_ty),
                                });
                            return self.types.error();
                        }
                        left_ty
                    }
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        self.types.bool()
                    }
                    BinOp::And | BinOp::Or => {
                        if left_ty != self.types.bool() {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: format!("{:?}", op).to_lowercase(),
                                    ty: self.type_name(left_ty),
                                });
                            return self.types.error();
                        }
                        self.types.bool()
                    }
                }
            }

            Expr::Unary { op, operand } => {
                let operand_ty = self.check_expr(operand, locals);
                if operand_ty == self.types.error() {
                    return self.types.error();
                }

                // TypeVar: can't validate unary ops on type params
                if self.is_type_var(operand_ty) {
                    return match op {
                        UnaryOp::Not => self.types.bool(),
                        _ => operand_ty,
                    };
                }

                match op {
                    UnaryOp::Neg => {
                        if !self.is_signed_or_float(operand_ty) {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: "-".into(),
                                    ty: self.type_name(operand_ty),
                                });
                            return self.types.error();
                        }
                        operand_ty
                    }
                    UnaryOp::Not => {
                        if operand_ty != self.types.bool() {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: "!".into(),
                                    ty: self.type_name(operand_ty),
                                });
                            return self.types.error();
                        }
                        self.types.bool()
                    }
                    UnaryOp::BitNot => {
                        if !self.is_integer_type(operand_ty) {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: "~".into(),
                                    ty: self.type_name(operand_ty),
                                });
                            return self.types.error();
                        }
                        operand_ty
                    }
                }
            }

            Expr::Assign { target, value } => {
                let val_ty = self.check_expr(value, locals);
                if let Expr::Ident(name) = &target.node {
                    if let Some(info) = locals.get(name) {
                        if !info.is_mut {
                            self.diagnostics
                                .add(&CheckerDiagnostic::ImmutableAssignment {
                                    span: target.span,
                                    name: name.clone(),
                                });
                        }
                        let target_ty = info.ty;
                        if val_ty != target_ty
                            && val_ty != self.types.error()
                            && target_ty != self.types.error()
                            && !self.typevar_compatible(val_ty, target_ty).unwrap_or(false)
                        {
                            self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                span: value.span,
                                expected: self.type_name(target_ty),
                                found: self.type_name(val_ty),
                            });
                        }
                    } else {
                        self.diagnostics.add(&CheckerDiagnostic::UndefinedVariable {
                            span: target.span,
                            name: name.clone(),
                        });
                    }
                } else if let Expr::FieldAccess { object, field } = &target.node {
                    let obj_ty = self.check_expr(object, locals);
                    if obj_ty != self.types.error() {
                        match self.types.resolve(obj_ty).clone() {
                            TypeKind::Struct { name, fields } => {
                                if let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == field)
                                {
                                    if val_ty != *field_ty
                                        && val_ty != self.types.error()
                                        && *field_ty != self.types.error()
                                    {
                                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                            span: value.span,
                                            expected: self.type_name(*field_ty),
                                            found: self.type_name(val_ty),
                                        });
                                    }
                                } else {
                                    self.diagnostics.add(&CheckerDiagnostic::UnknownField {
                                        span: target.span,
                                        name: name.clone(),
                                        field: field.clone(),
                                    });
                                }
                            }
                            // TypeVar: skip field assignment validation
                            TypeKind::TypeVar { .. } => {}
                            _ => {
                                self.diagnostics
                                    .add(&CheckerDiagnostic::FieldAccessOnNonStruct {
                                        span: target.span,
                                        ty: self.type_name(obj_ty),
                                    });
                            }
                        }
                    }
                } else if let Expr::IndexAccess { object, index } = &target.node {
                    let obj_ty = self.check_expr(object, locals);
                    let idx_ty = self.check_expr(index, locals);
                    if obj_ty != self.types.error() && idx_ty != self.types.error() {
                        if !self.is_integer_type(idx_ty) {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: index.span,
                                    op: "index".into(),
                                    ty: self.type_name(idx_ty),
                                });
                        }
                        match self.types.resolve(obj_ty).clone() {
                            TypeKind::FixedArray { element, .. }
                            | TypeKind::DynamicArray { element } => {
                                if val_ty != element
                                    && val_ty != self.types.error()
                                    && element != self.types.error()
                                {
                                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                        span: value.span,
                                        expected: self.type_name(element),
                                        found: self.type_name(val_ty),
                                    });
                                }
                            }
                            TypeKind::Map {
                                value: val_type, ..
                            } => {
                                if val_ty != val_type
                                    && val_ty != self.types.error()
                                    && val_type != self.types.error()
                                {
                                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                        span: value.span,
                                        expected: self.type_name(val_type),
                                        found: self.type_name(val_ty),
                                    });
                                }
                            }
                            // TypeVar: skip index assignment validation
                            TypeKind::TypeVar { .. } => {}
                            _ => {
                                self.diagnostics
                                    .add(&CheckerDiagnostic::InvalidOperatorType {
                                        span: target.span,
                                        op: "index".into(),
                                        ty: self.type_name(obj_ty),
                                    });
                            }
                        }
                    }
                }
                self.types.unit()
            }

            Expr::CompoundAssign { op, target, value } => {
                let val_ty = self.check_expr(value, locals);
                if let Expr::Ident(name) = &target.node {
                    if let Some(info) = locals.get(name) {
                        if !info.is_mut {
                            self.diagnostics
                                .add(&CheckerDiagnostic::ImmutableAssignment {
                                    span: target.span,
                                    name: name.clone(),
                                });
                        }
                        let target_ty = info.ty;
                        // TypeVar: skip numeric validation
                        let is_valid = if self.is_type_var(target_ty) {
                            true
                        } else {
                            match op {
                                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                                    self.is_integer_type(target_ty)
                                }
                                _ => self.is_numeric(target_ty),
                            }
                        };
                        if !is_valid && target_ty != self.types.error() {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: format!("{:?}", op).to_lowercase(),
                                    ty: self.type_name(target_ty),
                                });
                        }
                        if val_ty != target_ty
                            && val_ty != self.types.error()
                            && target_ty != self.types.error()
                        {
                            self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                span: value.span,
                                expected: self.type_name(target_ty),
                                found: self.type_name(val_ty),
                            });
                        }
                    } else {
                        self.diagnostics.add(&CheckerDiagnostic::UndefinedVariable {
                            span: target.span,
                            name: name.clone(),
                        });
                    }
                }
                self.types.unit()
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.check_expr(condition, locals);
                if cond_ty != self.types.bool()
                    && cond_ty != self.types.error()
                    && !self.is_type_var(cond_ty)
                {
                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                        span: condition.span,
                        expected: "bool".into(),
                        found: self.type_name(cond_ty),
                    });
                }

                let then_ty = self.check_block(&then_branch.node, locals);

                if let Some(else_expr) = else_branch {
                    let else_ty = self.check_expr(else_expr, locals);
                    if then_ty != else_ty
                        && then_ty != self.types.error()
                        && else_ty != self.types.error()
                        && !self.typevar_compatible(then_ty, else_ty).unwrap_or(false)
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: else_expr.span,
                            expected: self.type_name(then_ty),
                            found: self.type_name(else_ty),
                        });
                    }
                    then_ty
                } else {
                    self.types.unit()
                }
            }

            Expr::Cast { expr, target_type } => {
                let src_ty = self.check_expr(expr, locals);
                let dst_ty = self.resolve_type(target_type);
                if src_ty == self.types.error() || dst_ty == self.types.error() {
                    return self.types.error();
                }
                // TypeVar: can't validate casts on type variables
                if self.is_type_var(src_ty) || self.is_type_var(dst_ty) {
                    return dst_ty;
                }
                // Allow casts between numeric types, bool→int, char↔integer
                let is_valid = (self.is_numeric(src_ty) && self.is_numeric(dst_ty))
                    || (src_ty == self.types.bool() && self.is_integer_type(dst_ty))
                    || (src_ty == self.types.char_type() && self.is_integer_type(dst_ty))
                    || (self.is_integer_type(src_ty) && dst_ty == self.types.char_type())
                    || (src_ty == self.types.raw_ptr() && dst_ty == self.types.mut_raw_ptr())
                    || (src_ty == self.types.mut_raw_ptr() && dst_ty == self.types.raw_ptr())
                    || (src_ty == self.types.raw_ptr() && dst_ty == self.types.cstr())
                    || (src_ty == self.types.cstr() && dst_ty == self.types.raw_ptr());
                if !is_valid {
                    self.diagnostics
                        .add(&CheckerDiagnostic::InvalidOperatorType {
                            span: expr.span,
                            op: "as".into(),
                            ty: format!("{} as {}", self.type_name(src_ty), self.type_name(dst_ty)),
                        });
                    return self.types.error();
                }
                dst_ty
            }

            Expr::While {
                condition, body, ..
            } => {
                let cond_ty = self.check_expr(condition, locals);
                if cond_ty != self.types.bool()
                    && cond_ty != self.types.error()
                    && !self.is_type_var(cond_ty)
                {
                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                        span: condition.span,
                        expected: "bool".into(),
                        found: self.type_name(cond_ty),
                    });
                }
                self.check_block(&body.node, locals);
                self.types.unit()
            }

            Expr::Loop { body, .. } => {
                self.check_block(&body.node, locals);
                self.types.unit()
            }

            Expr::Break { value, .. } => {
                if let Some(val) = value {
                    self.check_expr(val, locals);
                }
                self.types.never()
            }

            Expr::Continue { .. } => self.types.never(),

            Expr::Grouped(inner) => self.check_expr(inner, locals),

            Expr::StructLiteral { name, fields } => {
                // Check if this is a generic struct that needs monomorphization
                if self.generic_structs.contains_key(name.as_str()) {
                    let generic_def = self.generic_structs.get(name).cloned().unwrap();

                    // Type-check all field values first to get concrete types
                    let mut field_types: Vec<(String, TypeId)> = Vec::new();
                    for (field_name, field_val) in fields {
                        let val_ty = self.check_expr(field_val, locals);
                        field_types.push((field_name.clone(), val_ty));
                    }

                    // Infer type args from field types
                    let mut inferred: HashMap<String, TypeId> = HashMap::new();
                    for (field_name, val_ty) in &field_types {
                        if *val_ty == self.types.error() {
                            continue;
                        }
                        // Find the field in the generic def
                        if let Some(field_def) =
                            generic_def.fields.iter().find(|f| &f.name == field_name)
                        {
                            self.unify_type_expr_with_concrete_pub(
                                &field_def.ty.node,
                                *val_ty,
                                &mut inferred,
                            );
                        }
                    }

                    // Build type args in order
                    let mut type_args = Vec::new();
                    let mut all_inferred = true;
                    for tp in &generic_def.type_params {
                        if let Some(&ty) = inferred.get(&tp.name) {
                            type_args.push(ty);
                        } else {
                            all_inferred = false;
                            self.diagnostics
                                .add(&CheckerDiagnostic::CannotInferTypeParam {
                                    span: expr.span,
                                    name: tp.name.clone(),
                                });
                        }
                    }

                    if !all_inferred {
                        return self.types.error();
                    }

                    // Create Spanned<TypeExpr> from inferred types for monomorphize_struct
                    let type_arg_exprs: Vec<Spanned<TypeExpr>> = type_args
                        .iter()
                        .map(|&ty| self.type_id_to_type_expr(ty, expr.span))
                        .collect();

                    let struct_ty = self.monomorphize_struct(name, &type_arg_exprs, expr.span);
                    if struct_ty == self.types.error() {
                        return self.types.error();
                    }

                    // Store resolution
                    let mangled = self.mangle_name(name, &type_args);
                    self.struct_resolutions.insert(expr.span, mangled);

                    // Now validate fields against the monomorphized struct
                    let expected_fields = match self.types.resolve(struct_ty).clone() {
                        TypeKind::Struct { fields: f, .. } => f,
                        _ => return self.types.error(),
                    };

                    for (field_name, val_ty) in &field_types {
                        if let Some((_, expected_ty)) =
                            expected_fields.iter().find(|(n, _)| n == field_name)
                        {
                            if *val_ty != *expected_ty
                                && *val_ty != self.types.error()
                                && *expected_ty != self.types.error()
                            {
                                // Field type mismatch (shouldn't happen normally since we inferred from these)
                            }
                        } else {
                            self.diagnostics.add(&CheckerDiagnostic::UnknownField {
                                span: expr.span,
                                name: name.clone(),
                                field: field_name.clone(),
                            });
                        }
                    }

                    for (expected_name, _) in &expected_fields {
                        if !fields.iter().any(|(n, _)| n == expected_name) {
                            self.diagnostics.add(&CheckerDiagnostic::MissingField {
                                span: expr.span,
                                name: name.clone(),
                                field: expected_name.clone(),
                            });
                        }
                    }

                    return struct_ty;
                }

                let struct_ty = if let Some(&ty) = self.structs.get(name.as_str()) {
                    ty
                } else {
                    self.diagnostics.add(&CheckerDiagnostic::UndefinedStruct {
                        span: expr.span,
                        name: name.clone(),
                    });
                    return self.types.error();
                };

                let expected_fields = match self.types.resolve(struct_ty).clone() {
                    TypeKind::Struct { fields: f, .. } => f,
                    _ => return self.types.error(),
                };

                // Check for unknown fields
                for (field_name, field_val) in fields {
                    let expected = expected_fields.iter().find(|(n, _)| n == field_name);
                    if let Some((_, expected_ty)) = expected {
                        let val_ty = self.check_expr(field_val, locals);
                        if val_ty != *expected_ty
                            && val_ty != self.types.error()
                            && *expected_ty != self.types.error()
                        {
                            self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                span: field_val.span,
                                expected: self.type_name(*expected_ty),
                                found: self.type_name(val_ty),
                            });
                        }
                    } else {
                        self.check_expr(field_val, locals);
                        self.diagnostics.add(&CheckerDiagnostic::UnknownField {
                            span: field_val.span,
                            name: name.clone(),
                            field: field_name.clone(),
                        });
                    }
                }

                // Check for missing fields
                for (expected_name, _) in &expected_fields {
                    if !fields.iter().any(|(n, _)| n == expected_name) {
                        self.diagnostics.add(&CheckerDiagnostic::MissingField {
                            span: expr.span,
                            name: name.clone(),
                            field: expected_name.clone(),
                        });
                    }
                }

                struct_ty
            }

            Expr::FieldAccess { object, field } => {
                let obj_ty = self.check_expr(object, locals);
                if obj_ty == self.types.error() {
                    return self.types.error();
                }

                match self.types.resolve(obj_ty).clone() {
                    TypeKind::Struct { name, fields } => {
                        if let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == field) {
                            *field_ty
                        } else {
                            self.diagnostics.add(&CheckerDiagnostic::UnknownField {
                                span: expr.span,
                                name: name.clone(),
                                field: field.clone(),
                            });
                            self.types.error()
                        }
                    }
                    TypeKind::Tuple(elements) => {
                        // Numeric field access on tuples: .0, .1, etc.
                        if let Ok(idx) = field.parse::<usize>() {
                            if idx < elements.len() {
                                elements[idx]
                            } else {
                                self.diagnostics
                                    .add(&CheckerDiagnostic::InvalidOperatorType {
                                        span: expr.span,
                                        op: "tuple index".into(),
                                        ty: format!(
                                            "index {} out of bounds for {}-element tuple",
                                            idx,
                                            elements.len()
                                        ),
                                    });
                                self.types.error()
                            }
                        } else {
                            self.diagnostics
                                .add(&CheckerDiagnostic::FieldAccessOnNonStruct {
                                    span: expr.span,
                                    ty: self.type_name(obj_ty),
                                });
                            self.types.error()
                        }
                    }
                    // TypeVar: can't access fields on a type variable
                    TypeKind::TypeVar { .. } => self.types.error(),
                    _ => {
                        self.diagnostics
                            .add(&CheckerDiagnostic::FieldAccessOnNonStruct {
                                span: expr.span,
                                ty: self.type_name(obj_ty),
                            });
                        self.types.error()
                    }
                }
            }

            Expr::TupleLiteral(elements) => {
                if elements.is_empty() {
                    return self.types.unit();
                }
                let element_types: Vec<TypeId> = elements
                    .iter()
                    .map(|e| self.check_expr(e, locals))
                    .collect();
                self.types.intern(TypeKind::Tuple(element_types))
            }

            Expr::ArrayLiteral(elements) => {
                if elements.is_empty() {
                    // Empty array — can't infer type without annotation
                    // For now return error; type annotation needed
                    self.types.error()
                } else {
                    let first_ty = self.check_expr(&elements[0], locals);
                    for elem in &elements[1..] {
                        let elem_ty = self.check_expr(elem, locals);
                        if elem_ty != first_ty
                            && elem_ty != self.types.error()
                            && first_ty != self.types.error()
                        {
                            self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                span: elem.span,
                                expected: self.type_name(first_ty),
                                found: self.type_name(elem_ty),
                            });
                        }
                    }
                    self.types.intern(TypeKind::FixedArray {
                        element: first_ty,
                        length: elements.len(),
                    })
                }
            }

            Expr::ArrayRepeat { value, count } => {
                let elem_ty = self.check_expr(value, locals);
                self.types.intern(TypeKind::FixedArray {
                    element: elem_ty,
                    length: *count,
                })
            }

            Expr::IndexAccess { object, index } => {
                let obj_ty = self.check_expr(object, locals);
                let idx_ty = self.check_expr(index, locals);

                if obj_ty == self.types.error() || idx_ty == self.types.error() {
                    return self.types.error();
                }

                if !self.is_integer_type(idx_ty) {
                    self.diagnostics
                        .add(&CheckerDiagnostic::InvalidOperatorType {
                            span: index.span,
                            op: "index".into(),
                            ty: self.type_name(idx_ty),
                        });
                    return self.types.error();
                }

                match self.types.resolve(obj_ty).clone() {
                    TypeKind::FixedArray { element, .. } => element,
                    TypeKind::DynamicArray { element } => element,
                    TypeKind::Map { value, .. } => value,
                    TypeKind::String => self.types.char_type(),
                    // TypeVar: can't resolve index on type variable
                    TypeKind::TypeVar { .. } => self.types.error(),
                    _ => {
                        self.diagnostics
                            .add(&CheckerDiagnostic::InvalidOperatorType {
                                span: expr.span,
                                op: "index".into(),
                                ty: self.type_name(obj_ty),
                            });
                        self.types.error()
                    }
                }
            }

            Expr::Range { start, end, .. } => {
                let start_ty = self.check_expr(start, locals);
                let end_ty = self.check_expr(end, locals);

                if start_ty == self.types.error() || end_ty == self.types.error() {
                    return self.types.error();
                }

                if start_ty != end_ty {
                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                        span: end.span,
                        expected: self.type_name(start_ty),
                        found: self.type_name(end_ty),
                    });
                }

                if !self.is_integer_type(start_ty) && !self.is_type_var(start_ty) {
                    self.diagnostics
                        .add(&CheckerDiagnostic::InvalidOperatorType {
                            span: expr.span,
                            op: "range".into(),
                            ty: self.type_name(start_ty),
                        });
                }

                // Range type is unit for now — ranges are only used in for-loops
                self.types.unit()
            }

            Expr::For {
                binding,
                iter,
                body,
            } => {
                let iter_ty = self.check_expr(iter, locals);

                // Determine the element type based on the iterator
                let elem_ty = match &iter.node {
                    Expr::Range { start, .. } => {
                        // For ranges, element type is the range's integer type
                        self.check_expr(start, locals)
                    }
                    _ => {
                        // For arrays, element type is the array's element type
                        if iter_ty != self.types.error() {
                            match self.types.resolve(iter_ty).clone() {
                                TypeKind::FixedArray { element, .. } => element,
                                TypeKind::DynamicArray { element } => element,
                                // TypeVar: can't iterate over bare type variable
                                TypeKind::TypeVar { .. } => self.types.error(),
                                _ => {
                                    self.diagnostics
                                        .add(&CheckerDiagnostic::InvalidOperatorType {
                                            span: iter.span,
                                            op: "for-in".into(),
                                            ty: self.type_name(iter_ty),
                                        });
                                    self.types.error()
                                }
                            }
                        } else {
                            self.types.error()
                        }
                    }
                };

                locals.push_scope();
                locals.insert(
                    binding.clone(),
                    LocalInfo {
                        ty: elem_ty,
                        is_mut: false,
                    },
                );
                self.check_block(&body.node, locals);
                locals.pop_scope();
                self.types.unit()
            }

            Expr::EnumLiteral {
                enum_name,
                variant,
                args,
            } => {
                // Look up enum type
                if let Some(&enum_ty) = self.enums.get(enum_name.as_str()) {
                    let variants = match self.types.resolve(enum_ty).clone() {
                        TypeKind::Enum { variants, .. } => variants,
                        _ => return self.types.error(),
                    };
                    if let Some(var) = variants.iter().find(|v| v.name == *variant) {
                        // Check arg count
                        if args.len() != var.fields.len() {
                            self.diagnostics
                                .add(&CheckerDiagnostic::ArgumentCountMismatch {
                                    span: expr.span,
                                    expected: var.fields.len().to_string(),
                                    found: args.len().to_string(),
                                });
                        }
                        // Check arg types
                        for (i, arg) in args.iter().enumerate() {
                            let arg_ty = self.check_expr(arg, locals);
                            if let Some((_, expected_ty)) = var.fields.get(i) {
                                if arg_ty != *expected_ty
                                    && arg_ty != self.types.error()
                                    && *expected_ty != self.types.error()
                                {
                                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                        span: arg.span,
                                        expected: self.type_name(*expected_ty),
                                        found: self.type_name(arg_ty),
                                    });
                                }
                            }
                        }
                    } else {
                        self.diagnostics.add(&CheckerDiagnostic::UnknownField {
                            span: expr.span,
                            name: enum_name.clone(),
                            field: variant.clone(),
                        });
                    }
                    enum_ty
                } else if self.generic_enums.contains_key(enum_name.as_str()) {
                    // Generic enum variant construction (e.g., Option::None in generic context)
                    let generic_def = self.generic_enums.get(enum_name).cloned().unwrap();

                    if let Some(variant_def) =
                        generic_def.variants.iter().find(|v| v.name == *variant)
                    {
                        // Type-check arguments to infer type parameters
                        let mut inferred: HashMap<String, TypeId> = HashMap::new();
                        let arg_types: Vec<TypeId> = args
                            .iter()
                            .map(|arg| self.check_expr(arg, locals))
                            .collect();

                        match &variant_def.kind {
                            VariantKind::Tuple(types) => {
                                for (i, ty_expr) in types.iter().enumerate() {
                                    if let Some(&arg_ty) = arg_types.get(i) {
                                        if arg_ty != self.types.error() {
                                            self.unify_type_expr_with_concrete_pub(
                                                &ty_expr.node,
                                                arg_ty,
                                                &mut inferred,
                                            );
                                        }
                                    }
                                }
                            }
                            VariantKind::Unit => {
                                // Unit variant (e.g., Option::None) — try to infer from return type context
                                if let Some(ret_ty) = self.current_return_type {
                                    if let TypeKind::Enum {
                                        name: ename,
                                        variants,
                                    } = self.types.resolve(ret_ty).clone()
                                    {
                                        // Check if this is a monomorphized version of our generic enum
                                        if ename == *enum_name
                                            || ename.starts_with(&format!("{}$", enum_name))
                                        {
                                            // Extract type args from the monomorphized variant fields
                                            for tp in &generic_def.type_params {
                                                // Look through variants to find fields that contain this type param
                                                for (gv, cv) in
                                                    generic_def.variants.iter().zip(variants.iter())
                                                {
                                                    if let VariantKind::Tuple(types) = &gv.kind {
                                                        for (j, ty_expr) in types.iter().enumerate()
                                                        {
                                                            if let TypeExpr::Named(n) =
                                                                &ty_expr.node
                                                            {
                                                                if *n == tp.name {
                                                                    if let Some((_, field_ty)) =
                                                                        cv.fields.get(j)
                                                                    {
                                                                        inferred.insert(
                                                                            tp.name.clone(),
                                                                            *field_ty,
                                                                        );
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            VariantKind::Struct(fields) => {
                                for (i, f) in fields.iter().enumerate() {
                                    if let Some(&arg_ty) = arg_types.get(i) {
                                        if arg_ty != self.types.error() {
                                            self.unify_type_expr_with_concrete_pub(
                                                &f.ty.node,
                                                arg_ty,
                                                &mut inferred,
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Build type args
                        let mut type_args = Vec::new();
                        let mut all_inferred = true;
                        for tp in &generic_def.type_params {
                            if let Some(&ty) = inferred.get(&tp.name) {
                                type_args.push(ty);
                            } else {
                                all_inferred = false;
                                // In generic context, don't emit error — will resolve at monomorphization
                                if self.type_param_scope.is_empty() {
                                    self.diagnostics.add(
                                        &CheckerDiagnostic::CannotInferTypeParam {
                                            span: expr.span,
                                            name: tp.name.clone(),
                                        },
                                    );
                                }
                            }
                        }

                        if all_inferred {
                            let type_arg_exprs: Vec<Spanned<TypeExpr>> = type_args
                                .iter()
                                .map(|&ty| self.type_id_to_type_expr(ty, expr.span))
                                .collect();

                            let enum_ty =
                                self.monomorphize_enum(enum_name, &type_arg_exprs, expr.span);
                            if enum_ty != self.types.error() {
                                let mangled = self.mangle_name(enum_name, &type_args);
                                self.enum_resolutions.insert(expr.span, mangled);
                            }
                            return enum_ty;
                        }
                        // In generic context, return error (resolved at monomorphization)
                        return self.types.error();
                    } else {
                        self.diagnostics.add(&CheckerDiagnostic::UnknownField {
                            span: expr.span,
                            name: enum_name.clone(),
                            field: variant.clone(),
                        });
                        return self.types.error();
                    }
                } else {
                    // Maybe it's a static call on a struct
                    let mangled_name = format!("{}__{}", enum_name, variant);
                    if let Some(sig) = self.functions.get(&mangled_name).cloned() {
                        let call_args: Vec<CallArg> = args
                            .iter()
                            .map(|a| CallArg {
                                name: None,
                                value: a.clone(),
                            })
                            .collect();
                        return self.check_call_args(expr.span, &sig, &call_args, locals, 0);
                    }
                    self.diagnostics.add(&CheckerDiagnostic::UndefinedStruct {
                        span: expr.span,
                        name: enum_name.clone(),
                    });
                    self.types.error()
                }
            }

            Expr::Match {
                expr: scrutinee,
                arms,
            } => {
                let scrutinee_ty = self.check_expr(scrutinee, locals);
                let mut result_ty = None;

                for arm in arms {
                    locals.push_scope();
                    // Introduce bindings from the pattern
                    self.check_pattern(&arm.pattern, scrutinee_ty, locals);

                    if let Some(guard) = &arm.guard {
                        let guard_ty = self.check_expr(guard, locals);
                        if guard_ty != self.types.bool() && guard_ty != self.types.error() {
                            self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                span: guard.span,
                                expected: "bool".into(),
                                found: self.type_name(guard_ty),
                            });
                        }
                    }

                    let body_ty = self.check_expr(&arm.body, locals);
                    locals.pop_scope();

                    // Never type (!) is compatible with any type in match arms
                    // (e.g., panic() or return in one arm, value in another)
                    if body_ty == self.types.never() {
                        // Don't update result_ty — the never arm doesn't contribute a type
                    } else if let Some(prev_ty) = result_ty {
                        if prev_ty == self.types.never() {
                            // Previous arm was never — update with this arm's type
                            result_ty = Some(body_ty);
                        } else if body_ty != prev_ty
                            && body_ty != self.types.error()
                            && prev_ty != self.types.error()
                        {
                            self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                span: arm.body.span,
                                expected: self.type_name(prev_ty),
                                found: self.type_name(body_ty),
                            });
                        }
                    } else {
                        result_ty = Some(body_ty);
                    }
                }

                result_ty.unwrap_or(self.types.unit())
            }

            Expr::IfLet {
                pattern,
                expr: scrutinee,
                then_branch,
                else_branch,
            } => {
                let scrutinee_ty = self.check_expr(scrutinee, locals);

                locals.push_scope();
                self.check_pattern(pattern, scrutinee_ty, locals);
                let then_ty = self.check_block(&then_branch.node, locals);
                locals.pop_scope();

                if let Some(else_expr) = else_branch {
                    let else_ty = self.check_expr(else_expr, locals);
                    if then_ty != else_ty
                        && then_ty != self.types.error()
                        && else_ty != self.types.error()
                        && !self.typevar_compatible(then_ty, else_ty).unwrap_or(false)
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: else_expr.span,
                            expected: self.type_name(then_ty),
                            found: self.type_name(else_ty),
                        });
                    }
                    then_ty
                } else {
                    self.types.unit()
                }
            }

            Expr::Closure {
                params,
                return_type,
                body,
            } => {
                // Type-check the closure body in a new scope.
                // If there's a type hint from the calling context, use it to
                // infer types for untyped closure parameters (e.g. implicit `it`).
                let hint_params = self.closure_type_hint.take().and_then(|hint_ty| {
                    if let TypeKind::Function { params, .. } = self.types.resolve(hint_ty).clone() {
                        Some(params)
                    } else {
                        None
                    }
                });

                locals.push_scope();
                let mut param_types = Vec::new();
                for (i, p) in params.iter().enumerate() {
                    let ty = if let Some(type_expr) = &p.ty {
                        self.resolve_type(type_expr)
                    } else if let Some(ref hint) = hint_params {
                        // Infer from expected function type
                        hint.get(i).copied().unwrap_or(self.types.i32())
                    } else {
                        // Fallback: infer as i32 if no type given
                        self.types.i32()
                    };
                    param_types.push(ty);
                    locals.insert(p.name.clone(), LocalInfo { ty, is_mut: false });
                }
                // Set up return type context for the closure body so that
                // `return` statements are checked against the closure's return
                // type, not the enclosing function's.
                let saved_return_type = self.current_return_type.take();
                if let Some(rt) = return_type {
                    self.current_return_type = Some(self.resolve_type(rt));
                }
                let body_ty = self.check_expr(body, locals);
                let closure_return_type = self.current_return_type.take();
                self.current_return_type = saved_return_type;
                locals.pop_scope();

                let ret_ty = if let Some(rt) = return_type {
                    let declared = self.resolve_type(rt);
                    // Check that the body type matches the declared return type
                    if body_ty != declared
                        && body_ty != self.types.error()
                        && declared != self.types.error()
                        && body_ty != self.types.never()
                    {
                        self.diagnostics
                            .add(&CheckerDiagnostic::ReturnTypeMismatch {
                                span: body.span,
                                expected: self.type_name(declared),
                                found: self.type_name(body_ty),
                            });
                    }
                    declared
                } else if body_ty == self.types.never() {
                    // Block ended with `return` — use the return expression type
                    closure_return_type.unwrap_or(body_ty)
                } else {
                    body_ty
                };

                self.types.intern(TypeKind::Function {
                    params: param_types,
                    ret: ret_ty,
                })
            }

            Expr::QuestionMark(inner) => {
                // ? operator: extracts value from Option/Result, or propagates error
                let inner_ty = self.check_expr(inner, locals);
                // Check if inner type is Option (Some(T)/None) or Result (Ok(T)/Err(E))
                // and extract the success type T
                if let TypeKind::Enum { name, variants } = self.types.resolve(inner_ty).clone() {
                    if name == "Option" || name.starts_with("Option$") {
                        // Option: extract the type from Some(T) variant
                        if let Some(some_variant) = variants.iter().find(|v| v.name == "Some") {
                            if let Some((_, field_ty)) = some_variant.fields.first() {
                                return *field_ty;
                            }
                        }
                    } else if name == "Result" || name.starts_with("Result$") {
                        // Result: extract the type from Ok(T) variant
                        if let Some(ok_variant) = variants.iter().find(|v| v.name == "Ok") {
                            if let Some((_, field_ty)) = ok_variant.fields.first() {
                                return *field_ty;
                            }
                        }
                    }
                }
                // Fallback: pass through the type
                inner_ty
            }
        }
    }

    fn check_pattern(
        &mut self,
        pattern: &Spanned<Pattern>,
        scrutinee_ty: TypeId,
        locals: &mut ScopedLocals<LocalInfo>,
    ) {
        match &pattern.node {
            Pattern::Wildcard => {}
            Pattern::Binding(name) => {
                locals.insert(
                    name.clone(),
                    LocalInfo {
                        ty: scrutinee_ty,
                        is_mut: false,
                    },
                );
            }
            Pattern::Literal(_) => {
                // Literal patterns - type already checked by virtue of being in a match
            }
            Pattern::Tuple(elements) => {
                if let TypeKind::Tuple(elem_types) = self.types.resolve(scrutinee_ty).clone() {
                    for (i, pat) in elements.iter().enumerate() {
                        let elem_ty = elem_types.get(i).copied().unwrap_or(self.types.error());
                        self.check_pattern(pat, elem_ty, locals);
                    }
                }
            }
            Pattern::Enum {
                enum_name,
                variant,
                fields,
            } => {
                // Find the variant and introduce bindings for its fields
                let enum_ty = if let Some(name) = enum_name {
                    // First check concrete enums, then look for monomorphized generic enums
                    if let Some(&ty) = self.enums.get(name.as_str()) {
                        Some(ty)
                    } else {
                        // Try to find the scrutinee as a monomorphized version of this generic enum
                        if self.generic_enums.contains_key(name.as_str())
                            && self.types.is_enum(scrutinee_ty)
                        {
                            Some(scrutinee_ty)
                        } else {
                            None
                        }
                    }
                } else {
                    // Infer from scrutinee
                    if self.types.is_enum(scrutinee_ty) {
                        Some(scrutinee_ty)
                    } else {
                        None
                    }
                };

                if let Some(enum_ty) = enum_ty {
                    if let TypeKind::Enum { variants, .. } = self.types.resolve(enum_ty).clone() {
                        if let Some(var) = variants.iter().find(|v| v.name == *variant) {
                            for (i, pat) in fields.iter().enumerate() {
                                let field_ty = var
                                    .fields
                                    .get(i)
                                    .map(|(_, ty)| *ty)
                                    .unwrap_or(self.types.error());
                                self.check_pattern(pat, field_ty, locals);
                            }
                        }
                    }
                } else if self.is_type_var(scrutinee_ty) || scrutinee_ty == self.types.error() {
                    // During generic body checking, the scrutinee is a TypeVar or error type.
                    // Introduce bindings with error type (will be resolved at monomorphization).
                    for pat in fields {
                        self.check_pattern(pat, self.types.error(), locals);
                    }
                }
            }
            Pattern::Struct { name, fields, .. } => {
                let struct_ty = self.structs.get(name).copied().unwrap_or(scrutinee_ty);
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
                            .unwrap_or(self.types.error());
                        self.check_pattern(pat, field_ty, locals);
                    }
                }
            }
            Pattern::Array { prefix, suffix, .. } => {
                // Extract element type from the scrutinee type
                let elem_ty = match self.types.resolve(scrutinee_ty).clone() {
                    TypeKind::DynamicArray { element } => element,
                    TypeKind::FixedArray { element, .. } => element,
                    _ => self.types.error(),
                };
                for pat in prefix.iter().chain(suffix.iter()) {
                    self.check_pattern(pat, elem_ty, locals);
                }
            }
        }
    }
}
