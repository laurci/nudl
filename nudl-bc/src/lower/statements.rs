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
        let scope_types = self.local_types.current_scope_entries();
        for (name, type_id) in &scope_types {
            if let Some(&reg) = self.locals.get(name) {
                if self.types.is_struct(*type_id) || self.types.is_enum(*type_id) {
                    self.push_inst(Instruction::Release(reg, Some(*type_id)));
                } else if let nudl_core::types::TypeKind::FixedArray { element, length } =
                    self.types.resolve(*type_id)
                {
                    let elem = *element;
                    let len = *length;
                    if self.types.is_reference_type(elem) {
                        for idx in 0..len {
                            let idx_reg = self.alloc_register();
                            self.push_inst(Instruction::Const(
                                idx_reg,
                                ConstValue::I32(idx as i32),
                            ));
                            let elem_reg = self.alloc_register();
                            self.push_inst(Instruction::IndexLoad(elem_reg, reg, idx_reg, elem));
                            self.push_inst(Instruction::Release(elem_reg, Some(elem)));
                        }
                    }
                }
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
            Stmt::Let { name, value, .. } | Stmt::Const { name, value, .. } => {
                let reg = self.lower_expr(value);
                self.locals.insert(name.clone(), reg);

                // Track typed locals for scope-exit Release, field/index type inference,
                // and float type propagation
                if let Some(type_id) = self.infer_expr_type(value) {
                    self.local_types.insert(name.clone(), type_id);
                }
            }
            Stmt::LetPattern {
                pattern, value, ..
            } => {
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
                            self.push_inst(Instruction::TupleLoad(
                                elem_reg,
                                val_reg,
                                i as u32,
                            ));
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
                                self.push_inst(Instruction::Load(
                                    field_reg,
                                    val_reg,
                                    idx as u32,
                                ));
                                self.lower_pattern_binding(
                                    &pat.node,
                                    field_reg,
                                    Some(ft),
                                );
                            }
                        }
                    }
                }
            }
            nudl_ast::ast::Pattern::Enum { .. } => {
                // Enum destructuring in let is not common, skip for now
            }
            nudl_ast::ast::Pattern::Literal(_) => {} // nothing to bind
        }
    }
}
