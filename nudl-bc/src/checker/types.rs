use super::*;

impl Checker {
    pub(super) fn register_builtins(&mut self) {
        let string_ty = self.types.string();
        let raw_ptr_ty = self.types.raw_ptr();
        let u64_ty = self.types.u64();

        self.functions.insert(
            "__str_ptr".into(),
            FunctionSig {
                name: "__str_ptr".into(),
                params: vec![("s".into(), string_ty)],
                return_type: raw_ptr_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
            },
        );

        self.functions.insert(
            "__str_len".into(),
            FunctionSig {
                name: "__str_len".into(),
                params: vec![("s".into(), string_ty)],
                return_type: u64_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
            },
        );
    }

    pub(super) fn resolve_type(&mut self, ty: &Spanned<TypeExpr>) -> TypeId {
        match &ty.node {
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
                "RawPtr" => self.types.raw_ptr(),
                "MutRawPtr" => self.types.mut_raw_ptr(),
                "CStr" => self.types.cstr(),
                _ => {
                    if let Some(&struct_ty) = self.structs.get(name.as_str()) {
                        return struct_ty;
                    }
                    if let Some(&enum_ty) = self.enums.get(name.as_str()) {
                        return enum_ty;
                    }
                    if let Some(&iface_ty) = self.interfaces.get(name.as_str()) {
                        return iface_ty;
                    }
                    self.diagnostics.add(&CheckerDiagnostic::UnknownType {
                        span: ty.span,
                        name: name.clone(),
                    });
                    self.types.error()
                }
            },
            TypeExpr::Tuple(elements) => {
                let element_types: Vec<TypeId> =
                    elements.iter().map(|e| self.resolve_type(e)).collect();
                self.types.intern(TypeKind::Tuple(element_types))
            }
            TypeExpr::FixedArray { element, length } => {
                let elem_ty = self.resolve_type(element);
                self.types.intern(TypeKind::FixedArray {
                    element: elem_ty,
                    length: *length,
                })
            }
            TypeExpr::Generic { name, args } => {
                match name.as_str() {
                    "Map" if args.len() == 2 => {
                        let key = self.resolve_type(&args[0]);
                        let value = self.resolve_type(&args[1]);
                        self.types.intern(TypeKind::Map { key, value })
                    }
                    _ => {
                        // For generic types, resolve args but return the base type
                        // This is a simplified approach; full generics would need monomorphization
                        for arg in args {
                            self.resolve_type(arg);
                        }
                        // Try to find as a struct or enum name
                        if let Some(&ty) = self.structs.get(name.as_str()) {
                            return ty;
                        }
                        if let Some(&ty) = self.enums.get(name.as_str()) {
                            return ty;
                        }
                        self.diagnostics.add(&CheckerDiagnostic::UnknownType {
                            span: ty.span,
                            name: name.clone(),
                        });
                        self.types.error()
                    }
                }
            }
            TypeExpr::DynamicArray { element } => {
                let elem_ty = self.resolve_type(element);
                self.types.intern(TypeKind::DynamicArray { element: elem_ty })
            }
            TypeExpr::DynInterface { name } => {
                self.types.intern(TypeKind::DynInterface { name: name.clone() })
            }
        }
    }

    pub(super) fn type_name(&self, ty: TypeId) -> String {
        match self.types.resolve(ty) {
            TypeKind::Primitive(p) => match p {
                PrimitiveType::Char => "char".into(),
                p => format!("{:?}", p).to_lowercase(),
            },
            TypeKind::String => "string".into(),
            TypeKind::RawPtr => "RawPtr".into(),
            TypeKind::MutRawPtr => "MutRawPtr".into(),
            TypeKind::CStr => "CStr".into(),
            TypeKind::Never => "!".into(),
            TypeKind::Function { .. } => "fn(...)".into(),
            TypeKind::Struct { name, .. } => name.clone(),
            TypeKind::Enum { name, .. } => name.clone(),
            TypeKind::Interface { name, .. } => name.clone(),
            TypeKind::DynInterface { name } => format!("dyn {}", name),
            TypeKind::Tuple(elements) => {
                let parts: Vec<String> = elements.iter().map(|e| self.type_name(*e)).collect();
                format!("({})", parts.join(", "))
            }
            TypeKind::FixedArray { element, length } => {
                format!("[{}; {}]", self.type_name(*element), length)
            }
            TypeKind::DynamicArray { element } => {
                format!("{}[]", self.type_name(*element))
            }
            TypeKind::Map { key, value } => {
                format!("Map<{}, {}>", self.type_name(*key), self.type_name(*value))
            }
            TypeKind::Error => "<error>".into(),
        }
    }

    pub(super) fn is_numeric(&self, ty: TypeId) -> bool {
        matches!(
            self.types.resolve(ty),
            TypeKind::Primitive(p) if p.is_numeric()
        )
    }

    pub(super) fn is_integer_type(&self, ty: TypeId) -> bool {
        matches!(
            self.types.resolve(ty),
            TypeKind::Primitive(p) if p.is_integer()
        )
    }

    pub(super) fn is_unsuffixed_int_literal(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Literal(Literal::Int(_, None)))
            || matches!(expr, Expr::Unary { op: UnaryOp::Neg, operand } if matches!(&operand.node, Expr::Literal(Literal::Int(_, None))))
    }

    pub(super) fn is_signed_or_float(&self, ty: TypeId) -> bool {
        matches!(
            self.types.resolve(ty),
            TypeKind::Primitive(p) if p.is_signed()
        )
    }
}
