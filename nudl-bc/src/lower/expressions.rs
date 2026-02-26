use nudl_ast::ast::*;

use crate::ir::*;

use super::context::FunctionLowerCtx;
use super::parse_int_const;

impl<'a> FunctionLowerCtx<'a> {
    pub(super) fn lower_expr(&mut self, expr: &nudl_core::span::Spanned<Expr>) -> Register {
        self.current_span = expr.span;
        match &expr.node {
            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = &callee.node {
                    if let Some(sig) = self.function_sigs.get(name).cloned() {
                        return match sig.kind {
                            crate::checker::FunctionKind::Builtin => {
                                self.lower_builtin_call(name, args)
                            }
                            crate::checker::FunctionKind::Extern => {
                                self.lower_resolved_call(name, &sig, args, true, 0)
                            }
                            crate::checker::FunctionKind::UserDefined => {
                                self.lower_resolved_call(name, &sig, args, false, 0)
                            }
                        };
                    }
                }
                // Fallback: emit unit
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                // First, infer receiver type name for mangled lookup
                let type_name = self.infer_receiver_type_name(object);
                if let Some(type_name) = type_name {
                    let mangled_name = format!("{}__{}", type_name, method);
                    if let Some(sig) = self.function_sigs.get(&mangled_name).cloned() {
                        // Lower the object (self argument)
                        let self_reg = self.lower_expr(object);
                        return self.lower_method_call(&mangled_name, &sig, self_reg, args);
                    }
                }
                // Fallback
                self.lower_expr(object);
                for arg in args {
                    self.lower_expr(&arg.value);
                }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::StaticCall {
                type_name,
                method,
                args,
            } => {
                let mangled_name = format!("{}__{}", type_name, method);
                if let Some(sig) = self.function_sigs.get(&mangled_name).cloned() {
                    return self.lower_resolved_call(&mangled_name, &sig, args, false, 0);
                }
                // Fallback
                for arg in args {
                    self.lower_expr(&arg.value);
                }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Literal(Literal::String(s)) => {
                // Deduplicate string constants
                let idx = if let Some(pos) = self.string_constants.iter().position(|c| c == s) {
                    pos as u32
                } else {
                    let idx = self.string_constants.len() as u32;
                    self.string_constants.push(s.clone());
                    idx
                };
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, ConstValue::StringLiteral(idx)));
                reg
            }

            Expr::Literal(Literal::Int(s, suffix)) => {
                let const_val = parse_int_const(s, *suffix);
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, const_val));
                reg
            }

            Expr::Literal(Literal::Float(s)) => {
                let val: f64 = s.parse().unwrap_or(0.0);
                let f64_ty = self.types.f64();
                let reg = self.alloc_typed_register(f64_ty);
                self.push_inst(Instruction::Const(reg, ConstValue::F64(val)));
                reg
            }

            Expr::Literal(Literal::Bool(b)) => {
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, ConstValue::Bool(*b)));
                reg
            }

            Expr::Literal(Literal::Char(c)) => {
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, ConstValue::Char(*c)));
                reg
            }

            Expr::Ident(name) => {
                if let Some(&reg) = self.locals.get(name) {
                    reg
                } else {
                    // Should have been caught by checker
                    let reg = self.alloc_register();
                    self.push_inst(Instruction::ConstUnit(reg));
                    reg
                }
            }

            Expr::Return(value) => {
                let ret_reg = if let Some(inner) = value {
                    self.lower_expr(inner)
                } else {
                    let reg = self.alloc_register();
                    self.push_inst(Instruction::ConstUnit(reg));
                    reg
                };
                // Terminate current block with a Return and start a dead block
                // for any subsequent code in the same scope.
                let dead_block = self.new_block_id();
                self.finish_block(Terminator::Return(ret_reg));
                self.start_block(dead_block);
                // Return unit since the code after this is unreachable
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Binary { op, left, right } => {
                // Short-circuit for && and ||
                match op {
                    BinOp::And => return self.lower_short_circuit_and(left, right),
                    BinOp::Or => return self.lower_short_circuit_or(left, right),
                    _ => {}
                }

                let binop_span = self.current_span;
                let lhs = self.lower_expr(left);
                let rhs = self.lower_expr(right);
                self.current_span = binop_span;

                // Check if this is a float operation (propagate lhs type for arithmetic)
                let lhs_type = self.register_types[lhs.0 as usize];
                let is_float = matches!(
                    self.types.resolve(lhs_type),
                    nudl_core::types::TypeKind::Primitive(p) if p.is_float()
                );

                // Comparisons always produce i64 (bool); arithmetic propagates operand type
                let is_comparison = matches!(
                    op,
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                );

                let dst = if is_float && !is_comparison {
                    self.alloc_typed_register(lhs_type)
                } else {
                    self.alloc_register()
                };

                let inst = match op {
                    BinOp::Add => Instruction::Add(dst, lhs, rhs),
                    BinOp::Sub => Instruction::Sub(dst, lhs, rhs),
                    BinOp::Mul => Instruction::Mul(dst, lhs, rhs),
                    BinOp::Div => Instruction::Div(dst, lhs, rhs),
                    BinOp::Mod => Instruction::Mod(dst, lhs, rhs),
                    BinOp::Shl => Instruction::Shl(dst, lhs, rhs),
                    BinOp::Shr => Instruction::Shr(dst, lhs, rhs),
                    BinOp::BitAnd => Instruction::BitAnd(dst, lhs, rhs),
                    BinOp::BitOr => Instruction::BitOr(dst, lhs, rhs),
                    BinOp::BitXor => Instruction::BitXor(dst, lhs, rhs),
                    BinOp::Eq => Instruction::Eq(dst, lhs, rhs),
                    BinOp::Ne => Instruction::Ne(dst, lhs, rhs),
                    BinOp::Lt => Instruction::Lt(dst, lhs, rhs),
                    BinOp::Le => Instruction::Le(dst, lhs, rhs),
                    BinOp::Gt => Instruction::Gt(dst, lhs, rhs),
                    BinOp::Ge => Instruction::Ge(dst, lhs, rhs),
                    BinOp::And | BinOp::Or => unreachable!(),
                };
                self.push_inst(inst);
                dst
            }

            Expr::Unary { op, operand } => {
                let src = self.lower_expr(operand);
                let src_type = self.register_types[src.0 as usize];
                let is_float = matches!(
                    self.types.resolve(src_type),
                    nudl_core::types::TypeKind::Primitive(p) if p.is_float()
                );
                let dst = if is_float && matches!(op, UnaryOp::Neg) {
                    self.alloc_typed_register(src_type)
                } else {
                    self.alloc_register()
                };
                let inst = match op {
                    UnaryOp::Neg => Instruction::Neg(dst, src),
                    UnaryOp::Not => Instruction::Not(dst, src),
                    UnaryOp::BitNot => Instruction::BitNot(dst, src),
                };
                self.push_inst(inst);
                dst
            }

            Expr::StructLiteral { name, fields } => {
                let type_id = self.struct_defs.get(name.as_str()).copied().unwrap();
                let dst = self.alloc_register();
                self.push_inst(Instruction::Alloc(dst, type_id));

                // Resolve field order from the type definition
                let struct_fields = match self.types.resolve(type_id).clone() {
                    nudl_core::types::TypeKind::Struct { fields: f, .. } => f,
                    _ => vec![],
                };

                for (field_name, field_val) in fields {
                    let val_reg = self.lower_expr(field_val);
                    // Find field index in the struct definition
                    let field_idx = struct_fields
                        .iter()
                        .position(|(n, _)| n == field_name)
                        .unwrap() as u32;
                    self.push_inst(Instruction::Store(dst, field_idx, val_reg));
                }
                dst
            }

            Expr::FieldAccess { object, field } => {
                let obj_reg = self.lower_expr(object);
                // Check if this is a tuple field access (.0, .1, etc.)
                if let Ok(idx) = field.parse::<u32>() {
                    let obj_type = self.infer_expr_type(object);
                    if let Some(tid) = obj_type {
                        if self.types.is_tuple(tid) {
                            let dst = self.alloc_register();
                            self.push_inst(Instruction::TupleLoad(dst, obj_reg, idx));
                            return dst;
                        }
                    }
                }
                // Resolve object type to find field index (struct field access)
                let field_idx = self.resolve_field_index(object, field);
                let dst = self.alloc_register();
                self.push_inst(Instruction::Load(dst, obj_reg, field_idx));
                dst
            }

            Expr::Assign { target, value } => {
                let val_reg = self.lower_expr(value);
                if let Expr::Ident(name) = &target.node {
                    // Release old value if reassigning a struct-typed variable
                    if let Some(&type_id) = self.local_types.get(name) {
                        if self.types.is_struct(type_id) {
                            if let Some(&old_reg) = self.locals.get(name) {
                                self.push_inst(Instruction::Release(old_reg, Some(type_id)));
                            }
                        }
                    }
                    self.locals.update(name, val_reg);
                } else if let Expr::FieldAccess { object, field } = &target.node {
                    let obj_reg = self.lower_expr(object);
                    let field_idx = self.resolve_field_index(object, field);
                    self.push_inst(Instruction::Store(obj_reg, field_idx, val_reg));
                } else if let Expr::IndexAccess { object, index } = &target.node {
                    let obj_reg = self.lower_expr(object);
                    let idx_reg = self.lower_expr(index);
                    self.push_inst(Instruction::IndexStore(obj_reg, idx_reg, val_reg));
                }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::CompoundAssign { op, target, value } => {
                // target = target op value
                let target_reg = self.lower_expr(target);
                let val_reg = self.lower_expr(value);
                let result_reg = self.alloc_register();
                let inst = match op {
                    BinOp::Add => Instruction::Add(result_reg, target_reg, val_reg),
                    BinOp::Sub => Instruction::Sub(result_reg, target_reg, val_reg),
                    BinOp::Mul => Instruction::Mul(result_reg, target_reg, val_reg),
                    BinOp::Div => Instruction::Div(result_reg, target_reg, val_reg),
                    BinOp::Mod => Instruction::Mod(result_reg, target_reg, val_reg),
                    BinOp::Shl => Instruction::Shl(result_reg, target_reg, val_reg),
                    BinOp::Shr => Instruction::Shr(result_reg, target_reg, val_reg),
                    BinOp::BitAnd => Instruction::BitAnd(result_reg, target_reg, val_reg),
                    BinOp::BitOr => Instruction::BitOr(result_reg, target_reg, val_reg),
                    BinOp::BitXor => Instruction::BitXor(result_reg, target_reg, val_reg),
                    _ => unreachable!(),
                };
                self.push_inst(inst);
                if let Expr::Ident(name) = &target.node {
                    self.locals.update(name, result_reg);
                }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_reg = self.lower_expr(condition);

                let then_block = self.new_block_id();
                let else_block = self.new_block_id();
                let merge_block = self.new_block_id();

                // Pre-allocate result register
                let result_reg = self.alloc_register();

                // Snapshot locals before branching so we can reconcile mutations
                let pre_if_locals = self.locals.flatten();

                // End current block with branch
                self.finish_block(Terminator::Branch(cond_reg, then_block, else_block));

                // Then block
                self.start_block(then_block);
                let then_result = self.lower_block_expr(&then_branch.node);
                self.push_inst(Instruction::Copy(result_reg, then_result));
                self.finish_block(Terminator::Jump(merge_block));
                let then_block_idx = self.blocks.len() - 1;
                let post_then_locals = self.locals.flatten();

                // Restore locals to pre-if state before lowering else branch
                for (name, reg) in &pre_if_locals {
                    self.locals.update(name, *reg);
                }

                // Else block
                self.start_block(else_block);
                if let Some(else_expr) = else_branch {
                    let else_result = self.lower_expr(else_expr);
                    self.push_inst(Instruction::Copy(result_reg, else_result));
                } else {
                    self.push_inst(Instruction::ConstUnit(result_reg));
                }
                self.finish_block(Terminator::Jump(merge_block));
                let else_block_idx = self.blocks.len() - 1;
                let post_else_locals = self.locals.flatten();

                // Reconcile: for any variable mutated differently in the two
                // branches, emit Copy instructions into both branch blocks so
                // they agree on a single merge register.
                for (name, pre_reg) in &pre_if_locals {
                    let then_reg = post_then_locals.get(name).copied().unwrap_or(*pre_reg);
                    let else_reg = post_else_locals.get(name).copied().unwrap_or(*pre_reg);

                    if then_reg != else_reg {
                        // Both branches have different registers — merge them
                        let merge_reg = self.alloc_register();
                        self.blocks[then_block_idx]
                            .instructions
                            .push(Instruction::Copy(merge_reg, then_reg));
                        self.blocks[then_block_idx].spans.push(self.current_span);
                        self.blocks[else_block_idx]
                            .instructions
                            .push(Instruction::Copy(merge_reg, else_reg));
                        self.blocks[else_block_idx].spans.push(self.current_span);
                        self.locals.update(name, merge_reg);
                    } else if then_reg != *pre_reg {
                        // Both branches mutated to the same register
                        self.locals.update(name, then_reg);
                    }
                }

                // Merge block
                self.start_block(merge_block);
                result_reg
            }

            Expr::Cast { expr, target_type } => {
                let src = self.lower_expr(expr);
                let target_id = self.resolve_type_expr(&target_type.node);
                let dst = self.alloc_typed_register(target_id);
                self.push_inst(Instruction::Cast(dst, src, target_id));
                dst
            }

            Expr::While {
                condition,
                body,
                label,
            } => {
                let cond_block = self.new_block_id();
                let body_block = self.new_block_id();
                let exit_block = self.new_block_id();

                // Jump to condition block
                self.finish_block(Terminator::Jump(cond_block));

                // Snapshot locals before condition (these are the registers the condition uses)
                self.start_block(cond_block);
                let pre_loop_locals = self.locals.clone();
                let cond_reg = self.lower_expr(condition);
                self.finish_block(Terminator::Branch(cond_reg, body_block, exit_block));

                // Body block
                self.start_block(body_block);
                self.loop_stack.push(super::LoopContext {
                    label: label.clone(),
                    continue_block: cond_block,
                    break_block: exit_block,
                    pre_loop_locals: pre_loop_locals.clone(),
                });
                self.lower_block_expr(&body.node);
                self.loop_stack.pop();

                // Copy-back: emit Copy instructions for any locals whose register changed
                // so that the condition block sees updated values on next iteration
                self.emit_loop_copyback(&pre_loop_locals);
                self.finish_block(Terminator::Jump(cond_block));

                // Exit block — restore locals to pre-loop state
                self.start_block(exit_block);
                self.locals = pre_loop_locals;
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Loop { body, label } => {
                let body_block = self.new_block_id();
                let exit_block = self.new_block_id();

                // Jump to body block
                self.finish_block(Terminator::Jump(body_block));

                // Body block
                self.start_block(body_block);
                let pre_loop_locals = self.locals.clone();
                self.loop_stack.push(super::LoopContext {
                    label: label.clone(),
                    continue_block: body_block,
                    break_block: exit_block,
                    pre_loop_locals: pre_loop_locals.clone(),
                });
                self.lower_block_expr(&body.node);
                self.loop_stack.pop();

                // Copy-back for loop variables
                self.emit_loop_copyback(&pre_loop_locals);
                self.finish_block(Terminator::Jump(body_block));

                // Exit block (reached by break) — restore locals
                self.start_block(exit_block);
                self.locals = pre_loop_locals;
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Break { label, .. } => {
                let lc_info = if let Some(label) = label {
                    self.loop_stack
                        .iter()
                        .rev()
                        .find(|lc| lc.label.as_deref() == Some(label))
                        .map(|lc| (lc.break_block, lc.pre_loop_locals.clone()))
                } else {
                    self.loop_stack
                        .last()
                        .map(|lc| (lc.break_block, lc.pre_loop_locals.clone()))
                };
                if let Some((break_block, pre_loop_locals)) = lc_info {
                    self.emit_loop_copyback(&pre_loop_locals);
                    self.finish_block(Terminator::Jump(break_block));
                    let dead = self.new_block_id();
                    self.start_block(dead);
                }
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }

            Expr::Continue { label } => {
                let lc_info = if let Some(label) = label {
                    self.loop_stack
                        .iter()
                        .rev()
                        .find(|lc| lc.label.as_deref() == Some(label))
                        .map(|lc| (lc.continue_block, lc.pre_loop_locals.clone()))
                } else {
                    self.loop_stack
                        .last()
                        .map(|lc| (lc.continue_block, lc.pre_loop_locals.clone()))
                };
                if let Some((continue_block, pre_loop_locals)) = lc_info {
                    self.emit_loop_copyback(&pre_loop_locals);
                    self.finish_block(Terminator::Jump(continue_block));
                    let dead = self.new_block_id();
                    self.start_block(dead);
                }
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }

            Expr::Grouped(inner) => self.lower_expr(inner),

            Expr::Block(block) => self.lower_block_expr(block),

            Expr::TupleLiteral(elements) => {
                let elem_regs: Vec<Register> =
                    elements.iter().map(|e| self.lower_expr(e)).collect();
                let elem_types: Vec<nudl_core::types::TypeId> = elements
                    .iter()
                    .map(|e| self.infer_expr_type(e).unwrap_or(self.types.i32()))
                    .collect();
                let type_id = self
                    .types
                    .intern(nudl_core::types::TypeKind::Tuple(elem_types));
                let dst = self.alloc_register();
                self.push_inst(Instruction::TupleAlloc(dst, type_id, elem_regs));
                dst
            }

            Expr::ArrayLiteral(elements) => {
                let elem_regs: Vec<Register> =
                    elements.iter().map(|e| self.lower_expr(e)).collect();
                let elem_type = if !elements.is_empty() {
                    self.infer_expr_type(&elements[0])
                        .unwrap_or(self.types.i32())
                } else {
                    self.types.i32()
                };
                let type_id = self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_type,
                    length: elements.len(),
                });
                let dst = self.alloc_register();
                self.push_inst(Instruction::FixedArrayAlloc(dst, type_id, elem_regs));
                dst
            }

            Expr::ArrayRepeat { value, count } => {
                let val_reg = self.lower_expr(value);
                let elem_type = self.infer_expr_type(value).unwrap_or(self.types.i32());
                let elem_regs: Vec<Register> = (0..*count).map(|_| val_reg).collect();
                let type_id = self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_type,
                    length: *count,
                });
                let dst = self.alloc_register();
                self.push_inst(Instruction::FixedArrayAlloc(dst, type_id, elem_regs));
                dst
            }

            Expr::IndexAccess { object, index } => {
                let obj_reg = self.lower_expr(object);
                let idx_reg = self.lower_expr(index);
                let elem_type = self.infer_index_element_type(object);
                let dst = self.alloc_register();
                self.push_inst(Instruction::IndexLoad(dst, obj_reg, idx_reg, elem_type));
                dst
            }

            Expr::Range { .. } => {
                // Ranges are only used in for-loops; standalone range expressions
                // produce unit for now
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }

            Expr::For {
                binding,
                iter,
                body,
            } => {
                // Desugar for-in to while loop
                match &iter.node {
                    Expr::Range {
                        start,
                        end,
                        inclusive,
                    } => self.lower_for_range(binding, start, end, *inclusive, &body.node),
                    _ => {
                        // Array iteration
                        self.lower_for_array(binding, iter, &body.node)
                    }
                }
            }

            _ => {
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }
        }
    }

    pub(super) fn lower_short_circuit_and(
        &mut self,
        left: &nudl_core::span::Spanned<Expr>,
        right: &nudl_core::span::Spanned<Expr>,
    ) -> Register {
        let result_reg = self.alloc_register();
        let lhs = self.lower_expr(left);

        let rhs_block = self.new_block_id();
        let merge_block = self.new_block_id();
        let false_block = self.new_block_id();

        self.finish_block(Terminator::Branch(lhs, rhs_block, false_block));

        // If lhs is true, evaluate rhs
        self.start_block(rhs_block);
        let rhs = self.lower_expr(right);
        self.push_inst(Instruction::Copy(result_reg, rhs));
        self.finish_block(Terminator::Jump(merge_block));

        // If lhs is false, short-circuit
        self.start_block(false_block);
        self.push_inst(Instruction::Const(result_reg, ConstValue::Bool(false)));
        self.finish_block(Terminator::Jump(merge_block));

        self.start_block(merge_block);
        result_reg
    }

    pub(super) fn lower_short_circuit_or(
        &mut self,
        left: &nudl_core::span::Spanned<Expr>,
        right: &nudl_core::span::Spanned<Expr>,
    ) -> Register {
        let result_reg = self.alloc_register();
        let lhs = self.lower_expr(left);

        let true_block = self.new_block_id();
        let rhs_block = self.new_block_id();
        let merge_block = self.new_block_id();

        self.finish_block(Terminator::Branch(lhs, true_block, rhs_block));

        // If lhs is true, short-circuit
        self.start_block(true_block);
        self.push_inst(Instruction::Const(result_reg, ConstValue::Bool(true)));
        self.finish_block(Terminator::Jump(merge_block));

        // If lhs is false, evaluate rhs
        self.start_block(rhs_block);
        let rhs = self.lower_expr(right);
        self.push_inst(Instruction::Copy(result_reg, rhs));
        self.finish_block(Terminator::Jump(merge_block));

        self.start_block(merge_block);
        result_reg
    }
}
