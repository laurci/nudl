use super::*;

pub(super) fn emit_call<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    program: &Program,
    _caller_func: &Function,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_ptr_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_len_allocas: &HashMap<u32, PointerValue<'ctx>>,
    reg_string_info: &mut HashMap<u32, RegStringInfo>,
    string_constants: &[(GlobalValue<'ctx>, u64)],
    function_map: &HashMap<u32, FunctionValue<'ctx>>,
    types: &TypeInterner,
    result_reg: &Register,
    func_ref: &FunctionRef,
    args: &[Register],
) -> Result<(), BackendError> {
    // Resolve callee
    let (callee_func_id, callee_fn_val) = match func_ref {
        FunctionRef::Named(sym) => {
            let fid = program
                .functions
                .iter()
                .find(|f| f.name == *sym)
                .map(|f| f.id.0);
            (fid, fid.and_then(|id| function_map.get(&id).copied()))
        }
        FunctionRef::Extern(sym) => {
            let f = program
                .functions
                .iter()
                .find(|f| f.name == *sym && f.is_extern);
            let fid = f.map(|f| f.id.0);
            (fid, fid.and_then(|id| function_map.get(&id).copied()))
        }
        FunctionRef::Builtin(_) => (None, None),
    };

    let callee_fn = match callee_fn_val {
        Some(f) => f,
        None => return Ok(()),
    };

    let callee_func = callee_func_id.and_then(|id| program.functions.iter().find(|f| f.id.0 == id));
    let callee_layout = callee_func.map(|f| ParamLayout::compute(f, types));

    // Marshal arguments
    let mut llvm_args: Vec<BasicValueEnum<'ctx>> = Vec::new();

    if let Some(ref cl) = callee_layout {
        for (i, arg_reg) in args.iter().enumerate() {
            if i >= cl.entries.len() {
                break;
            }
            let (_, count) = cl.entries[i];

            if count == 2 {
                // String argument: expand to (ptr, len)
                match reg_string_info.get(&arg_reg.0) {
                    Some(RegStringInfo::StringLiteral(idx)) => {
                        let (global, len) = &string_constants[*idx as usize];
                        let ptr = gep_string_ptr(context, builder, global, *len)?;
                        let len_val = context.i64_type().const_int(*len, false);
                        llvm_args.push(ptr.into());
                        llvm_args.push(len_val.into());
                    }
                    Some(RegStringInfo::StringParam(ptr_alloca, len_alloca)) => {
                        let ptr_alloca = *ptr_alloca;
                        let len_alloca = *len_alloca;
                        let ptr = builder
                            .build_load(
                                context.ptr_type(AddressSpace::default()),
                                ptr_alloca,
                                "str_param_ptr",
                            )
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        let len = builder
                            .build_load(context.i64_type(), len_alloca, "str_param_len")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        llvm_args.push(ptr);
                        llvm_args.push(len);
                    }
                    _ => {
                        // Fallback: load from companion allocas (handles control-flow
                        // cases where reg_string_info was overwritten by another branch)
                        if let (Some(&ptr_al), Some(&len_al)) = (
                            str_ptr_allocas.get(&arg_reg.0),
                            str_len_allocas.get(&arg_reg.0),
                        ) {
                            let ptr = builder
                                .build_load(
                                    context.ptr_type(AddressSpace::default()),
                                    ptr_al,
                                    "str_alloca_ptr",
                                )
                                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                            let len = builder
                                .build_load(context.i64_type(), len_al, "str_alloca_len")
                                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                            llvm_args.push(ptr);
                            llvm_args.push(len);
                        } else {
                            llvm_args.push(
                                context
                                    .ptr_type(AddressSpace::default())
                                    .const_null()
                                    .into(),
                            );
                            llvm_args.push(context.i64_type().const_zero().into());
                        }
                    }
                }
            } else {
                // Check if the callee param is float
                let param_is_float = callee_func
                    .and_then(|cf| cf.params.get(i))
                    .map(|(_, pty)| matches!(types.resolve(*pty), TypeKind::Primitive(p) if p.is_float()))
                    .unwrap_or(false);
                if param_is_float {
                    let val = load_f64(context, builder, register_allocas, arg_reg.0)?;
                    llvm_args.push(val.into());
                } else {
                    let val = load_i64(context, builder, register_allocas, arg_reg.0)?;
                    llvm_args.push(val.into());
                }
            }
        }
    } else {
        for arg_reg in args {
            let val = load_i64(context, builder, register_allocas, arg_reg.0)?;
            llvm_args.push(val.into());
        }
    }

    let llvm_args_meta: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
        llvm_args.iter().map(|a| (*a).into()).collect();

    let call_result = builder
        .build_direct_call(callee_fn, &llvm_args_meta, "call")
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;

    // Store result
    if let Some(ret_val) = call_result.try_as_basic_value().basic() {
        match ret_val {
            BasicValueEnum::IntValue(iv) => {
                if iv.get_type().get_bit_width() == 32 {
                    let extended = builder
                        .build_int_s_extend(iv, context.i64_type(), "sext")
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                    store(builder, register_allocas, result_reg.0, extended)?;
                } else {
                    store(builder, register_allocas, result_reg.0, iv)?;
                }
            }
            BasicValueEnum::FloatValue(fv) => {
                store(builder, register_allocas, result_reg.0, fv)?;
            }
            _ => {
                builder
                    .build_store(register_allocas[&result_reg.0], ret_val)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            }
        }
    }

    Ok(())
}
