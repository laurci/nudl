use nudl_core::types::EnumVariant;

use super::*;

impl Checker {
    pub(super) fn register_builtins(&mut self) {
        let string_ty = self.types.string();
        let raw_ptr_ty = self.types.raw_ptr();
        let u64_ty = self.types.u64();
        let i32_ty = self.types.i32();
        let i64_ty = self.types.i64();
        let f64_ty = self.types.f64();
        let bool_ty = self.types.bool();
        let char_ty = self.types.char_type();
        let unit_ty = self.types.unit();
        let never_ty = self.types.never();

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
                generic_def: None,
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
                generic_def: None,
            },
        );

        // String concatenation builtin
        self.functions.insert(
            "__str_concat".into(),
            FunctionSig {
                name: "__str_concat".into(),
                params: vec![("a".into(), string_ty), ("b".into(), string_ty)],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 2,
                has_default: vec![false, false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );

        // Conversion builtins for template string interpolation
        self.functions.insert(
            "__i32_to_str".into(),
            FunctionSig {
                name: "__i32_to_str".into(),
                params: vec![("v".into(), i32_ty)],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );

        self.functions.insert(
            "__i64_to_str".into(),
            FunctionSig {
                name: "__i64_to_str".into(),
                params: vec![("v".into(), i64_ty)],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );

        self.functions.insert(
            "__f64_to_str".into(),
            FunctionSig {
                name: "__f64_to_str".into(),
                params: vec![("v".into(), f64_ty)],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );

        self.functions.insert(
            "__bool_to_str".into(),
            FunctionSig {
                name: "__bool_to_str".into(),
                params: vec![("v".into(), bool_ty)],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );

        self.functions.insert(
            "__char_to_str".into(),
            FunctionSig {
                name: "__char_to_str".into(),
                params: vec![("v".into(), char_ty)],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );

        // Panic builtin: panic(msg: string) -> !
        self.functions.insert(
            "panic".into(),
            FunctionSig {
                name: "panic".into(),
                params: vec![("msg".into(), string_ty)],
                return_type: never_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );

        // Assert builtin: assert(condition: bool, msg: string)
        self.functions.insert(
            "assert".into(),
            FunctionSig {
                name: "assert".into(),
                params: vec![("condition".into(), bool_ty), ("msg".into(), string_ty)],
                return_type: unit_ty,
                kind: FunctionKind::Builtin,
                required_params: 2,
                has_default: vec![false, false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );

        // Exit builtin: exit(code: i32) -> !
        self.functions.insert(
            "exit".into(),
            FunctionSig {
                name: "exit".into(),
                params: vec![("code".into(), i32_ty)],
                return_type: never_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );

        // cptr builtin: cptr(value) -> RawPtr
        // Takes any value type and returns a raw pointer to a C-layout copy.
        // Type-checking is lenient: accepts any single argument.
        self.functions.insert(
            "cptr".into(),
            FunctionSig {
                name: "cptr".into(),
                params: vec![("value".into(), i64_ty)],
                return_type: raw_ptr_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );

        // String operation builtins
        // __str_substr(s: string, start: i64, end: i64) -> string
        self.functions.insert(
            "__str_substr".into(),
            FunctionSig {
                name: "__str_substr".into(),
                params: vec![
                    ("s".into(), string_ty),
                    ("start".into(), i64_ty),
                    ("end".into(), i64_ty),
                ],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 3,
                has_default: vec![false, false, false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );
        // __str_indexof(haystack: string, needle: string) -> i64
        self.functions.insert(
            "__str_indexof".into(),
            FunctionSig {
                name: "__str_indexof".into(),
                params: vec![("haystack".into(), string_ty), ("needle".into(), string_ty)],
                return_type: i64_ty,
                kind: FunctionKind::Builtin,
                required_params: 2,
                has_default: vec![false, false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );
        // __str_trim(s: string) -> string
        self.functions.insert(
            "__str_trim".into(),
            FunctionSig {
                name: "__str_trim".into(),
                params: vec![("s".into(), string_ty)],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );
        // __str_contains(haystack: string, needle: string) -> i64
        self.functions.insert(
            "__str_contains".into(),
            FunctionSig {
                name: "__str_contains".into(),
                params: vec![("haystack".into(), string_ty), ("needle".into(), string_ty)],
                return_type: i64_ty,
                kind: FunctionKind::Builtin,
                required_params: 2,
                has_default: vec![false, false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );
        // __str_starts_with(s: string, prefix: string) -> i64
        self.functions.insert(
            "__str_starts_with".into(),
            FunctionSig {
                name: "__str_starts_with".into(),
                params: vec![("s".into(), string_ty), ("prefix".into(), string_ty)],
                return_type: i64_ty,
                kind: FunctionKind::Builtin,
                required_params: 2,
                has_default: vec![false, false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );
        // __str_ends_with(s: string, suffix: string) -> i64
        self.functions.insert(
            "__str_ends_with".into(),
            FunctionSig {
                name: "__str_ends_with".into(),
                params: vec![("s".into(), string_ty), ("suffix".into(), string_ty)],
                return_type: i64_ty,
                kind: FunctionKind::Builtin,
                required_params: 2,
                has_default: vec![false, false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );
        // __str_to_upper(s: string) -> string
        self.functions.insert(
            "__str_to_upper".into(),
            FunctionSig {
                name: "__str_to_upper".into(),
                params: vec![("s".into(), string_ty)],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );
        // __str_to_lower(s: string) -> string
        self.functions.insert(
            "__str_to_lower".into(),
            FunctionSig {
                name: "__str_to_lower".into(),
                params: vec![("s".into(), string_ty)],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 1,
                has_default: vec![false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );
        // __str_replace(s: string, old: string, new_str: string) -> string
        self.functions.insert(
            "__str_replace".into(),
            FunctionSig {
                name: "__str_replace".into(),
                params: vec![
                    ("s".into(), string_ty),
                    ("old".into(), string_ty),
                    ("new_str".into(), string_ty),
                ],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 3,
                has_default: vec![false, false, false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
            },
        );
        // __str_repeat(s: string, count: i64) -> string
        self.functions.insert(
            "__str_repeat".into(),
            FunctionSig {
                name: "__str_repeat".into(),
                params: vec![("s".into(), string_ty), ("count".into(), i64_ty)],
                return_type: string_ty,
                kind: FunctionKind::Builtin,
                required_params: 2,
                has_default: vec![false, false],
                is_method: false,
                is_mut_method: false,
                generic_def: None,
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
                // Self type — resolved via type_param_scope when inside impl blocks
                "Self" => {
                    if let Some(&ty) = self.type_param_scope.get("Self") {
                        return ty;
                    }
                    // Fallback for interface method signatures
                    self.types.unit()
                }
                _ => {
                    // Check type parameter scope (for generic body checking)
                    if let Some(&type_var) = self.type_param_scope.get(name.as_str()) {
                        return type_var;
                    }
                    if let Some(&struct_ty) = self.structs.get(name.as_str()) {
                        return struct_ty;
                    }
                    if let Some(&enum_ty) = self.enums.get(name.as_str()) {
                        return enum_ty;
                    }
                    if let Some(&iface_ty) = self.interfaces.get(name.as_str()) {
                        return iface_ty;
                    }
                    if let Some(&alias_ty) = self.type_aliases.get(name.as_str()) {
                        return alias_ty;
                    }
                    // Check if it's a generic struct/enum used without type args
                    if self.generic_structs.contains_key(name.as_str())
                        || self.generic_enums.contains_key(name.as_str())
                    {
                        self.diagnostics
                            .add(&CheckerDiagnostic::TypeArgCountMismatch {
                                span: ty.span,
                                expected: 1, // approximate
                                found: 0,
                            });
                        return self.types.error();
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
                        // Check if this is a generic struct/enum that needs monomorphization
                        if self.generic_structs.contains_key(name.as_str()) {
                            return self.monomorphize_struct(name, args, ty.span);
                        }
                        if self.generic_enums.contains_key(name.as_str()) {
                            return self.monomorphize_enum(name, args, ty.span);
                        }

                        // For non-generic types, resolve args but return the base type
                        for arg in args {
                            self.resolve_type(arg);
                        }
                        // Try to find as a struct or enum name
                        if let Some(&ty_id) = self.structs.get(name.as_str()) {
                            return ty_id;
                        }
                        if let Some(&ty_id) = self.enums.get(name.as_str()) {
                            return ty_id;
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
                self.types
                    .intern(TypeKind::DynamicArray { element: elem_ty })
            }
            TypeExpr::DynInterface { name } => self
                .types
                .intern(TypeKind::DynInterface { name: name.clone() }),
            TypeExpr::FnType {
                params,
                return_type,
            } => {
                let param_types: Vec<TypeId> =
                    params.iter().map(|p| self.resolve_type(p)).collect();
                let ret = self.resolve_type(return_type);
                self.types.intern(TypeKind::Function {
                    params: param_types,
                    ret,
                })
            }
        }
    }

    /// Resolve a type expression using a substitution map (for monomorphization).
    /// Type parameters in `subst` are replaced with their concrete types.
    pub(super) fn resolve_type_with_subst(
        &mut self,
        ty: &Spanned<TypeExpr>,
        subst: &HashMap<String, TypeId>,
    ) -> TypeId {
        match &ty.node {
            TypeExpr::Named(name) => {
                // Check substitution map first
                if let Some(&concrete) = subst.get(name.as_str()) {
                    return concrete;
                }
                // Fall through to normal resolution
                self.resolve_type(ty)
            }
            TypeExpr::Generic { name, args } => {
                // Check if name itself is a type parameter (shouldn't normally happen for Generic)
                if let Some(&concrete) = subst.get(name.as_str()) {
                    return concrete;
                }

                // Resolve all args to concrete TypeIds directly (handles compound types like tuples)
                let concrete_args: Vec<TypeId> = args
                    .iter()
                    .map(|a| self.resolve_type_with_subst(a, subst))
                    .collect();

                match name.as_str() {
                    "Map" if concrete_args.len() == 2 => self.types.intern(TypeKind::Map {
                        key: concrete_args[0],
                        value: concrete_args[1],
                    }),
                    _ => {
                        // Check if it's a generic struct/enum — use concrete monomorphization
                        if self.generic_structs.contains_key(name.as_str()) {
                            return self.monomorphize_struct_concrete(
                                name,
                                &concrete_args,
                                args.len(),
                                ty.span,
                            );
                        }
                        if self.generic_enums.contains_key(name.as_str()) {
                            return self.monomorphize_enum_concrete(
                                name,
                                &concrete_args,
                                args.len(),
                                ty.span,
                            );
                        }
                        // Fallback: resolve normally
                        self.resolve_type(ty)
                    }
                }
            }
            TypeExpr::Tuple(elements) => {
                let element_types: Vec<TypeId> = elements
                    .iter()
                    .map(|e| self.resolve_type_with_subst(e, subst))
                    .collect();
                self.types.intern(TypeKind::Tuple(element_types))
            }
            TypeExpr::FixedArray { element, length } => {
                let elem_ty = self.resolve_type_with_subst(element, subst);
                self.types.intern(TypeKind::FixedArray {
                    element: elem_ty,
                    length: *length,
                })
            }
            TypeExpr::DynamicArray { element } => {
                let elem_ty = self.resolve_type_with_subst(element, subst);
                self.types
                    .intern(TypeKind::DynamicArray { element: elem_ty })
            }
            TypeExpr::FnType {
                params,
                return_type,
            } => {
                let param_types: Vec<TypeId> = params
                    .iter()
                    .map(|p| self.resolve_type_with_subst(p, subst))
                    .collect();
                let ret = self.resolve_type_with_subst(return_type, subst);
                self.types.intern(TypeKind::Function {
                    params: param_types,
                    ret,
                })
            }
            _ => self.resolve_type(ty),
        }
    }

    /// Build the mangled name for a monomorphized type/function.
    pub(super) fn mangle_name(&self, base: &str, type_args: &[TypeId]) -> String {
        let mut name = base.to_string();
        for &ty in type_args {
            name.push('$');
            name.push_str(&self.type_name(ty));
        }
        name
    }

    /// Monomorphize a generic struct with concrete type arguments.
    pub(super) fn monomorphize_struct(
        &mut self,
        base_name: &str,
        type_args: &[Spanned<TypeExpr>],
        span: Span,
    ) -> TypeId {
        let concrete_args: Vec<TypeId> = type_args.iter().map(|a| self.resolve_type(a)).collect();
        self.monomorphize_struct_concrete(base_name, &concrete_args, type_args.len(), span)
    }

    /// Monomorphize a generic struct with pre-resolved concrete type arguments.
    pub(super) fn monomorphize_struct_concrete(
        &mut self,
        base_name: &str,
        concrete_args: &[TypeId],
        arg_count: usize,
        span: Span,
    ) -> TypeId {
        // Check for any error types
        if concrete_args.iter().any(|&t| t == self.types.error()) {
            return self.types.error();
        }

        let mangled = self.mangle_name(base_name, concrete_args);

        // Check cache
        if let Some(&ty) = self.structs.get(&mangled) {
            return ty;
        }

        // Get the generic def (need to clone to avoid borrow issues)
        let generic_def = match self.generic_structs.get(base_name) {
            Some(def) => def.clone(),
            None => return self.types.error(),
        };

        // Check type arg count
        if arg_count != generic_def.type_params.len() {
            self.diagnostics
                .add(&CheckerDiagnostic::TypeArgCountMismatch {
                    span,
                    expected: generic_def.type_params.len(),
                    found: arg_count,
                });
            return self.types.error();
        }

        // Build substitution map
        let mut subst = HashMap::new();
        for (param, &concrete) in generic_def.type_params.iter().zip(concrete_args.iter()) {
            subst.insert(param.name.clone(), concrete);
        }

        // Resolve fields with substitution
        let resolved_fields: Vec<(String, TypeId)> = generic_def
            .fields
            .iter()
            .map(|f| (f.name.clone(), self.resolve_type_with_subst(&f.ty, &subst)))
            .collect();

        // Intern the monomorphized struct (generic structs are never extern)
        let type_id = self.types.intern(TypeKind::Struct {
            name: mangled.clone(),
            fields: resolved_fields,
            is_extern: false,
        });

        self.structs.insert(mangled.clone(), type_id);
        self.mono_type_args.insert(
            mangled.clone(),
            (base_name.to_string(), concrete_args.to_vec()),
        );

        // Instantiate impl methods for this monomorphization
        self.instantiate_impl_methods(base_name, &mangled, type_id, &subst);

        type_id
    }

    /// Monomorphize a generic enum with concrete type arguments.
    pub(super) fn monomorphize_enum(
        &mut self,
        base_name: &str,
        type_args: &[Spanned<TypeExpr>],
        span: Span,
    ) -> TypeId {
        let concrete_args: Vec<TypeId> = type_args.iter().map(|a| self.resolve_type(a)).collect();
        self.monomorphize_enum_concrete(base_name, &concrete_args, type_args.len(), span)
    }

    /// Monomorphize a generic enum with pre-resolved concrete type arguments.
    pub(super) fn monomorphize_enum_concrete(
        &mut self,
        base_name: &str,
        concrete_args: &[TypeId],
        arg_count: usize,
        span: Span,
    ) -> TypeId {
        if concrete_args.iter().any(|&t| t == self.types.error()) {
            return self.types.error();
        }

        let mangled = self.mangle_name(base_name, concrete_args);

        // Check cache
        if let Some(&ty) = self.enums.get(&mangled) {
            return ty;
        }

        let generic_def = match self.generic_enums.get(base_name) {
            Some(def) => def.clone(),
            None => return self.types.error(),
        };

        if arg_count != generic_def.type_params.len() {
            self.diagnostics
                .add(&CheckerDiagnostic::TypeArgCountMismatch {
                    span,
                    expected: generic_def.type_params.len(),
                    found: arg_count,
                });
            return self.types.error();
        }

        let mut subst = HashMap::new();
        for (param, &concrete) in generic_def.type_params.iter().zip(concrete_args.iter()) {
            subst.insert(param.name.clone(), concrete);
        }

        let resolved_variants: Vec<EnumVariant> = generic_def
            .variants
            .iter()
            .map(|v| {
                let fields = match &v.kind {
                    VariantKind::Unit => Vec::new(),
                    VariantKind::Tuple(types) => types
                        .iter()
                        .enumerate()
                        .map(|(i, t)| (format!("{}", i), self.resolve_type_with_subst(t, &subst)))
                        .collect(),
                    VariantKind::Struct(struct_fields) => struct_fields
                        .iter()
                        .map(|f| (f.name.clone(), self.resolve_type_with_subst(&f.ty, &subst)))
                        .collect(),
                };
                EnumVariant {
                    name: v.name.clone(),
                    fields,
                }
            })
            .collect();

        let type_id = self.types.intern(TypeKind::Enum {
            name: mangled.clone(),
            variants: resolved_variants,
        });

        self.enums.insert(mangled.clone(), type_id);
        self.mono_type_args.insert(
            mangled.clone(),
            (base_name.to_string(), concrete_args.to_vec()),
        );

        // Instantiate impl methods
        self.instantiate_impl_methods(base_name, &mangled, type_id, &subst);

        type_id
    }

    /// Instantiate generic impl methods for a monomorphized struct/enum.
    fn instantiate_impl_methods(
        &mut self,
        base_type_name: &str,
        mangled_type_name: &str,
        self_ty: TypeId,
        subst: &HashMap<String, TypeId>,
    ) {
        let methods = match self.generic_impl_methods.get(base_type_name) {
            Some(m) => m.clone(),
            None => return,
        };

        // Add Self to the substitution map
        let mut subst = subst.clone();
        subst.insert("Self".into(), self_ty);

        for method in &methods {
            let mangled_method = format!("{}__{}", mangled_type_name, method.method_name);

            if self.functions.contains_key(&mangled_method)
                || self.mono_cache.contains(&mangled_method)
            {
                continue;
            }

            // Resolve params with substitution
            let resolved_params: Vec<(String, TypeId)> = method
                .ast_params
                .iter()
                .map(|p| {
                    if p.is_self {
                        (p.name.clone(), self_ty)
                    } else {
                        (p.name.clone(), self.resolve_type_with_subst(&p.ty, &subst))
                    }
                })
                .collect();

            let has_default: Vec<bool> = method
                .ast_params
                .iter()
                .map(|p| p.default_value.is_some())
                .collect();
            let required_params = has_default.iter().take_while(|d| !*d).count();
            let is_method = method.ast_params.first().map_or(false, |p| p.is_self);
            let is_mut_method = is_method && method.ast_params.first().map_or(false, |p| p.is_mut);

            let ret_ty = method
                .ast_return_type
                .as_ref()
                .map(|t| self.resolve_type_with_subst(t, &subst))
                .unwrap_or_else(|| self.types.unit());

            self.functions.insert(
                mangled_method.clone(),
                FunctionSig {
                    name: method.method_name.clone(),
                    params: resolved_params,
                    return_type: ret_ty,
                    kind: FunctionKind::UserDefined,
                    required_params,
                    has_default,
                    is_method,
                    is_mut_method,
                    generic_def: None,
                },
            );

            self.mono_cache.insert(mangled_method.clone());
            self.mono_fn_bodies.insert(
                mangled_method.clone(),
                (
                    method.ast_params.clone(),
                    method.ast_body.clone(),
                    subst.clone(),
                ),
            );
            self.pending_mono_checks.push((
                mangled_method,
                method.ast_params.clone(),
                method.ast_body.clone(),
                subst.clone(),
            ));
        }
    }

    /// Monomorphize a generic function with concrete type arguments.
    /// Returns the mangled name of the monomorphized function.
    pub(super) fn monomorphize_function(
        &mut self,
        base_name: &str,
        generic_def: &GenericFunctionDef,
        concrete_type_args: &[TypeId],
        call_span: Span,
    ) -> Option<String> {
        let mangled = self.mangle_name(base_name, concrete_type_args);

        // Check cache
        if self.mono_cache.contains(&mangled) {
            return Some(mangled);
        }

        // Check type arg count
        if concrete_type_args.len() != generic_def.type_params.len() {
            self.diagnostics
                .add(&CheckerDiagnostic::TypeArgCountMismatch {
                    span: call_span,
                    expected: generic_def.type_params.len(),
                    found: concrete_type_args.len(),
                });
            return None;
        }

        // Build substitution map
        let mut subst = HashMap::new();
        for (param, &concrete) in generic_def
            .type_params
            .iter()
            .zip(concrete_type_args.iter())
        {
            subst.insert(param.name.clone(), concrete);
        }

        // Resolve params with substitution
        let resolved_params: Vec<(String, TypeId)> = generic_def
            .ast_params
            .iter()
            .map(|p| (p.name.clone(), self.resolve_type_with_subst(&p.ty, &subst)))
            .collect();

        let has_default: Vec<bool> = generic_def
            .ast_params
            .iter()
            .map(|p| p.default_value.is_some())
            .collect();
        let required_params = has_default.iter().take_while(|d| !*d).count();
        let is_method = generic_def.ast_params.first().map_or(false, |p| p.is_self);
        let is_mut_method = is_method && generic_def.ast_params.first().map_or(false, |p| p.is_mut);

        let ret_ty = generic_def
            .ast_return_type
            .as_ref()
            .map(|t| self.resolve_type_with_subst(t, &subst))
            .unwrap_or_else(|| self.types.unit());

        // Register the concrete function
        self.functions.insert(
            mangled.clone(),
            FunctionSig {
                name: mangled.clone(),
                params: resolved_params,
                return_type: ret_ty,
                kind: FunctionKind::UserDefined,
                required_params,
                has_default,
                is_method,
                is_mut_method,
                generic_def: None,
            },
        );

        self.mono_cache.insert(mangled.clone());
        self.mono_fn_bodies.insert(
            mangled.clone(),
            (
                generic_def.ast_params.clone(),
                generic_def.ast_body.clone(),
                subst.clone(),
            ),
        );
        self.pending_mono_checks.push((
            mangled.clone(),
            generic_def.ast_params.clone(),
            generic_def.ast_body.clone(),
            subst,
        ));

        Some(mangled)
    }

    /// Try to infer type arguments for a generic function from the call arguments.
    /// Returns None if inference fails for any type parameter.
    pub(super) fn infer_type_args(
        &mut self,
        generic_def: &GenericFunctionDef,
        args: &[CallArg],
        locals: &mut ScopedLocals<LocalInfo>,
    ) -> Option<Vec<TypeId>> {
        let mut inferred: HashMap<String, TypeId> = HashMap::new();

        // Check each argument against the corresponding parameter
        for (i, arg) in args.iter().enumerate() {
            if i >= generic_def.ast_params.len() {
                break;
            }
            let param = &generic_def.ast_params[i];

            // If the parameter is a function type and we have partial inferences,
            // try to resolve it as a closure type hint so untyped closure params
            // (like implicit `it`) can be inferred.
            if let TypeExpr::FnType { .. } = &param.ty.node {
                if !inferred.is_empty() {
                    // Temporarily set type_param_scope with all type params so that
                    // unresolved ones (like K when only T is known) don't produce
                    // "unknown type" diagnostics — they just resolve to error().
                    let old_scope = self.type_param_scope.clone();
                    for tp in &generic_def.type_params {
                        if !inferred.contains_key(&tp.name) {
                            self.type_param_scope
                                .insert(tp.name.clone(), self.types.error());
                        }
                    }
                    let hint_ty = self.resolve_type_with_subst(&param.ty, &inferred);
                    self.type_param_scope = old_scope;
                    self.set_closure_hint_if_fn(hint_ty);
                }
            }

            // Type-check the argument to get its concrete type
            let arg_ty = self.check_expr(&arg.value, locals);
            if arg_ty == self.types.error() {
                continue;
            }

            // Try to unify the parameter's type expression with the concrete arg type
            self.unify_type_expr_with_concrete(&param.ty.node, arg_ty, &mut inferred);
        }

        // Build the result in order of type parameters
        let mut result = Vec::new();
        for tp in &generic_def.type_params {
            match inferred.get(&tp.name) {
                Some(&ty) => result.push(ty),
                None => return None,
            }
        }
        Some(result)
    }

    /// Public wrapper for unify_type_expr_with_concrete (used from check_expr for struct literal inference)
    pub(super) fn unify_type_expr_with_concrete_pub(
        &self,
        type_expr: &TypeExpr,
        concrete: TypeId,
        inferred: &mut HashMap<String, TypeId>,
    ) {
        self.unify_type_expr_with_concrete(type_expr, concrete, inferred);
    }

    /// Unify a TypeExpr (which may contain type parameters) with a concrete TypeId,
    /// recording discovered mappings in `inferred`.
    fn unify_type_expr_with_concrete(
        &self,
        type_expr: &TypeExpr,
        concrete: TypeId,
        inferred: &mut HashMap<String, TypeId>,
    ) {
        match type_expr {
            TypeExpr::Named(name) => {
                // If this name is a type parameter, record the mapping
                // Type parameters are single-character uppercase or not a known type
                if !matches!(
                    name.as_str(),
                    "i8" | "i16"
                        | "i32"
                        | "i64"
                        | "u8"
                        | "u16"
                        | "u32"
                        | "u64"
                        | "f32"
                        | "f64"
                        | "bool"
                        | "char"
                        | "string"
                        | "RawPtr"
                        | "MutRawPtr"
                        | "CStr"
                ) && !self.structs.contains_key(name)
                    && !self.enums.contains_key(name)
                    && !self.interfaces.contains_key(name)
                    && !self.type_aliases.contains_key(name)
                {
                    // Looks like a type parameter
                    if let Some(&existing) = inferred.get(name) {
                        // Already inferred — just check consistency (but don't error here)
                        let _ = existing;
                    } else {
                        inferred.insert(name.clone(), concrete);
                    }
                }
            }
            TypeExpr::Generic { name, args } => {
                // Try to unify nested type args
                // E.g., if param is `Option<K>` and concrete is `Option$(i32, i32)`, unify K with (i32, i32)
                match self.types.resolve(concrete) {
                    TypeKind::Map { key, value } if args.len() == 2 => {
                        self.unify_type_expr_with_concrete(&args[0].node, *key, inferred);
                        self.unify_type_expr_with_concrete(&args[1].node, *value, inferred);
                    }
                    TypeKind::Struct {
                        name: concrete_name,
                        ..
                    }
                    | TypeKind::Enum {
                        name: concrete_name,
                        ..
                    } => {
                        // Look up the monomorphization's type args via reverse mapping
                        let concrete_name = concrete_name.clone();
                        if let Some((base_name, concrete_type_args)) =
                            self.mono_type_args.get(&concrete_name).cloned()
                        {
                            if base_name == *name && args.len() == concrete_type_args.len() {
                                for (te, &ce) in args.iter().zip(concrete_type_args.iter()) {
                                    self.unify_type_expr_with_concrete(&te.node, ce, inferred);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            TypeExpr::Tuple(elements) => {
                if let TypeKind::Tuple(concrete_elems) = self.types.resolve(concrete) {
                    let concrete_elems = concrete_elems.clone();
                    for (te, &ce) in elements.iter().zip(concrete_elems.iter()) {
                        self.unify_type_expr_with_concrete(&te.node, ce, inferred);
                    }
                }
            }
            TypeExpr::DynamicArray { element } => {
                if let TypeKind::DynamicArray { element: ce } = self.types.resolve(concrete) {
                    self.unify_type_expr_with_concrete(&element.node, *ce, inferred);
                }
            }
            TypeExpr::FixedArray { element, .. } => {
                if let TypeKind::FixedArray { element: ce, .. } = self.types.resolve(concrete) {
                    self.unify_type_expr_with_concrete(&element.node, *ce, inferred);
                }
            }
            TypeExpr::FnType {
                params,
                return_type,
            } => {
                if let TypeKind::Function {
                    params: concrete_params,
                    ret,
                } = self.types.resolve(concrete)
                {
                    let concrete_params = concrete_params.clone();
                    let ret = *ret;
                    for (te, &ce) in params.iter().zip(concrete_params.iter()) {
                        self.unify_type_expr_with_concrete(&te.node, ce, inferred);
                    }
                    self.unify_type_expr_with_concrete(&return_type.node, ret, inferred);
                }
            }
            _ => {}
        }
    }

    /// Convert a concrete TypeId back to a TypeExpr for use in monomorphization.
    /// Unlike `type_name()` which produces a flat string, this correctly represents
    /// compound types (tuples, arrays, functions, maps) as structured TypeExpr nodes.
    pub(super) fn type_id_to_type_expr(&self, ty: TypeId, span: Span) -> Spanned<TypeExpr> {
        let node = match self.types.resolve(ty) {
            TypeKind::Primitive(PrimitiveType::Unit) => TypeExpr::Unit,
            TypeKind::Tuple(elements) => {
                let elements = elements.clone();
                TypeExpr::Tuple(
                    elements
                        .iter()
                        .map(|e| self.type_id_to_type_expr(*e, span))
                        .collect(),
                )
            }
            TypeKind::DynamicArray { element } => {
                let element = *element;
                TypeExpr::DynamicArray {
                    element: Box::new(self.type_id_to_type_expr(element, span)),
                }
            }
            TypeKind::FixedArray { element, length } => {
                let element = *element;
                let length = *length;
                TypeExpr::FixedArray {
                    element: Box::new(self.type_id_to_type_expr(element, span)),
                    length,
                }
            }
            TypeKind::Map { key, value } => {
                let key = *key;
                let value = *value;
                TypeExpr::Generic {
                    name: "Map".into(),
                    args: vec![
                        self.type_id_to_type_expr(key, span),
                        self.type_id_to_type_expr(value, span),
                    ],
                }
            }
            TypeKind::Function { params, ret } => {
                let params = params.clone();
                let ret = *ret;
                TypeExpr::FnType {
                    params: params
                        .iter()
                        .map(|p| self.type_id_to_type_expr(*p, span))
                        .collect(),
                    return_type: Box::new(self.type_id_to_type_expr(ret, span)),
                }
            }
            _ => TypeExpr::Named(self.type_name(ty)),
        };
        Spanned { node, span }
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
            TypeKind::Function { params, ret } => {
                let param_strs: Vec<String> = params.iter().map(|p| self.type_name(*p)).collect();
                let is_unit = matches!(
                    self.types.resolve(*ret),
                    TypeKind::Primitive(PrimitiveType::Unit)
                );
                if is_unit {
                    format!("|{}|", param_strs.join(", "))
                } else {
                    format!("|{}| -> {}", param_strs.join(", "), self.type_name(*ret))
                }
            }
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
            TypeKind::TypeVar { name, .. } => name.clone(),
            TypeKind::Error => "<error>".into(),
        }
    }

    /// Set the closure type hint if `ty` is a function type.
    /// This allows closures with untyped params to infer types from context.
    pub(super) fn set_closure_hint_if_fn(&mut self, ty: TypeId) {
        if matches!(self.types.resolve(ty), TypeKind::Function { .. }) {
            self.closure_type_hint = Some(ty);
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

    pub(super) fn is_type_var(&self, ty: TypeId) -> bool {
        matches!(self.types.resolve(ty), TypeKind::TypeVar { .. })
    }

    /// Check TypeVar compatibility during shallow generic checking.
    /// Returns None if neither type is a TypeVar (caller should use normal comparison).
    /// Returns Some(true) if compatible, Some(false) if incompatible.
    pub(super) fn typevar_compatible(&self, a: TypeId, b: TypeId) -> Option<bool> {
        let a_tv = self.is_type_var(a);
        let b_tv = self.is_type_var(b);
        match (a_tv, b_tv) {
            (false, false) => None,       // neither is TypeVar — caller uses normal check
            (true, true) => Some(a == b), // both TypeVars — must be same TypeId
            _ => Some(true),              // one TypeVar + one concrete — defer to monomorphization
        }
    }

    pub(super) fn type_var_has_bound(&self, ty: TypeId, bound: &str) -> bool {
        if let TypeKind::TypeVar { bounds, .. } = self.types.resolve(ty) {
            bounds.iter().any(|b| b == bound)
        } else {
            false
        }
    }

    pub(super) fn is_signed_or_float(&self, ty: TypeId) -> bool {
        matches!(
            self.types.resolve(ty),
            TypeKind::Primitive(p) if p.is_signed()
        )
    }
}
