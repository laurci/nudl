use nudl_core::intern::Symbol;

use super::*;

/// Load a string argument's (ptr, len) pair from a register.
fn load_string_arg<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    reg_string_info: &HashMap<u32, RegStringInfo>,
    string_constants: &[(GlobalValue<'ctx>, u64)],
    str_ptr_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_len_allocas: &HashMap<u32, PointerValue<'ctx>>,
    reg: u32,
) -> Result<(BasicValueEnum<'ctx>, BasicValueEnum<'ctx>), BackendError> {
    match reg_string_info.get(&reg) {
        Some(RegStringInfo::StringLiteral(idx)) => {
            let (global, len) = &string_constants[*idx as usize];
            let ptr = gep_string_ptr(context, builder, global, *len)?;
            let len_val = context.i64_type().const_int(*len, false);
            Ok((ptr.into(), len_val.into()))
        }
        Some(RegStringInfo::StringParam(ptr_alloca, len_alloca)) => {
            let ptr = builder
                .build_load(
                    context.ptr_type(AddressSpace::default()),
                    *ptr_alloca,
                    "str_param_ptr",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let len = builder
                .build_load(context.i64_type(), *len_alloca, "str_param_len")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            Ok((ptr, len))
        }
        _ => {
            // Fallback: load from companion allocas
            if let (Some(&ptr_al), Some(&len_al)) =
                (str_ptr_allocas.get(&reg), str_len_allocas.get(&reg))
            {
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
                Ok((ptr, len))
            } else {
                Ok((
                    context
                        .ptr_type(AddressSpace::default())
                        .const_null()
                        .into(),
                    context.i64_type().const_zero().into(),
                ))
            }
        }
    }
}

/// Extract (data_ptr, length) from a heap string object returned by a runtime function.
///
/// Heap string layout: [16-byte ARC header][i64 length @ offset 16][char data @ offset 24]
fn extract_heap_string<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    heap_ptr: PointerValue<'ctx>,
) -> Result<(PointerValue<'ctx>, inkwell::values::IntValue<'ctx>), BackendError> {
    let i8_ty = context.i8_type();
    let i64_ty = context.i64_type();

    // GEP to offset 16 to load the length field
    let len_field_ptr = unsafe {
        builder
            .build_gep(
                i8_ty,
                heap_ptr,
                &[i64_ty.const_int(16, false)],
                "heap_str_len_ptr",
            )
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
    };
    let len_val = builder
        .build_load(i64_ty, len_field_ptr, "heap_str_len")
        .map_err(|e| BackendError::LlvmError(e.to_string()))?
        .into_int_value();

    // GEP to offset 24 to get the data pointer
    let data_ptr = unsafe {
        builder
            .build_gep(
                i8_ty,
                heap_ptr,
                &[i64_ty.const_int(24, false)],
                "heap_str_data",
            )
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
    };

    Ok((data_ptr, len_val))
}

/// Store a (ptr, len) string result into the destination register's companion allocas
/// and update reg_string_info.
fn store_string_result<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    str_ptr_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_len_allocas: &HashMap<u32, PointerValue<'ctx>>,
    reg_string_info: &mut HashMap<u32, RegStringInfo>,
    dst_reg: u32,
    data_ptr: PointerValue<'ctx>,
    len_val: inkwell::values::IntValue<'ctx>,
) -> Result<(), BackendError> {
    let ptr_alloca = str_ptr_allocas[&dst_reg];
    let len_alloca = str_len_allocas[&dst_reg];

    builder
        .build_store(ptr_alloca, data_ptr)
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    builder
        .build_store(len_alloca, len_val)
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;

    reg_string_info.insert(
        dst_reg,
        RegStringInfo::StringParam(unsafe { extend_ptr_lifetime(ptr_alloca) }, unsafe {
            extend_ptr_lifetime(len_alloca)
        }),
    );

    // Also store the raw heap pointer into the register alloca (for ARC, if needed later)
    // We store the data_ptr as an i64 so the register has a valid value.
    let ptr_as_i64 = builder
        .build_ptr_to_int(data_ptr, context.i64_type(), "str_ptr_i64")
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    // (not strictly needed for string semantics but keeps register consistent)
    let _ = ptr_as_i64;

    Ok(())
}

/// Emit a builtin call (string builtins, panic, assert, etc.).
fn emit_builtin_call<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    program: &Program,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_ptr_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_len_allocas: &HashMap<u32, PointerValue<'ctx>>,
    reg_string_info: &mut HashMap<u32, RegStringInfo>,
    string_constants: &[(GlobalValue<'ctx>, u64)],
    string_builtins: &StringBuiltins<'ctx>,
    result_reg: &Register,
    builtin_sym: &Symbol,
    args: &[Register],
) -> Result<(), BackendError> {
    let name = program.interner.resolve(*builtin_sym);

    match name {
        "__str_concat" => {
            // args: [string_a, string_b]
            // Each string arg needs (ptr, len) expansion
            let (a_ptr, a_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let (b_ptr, b_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[1].0,
            )?;

            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![a_ptr.into(), a_len.into(), b_ptr.into(), b_len.into()];

            let call_result = builder
                .build_direct_call(string_builtins.str_concat, &call_args, "concat_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            let heap_ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("string builtin should return a pointer")
                .into_pointer_value();

            let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
            store_string_result(
                context,
                builder,
                str_ptr_allocas,
                str_len_allocas,
                reg_string_info,
                result_reg.0,
                data_ptr,
                len_val,
            )?;
        }

        "__i32_to_str" | "__i64_to_str" | "__bool_to_str" | "__char_to_str" => {
            let arg_val = load_i64(context, builder, register_allocas, args[0].0)?;
            let rt_fn = match name {
                "__i32_to_str" | "__i64_to_str" => string_builtins.i64_to_str,
                "__bool_to_str" => string_builtins.bool_to_str,
                "__char_to_str" => string_builtins.char_to_str,
                _ => unreachable!(),
            };

            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![arg_val.into()];

            let call_result = builder
                .build_direct_call(rt_fn, &call_args, "to_str_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            let heap_ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("string builtin should return a pointer")
                .into_pointer_value();

            let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
            store_string_result(
                context,
                builder,
                str_ptr_allocas,
                str_len_allocas,
                reg_string_info,
                result_reg.0,
                data_ptr,
                len_val,
            )?;
        }

        "__f64_to_str" => {
            let arg_val = load_f64(context, builder, register_allocas, args[0].0)?;

            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![arg_val.into()];

            let call_result = builder
                .build_direct_call(string_builtins.f64_to_str, &call_args, "f64_to_str_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            let heap_ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("string builtin should return a pointer")
                .into_pointer_value();

            let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
            store_string_result(
                context,
                builder,
                str_ptr_allocas,
                str_len_allocas,
                reg_string_info,
                result_reg.0,
                data_ptr,
                len_val,
            )?;
        }

        // Unknown builtins — silently skip (panic/assert not yet wired)
        _ => {}
    }

    Ok(())
}

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
    string_builtins: &StringBuiltins<'ctx>,
    result_reg: &Register,
    func_ref: &FunctionRef,
    args: &[Register],
) -> Result<(), BackendError> {
    // Handle builtins separately
    if let FunctionRef::Builtin(sym) = func_ref {
        return emit_builtin_call(
            context,
            builder,
            program,
            register_allocas,
            str_ptr_allocas,
            str_len_allocas,
            reg_string_info,
            string_constants,
            string_builtins,
            result_reg,
            sym,
            args,
        );
    }

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
        FunctionRef::Builtin(_) => unreachable!(),
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
                let (ptr, len) = load_string_arg(
                    context,
                    builder,
                    reg_string_info,
                    string_constants,
                    str_ptr_allocas,
                    str_len_allocas,
                    arg_reg.0,
                )?;
                llvm_args.push(ptr);
                llvm_args.push(len);
            } else {
                // Check if the callee param is float
                let param_is_float = callee_func
                    .and_then(|cf| cf.params.get(i))
                    .map(|(_, pty)| {
                        matches!(types.resolve(*pty), TypeKind::Primitive(p) if p.is_float())
                    })
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
