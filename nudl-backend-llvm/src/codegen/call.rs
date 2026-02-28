use nudl_core::intern::Symbol;
use nudl_core::types::TypeId;

use super::*;

/// Pack a FixedArray from an ARC heap object into a C-compatible LLVM array value.
/// The ARC object stores each element as i64 at offset ARC_HEADER_SIZE + i * FIELD_SIZE.
/// This extracts each element, truncates to the C element type, and builds an LLVM array.
fn pack_fixed_array_for_ffi<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    types: &TypeInterner,
    reg: u32,
    element: TypeId,
    length: usize,
) -> Result<BasicValueEnum<'ctx>, BackendError> {
    let i8_ty = context.i8_type();
    let i64_ty = context.i64_type();

    // Get the C element type
    let elem_kind = types.resolve(element);
    let c_elem_ty = match elem_kind {
        TypeKind::Primitive(p) => primitive_to_c_type(p, context),
        _ => i64_ty.into(),
    };

    // Load the ARC object pointer from the register
    let obj_ptr = load_ptr(context, builder, register_allocas, reg)?;

    // Build the array value by extracting each element
    let array_ty = match c_elem_ty {
        inkwell::types::BasicTypeEnum::IntType(t) => t.array_type(length as u32),
        inkwell::types::BasicTypeEnum::FloatType(t) => t.array_type(length as u32),
        _ => i64_ty.array_type(length as u32),
    };
    let mut array_val = array_ty.const_zero();

    for i in 0..length {
        let byte_offset = ARC_HEADER_SIZE + (i as u64) * FIELD_SIZE;
        let elem_ptr = unsafe {
            builder
                .build_gep(
                    i8_ty,
                    obj_ptr,
                    &[i64_ty.const_int(byte_offset, false)],
                    &format!("ffi_arr_elem_ptr_{}", i),
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
        };

        // Load as i64 (that's how elements are stored internally)
        let elem_i64 = builder
            .build_load(i64_ty, elem_ptr, &format!("ffi_arr_elem_{}", i))
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
            .into_int_value();

        // Truncate/convert to the C element type and insert into array
        let c_val: BasicValueEnum = match c_elem_ty {
            inkwell::types::BasicTypeEnum::IntType(t) => {
                if t.get_bit_width() < 64 {
                    builder
                        .build_int_truncate(elem_i64, t, &format!("ffi_trunc_{}", i))
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                        .into()
                } else {
                    elem_i64.into()
                }
            }
            inkwell::types::BasicTypeEnum::FloatType(t) => builder
                .build_bit_cast(elem_i64, t, &format!("ffi_f_{}", i))
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
                .into(),
            _ => elem_i64.into(),
        };

        array_val = builder
            .build_insert_value(array_val, c_val, i as u32, &format!("ffi_arr_insert_{}", i))
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
            .into_array_value();
    }

    Ok(array_val.into())
}

/// Pack an extern struct from a TupleAlloc (stack-allocated i64 slots) into a C-compatible LLVM struct value.
/// Each field is loaded from the tuple alloca (ARC_HEADER_SIZE + field_index * FIELD_SIZE),
/// truncated to the C field type, and inserted into an LLVM struct.
fn pack_extern_struct_for_ffi<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    types: &TypeInterner,
    reg: u32,
    fields: &[(String, nudl_core::types::TypeId)],
) -> Result<BasicValueEnum<'ctx>, BackendError> {
    let i64_ty = context.i64_type();
    let c_struct_ty = build_c_struct_type(context, types, fields);

    // Load the ARC object pointer from the register (TupleAlloc stores a pointer as i64)
    let obj_ptr = load_ptr(context, builder, register_allocas, reg)?;

    let mut struct_val = c_struct_ty.get_undef();
    for (i, (_, fty)) in fields.iter().enumerate() {
        let byte_offset = ARC_HEADER_SIZE + (i as u64) * FIELD_SIZE;
        let field_ptr = unsafe {
            builder
                .build_gep(
                    context.i8_type(),
                    obj_ptr,
                    &[i64_ty.const_int(byte_offset, false)],
                    &format!("es_field_ptr_{}", i),
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
        };

        let c_field_ty = match types.resolve(*fty) {
            TypeKind::Primitive(p) => primitive_to_c_type(p, context),
            TypeKind::Struct {
                fields: inner_fields,
                is_extern: true,
                ..
            } => build_c_struct_type(context, types, inner_fields).into(),
            _ => i64_ty.into(),
        };

        let c_val: BasicValueEnum = match c_field_ty {
            inkwell::types::BasicTypeEnum::IntType(t) => {
                let raw = builder
                    .build_load(i64_ty, field_ptr, &format!("es_field_{}", i))
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    .into_int_value();
                if t.get_bit_width() < 64 {
                    builder
                        .build_int_truncate(raw, t, &format!("es_trunc_{}", i))
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                        .into()
                } else {
                    raw.into()
                }
            }
            inkwell::types::BasicTypeEnum::FloatType(t) => {
                let raw = builder
                    .build_load(i64_ty, field_ptr, &format!("es_field_{}", i))
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    .into_int_value();
                builder
                    .build_bit_cast(raw, t, &format!("es_float_{}", i))
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    .into()
            }
            _ => builder
                .build_load(i64_ty, field_ptr, &format!("es_field_{}", i))
                .map_err(|e| BackendError::LlvmError(e.to_string()))?,
        };

        struct_val = builder
            .build_insert_value(struct_val, c_val, i as u32, &format!("es_insert_{}", i))
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
            .into_struct_value();
    }

    // Coerce the struct to the ABI type (integer) for correct parameter passing.
    // LLVM scalarizes struct parameters into separate registers, but the C ABI
    // expects small structs packed into a single register.
    let abi_ty = extern_struct_abi_type(context, types, fields);
    let coerced = match abi_ty {
        inkwell::types::BasicTypeEnum::IntType(int_ty) => {
            // alloca struct, store, load as int
            let tmp = builder
                .build_alloca(c_struct_ty, "es_coerce_tmp")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            builder
                .build_store(tmp, struct_val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            builder
                .build_load(int_ty, tmp, "es_coerced")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
        }
        _ => {
            // Large structs (>8 bytes): pass as-is (LLVM struct type)
            struct_val.into()
        }
    };

    Ok(coerced)
}

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

/// Emit a builtin call (string builtins, panic, assert, cptr, etc.).
fn emit_builtin_call<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    program: &Program,
    caller_func: &Function,
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

        "cptr" => {
            // cptr(value) -> RawPtr: get a pointer to a C-layout copy of the value
            let types = &program.types;
            let arg_ty = caller_func.register_types.get(args[0].0 as usize).copied();

            if let Some(ty) = arg_ty {
                match types.resolve(ty) {
                    TypeKind::Struct {
                        fields,
                        is_extern: true,
                        ..
                    } => {
                        // Extern struct: alloca C struct, pack fields, return pointer
                        let c_struct_ty = build_c_struct_type(context, types, fields);
                        let struct_val = pack_extern_struct_for_ffi(
                            context,
                            builder,
                            register_allocas,
                            types,
                            args[0].0,
                            fields,
                        )?;
                        let alloca = builder
                            .build_alloca(c_struct_ty, "cptr_struct")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        builder
                            .build_store(alloca, struct_val)
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        let ptr_as_i64 = builder
                            .build_ptr_to_int(alloca, context.i64_type(), "cptr_i64")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        store(builder, register_allocas, result_reg.0, ptr_as_i64)?;
                    }
                    TypeKind::FixedArray { element, length } => {
                        // Fixed array: alloca C array, pack elements, return pointer
                        let arr_val = pack_fixed_array_for_ffi(
                            context,
                            builder,
                            register_allocas,
                            types,
                            args[0].0,
                            *element,
                            *length,
                        )?;
                        let c_arr_ty = match type_to_c_llvm_type(context, types, ty) {
                            Some(t) => t,
                            None => context.i64_type().array_type(*length as u32).into(),
                        };
                        let alloca = builder
                            .build_alloca(c_arr_ty, "cptr_array")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        builder
                            .build_store(alloca, arr_val)
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        let ptr_as_i64 = builder
                            .build_ptr_to_int(alloca, context.i64_type(), "cptr_i64")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        store(builder, register_allocas, result_reg.0, ptr_as_i64)?;
                    }
                    TypeKind::DynamicArray { .. } => {
                        // Dynamic array: return data pointer from ARC array object
                        // Layout: [ARC header 16 bytes][ptr 8 bytes][len 8 bytes][cap 8 bytes]
                        let obj_ptr = load_ptr(context, builder, register_allocas, args[0].0)?;
                        let data_field_ptr = unsafe {
                            builder
                                .build_gep(
                                    context.i8_type(),
                                    obj_ptr,
                                    &[context.i64_type().const_int(ARC_HEADER_SIZE, false)],
                                    "darr_data_field",
                                )
                                .map_err(|e| BackendError::LlvmError(e.to_string()))?
                        };
                        let data_ptr = builder
                            .build_load(context.i64_type(), data_field_ptr, "darr_data_ptr")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?
                            .into_int_value();
                        store(builder, register_allocas, result_reg.0, data_ptr)?;
                    }
                    _ => {
                        // For primitives: alloca, store, return pointer
                        let val = load_i64(context, builder, register_allocas, args[0].0)?;
                        let alloca = builder
                            .build_alloca(context.i64_type(), "cptr_val")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        builder
                            .build_store(alloca, val)
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        let ptr_as_i64 = builder
                            .build_ptr_to_int(alloca, context.i64_type(), "cptr_i64")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        store(builder, register_allocas, result_reg.0, ptr_as_i64)?;
                    }
                }
            }
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
    arc: &ArcIntrinsics<'ctx>,
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
            _caller_func,
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
                // Check if the callee param is a FixedArray in an extern call
                let param_type = callee_func
                    .and_then(|cf| cf.params.get(i))
                    .map(|(_, pty)| *pty);
                let is_extern = callee_func.map(|f| f.is_extern).unwrap_or(false);

                if let Some(pty) = param_type {
                    if is_extern {
                        match types.resolve(pty) {
                            TypeKind::FixedArray { element, length } => {
                                // Pack elements from ARC object into C-compatible array
                                let arr_val = pack_fixed_array_for_ffi(
                                    context,
                                    builder,
                                    register_allocas,
                                    types,
                                    arg_reg.0,
                                    *element,
                                    *length,
                                )?;
                                llvm_args.push(arr_val);
                                continue;
                            }
                            TypeKind::Struct {
                                fields,
                                is_extern: true,
                                ..
                            } => {
                                // Pack extern struct fields into C-compatible struct
                                let struct_val = pack_extern_struct_for_ffi(
                                    context,
                                    builder,
                                    register_allocas,
                                    types,
                                    arg_reg.0,
                                    fields,
                                )?;
                                llvm_args.push(struct_val);
                                continue;
                            }
                            TypeKind::String => {
                                // Extern string param: pass just the ptr (not ptr+len)
                                let (ptr, _len) = load_string_arg(
                                    context,
                                    builder,
                                    reg_string_info,
                                    string_constants,
                                    str_ptr_allocas,
                                    str_len_allocas,
                                    arg_reg.0,
                                )?;
                                llvm_args.push(ptr);
                                continue;
                            }
                            _ => {}
                        }
                    }
                }

                // Check if the callee param is float
                let param_is_float = param_type
                    .map(|pty| matches!(types.resolve(pty), TypeKind::Primitive(p) if p.is_float()))
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

    // Check if callee returns an extern struct or fixed array (C-compatible value types)
    let callee_returns_extern_struct = callee_func
        .map(|f| f.is_extern && types.is_extern_struct(f.return_type))
        .unwrap_or(false);
    let callee_returns_fixed_array = callee_func
        .map(|f| f.is_extern && matches!(types.resolve(f.return_type), TypeKind::FixedArray { .. }))
        .unwrap_or(false);

    // Store result
    let is_extern = callee_func.map(|f| f.is_extern).unwrap_or(false);
    if callee_returns_extern_struct {
        // Extern function returning an extern struct: the return value is an ABI-coerced
        // integer (e.g., i32 for a 4-byte struct). We need to:
        // 1. Allocate a TupleAlloc buffer (ARC header + i64 fields) for the result register
        // 2. Store the coerced return into a temp, load as struct, extract fields
        // 3. Widen each field to i64 and store into the TupleAlloc buffer
        if let Some(ret_val) = call_result.try_as_basic_value().basic() {
            let ret_ty = callee_func.unwrap().return_type;
            if let TypeKind::Struct { fields, .. } = types.resolve(ret_ty) {
                let c_struct_ty = build_c_struct_type(context, types, fields);

                // Allocate a TupleAlloc buffer for the result
                let num_fields = fields.len() as u64;
                let total_size = context
                    .i64_type()
                    .const_int(ARC_HEADER_SIZE + num_fields * FIELD_SIZE, false);
                let type_tag = context.i32_type().const_int(ret_ty.0 as u64, false);
                let alloc_result = builder
                    .build_direct_call(
                        arc.arc_alloc,
                        &[total_size.into(), type_tag.into()],
                        "es_ret_alloc",
                    )
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                let obj_ptr = alloc_result
                    .try_as_basic_value()
                    .basic()
                    .expect("arc_alloc should return a pointer")
                    .into_pointer_value();

                // Store the pointer in the result register
                let ptr_as_i64 = builder
                    .build_ptr_to_int(obj_ptr, context.i64_type(), "es_ret_ptr_i64")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, result_reg.0, ptr_as_i64)?;

                // Store ABI-coerced return value, then load as struct to extract fields
                let tmp = builder
                    .build_alloca(c_struct_ty, "es_ret_tmp")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                builder
                    .build_store(tmp, ret_val)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;

                let struct_val = builder
                    .build_load(c_struct_ty, tmp, "es_ret_struct")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    .into_struct_value();

                // Extract each field, widen to i64, store into TupleAlloc buffer
                for (i, (_, fty)) in fields.iter().enumerate() {
                    let c_val = builder
                        .build_extract_value(struct_val, i as u32, &format!("es_ret_{}", i))
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                    let i64_val: inkwell::values::IntValue = match c_val {
                        BasicValueEnum::IntValue(iv) => {
                            let is_signed = matches!(types.resolve(*fty), TypeKind::Primitive(p) if p.is_signed());
                            if iv.get_type().get_bit_width() < 64 {
                                if is_signed {
                                    builder
                                        .build_int_s_extend(
                                            iv,
                                            context.i64_type(),
                                            &format!("es_sext_{}", i),
                                        )
                                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                                } else {
                                    builder
                                        .build_int_z_extend(
                                            iv,
                                            context.i64_type(),
                                            &format!("es_zext_{}", i),
                                        )
                                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                                }
                            } else {
                                iv
                            }
                        }
                        BasicValueEnum::FloatValue(fv) => builder
                            .build_bit_cast(fv, context.i64_type(), &format!("es_fcast_{}", i))
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?
                            .into_int_value(),
                        _ => context.i64_type().const_zero(),
                    };
                    let byte_offset = ARC_HEADER_SIZE + (i as u64) * FIELD_SIZE;
                    let field_ptr = unsafe {
                        builder
                            .build_gep(
                                context.i8_type(),
                                obj_ptr,
                                &[context.i64_type().const_int(byte_offset, false)],
                                &format!("es_ret_ptr_{}", i),
                            )
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    };
                    builder
                        .build_store(field_ptr, i64_val)
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                }
            }
        }
    } else if callee_returns_fixed_array {
        // Extern function returning a fixed array: unpack C array into tuple alloca
        if let Some(ret_val) = call_result.try_as_basic_value().basic() {
            let array_val = ret_val.into_array_value();
            let ret_ty = callee_func.unwrap().return_type;
            if let TypeKind::FixedArray { element, length } = types.resolve(ret_ty) {
                let obj_ptr = load_ptr(context, builder, register_allocas, result_reg.0)?;
                let is_signed =
                    matches!(types.resolve(*element), TypeKind::Primitive(p) if p.is_signed());
                for i in 0..*length {
                    let c_val = builder
                        .build_extract_value(array_val, i as u32, &format!("arr_ret_{}", i))
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                    let i64_val: inkwell::values::IntValue = match c_val {
                        BasicValueEnum::IntValue(iv) => {
                            if iv.get_type().get_bit_width() < 64 {
                                if is_signed {
                                    builder
                                        .build_int_s_extend(
                                            iv,
                                            context.i64_type(),
                                            &format!("arr_sext_{}", i),
                                        )
                                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                                } else {
                                    builder
                                        .build_int_z_extend(
                                            iv,
                                            context.i64_type(),
                                            &format!("arr_zext_{}", i),
                                        )
                                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                                }
                            } else {
                                iv
                            }
                        }
                        BasicValueEnum::FloatValue(fv) => builder
                            .build_bit_cast(fv, context.i64_type(), &format!("arr_fcast_{}", i))
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?
                            .into_int_value(),
                        _ => context.i64_type().const_zero(),
                    };
                    let byte_offset = ARC_HEADER_SIZE + (i as u64) * FIELD_SIZE;
                    let field_ptr = unsafe {
                        builder
                            .build_gep(
                                context.i8_type(),
                                obj_ptr,
                                &[context.i64_type().const_int(byte_offset, false)],
                                &format!("arr_ret_ptr_{}", i),
                            )
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?
                    };
                    builder
                        .build_store(field_ptr, i64_val)
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                }
            }
        }
    } else if callee_returns_string {
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
