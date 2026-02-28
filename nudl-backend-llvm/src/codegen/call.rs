use nudl_core::intern::Symbol;

use super::*;

/// Load a string argument's (ptr, len) pair from a register.
pub(super) fn load_string_arg<'ctx>(
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
pub(super) fn extract_heap_string<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    heap_ptr: PointerValue<'ctx>,
) -> Result<(PointerValue<'ctx>, inkwell::values::IntValue<'ctx>), BackendError> {
    let i8_ty = context.i8_type();
    let i64_ty = context.i64_type();

    // GEP to STRING_LEN_OFFSET to load the length field
    let len_field_ptr = unsafe {
        builder
            .build_gep(
                i8_ty,
                heap_ptr,
                &[i64_ty.const_int(STRING_LEN_OFFSET, false)],
                "heap_str_len_ptr",
            )
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
    };
    let len_val = builder
        .build_load(i64_ty, len_field_ptr, "heap_str_len")
        .map_err(|e| BackendError::LlvmError(e.to_string()))?
        .into_int_value();

    // GEP to STRING_DATA_OFFSET to get the data pointer
    let data_ptr = unsafe {
        builder
            .build_gep(
                i8_ty,
                heap_ptr,
                &[i64_ty.const_int(STRING_DATA_OFFSET, false)],
                "heap_str_data",
            )
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
    };

    Ok((data_ptr, len_val))
}

/// Store a (ptr, len) string result into the destination register's companion allocas
/// and update reg_string_info.
pub(super) fn store_string_result<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    str_ptr_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_len_allocas: &HashMap<u32, PointerValue<'ctx>>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
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

    // Compute the ARC heap object pointer (data_ptr - STRING_DATA_OFFSET) and store in the
    // register alloca. This is needed by DynArrayPush, ARC retain/release,
    // and any code that reads the register as an i64.
    let i8_ty = context.i8_type();
    let neg_offset = context
        .i64_type()
        .const_int(-(STRING_DATA_OFFSET as i64) as u64, true);
    let heap_ptr = unsafe {
        builder
            .build_gep(i8_ty, data_ptr, &[neg_offset], "str_arc_ptr")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
    };
    let heap_as_i64 = builder
        .build_ptr_to_int(heap_ptr, context.i64_type(), "str_heap_i64")
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    let reg_alloca = register_allocas[&dst_reg];
    builder
        .build_store(reg_alloca, heap_as_i64)
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;

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
                register_allocas,
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
                register_allocas,
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
                register_allocas,
                reg_string_info,
                result_reg.0,
                data_ptr,
                len_val,
            )?;
        }

        // String operations returning string (heap ptr -> extract ptr/len)
        "__str_substr" => {
            // args: [string, i64_start, i64_end]
            let (s_ptr, s_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let start_val = load_i64(context, builder, register_allocas, args[1].0)?;
            let end_val = load_i64(context, builder, register_allocas, args[2].0)?;

            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![s_ptr.into(), s_len.into(), start_val.into(), end_val.into()];
            let call_result = builder
                .build_direct_call(string_builtins.str_substr, &call_args, "substr_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let heap_ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("substr should return a pointer")
                .into_pointer_value();
            let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
            store_string_result(
                context,
                builder,
                str_ptr_allocas,
                str_len_allocas,
                register_allocas,
                reg_string_info,
                result_reg.0,
                data_ptr,
                len_val,
            )?;
        }

        "__str_trim" => {
            let (s_ptr, s_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![s_ptr.into(), s_len.into()];
            let call_result = builder
                .build_direct_call(string_builtins.str_trim, &call_args, "trim_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let heap_ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("trim should return a pointer")
                .into_pointer_value();
            let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
            store_string_result(
                context,
                builder,
                str_ptr_allocas,
                str_len_allocas,
                register_allocas,
                reg_string_info,
                result_reg.0,
                data_ptr,
                len_val,
            )?;
        }

        "__str_to_upper" => {
            let (s_ptr, s_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![s_ptr.into(), s_len.into()];
            let call_result = builder
                .build_direct_call(string_builtins.str_to_upper, &call_args, "to_upper_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let heap_ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("to_upper should return a pointer")
                .into_pointer_value();
            let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
            store_string_result(
                context,
                builder,
                str_ptr_allocas,
                str_len_allocas,
                register_allocas,
                reg_string_info,
                result_reg.0,
                data_ptr,
                len_val,
            )?;
        }

        "__str_to_lower" => {
            let (s_ptr, s_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![s_ptr.into(), s_len.into()];
            let call_result = builder
                .build_direct_call(string_builtins.str_to_lower, &call_args, "to_lower_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let heap_ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("to_lower should return a pointer")
                .into_pointer_value();
            let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
            store_string_result(
                context,
                builder,
                str_ptr_allocas,
                str_len_allocas,
                register_allocas,
                reg_string_info,
                result_reg.0,
                data_ptr,
                len_val,
            )?;
        }

        "__str_replace" => {
            // args: [string, old_string, new_string]
            let (s_ptr, s_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let (old_ptr, old_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[1].0,
            )?;
            let (new_ptr, new_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[2].0,
            )?;
            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> = vec![
                s_ptr.into(),
                s_len.into(),
                old_ptr.into(),
                old_len.into(),
                new_ptr.into(),
                new_len.into(),
            ];
            let call_result = builder
                .build_direct_call(string_builtins.str_replace, &call_args, "replace_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let heap_ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("replace should return a pointer")
                .into_pointer_value();
            let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
            store_string_result(
                context,
                builder,
                str_ptr_allocas,
                str_len_allocas,
                register_allocas,
                reg_string_info,
                result_reg.0,
                data_ptr,
                len_val,
            )?;
        }

        "__str_repeat" => {
            // args: [string, i64_count]
            let (s_ptr, s_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let count_val = load_i64(context, builder, register_allocas, args[1].0)?;
            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![s_ptr.into(), s_len.into(), count_val.into()];
            let call_result = builder
                .build_direct_call(string_builtins.str_repeat, &call_args, "repeat_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let heap_ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("repeat should return a pointer")
                .into_pointer_value();
            let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
            store_string_result(
                context,
                builder,
                str_ptr_allocas,
                str_len_allocas,
                register_allocas,
                reg_string_info,
                result_reg.0,
                data_ptr,
                len_val,
            )?;
        }

        // String operations returning i64
        "__str_indexof" => {
            let (h_ptr, h_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let (n_ptr, n_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[1].0,
            )?;
            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![h_ptr.into(), h_len.into(), n_ptr.into(), n_len.into()];
            let call_result = builder
                .build_direct_call(string_builtins.str_indexof, &call_args, "indexof_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let result_val = call_result
                .try_as_basic_value()
                .basic()
                .expect("indexof should return i64")
                .into_int_value();
            let alloca = register_allocas.get(&result_reg.0).ok_or_else(|| {
                BackendError::LlvmError(format!("no alloca for r{}", result_reg.0))
            })?;
            builder
                .build_store(*alloca, result_val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }

        "__str_contains" => {
            let (h_ptr, h_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let (n_ptr, n_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[1].0,
            )?;
            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![h_ptr.into(), h_len.into(), n_ptr.into(), n_len.into()];
            let call_result = builder
                .build_direct_call(string_builtins.str_contains, &call_args, "contains_result")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let result_val = call_result
                .try_as_basic_value()
                .basic()
                .expect("contains should return i64")
                .into_int_value();
            let alloca = register_allocas.get(&result_reg.0).ok_or_else(|| {
                BackendError::LlvmError(format!("no alloca for r{}", result_reg.0))
            })?;
            builder
                .build_store(*alloca, result_val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }

        "__str_starts_with" => {
            let (s_ptr, s_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let (p_ptr, p_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[1].0,
            )?;
            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![s_ptr.into(), s_len.into(), p_ptr.into(), p_len.into()];
            let call_result = builder
                .build_direct_call(
                    string_builtins.str_starts_with,
                    &call_args,
                    "starts_with_result",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let result_val = call_result
                .try_as_basic_value()
                .basic()
                .expect("starts_with should return i64")
                .into_int_value();
            let alloca = register_allocas.get(&result_reg.0).ok_or_else(|| {
                BackendError::LlvmError(format!("no alloca for r{}", result_reg.0))
            })?;
            builder
                .build_store(*alloca, result_val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }

        "__str_ends_with" => {
            let (s_ptr, s_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[0].0,
            )?;
            let (sf_ptr, sf_len) = load_string_arg(
                context,
                builder,
                reg_string_info,
                string_constants,
                str_ptr_allocas,
                str_len_allocas,
                args[1].0,
            )?;
            let call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![s_ptr.into(), s_len.into(), sf_ptr.into(), sf_len.into()];
            let call_result = builder
                .build_direct_call(
                    string_builtins.str_ends_with,
                    &call_args,
                    "ends_with_result",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let result_val = call_result
                .try_as_basic_value()
                .basic()
                .expect("ends_with should return i64")
                .into_int_value();
            let alloca = register_allocas.get(&result_reg.0).ok_or_else(|| {
                BackendError::LlvmError(format!("no alloca for r{}", result_reg.0))
            })?;
            builder
                .build_store(*alloca, result_val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
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

    // Check if callee returns a string (returns {ptr, i64} struct)
    let callee_returns_string = callee_func
        .map(|f| matches!(types.resolve(f.return_type), TypeKind::String))
        .unwrap_or(false);

    // Store result
    let is_extern = callee_func.map(|f| f.is_extern).unwrap_or(false);
    if callee_returns_string {
        if let Some(ret_val) = call_result.try_as_basic_value().basic() {
            if is_extern {
                // Extern returns raw ptr → extract_heap_string
                let heap_ptr = ret_val.into_pointer_value();
                let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
                store_string_result(
                    context,
                    builder,
                    str_ptr_allocas,
                    str_len_allocas,
                    register_allocas,
                    reg_string_info,
                    result_reg.0,
                    data_ptr,
                    len_val,
                )?;
            } else {
                // Named function returns {ptr, i64} struct → unpack
                let struct_val = ret_val.into_struct_value();
                let data_ptr = builder
                    .build_extract_value(struct_val, 0, "call_str_ptr")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    .into_pointer_value();
                let len_val = builder
                    .build_extract_value(struct_val, 1, "call_str_len")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    .into_int_value();
                store_string_result(
                    context,
                    builder,
                    str_ptr_allocas,
                    str_len_allocas,
                    register_allocas,
                    reg_string_info,
                    result_reg.0,
                    data_ptr,
                    len_val,
                )?;
            }
        }
    } else if let Some(ret_val) = call_result.try_as_basic_value().basic() {
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
