use super::*;

pub(super) fn emit_terminator<'ctx>(
    term: &Terminator,
    func: &Function,
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    block_map: &HashMap<u32, inkwell::basic_block::BasicBlock<'ctx>>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    types: &TypeInterner,
    is_entry: bool,
) -> Result<(), BackendError> {
    match term {
        Terminator::Return(ret_reg) => {
            if is_entry {
                let zero = context.i32_type().const_zero();
                builder
                    .build_return(Some(&zero))
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            } else if is_float_register(func, ret_reg.0, types) {
                let val = load_f64(context, builder, register_allocas, ret_reg.0)?;
                builder
                    .build_return(Some(&val))
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            } else {
                let val = load_i64(context, builder, register_allocas, ret_reg.0)?;
                builder
                    .build_return(Some(&val))
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            }
        }
        Terminator::Jump(target) => {
            builder
                .build_unconditional_branch(block_map[&target.0])
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
        Terminator::Branch(cond, then_block, else_block) => {
            let cond_val = load_i64(context, builder, register_allocas, cond.0)?;
            let zero = context.i64_type().const_zero();
            let cmp = builder
                .build_int_compare(inkwell::IntPredicate::NE, cond_val, zero, "branch_cond")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            builder
                .build_conditional_branch(cmp, block_map[&then_block.0], block_map[&else_block.0])
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
        Terminator::Unreachable => {
            builder
                .build_unreachable()
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
    }
    Ok(())
}

// --- Helpers ---
