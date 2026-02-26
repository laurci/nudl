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
                if self.types.is_struct(*type_id) {
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
            Stmt::Item(_) => {} // nested items not supported yet
        }
    }
}
