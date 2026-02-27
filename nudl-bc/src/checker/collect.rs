use nudl_core::types::EnumVariant;

use super::*;

impl Checker {
    // --- Pass 1: Collect declarations ---

    pub(super) fn collect_fn_sig(
        &mut self,
        name: &str,
        params: &[Param],
        return_type: &Option<Spanned<TypeExpr>>,
        span: Span,
    ) {
        if self.functions.contains_key(name) {
            self.diagnostics.add(&CheckerDiagnostic::DuplicateFunction {
                span,
                name: name.into(),
            });
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
            },
        );
    }

    pub(super) fn collect_item(&mut self, item: &SpannedItem) {
        match &item.node {
            Item::FnDef {
                name,
                params,
                return_type,
                ..
            } => {
                if name == "main" {
                    self.found_main = true;
                    if !params.is_empty() || return_type.is_some() {
                        self.diagnostics
                            .add(&CheckerDiagnostic::InvalidMainSignature { span: item.span });
                    }
                }

                self.collect_fn_sig(name, params, return_type, item.span);
            }
            Item::StructDef { name, fields, .. } => {
                if self.structs.contains_key(name) {
                    self.diagnostics.add(&CheckerDiagnostic::DuplicateStruct {
                        span: item.span,
                        name: name.clone(),
                    });
                    return;
                }

                let resolved_fields: Vec<(String, TypeId)> = fields
                    .iter()
                    .map(|f| (f.name.clone(), self.resolve_type(&f.ty)))
                    .collect();

                let type_id = self.types.intern(TypeKind::Struct {
                    name: name.clone(),
                    fields: resolved_fields,
                });

                self.structs.insert(name.clone(), type_id);
            }
            Item::EnumDef { name, variants, .. } => {
                if self.enums.contains_key(name) || self.structs.contains_key(name) {
                    self.diagnostics.add(&CheckerDiagnostic::DuplicateStruct {
                        span: item.span,
                        name: name.clone(),
                    });
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
            Item::InterfaceDef { name, methods, .. } => {
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
                interface_name,
                methods,
                ..
            } => {
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

                // Register each method as a mangled function: TypeName__methodname
                for method_item in methods {
                    if let Item::FnDef {
                        name: method_name,
                        params,
                        return_type,
                        ..
                    } = &method_item.node
                    {
                        let mangled_name = format!("{}__{}", type_name, method_name);

                        // Resolve params, replacing Self type with the actual type
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
                            },
                        );
                    }
                }
            }
            Item::Import { .. } => {
                // Imports are handled at the pipeline level
            }
            Item::TypeAlias { name, ty, .. } => {
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
                        },
                    );
                }
            }
        }
    }

    // --- Pass 2: Check bodies ---
}
