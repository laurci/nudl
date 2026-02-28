use nudl_ast::ast::*;

use crate::ir::*;

use super::context::FunctionLowerCtx;

impl<'a> FunctionLowerCtx<'a> {
    /// Lower a block and return the register holding its value.
    /// Pushes a new scope so variables defined inside are not visible outside.
    pub(super) fn lower_block_expr(&mut self, block: &Block) -> Register {
        self.locals.push_scope();
        self.local_types.push_scope();
        for stmt in &block.stmts {
            self.lower_stmt(stmt);
        }
        let result = if let Some(tail) = &block.tail_expr {
            self.lower_expr(tail)
        } else {
            let reg = self.alloc_register();
            self.push_inst(Instruction::ConstUnit(reg));
            reg
        };

        // Scope release: emit Release for reference-typed locals defined in this scope
        // Skip releasing the result register — it's the block's return value and
        // will be consumed by the caller (or by the function's Return instruction).
        let scope_types = self.local_types.current_scope_entries();
        for (name, type_id) in &scope_types {
            if let Some(&reg) = self.locals.get(name) {
                if reg == result {
                    continue;
                }
                self.emit_release_for_type(reg, *type_id);
            }
        }

        self.local_types.pop_scope();
        self.locals.pop_scope();
        result
    }

    pub(super) fn lower_stmt(&mut self, stmt: &nudl_core::span::Spanned<Stmt>) {
        self.current_span = stmt.span;
        match &stmt.node {
            Stmt::Expr(expr) => {
                self.lower_expr(expr);
            }
            Stmt::Let {
                name, ty, value, ..
            }
            | Stmt::Const {
                name, ty, value, ..
            } => {
                // If there's a type annotation, resolve it first
                let annotated_type = ty.as_ref().map(|t| self.resolve_type_expr(&t.node));

                // If the value is an empty array literal and the type is DynArray,
                // emit DynArrayAlloc instead of FixedArrayAlloc
                // If the value is Map::new() and the type annotation is Map<K,V>,
                // use the annotated type for the MapAlloc instruction
                let reg = if let (
                    Some(type_id),
                    Expr::StaticCall {
                        type_name,
                        method,
                        args,
                        ..
                    },
                ) = (annotated_type, &value.node)
                {
                    if type_name == "Map" && method == "new" && args.is_empty() {
                        if let nudl_core::types::TypeKind::Map { .. } = self.types.resolve(type_id)
                        {
                            let dst = self.alloc_typed_register(type_id);
                            self.push_inst(Instruction::MapAlloc(dst, type_id));
                            dst
                        } else {
                            self.lower_expr(value)
                        }
                    } else {
                        self.lower_expr(value)
                    }
                } else if let (Some(type_id), Expr::ArrayLiteral(elems)) =
                    (annotated_type, &value.node)
                {
                    if elems.is_empty() {
                        if let nudl_core::types::TypeKind::DynamicArray { .. } =
                            self.types.resolve(type_id)
                        {
                            let dst = self.alloc_typed_register(type_id);
                            self.push_inst(Instruction::DynArrayAlloc(dst, type_id));
                            dst
                        } else {
                            self.lower_expr(value)
                        }
                    } else if let nudl_core::types::TypeKind::DynamicArray { .. } =
                        self.types.resolve(type_id)
                    {
                        // Non-empty array literal assigned to dynamic array type:
                        // alloc + push each element
                        let dst = self.alloc_typed_register(type_id);
                        self.push_inst(Instruction::DynArrayAlloc(dst, type_id));
                        for elem in elems {
                            let elem_reg = self.lower_expr(elem);
                            self.push_inst(Instruction::DynArrayPush(dst, elem_reg));
                        }
                        dst
                    } else {
                        self.lower_expr(value)
                    }
                } else {
                    self.lower_expr(value)
                };
                // For reference types, allocate a fresh register and copy.
                // This prevents aliasing with parameter registers (which would cause
                // double-release between scope-release and callee-release).
                let local_type = annotated_type.or_else(|| self.infer_expr_type(value));
                let reg = if let Some(type_id) = local_type {
                    if self.types.is_reference_type(type_id)
                        && !matches!(
                            self.types.resolve(type_id),
                            nudl_core::types::TypeKind::String
                        )
                    {
                        let fresh = self.alloc_typed_register(type_id);
                        self.push_inst(Instruction::Copy(fresh, reg));
                        fresh
                    } else {
                        reg
                    }
                } else {
                    reg
                };
                self.locals.insert(name.clone(), reg);

                // Track typed locals: prefer annotation, fall back to expression inference
                if let Some(type_id) = local_type {
                    self.local_types.insert(name.clone(), type_id);
                } else if let Some(type_id) = self.infer_expr_type(value) {
                    self.local_types.insert(name.clone(), type_id);
                }
            }
            Stmt::LetPattern { pattern, value, .. } => {
                let val_reg = self.lower_expr(value);
                let val_ty = self.infer_expr_type(value);
                self.lower_pattern_binding(&pattern.node, val_reg, val_ty);
            }
            Stmt::Defer { body } => {
                // Store deferred statements, they will be emitted at scope exit
                // For now, just lower inline (simplified defer)
                self.deferred_blocks.push(body.node.clone());
            }
            Stmt::Item(_) => {} // nested items not supported yet
        }
    }

    /// Lower a pattern destructuring, binding names to extracted values
    pub(super) fn lower_pattern_binding(
        &mut self,
        pattern: &nudl_ast::ast::Pattern,
        val_reg: Register,
        val_ty: Option<nudl_core::types::TypeId>,
    ) {
        match pattern {
            nudl_ast::ast::Pattern::Wildcard => {} // nothing to bind
            nudl_ast::ast::Pattern::Binding(name) => {
                self.locals.insert(name.clone(), val_reg);
                if let Some(ty) = val_ty {
                    self.local_types.insert(name.clone(), ty);
                }
            }
            nudl_ast::ast::Pattern::Tuple(elements) => {
                // Extract each tuple element
                if let Some(ty) = val_ty {
                    if let nudl_core::types::TypeKind::Tuple(elem_types) =
                        self.types.resolve(ty).clone()
                    {
                        for (i, pat) in elements.iter().enumerate() {
                            let elem_ty = elem_types.get(i).copied();
                            let elem_reg = self.alloc_register();
                            // Use TupleLoad to extract element i
                            self.push_inst(Instruction::TupleLoad(elem_reg, val_reg, i as u32));
                            if let Some(ety) = elem_ty {
                                self.register_types[elem_reg.0 as usize] = ety;
                            }
                            self.lower_pattern_binding(&pat.node, elem_reg, elem_ty);
                        }
                        return;
                    }
                }
                // Fallback: just bind each sub-pattern to val_reg
                for pat in elements {
                    self.lower_pattern_binding(&pat.node, val_reg, val_ty);
                }
            }
            nudl_ast::ast::Pattern::Struct { name, fields, .. } => {
                // Look up the struct type to find field indices
                if let Some(&struct_ty) = self.struct_defs.get(name) {
                    if let nudl_core::types::TypeKind::Struct {
                        fields: struct_fields,
                        ..
                    } = self.types.resolve(struct_ty).clone()
                    {
                        for (field_name, pat) in fields {
                            if let Some((idx, (_, field_ty))) = struct_fields
                                .iter()
                                .enumerate()
                                .find(|(_, (n, _))| n == field_name)
                            {
                                let ft = *field_ty;
                                let field_reg = self.alloc_typed_register(ft);
                                self.push_inst(Instruction::Load(field_reg, val_reg, idx as u32));
                                self.lower_pattern_binding(&pat.node, field_reg, Some(ft));
                            }
                        }
                    }
                }
            }
            nudl_ast::ast::Pattern::Enum { .. } => {
                // Enum destructuring in let is not common, skip for now
            }
            nudl_ast::ast::Pattern::Literal(_) => {} // nothing to bind
            nudl_ast::ast::Pattern::Array { prefix, suffix, .. } => {
                if let Some(ty) = val_ty {
                    let resolved = self.types.resolve(ty).clone();
                    let (elem_ty, is_dynamic) = match &resolved {
                        nudl_core::types::TypeKind::DynamicArray { element } => {
                            (Some(*element), true)
                        }
                        nudl_core::types::TypeKind::FixedArray { element, .. } => {
                            (Some(*element), false)
                        }
                        _ => (None, false),
                    };

                    // Extract prefix elements at indices 0..prefix.len()
                    for (i, pat) in prefix.iter().enumerate() {
                        let idx_reg = self.alloc_register();
                        self.push_inst(Instruction::Const(idx_reg, ConstValue::I64(i as i64)));
                        let elem_reg = if let Some(ety) = elem_ty {
                            self.alloc_typed_register(ety)
                        } else {
                            self.alloc_register()
                        };
                        if is_dynamic {
                            self.push_inst(Instruction::DynArrayGet(elem_reg, val_reg, idx_reg));
                        } else {
                            let ety = elem_ty.unwrap_or(self.types.i64());
                            self.push_inst(Instruction::IndexLoad(elem_reg, val_reg, idx_reg, ety));
                        }
                        self.lower_pattern_binding(&pat.node, elem_reg, elem_ty);
                    }

                    // Extract suffix elements at indices len-suffix.len()+j
                    if !suffix.is_empty() {
                        let len_reg = if is_dynamic {
                            let r = self.alloc_register();
                            self.push_inst(Instruction::DynArrayLen(r, val_reg));
                            r
                        } else {
                            let fixed_len =
                                if let nudl_core::types::TypeKind::FixedArray { length, .. } =
                                    &resolved
                                {
                                    *length
                                } else {
                                    0
                                };
                            let r = self.alloc_register();
                            self.push_inst(Instruction::Const(
                                r,
                                ConstValue::I64(fixed_len as i64),
                            ));
                            r
                        };
                        let suffix_len_reg = self.alloc_register();
                        self.push_inst(Instruction::Const(
                            suffix_len_reg,
                            ConstValue::I64(suffix.len() as i64),
                        ));
                        let suffix_start_reg = self.alloc_register();
                        self.push_inst(Instruction::Sub(suffix_start_reg, len_reg, suffix_len_reg));
                        for (j, pat) in suffix.iter().enumerate() {
                            let j_reg = self.alloc_register();
                            self.push_inst(Instruction::Const(j_reg, ConstValue::I64(j as i64)));
                            let idx_reg = self.alloc_register();
                            self.push_inst(Instruction::Add(idx_reg, suffix_start_reg, j_reg));
                            let elem_reg = if let Some(ety) = elem_ty {
                                self.alloc_typed_register(ety)
                            } else {
                                self.alloc_register()
                            };
                            if is_dynamic {
                                self.push_inst(Instruction::DynArrayGet(
                                    elem_reg, val_reg, idx_reg,
                                ));
                            } else {
                                let ety = elem_ty.unwrap_or(self.types.i64());
                                self.push_inst(Instruction::IndexLoad(
                                    elem_reg, val_reg, idx_reg, ety,
                                ));
                            }
                            self.lower_pattern_binding(&pat.node, elem_reg, elem_ty);
                        }
                    }
                }
            }
        }
    }
}
