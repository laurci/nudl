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
                                    return v;
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
                    {
                        self.diagnostics
                            .add(&CheckerDiagnostic::ReturnTypeMismatch {
                                span: inner.span,
                                expected: self.type_name(ret_ty),
                                found: self.type_name(val_ty),
                            });
                    }
                }
                self.types.unit()
            }
            Expr::Return(None) => self.types.unit(),

            Expr::Binary { op, left, right } => {
                let left_ty = self.check_expr(left, locals);
                let right_ty = self.check_expr(right, locals);

                if left_ty == self.types.error() || right_ty == self.types.error() {
                    return self.types.error();
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
                            TypeKind::FixedArray { element, .. } => {
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
                        let is_valid = match op {
                            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                                self.is_integer_type(target_ty)
                            }
                            _ => self.is_numeric(target_ty),
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
                if cond_ty != self.types.bool() && cond_ty != self.types.error() {
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
                // Allow casts between numeric types, bool→int, char↔u32
                let is_valid = (self.is_numeric(src_ty) && self.is_numeric(dst_ty))
                    || (src_ty == self.types.bool() && self.is_integer_type(dst_ty))
                    || (src_ty == self.types.char_type() && dst_ty == self.types.u32())
                    || (src_ty == self.types.u32() && dst_ty == self.types.char_type())
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
                if cond_ty != self.types.bool() && cond_ty != self.types.error() {
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
                self.types.unit()
            }

            Expr::Continue { .. } => self.types.unit(),

            Expr::Grouped(inner) => self.check_expr(inner, locals),

            Expr::StructLiteral { name, fields } => {
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

                if !self.is_integer_type(start_ty) {
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

            Expr::Match { expr: scrutinee, arms } => {
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

                    if let Some(prev_ty) = result_ty {
                        if body_ty != prev_ty
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
                // Type-check the closure body in a new scope
                locals.push_scope();
                let mut param_types = Vec::new();
                for p in params {
                    let ty = if let Some(type_expr) = &p.ty {
                        self.resolve_type(type_expr)
                    } else {
                        // Infer as i32 if no type given (best effort)
                        self.types.i32()
                    };
                    param_types.push(ty);
                    locals.insert(p.name.clone(), LocalInfo { ty, is_mut: false });
                }
                let body_ty = self.check_expr(body, locals);
                locals.pop_scope();

                let ret_ty = if let Some(rt) = return_type {
                    self.resolve_type(rt)
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
                    if name == "Option" {
                        // Option: extract the type from Some(T) variant
                        if let Some(some_variant) = variants.iter().find(|v| v.name == "Some") {
                            if let Some((_, field_ty)) = some_variant.fields.first() {
                                return *field_ty;
                            }
                        }
                    } else if name == "Result" {
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
                    self.enums.get(name.as_str()).copied()
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
                                let field_ty =
                                    var.fields.get(i).map(|(_, ty)| *ty).unwrap_or(self.types.error());
                                self.check_pattern(pat, field_ty, locals);
                            }
                        }
                    }
                }
            }
            Pattern::Struct { name, fields, .. } => {
                let struct_ty = self.structs.get(name).copied().unwrap_or(scrutinee_ty);
                if let TypeKind::Struct { fields: struct_fields, .. } = self.types.resolve(struct_ty).clone() {
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
        }
    }
}
