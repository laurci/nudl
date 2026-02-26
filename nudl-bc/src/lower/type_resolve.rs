use nudl_ast::ast::*;
use nudl_core::types::TypeKind;

use super::context::FunctionLowerCtx;

impl<'a> FunctionLowerCtx<'a> {
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
                    if let Some(&tid) = self.struct_defs.get(name.as_str()) {
                        tid
                    } else if let Some(&tid) = self.enum_defs.get(name.as_str()) {
                        tid
                    } else {
                        self.types.i64() // fallback
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
                        self.types.intern(nudl_core::types::TypeKind::Map { key, value })
                    }
                    _ => {
                        // Try struct or enum
                        if let Some(&tid) = self.struct_defs.get(name.as_str()) {
                            tid
                        } else if let Some(&tid) = self.enum_defs.get(name.as_str()) {
                            tid
                        } else {
                            self.types.i64() // fallback
                        }
                    }
                }
            }
            TypeExpr::DynamicArray { element } => {
                let elem_ty = self.resolve_type_expr(&element.node);
                self.types.intern(nudl_core::types::TypeKind::DynamicArray {
                    element: elem_ty,
                })
            }
            TypeExpr::DynInterface { name } => {
                self.types.intern(nudl_core::types::TypeKind::DynInterface { name: name.clone() })
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
            Expr::StructLiteral { name, .. } => self
                .struct_defs
                .get(name.as_str())
                .or_else(|| self.enum_defs.get(name.as_str()))
                .copied(),
            Expr::EnumLiteral { enum_name, .. } => self.enum_defs.get(enum_name.as_str()).copied(),
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
                    self.function_sigs.get(name).map(|sig| sig.return_type)
                } else {
                    None
                }
            }
            Expr::MethodCall { object, method, .. } => {
                let type_name = self.infer_receiver_type_name(object)?;
                let mangled = format!("{}__{}", type_name, method);
                self.function_sigs.get(&mangled).map(|sig| sig.return_type)
            }
            Expr::StaticCall {
                type_name, method, ..
            } => {
                // Check if this is an enum variant constructor
                if let Some(&enum_ty) = self.enum_defs.get(type_name.as_str()) {
                    if let nudl_core::types::TypeKind::Enum { variants, .. } =
                        self.types.resolve(enum_ty)
                    {
                        if variants.iter().any(|v| v.name == *method) {
                            return Some(enum_ty);
                        }
                    }
                }
                let mangled = format!("{}__{}", type_name, method);
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
        self.types.i64() // fallback
    }

    /// Infer the type name of the receiver expression for method resolution.
    pub(super) fn infer_receiver_type_name(
        &self,
        expr: &nudl_core::span::Spanned<Expr>,
    ) -> Option<String> {
        match &expr.node {
            Expr::Ident(name) => {
                if let Some(ty_id) = self.local_types.get(name) {
                    match self.types.resolve(*ty_id) {
                        TypeKind::Struct { name, .. } => return Some(name.clone()),
                        TypeKind::Enum { name, .. } => return Some(name.clone()),
                        _ => {}
                    }
                }
                None
            }
            Expr::StructLiteral { name, .. } => Some(name.clone()),
            Expr::StaticCall {
                type_name, method, ..
            } => {
                // Check if this is an enum variant constructor
                if let Some(&enum_ty) = self.enum_defs.get(type_name.as_str()) {
                    if let TypeKind::Enum { name, variants, .. } =
                        self.types.resolve(enum_ty)
                    {
                        if variants.iter().any(|v| v.name == *method) {
                            return Some(name.clone());
                        }
                    }
                }
                let mangled = format!("{}__{}", type_name, method);
                if let Some(sig) = self.function_sigs.get(&mangled) {
                    match self.types.resolve(sig.return_type) {
                        TypeKind::Struct { name, .. } => return Some(name.clone()),
                        TypeKind::Enum { name, .. } => return Some(name.clone()),
                        _ => {}
                    }
                }
                None
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(fn_name) = &callee.node {
                    if let Some(sig) = self.function_sigs.get(fn_name.as_str()) {
                        match self.types.resolve(sig.return_type) {
                            TypeKind::Struct { name, .. } => return Some(name.clone()),
                            TypeKind::Enum { name, .. } => return Some(name.clone()),
                            _ => {}
                        }
                    }
                }
                None
            }
            Expr::MethodCall { object, method, .. } => {
                // Recurse: infer receiver type, then look up method return type
                if let Some(type_name) = self.infer_receiver_type_name(object) {
                    let mangled = format!("{}__{}", type_name, method);
                    if let Some(sig) = self.function_sigs.get(&mangled) {
                        match self.types.resolve(sig.return_type) {
                            TypeKind::Struct { name, .. } => return Some(name.clone()),
                            TypeKind::Enum { name, .. } => return Some(name.clone()),
                            _ => {}
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}
