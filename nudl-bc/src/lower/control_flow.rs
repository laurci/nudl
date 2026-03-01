use nudl_ast::ast::*;

use crate::ir::*;
use crate::scoped_locals::ScopedLocals;

use super::LoopContext;
use super::context::FunctionLowerCtx;

impl<'a> FunctionLowerCtx<'a> {
    /// Emit Copy instructions at the end of a loop body to propagate updated
    /// local variables back to the registers that the loop header references.
    pub(super) fn emit_loop_copyback(&mut self, pre_loop_locals: &ScopedLocals<Register>) {
        let pre_flat = pre_loop_locals.flatten();
        for (name, pre_reg) in &pre_flat {
            if let Some(&current_reg) = self.locals.get(name) {
                if current_reg != *pre_reg {
                    self.push_inst(Instruction::Copy(*pre_reg, current_reg));
                    // Reset locals to use the original register so the condition
                    // block's hardcoded references remain valid
                    self.locals.update(name, *pre_reg);
                }
            }
        }
    }

    /// Desugar `for i in start..end { body }` to a while loop
    pub(super) fn lower_for_range(
        &mut self,
        binding: &str,
        start: &nudl_core::span::Spanned<Expr>,
        end: &nudl_core::span::Spanned<Expr>,
        inclusive: bool,
        body: &Block,
    ) -> Register {
        // let mut __iter = start;
        let iter_reg = self.lower_expr(start);
        let end_reg = self.lower_expr(end);

        let iter_name = format!("__for_iter_{}", binding);
        self.locals.insert(iter_name.clone(), iter_reg);

        let cond_block = self.new_block_id();
        let body_block = self.new_block_id();
        let incr_block = self.new_block_id();
        let exit_block = self.new_block_id();

        // Jump to condition
        self.finish_block(Terminator::Jump(cond_block));

        // Condition: __iter < end (or __iter <= end for inclusive)
        self.start_block(cond_block);
        let pre_loop_locals = self.locals.clone();
        let cur_iter = self.locals.get(&iter_name).copied().unwrap();
        let cond_reg = self.alloc_register();
        if inclusive {
            self.push_inst(Instruction::Le(cond_reg, cur_iter, end_reg));
        } else {
            self.push_inst(Instruction::Lt(cond_reg, cur_iter, end_reg));
        }
        self.finish_block(Terminator::Branch(cond_reg, body_block, exit_block));

        // Body block
        self.start_block(body_block);
        self.loop_stack.push(LoopContext {
            label: None,
            continue_block: incr_block,
            break_block: exit_block,
            pre_loop_locals: pre_loop_locals.clone(),
        });

        // let binding = __iter;
        let binding_reg = self.locals.get(&iter_name).copied().unwrap();
        self.locals.insert(binding.to_string(), binding_reg);

        self.lower_block_expr(body);

        self.loop_stack.pop();

        // Fall through to increment block
        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(incr_block));

        // Increment block: __iter = __iter + 1, then jump to cond
        self.start_block(incr_block);
        let cur_iter = self.locals.get(&iter_name).copied().unwrap();
        let one_reg = self.alloc_register();
        self.push_inst(Instruction::Const(one_reg, ConstValue::I32(1)));
        let next_iter = self.alloc_register();
        self.push_inst(Instruction::Add(next_iter, cur_iter, one_reg));
        self.locals.update(&iter_name, next_iter);

        // Copy-back loop variables
        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(cond_block));

        // Exit block
        self.start_block(exit_block);
        self.locals = pre_loop_locals;
        let unit_reg = self.alloc_register();
        self.push_inst(Instruction::ConstUnit(unit_reg));
        unit_reg
    }

    /// Desugar `for item in array { body }` to a while loop with indexing
    pub(super) fn lower_for_array(
        &mut self,
        binding: &str,
        iter_expr: &nudl_core::span::Spanned<Expr>,
        body: &Block,
    ) -> Register {
        let arr_reg = self.lower_expr(iter_expr);
        let arr_type = self.infer_expr_type(iter_expr);

        let is_dynamic = if let Some(tid) = arr_type {
            matches!(
                self.types.resolve(tid),
                nudl_core::types::TypeKind::DynamicArray { .. }
            )
        } else {
            false
        };

        let length = if !is_dynamic {
            if let Some(tid) = arr_type {
                match self.types.resolve(tid) {
                    nudl_core::types::TypeKind::FixedArray { length, .. } => *length,
                    _ => 0,
                }
            } else {
                0
            }
        } else {
            0 // Will use runtime length
        };

        let elem_type = if let Some(tid) = arr_type {
            match self.types.resolve(tid) {
                nudl_core::types::TypeKind::FixedArray { element, .. } => *element,
                nudl_core::types::TypeKind::DynamicArray { element } => *element,
                _ => self.types.i64(),
            }
        } else {
            self.types.i64()
        };

        // let mut __idx = 0;
        let idx_reg = self.alloc_register();
        self.push_inst(Instruction::Const(idx_reg, ConstValue::I32(0)));
        let idx_name = format!("__for_idx_{}", binding);
        self.locals.insert(idx_name.clone(), idx_reg);

        let len_reg = if is_dynamic {
            // For dynamic arrays, get length at runtime
            let len = self.alloc_register();
            self.push_inst(Instruction::DynArrayLen(len, arr_reg));
            len
        } else {
            let len = self.alloc_register();
            self.push_inst(Instruction::Const(len, ConstValue::I32(length as i32)));
            len
        };

        let cond_block = self.new_block_id();
        let body_block = self.new_block_id();
        let incr_block = self.new_block_id();
        let exit_block = self.new_block_id();

        self.finish_block(Terminator::Jump(cond_block));

        // Condition: __idx < len
        self.start_block(cond_block);
        let pre_loop_locals = self.locals.clone();
        let cur_idx = self.locals.get(&idx_name).copied().unwrap();
        let cond_reg = self.alloc_register();
        self.push_inst(Instruction::Lt(cond_reg, cur_idx, len_reg));
        self.finish_block(Terminator::Branch(cond_reg, body_block, exit_block));

        // Body block
        self.start_block(body_block);
        self.loop_stack.push(LoopContext {
            label: None,
            continue_block: incr_block,
            break_block: exit_block,
            pre_loop_locals: pre_loop_locals.clone(),
        });

        // let binding = arr[__idx];
        let cur_idx = self.locals.get(&idx_name).copied().unwrap();
        let elem_reg = self.alloc_typed_register(elem_type);
        if is_dynamic {
            self.push_inst(Instruction::DynArrayGet(elem_reg, arr_reg, cur_idx));
            // Retain extracted element — the array still owns one reference.
            if self.types.is_reference_type(elem_type) {
                self.push_inst(Instruction::Retain(elem_reg));
            }
        } else {
            self.push_inst(Instruction::IndexLoad(
                elem_reg, arr_reg, cur_idx, elem_type,
            ));
        }
        self.locals.insert(binding.to_string(), elem_reg);
        self.local_types.insert(binding.to_string(), elem_type);

        self.lower_block_expr(body);

        self.loop_stack.pop();

        // Release the loop element before copyback (balances the Retain at iteration start)
        if is_dynamic {
            if let Some(&cur_elem) = self.locals.get(binding) {
                self.emit_release_for_type(cur_elem, elem_type);
            }
        }

        // Fall through to increment block
        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(incr_block));

        // Increment block: __idx = __idx + 1, then jump to cond
        self.start_block(incr_block);
        let cur_idx = self.locals.get(&idx_name).copied().unwrap();
        let one_reg = self.alloc_register();
        self.push_inst(Instruction::Const(one_reg, ConstValue::I32(1)));
        let next_idx = self.alloc_register();
        self.push_inst(Instruction::Add(next_idx, cur_idx, one_reg));
        self.locals.update(&idx_name, next_idx);

        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(cond_block));

        // Exit block
        self.start_block(exit_block);
        self.locals = pre_loop_locals;
        let unit_reg = self.alloc_register();
        self.push_inst(Instruction::ConstUnit(unit_reg));
        unit_reg
    }

    /// Desugar `for ch in string { body }` to a while loop with StringCharAt
    pub(super) fn lower_for_string(
        &mut self,
        binding: &str,
        iter_expr: &nudl_core::span::Spanned<Expr>,
        body: &Block,
    ) -> Register {
        let str_reg = self.lower_expr(iter_expr);

        // let mut __idx = 0;
        let idx_reg = self.alloc_register();
        self.push_inst(Instruction::Const(idx_reg, ConstValue::I32(0)));
        let idx_name = format!("__for_idx_{}", binding);
        self.locals.insert(idx_name.clone(), idx_reg);

        // __len = StringLen(str_reg)
        let len_reg = self.alloc_register();
        self.push_inst(Instruction::StringLen(len_reg, str_reg));

        let cond_block = self.new_block_id();
        let body_block = self.new_block_id();
        let incr_block = self.new_block_id();
        let exit_block = self.new_block_id();

        self.finish_block(Terminator::Jump(cond_block));

        // Condition: __idx < len
        self.start_block(cond_block);
        let pre_loop_locals = self.locals.clone();
        let cur_idx = self.locals.get(&idx_name).copied().unwrap();
        let cond_reg = self.alloc_register();
        self.push_inst(Instruction::Lt(cond_reg, cur_idx, len_reg));
        self.finish_block(Terminator::Branch(cond_reg, body_block, exit_block));

        // Body block
        self.start_block(body_block);
        self.loop_stack.push(LoopContext {
            label: None,
            continue_block: incr_block,
            break_block: exit_block,
            pre_loop_locals: pre_loop_locals.clone(),
        });

        // let binding = str[__idx] (char — value type, no Retain needed)
        let cur_idx = self.locals.get(&idx_name).copied().unwrap();
        let char_type = self.types.char_type();
        let elem_reg = self.alloc_typed_register(char_type);
        self.push_inst(Instruction::StringCharAt(elem_reg, str_reg, cur_idx));
        self.locals.insert(binding.to_string(), elem_reg);
        self.local_types.insert(binding.to_string(), char_type);

        self.lower_block_expr(body);

        self.loop_stack.pop();

        // Fall through to increment block
        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(incr_block));

        // Increment block: __idx = __idx + 1, then jump to cond
        self.start_block(incr_block);
        let cur_idx = self.locals.get(&idx_name).copied().unwrap();
        let one_reg = self.alloc_register();
        self.push_inst(Instruction::Const(one_reg, ConstValue::I32(1)));
        let next_idx = self.alloc_register();
        self.push_inst(Instruction::Add(next_idx, cur_idx, one_reg));
        self.locals.update(&idx_name, next_idx);

        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(cond_block));

        // Exit block
        self.start_block(exit_block);
        self.locals = pre_loop_locals;
        let unit_reg = self.alloc_register();
        self.push_inst(Instruction::ConstUnit(unit_reg));
        unit_reg
    }

    /// Desugar `for x in iterable { body }` where iterable implements Iterable<T>.
    /// Calls `next()` in a loop, matching on Some(val)/None.
    pub(super) fn lower_for_iterable(
        &mut self,
        binding: &str,
        iter_expr: &nudl_core::span::Spanned<Expr>,
        body: &Block,
    ) -> Register {
        let iter_reg = self.lower_expr(iter_expr);
        let iter_ty = self.infer_expr_type(iter_expr);

        // Look up TypeName__next signature
        let type_name = iter_ty
            .and_then(|tid| crate::lower::type_resolve::type_kind_to_name(self.types.resolve(tid)))
            .expect("Iterable type must have a name");
        let next_name = format!("{}__{}", type_name, "next");
        let sig = self
            .function_sigs
            .get(&next_name)
            .cloned()
            .expect("Iterable type must have a next() method");

        // Determine element type from Option<T>'s Some variant
        let option_ty = sig.return_type;
        let elem_type = match self.types.resolve(option_ty).clone() {
            nudl_core::types::TypeKind::Enum { variants, .. } => variants
                .iter()
                .find(|v| v.name == "Some")
                .and_then(|v| v.fields.first())
                .map(|(_, ty)| *ty)
                .unwrap_or(self.types.i64()),
            _ => self.types.i64(),
        };

        let loop_block = self.new_block_id();
        let body_block = self.new_block_id();
        let exit_block = self.new_block_id();

        self.finish_block(Terminator::Jump(loop_block));

        // Loop block: call next(), check tag
        self.start_block(loop_block);
        let pre_loop_locals = self.locals.clone();

        // Retain self before passing to next() (caller-retain convention)
        let self_ty = iter_ty.unwrap_or(self.types.i64());
        self.emit_retain_for_type(iter_reg, self_ty);

        // Call TypeName__next(iter_reg)
        let sym = self.interner.intern(&next_name);
        let opt_reg = self.alloc_typed_register(option_ty);
        self.push_inst(Instruction::Call(
            opt_reg,
            FunctionRef::Named(sym),
            vec![iter_reg],
        ));

        // Load tag (field 0): Some=0, None=1
        let tag_reg = self.alloc_register();
        self.push_inst(Instruction::Load(tag_reg, opt_reg, 0));
        let none_tag = self.alloc_register();
        self.push_inst(Instruction::Const(none_tag, ConstValue::I32(1)));
        let is_none = self.alloc_register();
        self.push_inst(Instruction::Eq(is_none, tag_reg, none_tag));
        self.finish_block(Terminator::Branch(is_none, exit_block, body_block));

        // Body block: extract payload, run body
        self.start_block(body_block);
        self.loop_stack.push(LoopContext {
            label: None,
            continue_block: loop_block,
            break_block: exit_block,
            pre_loop_locals: pre_loop_locals.clone(),
        });

        // Extract payload from Some (field 1)
        let elem_reg = self.alloc_typed_register(elem_type);
        self.push_inst(Instruction::Load(elem_reg, opt_reg, 1));
        // Retain payload if reference type (Option still owns it, we need our own ref)
        if self.types.is_reference_type(elem_type) {
            self.push_inst(Instruction::Retain(elem_reg));
        }
        // Release the Option (we've extracted what we need)
        self.push_inst(Instruction::Release(opt_reg, Some(option_ty)));

        self.locals.insert(binding.to_string(), elem_reg);
        self.local_types.insert(binding.to_string(), elem_type);

        self.lower_block_expr(body);

        self.loop_stack.pop();

        // Release element if reference type
        if let Some(&cur_elem) = self.locals.get(binding) {
            self.emit_release_for_type(cur_elem, elem_type);
        }

        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(loop_block));

        // Exit block: release the None Option, done
        self.start_block(exit_block);
        self.push_inst(Instruction::Release(opt_reg, Some(option_ty)));
        self.locals = pre_loop_locals;
        let unit_reg = self.alloc_register();
        self.push_inst(Instruction::ConstUnit(unit_reg));
        unit_reg
    }
}
