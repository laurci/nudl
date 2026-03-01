use nudl_core::types::EnumVariant;

use super::*;

impl Checker {
    /// Merge where-clause bounds into type params.
    /// For each predicate `T: Bound1 + Bound2`, find the matching type param and add the bounds.
    fn merge_where_clauses(
        type_params: &[TypeParam],
        where_clauses: &[WherePredicate],
    ) -> Vec<TypeParam> {
        if where_clauses.is_empty() {
            return type_params.to_vec();
        }
        let mut merged: Vec<TypeParam> = type_params.to_vec();
        for pred in where_clauses {
            if let Some(tp) = merged.iter_mut().find(|tp| tp.name == pred.type_name) {
                for bound in &pred.bounds {
                    if !tp.bounds.contains(bound) {
                        tp.bounds.push(bound.clone());
                    }
                }
            }
        }
        merged
    }

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

        // Desugar `impl Trait` parameters into hidden type params
        let mut effective_type_params = type_params.to_vec();
        let mut effective_params = params.to_vec();
        let mut impl_counter = 0;
        for param in &mut effective_params {
            if let TypeExpr::ImplInterface {
                name: iface_name, ..
            } = &param.ty.node
            {
                // Generate a hidden type param name
                let hidden_name = format!("__impl_{}", impl_counter);
                impl_counter += 1;

                // Add the hidden type param with the interface as a bound
                effective_type_params.push(TypeParam {
                    name: hidden_name.clone(),
                    bounds: vec![iface_name.clone()],
                    span: param.ty.span,
                });

                // Replace the param type with the hidden type param name
                param.ty = Spanned::new(TypeExpr::Named(hidden_name), param.ty.span);
            }
        }
        let type_params = &effective_type_params;
        let params = &effective_params;

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
                where_clauses,
                ..
            } => {
                if name == "main" {
                    self.found_main = true;
                    if !params.is_empty() || return_type.is_some() {
                        self.diagnostics
                            .add(&CheckerDiagnostic::InvalidMainSignature { span: item.span });
                    }
                }

                // Merge where-clause bounds into type params
                let merged_type_params = Self::merge_where_clauses(type_params, where_clauses);

                self.collect_fn_sig(
                    name,
                    &merged_type_params,
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
                    fields.iter().map(|f| (f.name.clone(), f.is_pub)).collect(),
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
                type_params,
                methods,
                is_pub,
                ..
            } => {
                // Record type visibility
                self.type_visibility
                    .insert(name.clone(), (*is_pub, item.span.file_id));

                // Store method defs for default method body generation
                self.interface_method_defs
                    .insert(name.clone(), methods.clone());

                // Generic interfaces: store template, don't resolve types yet
                if !type_params.is_empty() {
                    self.generic_interfaces.insert(
                        name.clone(),
                        GenericInterfaceDef {
                            name: name.clone(),
                            type_params: type_params.clone(),
                            methods: methods.clone(),
                            span: item.span,
                        },
                    );
                    // Also register a placeholder interface type so lookups don't fail
                    let type_id = self.types.intern(TypeKind::Interface {
                        name: name.clone(),
                        methods: vec![],
                    });
                    self.interfaces.insert(name.clone(), type_id);
                    return;
                }

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
                interface_type_args,
                methods,
                where_clauses,
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
                            ..
                        } = &method_item.node
                        {
                            // Collect type params from the type_args of the impl block
                            let base_type_params: Vec<TypeParam> = type_args
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
                            // Merge where-clause bounds into the impl type params
                            let impl_type_params =
                                Self::merge_where_clauses(&base_type_params, where_clauses);

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

                // Resolve the type for self parameter (struct, enum, or primitive)
                let self_ty = self
                    .structs
                    .get(type_name)
                    .or_else(|| self.enums.get(type_name))
                    .copied()
                    .or_else(|| self.primitive_type_id(type_name));
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
                    // For generic interfaces (e.g., Iterator<i32>), use mangled name
                    if !interface_type_args.is_empty() {
                        let resolved_iface_args: Vec<TypeId> = interface_type_args
                            .iter()
                            .map(|ta| self.resolve_type(ta))
                            .collect();
                        let mangled_iface = self.mangle_name(iface_name, &resolved_iface_args);
                        self.interface_impls
                            .entry(mangled_iface.clone())
                            .or_default()
                            .push(type_name.clone());
                        // Also record under the base name for simpler lookups
                        self.interface_impls
                            .entry(iface_name.clone())
                            .or_default()
                            .push(type_name.clone());
                    } else {
                        self.interface_impls
                            .entry(iface_name.clone())
                            .or_default()
                            .push(type_name.clone());
                    }
                }

                // Set Self type scope so that Self resolves to the impl target type
                self.type_param_scope.insert("Self".into(), self_ty);

                // For generic interface impls, also set interface type params in scope
                // so that method types resolve correctly (e.g., Option<T> -> Option<i32>)
                if let Some(iface_name) = interface_name {
                    if !interface_type_args.is_empty() {
                        if let Some(generic_iface) =
                            self.generic_interfaces.get(iface_name).cloned()
                        {
                            for (i, tp) in generic_iface.type_params.iter().enumerate() {
                                if let Some(ta) = interface_type_args.get(i) {
                                    let resolved = self.resolve_type(ta);
                                    self.type_param_scope.insert(tp.name.clone(), resolved);
                                }
                            }
                        }
                    }
                }

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
                        let effective_pub = *method_is_pub || interface_name.is_some();

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

                // Validate interface completeness and generate default methods
                if let Some(iface_name) = interface_name {
                    // Get the AST method defs to check for default bodies and required methods
                    let method_defs = self.interface_method_defs.get(iface_name).cloned();
                    if let Some(method_defs) = method_defs {
                        // Collect names of methods the impl block provides
                        let provided_methods: HashSet<String> = methods
                            .iter()
                            .filter_map(|m| {
                                if let Item::FnDef { name, .. } = &m.node {
                                    Some(name.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        for method_def in &method_defs {
                            let mangled = format!("{}__{}", type_name, method_def.name);
                            if !provided_methods.contains(&method_def.name) {
                                // Method not provided — check for default body
                                if let Some(ref default_body) = method_def.body {
                                    // Generate the default method
                                    let resolved_params: Vec<(String, TypeId)> = method_def
                                        .params
                                        .iter()
                                        .map(|p| {
                                            if p.is_self {
                                                (p.name.clone(), self_ty)
                                            } else {
                                                (p.name.clone(), self.resolve_type(&p.ty))
                                            }
                                        })
                                        .collect();
                                    let has_default: Vec<bool> = method_def
                                        .params
                                        .iter()
                                        .map(|p| p.default_value.is_some())
                                        .collect();
                                    let required_params =
                                        has_default.iter().take_while(|d| !*d).count();
                                    let is_method =
                                        method_def.params.first().map_or(false, |p| p.is_self);
                                    let is_mut_method = is_method
                                        && method_def.params.first().map_or(false, |p| p.is_mut);
                                    let ret_ty = method_def
                                        .return_type
                                        .as_ref()
                                        .map(|t| self.resolve_type(t))
                                        .unwrap_or_else(|| self.types.unit());

                                    self.functions.insert(
                                        mangled.clone(),
                                        FunctionSig {
                                            name: method_def.name.clone(),
                                            params: resolved_params,
                                            return_type: ret_ty,
                                            kind: FunctionKind::UserDefined,
                                            required_params,
                                            has_default,
                                            is_method,
                                            is_mut_method,
                                            generic_def: None,
                                            is_pub: true,
                                            source_file_id: item.span.file_id,
                                        },
                                    );

                                    // Store the body for type-checking and codegen
                                    let subst = self.type_param_scope.clone();
                                    self.mono_fn_bodies.insert(
                                        mangled.clone(),
                                        (
                                            method_def.params.clone(),
                                            default_body.clone(),
                                            subst.clone(),
                                        ),
                                    );
                                    // Queue for type-checking
                                    self.pending_mono_checks.push((
                                        mangled,
                                        method_def.params.clone(),
                                        default_body.clone(),
                                        subst,
                                    ));
                                } else {
                                    // No default body — this is a missing required method
                                    self.diagnostics.add(
                                        &CheckerDiagnostic::MissingInterfaceMethod {
                                            span: item.span,
                                            type_name: type_name.clone(),
                                            interface_name: iface_name.clone(),
                                            method: method_def.name.clone(),
                                        },
                                    );
                                }
                            }
                            // Skip signature matching for generic interfaces (types were resolved
                            // with substitution, so the placeholder interface types won't match)
                        }
                    }

                    // Signature matching using AST method defs with current scope
                    // (Self is set to self_ty, so Self references resolve correctly)
                    if let Some(method_defs_for_check) =
                        self.interface_method_defs.get(iface_name).cloned()
                    {
                        for method_def in &method_defs_for_check {
                            let mangled = format!("{}__{}", type_name, method_def.name);
                            let impl_sig = self.functions.get(&mangled).cloned();
                            if let Some(impl_sig) = impl_sig {
                                // Resolve the expected types from AST with Self in scope
                                let expected_params: Vec<(String, TypeId)> = method_def
                                    .params
                                    .iter()
                                    .map(|p| {
                                        if p.is_self {
                                            (p.name.clone(), self_ty)
                                        } else {
                                            (p.name.clone(), self.resolve_type(&p.ty))
                                        }
                                    })
                                    .collect();
                                let expected_ret = method_def
                                    .return_type
                                    .as_ref()
                                    .map(|t| self.resolve_type(t))
                                    .unwrap_or_else(|| self.types.unit());

                                let ret_match = impl_sig.return_type == expected_ret;
                                let expected_non_self: Vec<_> =
                                    expected_params.iter().skip(1).collect();
                                let impl_non_self: Vec<_> =
                                    impl_sig.params.iter().skip(1).collect();
                                let params_match = expected_non_self.len() == impl_non_self.len()
                                    && expected_non_self
                                        .iter()
                                        .zip(impl_non_self.iter())
                                        .all(|((_, exp_ty), (_, impl_ty))| exp_ty == impl_ty);
                                if !ret_match || !params_match {
                                    let expected = format!(
                                        "fn({}) -> {}",
                                        expected_non_self
                                            .iter()
                                            .map(|(n, t)| format!("{}: {}", n, self.type_name(*t)))
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                        self.type_name(expected_ret)
                                    );
                                    let found = format!(
                                        "fn({}) -> {}",
                                        impl_non_self
                                            .iter()
                                            .map(|(n, t)| format!("{}: {}", n, self.type_name(*t)))
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                        self.type_name(impl_sig.return_type)
                                    );
                                    self.diagnostics.add(
                                        &CheckerDiagnostic::InterfaceMethodSignatureMismatch {
                                            span: item.span,
                                            type_name: type_name.clone(),
                                            interface_name: iface_name.clone(),
                                            method: method_def.name.clone(),
                                            expected,
                                            found,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }

                // Clear Self type scope and any interface type params
                self.type_param_scope.remove("Self");
                if let Some(iface_name) = interface_name {
                    if let Some(generic_iface) = self.generic_interfaces.get(iface_name).cloned() {
                        for tp in &generic_iface.type_params {
                            self.type_param_scope.remove(&tp.name);
                        }
                    }
                }
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
