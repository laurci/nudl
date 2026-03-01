use nudl_ast::ast::*;
use nudl_core::types::{PrimitiveType, TypeKind};

use super::context::FunctionLowerCtx;

/// Extract a type name from any TypeKind (structs, enums, primitives, string).
pub(crate) fn type_kind_to_name(kind: &TypeKind) -> Option<String> {
    match kind {
        TypeKind::Struct { name, .. } | TypeKind::Enum { name, .. } => Some(name.clone()),
        TypeKind::Primitive(p) => Some(
            match p {
                PrimitiveType::I8 => "i8",
                PrimitiveType::I16 => "i16",
                PrimitiveType::I32 => "i32",
                PrimitiveType::I64 => "i64",
                PrimitiveType::U8 => "u8",
                PrimitiveType::U16 => "u16",
                PrimitiveType::U32 => "u32",
                PrimitiveType::U64 => "u64",
                PrimitiveType::F32 => "f32",
                PrimitiveType::F64 => "f64",
                PrimitiveType::Bool => "bool",
                PrimitiveType::Char => "char",
                PrimitiveType::Unit => return None,
            }
            .into(),
        ),
        TypeKind::String => Some("string".into()),
        _ => None,
    }
}

impl<'a> FunctionLowerCtx<'a> {
    /// Infer the type of a struct field for a given object expression and field name.
    pub(super) fn infer_field_type(
        &mut self,
        object: &nudl_core::span::Spanned<Expr>,
        field: &str,
    ) -> Option<nudl_core::types::TypeId> {
        let type_id = self.infer_expr_type(object)?;
        match self.types.resolve(type_id) {
            nudl_core::types::TypeKind::Struct { fields, .. } => {
                fields.iter().find(|(n, _)| n == field).map(|(_, tid)| *tid)
            }
            _ => None,
        }
    }

    /// Resolve the field index for a field access expression.
    pub(super) fn resolve_field_index(
        &mut self,
        object: &nudl_core::span::Spanned<Expr>,
        field: &str,
    ) -> u32 {
        // Walk the object expression to find its type
        let type_id = self.infer_expr_type(object);
        if let Some(tid) = type_id {
            if let nudl_core::types::TypeKind::Struct { fields, .. } = self.types.resolve(tid) {
                if let Some(idx) = fields.iter().position(|(n, _)| n == field) {
                    return idx as u32;
                }
            }
        }
        0 // fallback (should have been caught by checker)
    }

    /// Resolve a TypeExpr to a TypeId (mirrors checker.rs resolve_type).
    pub(super) fn resolve_type_expr(&mut self, ty: &TypeExpr) -> nudl_core::types::TypeId {
        match ty {
            TypeExpr::Unit => self.types.unit(),
            TypeExpr::Named(name) => match name.as_str() {
                "i8" => self.types.i8(),
                "i16" => self.types.i16(),
                "i32" => self.types.i32(),
                "i64" => self.types.i64(),
                "u8" => self.types.u8(),
                "u16" => self.types.u16(),
                "u32" => self.types.u32(),
                "u64" => self.types.u64(),
                "f32" => self.types.f32(),
                "f64" => self.types.f64(),
                "bool" => self.types.bool(),
                "char" => self.types.char_type(),
                "string" => self.types.string(),
                _ => {
                    // Check type parameter substitution map first (for monomorphized functions)
                    if let Some(&tid) = self.type_param_subst.get(name.as_str()) {
                        tid
                    } else if let Some(&tid) = self.struct_defs.get(name.as_str()) {
                        tid
                    } else if let Some(&tid) = self.enum_defs.get(name.as_str()) {
                        tid
                    } else if self.interface_methods.contains_key(name.as_str()) {
                        // Interface name used as a type → DynInterface
                        self.types
                            .intern(nudl_core::types::TypeKind::DynInterface { name: name.clone() })
                    } else {
                        self.lowering_warnings.push(format!(
                            "unresolved named type '{}', falling back to i64",
                            name
                        ));
                        self.types.i64()
                    }
                }
            },
            TypeExpr::Tuple(elements) => {
                let elem_types: Vec<nudl_core::types::TypeId> = elements
                    .iter()
                    .map(|e| self.resolve_type_expr(&e.node))
                    .collect();
                self.types
                    .intern(nudl_core::types::TypeKind::Tuple(elem_types))
            }
            TypeExpr::FixedArray { element, length } => {
                let elem_ty = self.resolve_type_expr(&element.node);
                self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_ty,
                    length: *length,
                })
            }
            TypeExpr::Generic { name, args } => {
                match name.as_str() {
                    "Map" if args.len() == 2 => {
                        let key = self.resolve_type_expr(&args[0].node);
                        let value = self.resolve_type_expr(&args[1].node);
                        self.types
                            .intern(nudl_core::types::TypeKind::Map { key, value })
                    }
                    _ => {
                        // Resolve generic args and construct mangled name for lookup
                        let resolved_args: Vec<nudl_core::types::TypeId> = args
                            .iter()
                            .map(|a| self.resolve_type_expr(&a.node))
                            .collect();
                        let mangled = self.mangle_type_name(name, &resolved_args);
                        // Try struct or enum with mangled name
                        if let Some(&tid) = self.struct_defs.get(&mangled) {
                            tid
                        } else if let Some(&tid) = self.enum_defs.get(&mangled) {
                            tid
                        } else if let Some(&tid) = self.struct_defs.get(name.as_str()) {
                            tid
                        } else if let Some(&tid) = self.enum_defs.get(name.as_str()) {
                            tid
                        } else {
                            self.lowering_warnings.push(format!(
                                "unresolved generic type '{}', falling back to i64",
                                name
                            ));
                            self.types.i64()
                        }
                    }
                }
            }
            TypeExpr::DynamicArray { element } => {
                let elem_ty = self.resolve_type_expr(&element.node);
                self.types
                    .intern(nudl_core::types::TypeKind::DynamicArray { element: elem_ty })
            }
            TypeExpr::DynInterface { name } => self
                .types
                .intern(nudl_core::types::TypeKind::DynInterface { name: name.clone() }),
            TypeExpr::FnType {
                params,
                return_type,
            } => {
                let param_types: Vec<nudl_core::types::TypeId> = params
                    .iter()
                    .map(|p| self.resolve_type_expr(&p.node))
                    .collect();
                let ret = self.resolve_type_expr(&return_type.node);
                self.types.intern(nudl_core::types::TypeKind::Function {
                    params: param_types,
                    ret,
                })
            }
            TypeExpr::ImplInterface { .. } => {
                // impl Trait should be desugared before lowering
                self.types.error()
            }
        }
    }

    /// Best-effort type inference for an expression (used in lowerer for field lookups).
    pub(super) fn infer_expr_type(
        &mut self,
        expr: &nudl_core::span::Spanned<Expr>,
    ) -> Option<nudl_core::types::TypeId> {
        match &expr.node {
            Expr::Ident(name) => self.local_types.get(name).copied(),
            Expr::StructLiteral { name, .. } => {
                // Check if this was resolved to a monomorphized struct
                let resolved_name = self
                    .struct_resolutions
                    .get(&expr.span)
                    .map(|s| s.as_str())
                    .unwrap_or(name.as_str());
                self.struct_defs
                    .get(resolved_name)
                    .or_else(|| self.enum_defs.get(resolved_name))
                    .copied()
            }
            Expr::EnumLiteral { enum_name, .. } => {
                // Check if this was resolved to a monomorphized enum
                let resolved_name = self
                    .enum_resolutions
                    .get(&expr.span)
                    .map(|s| s.as_str())
                    .unwrap_or(enum_name.as_str());
                self.enum_defs.get(resolved_name).copied()
            }
            Expr::FieldAccess { object, field } => {
                let obj_type = self.infer_expr_type(object)?;
                match self.types.resolve(obj_type) {
                    nudl_core::types::TypeKind::Struct { fields, .. } => {
                        fields.iter().find(|(n, _)| n == field).map(|(_, ty)| *ty)
                    }
                    nudl_core::types::TypeKind::Tuple(elements) => field
                        .parse::<usize>()
                        .ok()
                        .and_then(|idx| elements.get(idx).copied()),
                    _ => None,
                }
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name) = &callee.node {
                    // Check if this call was resolved to a monomorphized function
                    let key = (self.current_fn_name.clone(), expr.span);
                    let resolved_name = self
                        .call_resolutions
                        .get(&key)
                        .map(|s| s.as_str())
                        .unwrap_or(name.as_str());
                    if let Some(sig) = self.function_sigs.get(resolved_name) {
                        Some(sig.return_type)
                    } else if let Some(callee_ty) = self.local_types.get(name) {
                        // Calling a local variable (closure): extract return type
                        if let nudl_core::types::TypeKind::Function { ret, .. } =
                            self.types.resolve(*callee_ty)
                        {
                            Some(*ret)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Expr::MethodCall { object, method, .. } => {
                // Check built-in methods on known types first
                if let Some(obj_ty) = self.infer_expr_type(object) {
                    match self.types.resolve(obj_ty).clone() {
                        nudl_core::types::TypeKind::Map { value, .. } => {
                            match method.as_str() {
                                "get" => {
                                    // map.get() returns Option<V>
                                    return self.find_option_type(value);
                                }
                                "contains_key" | "remove" => return Some(self.types.bool()),
                                "insert" => return Some(self.types.unit()),
                                "len" => return Some(self.types.i64()),
                                _ => {}
                            }
                        }
                        nudl_core::types::TypeKind::DynamicArray { element } => {
                            match method.as_str() {
                                "pop" => return Some(element),
                                "len" => return Some(self.types.i64()),
                                "push" => return Some(self.types.unit()),
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                let type_name = self.infer_receiver_type_name(object)?;
                let mangled = format!("{}__{}", type_name, method);
                self.function_sigs.get(&mangled).map(|sig| sig.return_type)
            }
            Expr::StaticCall {
                type_name, method, ..
            } => {
                // Check if this was resolved to a monomorphized enum
                let resolved_name = self
                    .enum_resolutions
                    .get(&expr.span)
                    .map(|s| s.as_str())
                    .unwrap_or(type_name.as_str());
                // Check if this is an enum variant constructor
                if let Some(&enum_ty) = self.enum_defs.get(resolved_name) {
                    if let nudl_core::types::TypeKind::Enum { variants, .. } =
                        self.types.resolve(enum_ty)
                    {
                        if variants.iter().any(|v| v.name == *method) {
                            return Some(enum_ty);
                        }
                    }
                }
                // Handle Map::new() built-in
                if type_name == "Map" && method == "new" {
                    let key_ty = self.types.i64();
                    let val_ty = self.types.i64();
                    return Some(self.types.intern(nudl_core::types::TypeKind::Map {
                        key: key_ty,
                        value: val_ty,
                    }));
                }
                // Check call_resolutions for monomorphized static methods
                let key = (self.current_fn_name.clone(), expr.span);
                let mangled = self
                    .call_resolutions
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| format!("{}__{}", type_name, method));
                self.function_sigs.get(&mangled).map(|sig| sig.return_type)
            }
            Expr::TupleLiteral(elements) => {
                let elem_types: Vec<nudl_core::types::TypeId> = elements
                    .iter()
                    .filter_map(|e| self.infer_expr_type(e))
                    .collect();
                if elem_types.len() == elements.len() {
                    Some(
                        self.types
                            .intern(nudl_core::types::TypeKind::Tuple(elem_types)),
                    )
                } else {
                    None
                }
            }
            Expr::ArrayLiteral(elements) => {
                if elements.is_empty() {
                    return None;
                }
                let elem_type = self.infer_expr_type(&elements[0])?;
                Some(self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_type,
                    length: elements.len(),
                }))
            }
            Expr::ArrayRepeat { value, count } => {
                let elem_type = self.infer_expr_type(value)?;
                Some(self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_type,
                    length: *count,
                }))
            }
            Expr::IndexAccess { object, .. } => {
                let obj_type = self.infer_expr_type(object)?;
                match self.types.resolve(obj_type) {
                    nudl_core::types::TypeKind::FixedArray { element, .. } => Some(*element),
                    nudl_core::types::TypeKind::DynamicArray { element } => Some(*element),
                    nudl_core::types::TypeKind::String => Some(self.types.char_type()),
                    nudl_core::types::TypeKind::Map { value, .. } => Some(*value),
                    _ => None,
                }
            }
            Expr::Literal(Literal::Int(_, Some(suffix))) => Some(match suffix {
                IntSuffix::I8 => self.types.i8(),
                IntSuffix::I16 => self.types.i16(),
                IntSuffix::I32 => self.types.i32(),
                IntSuffix::I64 => self.types.i64(),
                IntSuffix::U8 => self.types.u8(),
                IntSuffix::U16 => self.types.u16(),
                IntSuffix::U32 => self.types.u32(),
                IntSuffix::U64 => self.types.u64(),
            }),
            Expr::Literal(Literal::Int(_, None)) => Some(self.types.i32()),
            Expr::Literal(Literal::Float(_)) => Some(self.types.f64()),
            Expr::Literal(Literal::Bool(_)) => Some(self.types.bool()),
            Expr::Literal(Literal::Char(_)) => Some(self.types.char_type()),
            Expr::Literal(Literal::String(_)) => Some(self.types.string()),
            Expr::Closure {
                params,
                return_type,
                ..
            } => {
                let hint_params = self.closure_type_hint.as_ref().and_then(|hint_ty| {
                    if let nudl_core::types::TypeKind::Function { params, .. } =
                        self.types.resolve(*hint_ty).clone()
                    {
                        Some(params)
                    } else {
                        None
                    }
                });
                let param_types: Vec<nudl_core::types::TypeId> = params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        if let Some(ty_expr) = &p.ty {
                            self.resolve_type_expr(&ty_expr.node)
                        } else if let Some(ref hint) = hint_params {
                            hint.get(i).copied().unwrap_or(self.types.i32())
                        } else {
                            self.types.i32()
                        }
                    })
                    .collect();
                let ret = if let Some(rt) = return_type {
                    self.resolve_type_expr(&rt.node)
                } else {
                    self.types.i64()
                };
                Some(self.types.intern(nudl_core::types::TypeKind::Function {
                    params: param_types,
                    ret,
                }))
            }
            Expr::Cast { target_type, .. } => Some(self.resolve_type_expr(&target_type.node)),
            Expr::Binary { op, left, .. } => {
                use nudl_ast::ast::BinOp;
                match op {
                    // Comparisons and logical ops always return bool
                    BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
                    | BinOp::And
                    | BinOp::Or => Some(self.types.bool()),
                    // Arithmetic: result type matches operands
                    _ => self.infer_expr_type(left),
                }
            }
            Expr::Unary { operand, .. } => self.infer_expr_type(operand),
            Expr::If {
                then_branch,
                else_branch,
                ..
            } => {
                // Try then branch, fall back to else
                then_branch
                    .node
                    .tail_expr
                    .as_ref()
                    .and_then(|e| self.infer_expr_type(e))
                    .or_else(|| {
                        else_branch.as_ref().and_then(|eb| match &eb.node {
                            Expr::Block(block) => block
                                .tail_expr
                                .as_ref()
                                .and_then(|e| self.infer_expr_type(e)),
                            _ => self.infer_expr_type(eb),
                        })
                    })
            }
            Expr::Block(block) => block
                .tail_expr
                .as_ref()
                .and_then(|e| self.infer_expr_type(e)),
            _ => None,
        }
    }

    /// Infer element type for index access operations.
    pub(super) fn infer_index_element_type(
        &mut self,
        object: &nudl_core::span::Spanned<Expr>,
    ) -> nudl_core::types::TypeId {
        if let Some(obj_type) = self.infer_expr_type(object) {
            match self.types.resolve(obj_type) {
                nudl_core::types::TypeKind::FixedArray { element, .. } => return *element,
                nudl_core::types::TypeKind::DynamicArray { element } => return *element,
                _ => {}
            }
        }
        self.lowering_warnings
            .push("could not infer index element type, falling back to i64".to_string());
        self.types.i64()
    }

    /// Infer the type name of the receiver expression for method resolution.
    pub(super) fn infer_receiver_type_name(
        &self,
        expr: &nudl_core::span::Spanned<Expr>,
    ) -> Option<String> {
        match &expr.node {
            Expr::Ident(name) => {
                if let Some(ty_id) = self.local_types.get(name) {
                    if let Some(n) = type_kind_to_name(self.types.resolve(*ty_id)) {
                        return Some(n);
                    }
                }
                None
            }
            Expr::StructLiteral { name, .. } => {
                // Check if this was resolved to a monomorphized struct
                let resolved = self
                    .struct_resolutions
                    .get(&expr.span)
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                Some(resolved)
            }
            Expr::StaticCall {
                type_name, method, ..
            } => {
                // Check if this was resolved to a monomorphized enum
                let resolved_name = self
                    .enum_resolutions
                    .get(&expr.span)
                    .map(|s| s.as_str())
                    .unwrap_or(type_name.as_str());
                // Check if this is an enum variant constructor
                if let Some(&enum_ty) = self.enum_defs.get(resolved_name) {
                    if let TypeKind::Enum { name, variants, .. } = self.types.resolve(enum_ty) {
                        if variants.iter().any(|v| v.name == *method) {
                            return Some(name.clone());
                        }
                    }
                }
                let mangled = format!("{}__{}", type_name, method);
                if let Some(sig) = self.function_sigs.get(&mangled) {
                    if let Some(n) = type_kind_to_name(self.types.resolve(sig.return_type)) {
                        return Some(n);
                    }
                }
                None
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(fn_name) = &callee.node {
                    if let Some(sig) = self.function_sigs.get(fn_name.as_str()) {
                        if let Some(n) = type_kind_to_name(self.types.resolve(sig.return_type)) {
                            return Some(n);
                        }
                    }
                }
                None
            }
            Expr::MethodCall { object, method, .. } => {
                // Check built-in methods on known types first
                // For map.get() → returns Option<V>
                let obj_ty = if let Expr::Ident(name) = &object.node {
                    self.local_types.get(name).copied()
                } else {
                    None
                };
                if let Some(obj_ty) = obj_ty {
                    if let TypeKind::Map { value, .. } = self.types.resolve(obj_ty).clone() {
                        if method == "get" {
                            if let Some(option_ty) = self.find_option_type(value) {
                                if let TypeKind::Enum { name, .. } = self.types.resolve(option_ty) {
                                    return Some(name.clone());
                                }
                            }
                        }
                    }
                }
                // Recurse: infer receiver type, then look up method return type
                if let Some(type_name) = self.infer_receiver_type_name(object) {
                    let mangled = format!("{}__{}", type_name, method);
                    if let Some(sig) = self.function_sigs.get(&mangled) {
                        if let Some(n) = type_kind_to_name(self.types.resolve(sig.return_type)) {
                            return Some(n);
                        }
                    }
                }
                None
            }
            Expr::FieldAccess { object, field } => {
                // For field/tuple access (e.g., `tuple.0.method()`), determine
                // the object's type and look up the field's type.
                // First try: if object is an ident, look up its type directly.
                let obj_type = if let Expr::Ident(name) = &object.node {
                    self.local_types.get(name).copied()
                } else {
                    // For more complex expressions, try to get the type via
                    // the receiver type name (struct/enum) or known patterns.
                    if let Some(type_name) = self.infer_receiver_type_name(object) {
                        self.struct_defs
                            .get(&type_name)
                            .or_else(|| self.enum_defs.get(&type_name))
                            .copied()
                    } else {
                        None
                    }
                };
                if let Some(ty_id) = obj_type {
                    let field_ty = match self.types.resolve(ty_id) {
                        TypeKind::Struct { fields, .. } => {
                            fields.iter().find(|(n, _)| n == field).map(|(_, ty)| *ty)
                        }
                        TypeKind::Tuple(elements) => field
                            .parse::<usize>()
                            .ok()
                            .and_then(|idx| elements.get(idx).copied()),
                        _ => None,
                    };
                    if let Some(fty) = field_ty {
                        if let Some(n) = type_kind_to_name(self.types.resolve(fty)) {
                            return Some(n);
                        }
                    }
                }
                None
            }
            Expr::Literal(lit) => match lit {
                Literal::Int(_, Some(suffix)) => Some(
                    match suffix {
                        IntSuffix::I8 => "i8",
                        IntSuffix::I16 => "i16",
                        IntSuffix::I32 => "i32",
                        IntSuffix::I64 => "i64",
                        IntSuffix::U8 => "u8",
                        IntSuffix::U16 => "u16",
                        IntSuffix::U32 => "u32",
                        IntSuffix::U64 => "u64",
                    }
                    .into(),
                ),
                Literal::Int(_, None) => Some("i32".into()),
                Literal::Float(_) => Some("f64".into()),
                Literal::Bool(_) => Some("bool".into()),
                Literal::Char(_) => Some("char".into()),
                Literal::String(_) | Literal::TemplateString { .. } => Some("string".into()),
            },
            _ => None,
        }
    }

    /// Resolve the return type for a DynCall from interface method signatures.
    /// Looks up the interface method defs via dyn_call_resolutions and resolves the return type.
    pub(super) fn infer_dyn_call_return_type(
        &mut self,
        span: &nudl_core::span::Span,
    ) -> Option<nudl_core::types::TypeId> {
        let (iface_name, method_idx) = self.dyn_call_resolutions.get(span)?.clone();
        let methods = self.interface_methods.get(&iface_name)?;
        let _method_name = methods.get(method_idx)?;
        // Find any concrete implementation to get the return type from its function signature
        let impls = self.interface_impls.get(&iface_name)?;
        let first_concrete = impls.first()?;
        let mangled = format!("{}__{}", first_concrete, _method_name);
        self.function_sigs.get(&mangled).map(|sig| sig.return_type)
    }

    /// Find the monomorphized Option<V> enum type for a given value type.
    /// Searches enum_defs for an Option$* enum whose Some variant's field matches val_ty.
    pub(super) fn find_option_type(
        &self,
        val_ty: nudl_core::types::TypeId,
    ) -> Option<nudl_core::types::TypeId> {
        for (name, &enum_ty) in self.enum_defs.iter() {
            if !name.starts_with("Option$") {
                continue;
            }
            if let nudl_core::types::TypeKind::Enum { variants, .. } = self.types.resolve(enum_ty) {
                if let Some(some_var) = variants.iter().find(|v| v.name == "Some") {
                    if some_var.fields.len() == 1 && some_var.fields[0].1 == val_ty {
                        return Some(enum_ty);
                    }
                }
            }
        }
        None
    }
}
