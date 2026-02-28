use super::*;

pub(super) fn emit_terminator<'ctx>(
    term: &Terminator,
    func: &Function,
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    block_map: &HashMap<u32, inkwell::basic_block::BasicBlock<'ctx>>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_ptr_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_len_allocas: &HashMap<u32, PointerValue<'ctx>>,
    reg_string_info: &HashMap<u32, RegStringInfo>,
    string_constants: &[(GlobalValue<'ctx>, u64)],
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
            } else if matches!(types.resolve(func.return_type), TypeKind::String) {
                // String-returning functions return a {ptr, i64} struct
                let ptr_ty = context.ptr_type(AddressSpace::default());
                let i64_ty = context.i64_type();
                let struct_ty = context.struct_type(&[ptr_ty.into(), i64_ty.into()], false);

                // Load the string ptr and len from the return register
                let (ptr_val, len_val) = load_string_for_return(
                    context,
                    builder,
                    reg_string_info,
                    string_constants,
                    str_ptr_allocas,
                    str_len_allocas,
                    ret_reg.0,
                )?;

                // Pack into struct
                let mut struct_val = struct_ty.get_undef();
                struct_val = builder
                    .build_insert_value(struct_val, ptr_val, 0, "ret_str_ptr")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    .into_struct_value();
                struct_val = builder
                    .build_insert_value(struct_val, len_val, 1, "ret_str_len")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    .into_struct_value();

                builder
                    .build_return(Some(&struct_val))
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

/// Load the string (ptr, len) for a return register, consulting reg_string_info.
fn load_string_for_return<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    reg_string_info: &HashMap<u32, RegStringInfo>,
    string_constants: &[(GlobalValue<'ctx>, u64)],
    str_ptr_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_len_allocas: &HashMap<u32, PointerValue<'ctx>>,
    reg: u32,
) -> Result<(PointerValue<'ctx>, inkwell::values::IntValue<'ctx>), BackendError> {
    match reg_string_info.get(&reg) {
        Some(RegStringInfo::StringLiteral(idx)) => {
            let (global, len) = &string_constants[*idx as usize];
            let ptr = gep_string_ptr(context, builder, global, *len)?;
            let len_val = context.i64_type().const_int(*len, false);
            Ok((ptr, len_val))
        }
        Some(RegStringInfo::StringParam(ptr_alloca, len_alloca)) => {
            let ptr = builder
                .build_load(
                    context.ptr_type(AddressSpace::default()),
                    *ptr_alloca,
                    "ret_str_ptr",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
                .into_pointer_value();
            let len = builder
                .build_load(context.i64_type(), *len_alloca, "ret_str_len")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
                .into_int_value();
            Ok((ptr, len))
        }
        None => {
            // Fallback: load from companion allocas
            if let (Some(&ptr_al), Some(&len_al)) =
                (str_ptr_allocas.get(&reg), str_len_allocas.get(&reg))
            {
                let ptr = builder
                    .build_load(
                        context.ptr_type(AddressSpace::default()),
                        ptr_al,
                        "ret_str_ptr",
                    )
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    .into_pointer_value();
                let len = builder
                    .build_load(context.i64_type(), len_al, "ret_str_len")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    .into_int_value();
                Ok((ptr, len))
            } else {
                Ok((
                    context.ptr_type(AddressSpace::default()).const_null(),
                    context.i64_type().const_zero(),
                ))
            }
        }
    }
}

// --- Helpers ---
