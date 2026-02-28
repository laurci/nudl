use nudl_core::types::EnumVariant;

use super::*;

impl Checker {
    // --- Pass 1: Collect declarations ---

    pub(super) fn collect_fn_sig(
        &mut self,
        name: &str,
        type_params: &[TypeParam],
        params: &[Param],
        return_type: &Option<Spanned<TypeExpr>>,
        body: Option<&Spanned<Block>>,
        span: Span,
        is_pub: bool,
    ) {
        if self.functions.contains_key(name) {
            self.diagnostics.add(&CheckerDiagnostic::DuplicateFunction {
                span,
                name: name.into(),
            });
            return;
        }

        // If the function has type parameters, store as a generic template
        if !type_params.is_empty() {
            let generic_def = GenericFunctionDef {
                type_params: type_params.to_vec(),
                ast_params: params.to_vec(),
                ast_return_type: return_type.clone(),
                ast_body: body.cloned().unwrap_or_else(|| Spanned {
                    node: Block {
                        stmts: vec![],
                        tail_expr: None,
                    },
                    span,
                }),
                span,
            };

            // Register a placeholder sig with error types — will be replaced by monomorphized versions
            let _param_count = params.len();
            let has_default: Vec<bool> = params.iter().map(|p| p.default_value.is_some()).collect();
            let required_params = has_default.iter().take_while(|d| !*d).count();
            let is_method = params.first().map_or(false, |p| p.is_self);
            let is_mut_method = is_method && params.first().map_or(false, |p| p.is_mut);

            self.functions.insert(
                name.into(),
                FunctionSig {
                    name: name.into(),
                    params: params
                        .iter()
                        .map(|p| (p.name.clone(), self.types.error()))
                        .collect(),
                    return_type: self.types.error(),
                    kind: FunctionKind::UserDefined,
                    required_params,
                    has_default,
                    is_method,
                    is_mut_method,
                    generic_def: Some(generic_def),
                    is_pub,
                    source_file_id: span.file_id,
                },
            );
            return;
        }

        let resolved_params: Vec<(String, TypeId)> = params
            .iter()
            .map(|p| (p.name.clone(), self.resolve_type(&p.ty)))
            .collect();

        let has_default: Vec<bool> = params.iter().map(|p| p.default_value.is_some()).collect();
        let required_params = has_default.iter().take_while(|d| !*d).count();

        let is_method = params.first().map_or(false, |p| p.is_self);
        let is_mut_method = is_method && params.first().map_or(false, |p| p.is_mut);

        let ret_ty = return_type
            .as_ref()
            .map(|t| self.resolve_type(t))
            .unwrap_or_else(|| self.types.unit());

        self.functions.insert(
            name.into(),
            FunctionSig {
                name: name.into(),
                params: resolved_params,
                return_type: ret_ty,
                kind: FunctionKind::UserDefined,
                required_params,
                has_default,
                is_method,
                is_mut_method,
                generic_def: None,
                is_pub,
                source_file_id: span.file_id,
            },
        );
    }

    pub(super) fn collect_item(&mut self, item: &SpannedItem) {
        match &item.node {
            Item::FnDef {
                name,
                type_params,
                params,
                return_type,
                body,
                is_pub,
            } => {
                if name == "main" {
                    self.found_main = true;
                    if !params.is_empty() || return_type.is_some() {
                        self.diagnostics
                            .add(&CheckerDiagnostic::InvalidMainSignature { span: item.span });
                    }
                }

                self.collect_fn_sig(
                    name,
                    type_params,
                    params,
                    return_type,
                    Some(body),
                    item.span,
                    *is_pub,
                );
            }
            Item::StructDef {
                name,
                type_params,
                fields,
                is_pub,
                ..
            } => {
                if self.structs.contains_key(name) || self.generic_structs.contains_key(name) {
                    self.diagnostics.add(&CheckerDiagnostic::DuplicateStruct {
                        span: item.span,
                        name: name.clone(),
                    });
                    return;
                }

                // Record type visibility
                self.type_visibility
                    .insert(name.clone(), (*is_pub, item.span.file_id));
                // Record field visibility
                self.field_visibility.insert(
                    name.clone(),
                    fields
                        .iter()
                        .map(|f| (f.name.clone(), f.is_pub))
                        .collect(),
                );

                // If generic, store as template
                if !type_params.is_empty() {
                    self.generic_structs.insert(
                        name.clone(),
                        GenericStructDef {
                            type_params: type_params.clone(),
                            fields: fields.clone(),
                            span: item.span,
                        },
                    );
                    return;
                }

                let resolved_fields: Vec<(String, TypeId)> = fields
                    .iter()
                    .map(|f| (f.name.clone(), self.resolve_type(&f.ty)))
                    .collect();

                let is_extern = matches!(
                    &item.node,
                    Item::StructDef {
                        is_extern: true,
                        ..
                    }
                );
                let type_id = self.types.intern(TypeKind::Struct {
                    name: name.clone(),
                    fields: resolved_fields,
                    is_extern,
                });

                self.structs.insert(name.clone(), type_id);
            }
            Item::EnumDef {
                name,
                type_params,
                variants,
                is_pub,
            } => {
                if self.enums.contains_key(name)
                    || self.structs.contains_key(name)
                    || self.generic_enums.contains_key(name)
                {
                    self.diagnostics.add(&CheckerDiagnostic::DuplicateStruct {
                        span: item.span,
                        name: name.clone(),
                    });
                    return;
                }

                // Record type visibility
                self.type_visibility
                    .insert(name.clone(), (*is_pub, item.span.file_id));

                // If generic, store as template
                if !type_params.is_empty() {
                    self.generic_enums.insert(
                        name.clone(),
                        GenericEnumDef {
                            type_params: type_params.clone(),
                            variants: variants.clone(),
                            span: item.span,
                        },
                    );
                    return;
                }

                let resolved_variants: Vec<EnumVariant> = variants
                    .iter()
                    .map(|v| {
                        let fields = match &v.kind {
                            VariantKind::Unit => Vec::new(),
                            VariantKind::Tuple(types) => types
                                .iter()
                                .enumerate()
                                .map(|(i, t)| (format!("{}", i), self.resolve_type(t)))
                                .collect(),
                            VariantKind::Struct(struct_fields) => struct_fields
                                .iter()
                                .map(|f| (f.name.clone(), self.resolve_type(&f.ty)))
                                .collect(),
                        };
                        EnumVariant {
                            name: v.name.clone(),
                            fields,
                        }
                    })
                    .collect();

                let type_id = self.types.intern(TypeKind::Enum {
                    name: name.clone(),
                    variants: resolved_variants,
                });

                self.enums.insert(name.clone(), type_id);
            }
            Item::InterfaceDef {
                name,
                methods,
                is_pub,
                ..
            } => {
                // Record type visibility
                self.type_visibility
                    .insert(name.clone(), (*is_pub, item.span.file_id));
                let resolved_methods: Vec<nudl_core::types::InterfaceMethod> = methods
                    .iter()
                    .map(|m| {
                        let params: Vec<(String, TypeId)> = m
                            .params
                            .iter()
                            .map(|p| (p.name.clone(), self.resolve_type(&p.ty)))
                            .collect();
                        let return_type = m
                            .return_type
                            .as_ref()
                            .map(|t| self.resolve_type(t))
                            .unwrap_or_else(|| self.types.unit());
                        nudl_core::types::InterfaceMethod {
                            name: m.name.clone(),
                            params,
                            return_type,
                        }
                    })
                    .collect();

                let type_id = self.types.intern(TypeKind::Interface {
                    name: name.clone(),
                    methods: resolved_methods,
                });

                self.interfaces.insert(name.clone(), type_id);
            }
            Item::ImplBlock {
                type_name,
                type_args,
                interface_name,
                methods,
                ..
            } => {
                // Check if the impl block is for a generic type (has type_args like `impl Foo<T>`)
                let _is_generic_impl = !type_args.is_empty()
                    && type_args.iter().any(|a| {
                        matches!(&a.node, TypeExpr::Named(n) if self.generic_structs.contains_key(n.as_str()) || self.generic_enums.contains_key(n.as_str()) || n.chars().next().map_or(false, |c| c.is_uppercase() && n.len() == 1))
                    });

                // Also check if the base type is a generic struct/enum
                let base_is_generic = self.generic_structs.contains_key(type_name)
                    || self.generic_enums.contains_key(type_name);

                if base_is_generic {
                    // Store methods as generic impl methods
                    let methods_list = self
                        .generic_impl_methods
                        .entry(type_name.clone())
                        .or_default();

                    for method_item in methods {
                        if let Item::FnDef {
                            name: method_name,
                            type_params: _type_params,
                            params,
                            return_type,
                            body,
                            is_pub,
                        } = &method_item.node
                        {
                            // Collect type params from the type_args of the impl block
                            let impl_type_params: Vec<TypeParam> = type_args
                                .iter()
                                .filter_map(|a| {
                                    if let TypeExpr::Named(n) = &a.node {
                                        // Treat single uppercase names as type params
                                        Some(TypeParam {
                                            name: n.clone(),
                                            bounds: vec![],
                                            span: a.span,
                                        })
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            methods_list.push(GenericImplMethod {
                                type_params: impl_type_params,
                                method_name: method_name.clone(),
                                ast_params: params.clone(),
                                ast_return_type: return_type.clone(),
                                ast_body: body.clone(),
                                span: method_item.span,
                                is_pub: *is_pub,
                            });
                        }
                    }

                    // If this is an interface impl, record it for the base type name
                    if let Some(iface_name) = interface_name {
                        self.interface_impls
                            .entry(iface_name.clone())
                            .or_default()
                            .push(type_name.clone());
                    }
                    return;
                }

                // Resolve the type for self parameter (struct or enum)
                let self_ty = self
                    .structs
                    .get(type_name)
                    .or_else(|| self.enums.get(type_name))
                    .copied();
                if self_ty.is_none() {
                    self.diagnostics.add(&CheckerDiagnostic::UndefinedStruct {
                        span: item.span,
                        name: type_name.clone(),
                    });
                    return;
                }
                let self_ty = self_ty.unwrap();

                // If this is an interface impl, record it
                if let Some(iface_name) = interface_name {
                    self.interface_impls
                        .entry(iface_name.clone())
                        .or_default()
                        .push(type_name.clone());
                }

                // Set Self type scope so that Self resolves to the impl target type
                self.type_param_scope.insert("Self".into(), self_ty);

                // Register each method as a mangled function: TypeName__methodname
                for method_item in methods {
                    if let Item::FnDef {
                        name: method_name,
                        params,
                        return_type,
                        is_pub: method_is_pub,
                        ..
                    } = &method_item.node
                    {
                        let mangled_name = format!("{}__{}", type_name, method_name);

                        // Resolve params, replacing Self/self type with the actual type
                        let resolved_params: Vec<(String, TypeId)> = params
                            .iter()
                            .map(|p| {
                                if p.is_self {
                                    (p.name.clone(), self_ty)
                                } else {
                                    (p.name.clone(), self.resolve_type(&p.ty))
                                }
                            })
                            .collect();

                        let has_default: Vec<bool> =
                            params.iter().map(|p| p.default_value.is_some()).collect();
                        let required_params = has_default.iter().take_while(|d| !*d).count();

                        let is_method = params.first().map_or(false, |p| p.is_self);
                        let is_mut_method = is_method && params.first().map_or(false, |p| p.is_mut);

                        let ret_ty = return_type
                            .as_ref()
                            .map(|t| self.resolve_type(t))
                            .unwrap_or_else(|| self.types.unit());

                        if self.functions.contains_key(&mangled_name) {
                            self.diagnostics.add(&CheckerDiagnostic::DuplicateFunction {
                                span: method_item.span,
                                name: mangled_name,
                            });
                            continue;
                        }

                        // Interface impl methods are auto-pub
                        let effective_pub =
                            *method_is_pub || interface_name.is_some();

                        self.functions.insert(
                            mangled_name,
                            FunctionSig {
                                name: method_name.clone(),
                                params: resolved_params,
                                return_type: ret_ty,
                                kind: FunctionKind::UserDefined,
                                required_params,
                                has_default,
                                is_method,
                                is_mut_method,
                                generic_def: None,
                                is_pub: effective_pub,
                                source_file_id: method_item.span.file_id,
                            },
                        );
                    }
                }

                // Validate interface completeness
                if let Some(iface_name) = interface_name {
                    if let Some(&iface_ty) = self.interfaces.get(iface_name) {
                        if let TypeKind::Interface {
                            methods: iface_methods,
                            ..
                        } = self.types.resolve(iface_ty).clone()
                        {
                            for iface_method in &iface_methods {
                                let mangled = format!("{}__{}", type_name, iface_method.name);
                                if let Some(impl_sig) = self.functions.get(&mangled) {
                                    // Compare return types
                                    let ret_match =
                                        impl_sig.return_type == iface_method.return_type;
                                    // Compare non-self params (skip first param which is self)
                                    let iface_non_self: Vec<_> =
                                        iface_method.params.iter().skip(1).collect();
                                    let impl_non_self: Vec<_> =
                                        impl_sig.params.iter().skip(1).collect();
                                    let params_match = iface_non_self.len() == impl_non_self.len()
                                        && iface_non_self.iter().zip(impl_non_self.iter()).all(
                                            |((_, iface_ty), (_, impl_ty))| iface_ty == impl_ty,
                                        );
                                    if !ret_match || !params_match {
                                        let expected = format!(
                                            "fn({}) -> {}",
                                            iface_non_self
                                                .iter()
                                                .map(|(n, t)| format!(
                                                    "{}: {}",
                                                    n,
                                                    self.type_name(*t)
                                                ))
                                                .collect::<Vec<_>>()
                                                .join(", "),
                                            self.type_name(iface_method.return_type)
                                        );
                                        let found = format!(
                                            "fn({}) -> {}",
                                            impl_non_self
                                                .iter()
                                                .map(|(n, t)| format!(
                                                    "{}: {}",
                                                    n,
                                                    self.type_name(*t)
                                                ))
                                                .collect::<Vec<_>>()
                                                .join(", "),
                                            self.type_name(impl_sig.return_type)
                                        );
                                        self.diagnostics.add(
                                            &CheckerDiagnostic::InterfaceMethodSignatureMismatch {
                                                span: item.span,
                                                type_name: type_name.clone(),
                                                interface_name: iface_name.clone(),
                                                method: iface_method.name.clone(),
                                                expected,
                                                found,
                                            },
                                        );
                                    }
                                } else {
                                    self.diagnostics.add(
                                        &CheckerDiagnostic::MissingInterfaceMethod {
                                            span: item.span,
                                            type_name: type_name.clone(),
                                            interface_name: iface_name.clone(),
                                            method: iface_method.name.clone(),
                                        },
                                    );
                                }
                            }
                        }
                    }
                }

                // Clear Self type scope
                self.type_param_scope.remove("Self");
            }
            Item::Import { .. } => {
                // Imports are handled at the pipeline level
            }
            Item::TypeAlias { name, ty, is_pub } => {
                // Record type visibility
                self.type_visibility
                    .insert(name.clone(), (*is_pub, item.span.file_id));
                // Register the alias as mapping to the resolved type
                let resolved = self.resolve_type(ty);
                self.type_aliases.insert(name.clone(), resolved);
            }
            Item::ExternBlock { items, .. } => {
                for extern_fn in items {
                    let decl = &extern_fn.node;

                    if self.functions.contains_key(&decl.name) {
                        self.diagnostics.add(&CheckerDiagnostic::DuplicateFunction {
                            span: extern_fn.span,
                            name: decl.name.clone(),
                        });
                        continue;
                    }

                    let resolved_params: Vec<(String, TypeId)> = decl
                        .params
                        .iter()
                        .map(|p| (p.name.clone(), self.resolve_type(&p.ty)))
                        .collect();

                    let param_count = resolved_params.len();
                    let ret_ty = decl
                        .return_type
                        .as_ref()
                        .map(|t| self.resolve_type(t))
                        .unwrap_or_else(|| self.types.unit());

                    self.functions.insert(
                        decl.name.clone(),
                        FunctionSig {
                            name: decl.name.clone(),
                            params: resolved_params,
                            return_type: ret_ty,
                            kind: FunctionKind::Extern,
                            required_params: param_count,
                            has_default: vec![false; param_count],
                            is_method: false,
                            is_mut_method: false,
                            generic_def: None,
                            is_pub: true, // extern functions are implicitly pub
                            source_file_id: extern_fn.span.file_id,
                        },
                    );
                }
            }
        }
    }

    // --- Pass 2: Check bodies ---
}
