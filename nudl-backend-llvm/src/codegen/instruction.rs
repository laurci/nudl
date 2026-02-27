use super::*;

pub(super) fn emit_instruction<'ctx>(
    inst: &Instruction,
    program: &Program,
    func: &Function,
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    module: &Module<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_ptr_allocas: &HashMap<u32, PointerValue<'ctx>>,
    str_len_allocas: &HashMap<u32, PointerValue<'ctx>>,
    reg_string_info: &mut HashMap<u32, RegStringInfo>,
    string_constants: &[(GlobalValue<'ctx>, u64)],
    function_map: &HashMap<u32, FunctionValue<'ctx>>,
    types: &TypeInterner,
    arc: &ArcIntrinsics<'ctx>,
    drop_fns: &HashMap<nudl_core::types::TypeId, FunctionValue<'ctx>>,
) -> Result<(), BackendError> {
    match inst {
        Instruction::Const(reg, ConstValue::I32(val)) => {
            let v = context.i64_type().const_int(*val as i64 as u64, true);
            store(builder, register_allocas, reg.0, v)?;
        }
        Instruction::Const(reg, ConstValue::I64(val)) => {
            let v = context.i64_type().const_int(*val as u64, true);
            store(builder, register_allocas, reg.0, v)?;
        }
        Instruction::Const(reg, ConstValue::U64(val)) => {
            let v = context.i64_type().const_int(*val, false);
            store(builder, register_allocas, reg.0, v)?;
        }
        Instruction::Const(reg, ConstValue::Bool(val)) => {
            let v = context
                .i64_type()
                .const_int(if *val { 1 } else { 0 }, false);
            store(builder, register_allocas, reg.0, v)?;
        }
        Instruction::Const(reg, ConstValue::F32(val)) => {
            let v = context.f64_type().const_float(*val as f64);
            store(builder, register_allocas, reg.0, v)?;
        }
        Instruction::Const(reg, ConstValue::F64(val)) => {
            let v = context.f64_type().const_float(*val);
            store(builder, register_allocas, reg.0, v)?;
        }
        Instruction::Const(reg, ConstValue::Char(val)) => {
            let v = context.i64_type().const_int(*val as u64, false);
            store(builder, register_allocas, reg.0, v)?;
        }
        Instruction::Const(reg, ConstValue::StringLiteral(idx)) => {
            reg_string_info.insert(reg.0, RegStringInfo::StringLiteral(*idx));
            // Also store resolved ptr/len to companion allocas so string values
            // survive control flow (if-else, loops) where Copy propagates allocas.
            let (global, len) = &string_constants[*idx as usize];
            let ptr = gep_string_ptr(context, builder, global, *len)?;
            let len_val = context.i64_type().const_int(*len, false);
            if let Some(&ptr_alloca) = str_ptr_allocas.get(&reg.0) {
                builder
                    .build_store(ptr_alloca, ptr)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            }
            if let Some(&len_alloca) = str_len_allocas.get(&reg.0) {
                builder
                    .build_store(len_alloca, len_val)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            }
        }
        Instruction::Const(_, ConstValue::Unit) | Instruction::ConstUnit(_) => {}

        Instruction::StringPtr(dst, src) => {
            let ptr_val = match reg_string_info.get(&src.0) {
                Some(RegStringInfo::StringLiteral(idx)) => {
                    let (global, len) = &string_constants[*idx as usize];
                    gep_string_ptr(context, builder, global, *len)?
                }
                Some(RegStringInfo::StringParam(ptr_alloca, _)) => {
                    let ptr_alloca = *ptr_alloca;
                    builder
                        .build_load(
                            context.ptr_type(AddressSpace::default()),
                            ptr_alloca,
                            "param_ptr",
                        )
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                        .into_pointer_value()
                }
                _ => {
                    // Fallback to companion alloca
                    if let Some(&ptr_al) = str_ptr_allocas.get(&src.0) {
                        builder
                            .build_load(
                                context.ptr_type(AddressSpace::default()),
                                ptr_al,
                                "str_alloca_ptr",
                            )
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?
                            .into_pointer_value()
                    } else {
                        context.ptr_type(AddressSpace::default()).const_null()
                    }
                }
            };
            let v = builder
                .build_ptr_to_int(ptr_val, context.i64_type(), "ptr_to_i64")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, v)?;
        }
        Instruction::StringLen(dst, src) => {
            let len_val = match reg_string_info.get(&src.0) {
                Some(RegStringInfo::StringLiteral(idx)) => {
                    let (_, len) = &string_constants[*idx as usize];
                    context.i64_type().const_int(*len, false)
                }
                Some(RegStringInfo::StringParam(_, len_alloca)) => {
                    let len_alloca = *len_alloca;
                    builder
                        .build_load(context.i64_type(), len_alloca, "param_len")
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                        .into_int_value()
                }
                _ => context.i64_type().const_zero(),
            };
            store(builder, register_allocas, dst.0, len_val)?;
        }
        Instruction::StringConstPtr(dst, str_idx) => {
            let (global, len) = &string_constants[*str_idx as usize];
            let ptr = gep_string_ptr(context, builder, global, *len)?;
            let v = builder
                .build_ptr_to_int(ptr, context.i64_type(), "ptr_to_i64")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, v)?;
        }
        Instruction::StringConstLen(dst, str_idx) => {
            let (_, len) = &string_constants[*str_idx as usize];
            let v = context.i64_type().const_int(*len, false);
            store(builder, register_allocas, dst.0, v)?;
        }

        // Arithmetic
        Instruction::Add(dst, lhs, rhs) => {
            if is_float_register(func, dst.0, types) {
                let (lv, rv) = load_float_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
                let r = builder
                    .build_float_add(lv, rv, "fadd")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            } else {
                let (lv, rv) = load_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
                let r = builder
                    .build_int_add(lv, rv, "add")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            }
        }
        Instruction::Sub(dst, lhs, rhs) => {
            if is_float_register(func, dst.0, types) {
                let (lv, rv) = load_float_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
                let r = builder
                    .build_float_sub(lv, rv, "fsub")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            } else {
                let (lv, rv) = load_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
                let r = builder
                    .build_int_sub(lv, rv, "sub")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            }
        }
        Instruction::Mul(dst, lhs, rhs) => {
            if is_float_register(func, dst.0, types) {
                let (lv, rv) = load_float_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
                let r = builder
                    .build_float_mul(lv, rv, "fmul")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            } else {
                let (lv, rv) = load_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
                let r = builder
                    .build_int_mul(lv, rv, "mul")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            }
        }
        Instruction::Div(dst, lhs, rhs) => {
            if is_float_register(func, dst.0, types) {
                let (lv, rv) = load_float_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
                let r = builder
                    .build_float_div(lv, rv, "fdiv")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            } else {
                let (lv, rv) = load_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
                let r = builder
                    .build_int_signed_div(lv, rv, "sdiv")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            }
        }
        Instruction::Mod(dst, lhs, rhs) => {
            if is_float_register(func, dst.0, types) {
                let (lv, rv) = load_float_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
                let r = builder
                    .build_float_rem(lv, rv, "frem")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            } else {
                let (lv, rv) = load_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
                let r = builder
                    .build_int_signed_rem(lv, rv, "srem")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            }
        }
        Instruction::Shl(dst, lhs, rhs) => {
            let (lv, rv) = load_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
            let r = builder
                .build_left_shift(lv, rv, "shl")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, r)?;
        }
        Instruction::Shr(dst, lhs, rhs) => {
            let (lv, rv) = load_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
            let r = builder
                .build_right_shift(lv, rv, true, "ashr")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, r)?;
        }
        Instruction::BitAnd(dst, lhs, rhs) => {
            let (lv, rv) = load_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
            let r = builder
                .build_and(lv, rv, "and")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, r)?;
        }
        Instruction::BitOr(dst, lhs, rhs) => {
            let (lv, rv) = load_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
            let r = builder
                .build_or(lv, rv, "or")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, r)?;
        }
        Instruction::BitXor(dst, lhs, rhs) => {
            let (lv, rv) = load_binop(context, builder, register_allocas, lhs.0, rhs.0)?;
            let r = builder
                .build_xor(lv, rv, "xor")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, r)?;
        }
        Instruction::Neg(dst, src) => {
            if is_float_register(func, dst.0, types) {
                let sv = load_f64(context, builder, register_allocas, src.0)?;
                let r = builder
                    .build_float_neg(sv, "fneg")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            } else {
                let sv = load_i64(context, builder, register_allocas, src.0)?;
                let zero = context.i64_type().const_zero();
                let r = builder
                    .build_int_sub(zero, sv, "neg")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, r)?;
            }
        }
        Instruction::BitNot(dst, src) => {
            let sv = load_i64(context, builder, register_allocas, src.0)?;
            let r = builder
                .build_not(sv, "bitnot")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, r)?;
        }

        // Comparisons
        Instruction::Eq(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    FloatPredicate::OEQ,
                )?;
            } else {
                emit_icmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    inkwell::IntPredicate::EQ,
                )?;
            }
        }
        Instruction::Ne(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    FloatPredicate::ONE,
                )?;
            } else {
                emit_icmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    inkwell::IntPredicate::NE,
                )?;
            }
        }
        Instruction::Lt(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    FloatPredicate::OLT,
                )?;
            } else {
                emit_icmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    inkwell::IntPredicate::SLT,
                )?;
            }
        }
        Instruction::Le(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    FloatPredicate::OLE,
                )?;
            } else {
                emit_icmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    inkwell::IntPredicate::SLE,
                )?;
            }
        }
        Instruction::Gt(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    FloatPredicate::OGT,
                )?;
            } else {
                emit_icmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    inkwell::IntPredicate::SGT,
                )?;
            }
        }
        Instruction::Ge(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    FloatPredicate::OGE,
                )?;
            } else {
                emit_icmp(
                    context,
                    builder,
                    register_allocas,
                    dst.0,
                    lhs.0,
                    rhs.0,
                    inkwell::IntPredicate::SGE,
                )?;
            }
        }

        // Cast
        Instruction::Cast(dst, src, _target_type) => {
            let src_is_float = is_float_register(func, src.0, types);
            let dst_is_float = is_float_register(func, dst.0, types);
            if src_is_float && !dst_is_float {
                // float → int: fptosi
                let fv = load_f64(context, builder, register_allocas, src.0)?;
                let iv = builder
                    .build_float_to_signed_int(fv, context.i64_type(), "fptosi")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, iv)?;
            } else if !src_is_float && dst_is_float {
                // int → float: sitofp
                let iv = load_i64(context, builder, register_allocas, src.0)?;
                let fv = builder
                    .build_signed_int_to_float(iv, context.f64_type(), "sitofp")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, dst.0, fv)?;
            } else if src_is_float && dst_is_float {
                // float → float: copy
                let fv = load_f64(context, builder, register_allocas, src.0)?;
                store(builder, register_allocas, dst.0, fv)?;
            } else {
                // int → int: copy
                let iv = load_i64(context, builder, register_allocas, src.0)?;
                store(builder, register_allocas, dst.0, iv)?;
            }
        }

        // Logical NOT
        Instruction::Not(dst, src) => {
            let sv = load_i64(context, builder, register_allocas, src.0)?;
            let one = context.i64_type().const_int(1, false);
            let r = builder
                .build_xor(sv, one, "not")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, r)?;
        }

        // Call
        Instruction::Call(result_reg, func_ref, args) => {
            emit_call(
                context,
                builder,
                program,
                func,
                register_allocas,
                str_ptr_allocas,
                str_len_allocas,
                reg_string_info,
                string_constants,
                function_map,
                types,
                result_reg,
                func_ref,
                args,
            )?;
        }

        // Copy
        Instruction::Copy(dst, src) => {
            if is_float_register(func, src.0, types) {
                let val = load_f64(context, builder, register_allocas, src.0)?;
                store(builder, register_allocas, dst.0, val)?;
            } else {
                let val = load_i64(context, builder, register_allocas, src.0)?;
                store(builder, register_allocas, dst.0, val)?;
            }
            // Note: we intentionally do NOT propagate reg_string_info here.
            // The companion allocas (str_ptr_allocas, str_len_allocas) are copied
            // below and correctly handle control flow (if/else branches).
            // Propagating reg_string_info would cause the last-compiled branch
            // to hardcode its string literal for the destination register.

            // Copy string companion allocas for control-flow correctness
            if let (Some(&src_ptr), Some(&dst_ptr)) =
                (str_ptr_allocas.get(&src.0), str_ptr_allocas.get(&dst.0))
            {
                let ptr_val = builder
                    .build_load(
                        context.ptr_type(AddressSpace::default()),
                        src_ptr,
                        "copy_str_ptr",
                    )
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                builder
                    .build_store(dst_ptr, ptr_val)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            }
            if let (Some(&src_len), Some(&dst_len)) =
                (str_len_allocas.get(&src.0), str_len_allocas.get(&dst.0))
            {
                let len_val = builder
                    .build_load(context.i64_type(), src_len, "copy_str_len")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                builder
                    .build_store(dst_len, len_val)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            }
        }

        Instruction::Nop => {}

        // ARC / heap operations
        Instruction::Alloc(dst, type_id) => {
            // Header (16 bytes) + fields (8 bytes each)
            let header_size = 16u64;
            let field_size = match types.resolve(*type_id) {
                TypeKind::Struct { fields, .. } => fields.len() as u64 * 8,
                TypeKind::Enum { variants, .. } => {
                    // tag (1 slot) + max variant fields
                    let max_fields = variants.iter().map(|v| v.fields.len()).max().unwrap_or(0);
                    (1 + max_fields) as u64 * 8
                }
                TypeKind::DynamicArray { .. } => {
                    // ptr, len, capacity = 3 fields
                    3 * 8
                }
                TypeKind::Map { .. } => {
                    // Internal hash map: entries_ptr, len, capacity, key_hashes_ptr = 4 fields
                    4 * 8
                }
                _ => 0,
            };
            let total_size = context
                .i64_type()
                .const_int(header_size + field_size, false);
            let type_tag = context.i32_type().const_int(type_id.0 as u64, false);
            let call_result = builder
                .build_direct_call(
                    arc.arc_alloc,
                    &[total_size.into(), type_tag.into()],
                    "alloc",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("arc_alloc should return a pointer")
                .into_pointer_value();
            // Store pointer as i64 in the register alloca
            let ptr_as_i64 = builder
                .build_ptr_to_int(ptr, context.i64_type(), "ptr_to_i64")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, ptr_as_i64)?;
        }
        Instruction::Load(dst, ptr_reg, offset) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, ptr_reg.0)?;
            // Compute field address: ptr + 16 (header) + offset * 8
            let byte_offset = 16u64 + (*offset as u64) * 8;
            let field_ptr = unsafe {
                builder
                    .build_gep(
                        context.i8_type(),
                        obj_ptr,
                        &[context.i64_type().const_int(byte_offset, false)],
                        "field_ptr",
                    )
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            };
            let val = builder
                .build_load(context.i64_type(), field_ptr, "field_val")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
                .into_int_value();
            store(builder, register_allocas, dst.0, val)?;
        }
        Instruction::Store(ptr_reg, offset, src) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, ptr_reg.0)?;
            let byte_offset = 16u64 + (*offset as u64) * 8;
            let field_ptr = unsafe {
                builder
                    .build_gep(
                        context.i8_type(),
                        obj_ptr,
                        &[context.i64_type().const_int(byte_offset, false)],
                        "field_ptr",
                    )
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            };
            let val = load_i64(context, builder, register_allocas, src.0)?;
            builder
                .build_store(field_ptr, val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
        Instruction::Retain(reg) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, reg.0)?;
            builder
                .build_direct_call(arc.arc_retain, &[obj_ptr.into()], "")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
        Instruction::Release(reg, type_id) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, reg.0)?;
            let drop_fn_ptr = if let Some(tid) = type_id {
                if let Some(dfn) = drop_fns.get(tid) {
                    dfn.as_global_value().as_pointer_value()
                } else {
                    arc.drop_noop.as_global_value().as_pointer_value()
                }
            } else {
                arc.drop_noop.as_global_value().as_pointer_value()
            };
            builder
                .build_direct_call(arc.arc_release, &[obj_ptr.into(), drop_fn_ptr.into()], "")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }

        // Tuple/Array operations — heap-allocated like structs for now
        Instruction::TupleAlloc(dst, type_id, elements)
        | Instruction::FixedArrayAlloc(dst, type_id, elements) => {
            // Allocate: header (16 bytes) + elements (8 bytes each)
            let header_size = 16u64;
            let field_size = elements.len() as u64 * 8;
            let total_size = context
                .i64_type()
                .const_int(header_size + field_size, false);
            let type_tag = context.i32_type().const_int(type_id.0 as u64, false);
            let call_result = builder
                .build_direct_call(
                    arc.arc_alloc,
                    &[total_size.into(), type_tag.into()],
                    "alloc",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let ptr = call_result
                .try_as_basic_value()
                .basic()
                .expect("arc_alloc should return a pointer")
                .into_pointer_value();
            // Store elements
            for (i, elem_reg) in elements.iter().enumerate() {
                let byte_offset = 16u64 + (i as u64) * 8;
                let field_ptr = unsafe {
                    builder
                        .build_gep(
                            context.i8_type(),
                            ptr,
                            &[context.i64_type().const_int(byte_offset, false)],
                            "elem_ptr",
                        )
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                };
                let val = load_i64(context, builder, register_allocas, elem_reg.0)?;
                builder
                    .build_store(field_ptr, val)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            }
            // Store pointer as i64
            let ptr_as_i64 = builder
                .build_ptr_to_int(ptr, context.i64_type(), "ptr_to_i64")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, ptr_as_i64)?;
        }
        Instruction::TupleLoad(dst, ptr_reg, offset) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, ptr_reg.0)?;
            let byte_offset = 16u64 + (*offset as u64) * 8;
            let field_ptr = unsafe {
                builder
                    .build_gep(
                        context.i8_type(),
                        obj_ptr,
                        &[context.i64_type().const_int(byte_offset, false)],
                        "tuple_field_ptr",
                    )
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            };
            let val = builder
                .build_load(context.i64_type(), field_ptr, "tuple_field_val")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
                .into_int_value();
            store(builder, register_allocas, dst.0, val)?;
        }
        Instruction::TupleStore(ptr_reg, offset, src) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, ptr_reg.0)?;
            let byte_offset = 16u64 + (*offset as u64) * 8;
            let field_ptr = unsafe {
                builder
                    .build_gep(
                        context.i8_type(),
                        obj_ptr,
                        &[context.i64_type().const_int(byte_offset, false)],
                        "tuple_field_ptr",
                    )
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            };
            let val = load_i64(context, builder, register_allocas, src.0)?;
            builder
                .build_store(field_ptr, val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
        Instruction::IndexLoad(dst, ptr_reg, idx_reg, _elem_type) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, ptr_reg.0)?;
            let idx_val = load_i64(context, builder, register_allocas, idx_reg.0)?;
            // Compute byte offset: 16 (header) + idx * 8
            let eight = context.i64_type().const_int(8, false);
            let sixteen = context.i64_type().const_int(16, false);
            let idx_offset = builder
                .build_int_mul(idx_val, eight, "idx_offset")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let byte_offset = builder
                .build_int_add(sixteen, idx_offset, "byte_offset")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let field_ptr = unsafe {
                builder
                    .build_gep(context.i8_type(), obj_ptr, &[byte_offset], "index_ptr")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            };
            let val = builder
                .build_load(context.i64_type(), field_ptr, "index_val")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
                .into_int_value();
            store(builder, register_allocas, dst.0, val)?;
        }
        Instruction::IndexStore(ptr_reg, idx_reg, src) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, ptr_reg.0)?;
            let idx_val = load_i64(context, builder, register_allocas, idx_reg.0)?;
            let eight = context.i64_type().const_int(8, false);
            let sixteen = context.i64_type().const_int(16, false);
            let idx_offset = builder
                .build_int_mul(idx_val, eight, "idx_offset")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let byte_offset = builder
                .build_int_add(sixteen, idx_offset, "byte_offset")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let field_ptr = unsafe {
                builder
                    .build_gep(context.i8_type(), obj_ptr, &[byte_offset], "index_ptr")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            };
            let val = load_i64(context, builder, register_allocas, src.0)?;
            builder
                .build_store(field_ptr, val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }

        // ---- Closure operations ----

        Instruction::ClosureCreate(dst, thunk_fn_id, captures) => {
        // A closure value is an ARC object: [header(16)] [fn_id(8)] [env_ptr(8)]
        // The env is a separate ARC object: [header(16)] [cap0(8)] [cap1(8)] ...
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());

        // Allocate capture environment if there are captures
        let env_ptr_val = if captures.is_empty() {
            // No captures: env_ptr is null
            i64_ty.const_zero()
        } else {
            // Call __nudl_closure_env_alloc(num_captures)
            let env_alloc_fn = func.id; // placeholder — we'll declare the runtime fn
            let _ = env_alloc_fn;
            let num_captures = i64_ty.const_int(captures.len() as u64, false);

            // Get or declare __nudl_closure_env_alloc
            // module is passed as parameter
            let env_alloc = module
                .get_function("__nudl_closure_env_alloc")
                .unwrap_or_else(|| {
                    let fn_ty = ptr_ty.fn_type(&[i64_ty.into()], false);
                    module.add_function(
                        "__nudl_closure_env_alloc",
                        fn_ty,
                        Some(inkwell::module::Linkage::External),
                    )
                });
            let env_call = builder
                .build_direct_call(env_alloc, &[num_captures.into()], "env_alloc")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let env_ptr = env_call
                .try_as_basic_value()
                .basic()
                .expect("env_alloc returns ptr")
                .into_pointer_value();

            // Store captured values into the env: env + 16 + i*8
            for (i, cap_reg) in captures.iter().enumerate() {
                let byte_offset = 16u64 + (i as u64) * 8;
                let field_ptr = unsafe {
                    builder
                        .build_gep(
                            context.i8_type(),
                            env_ptr,
                            &[i64_ty.const_int(byte_offset, false)],
                            "cap_ptr",
                        )
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                };
                let val = load_i64(context, builder, register_allocas, cap_reg.0)?;
                builder
                    .build_store(field_ptr, val)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            }

            // Convert env pointer to i64
            builder
                .build_ptr_to_int(env_ptr, i64_ty, "env_as_i64")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
        };

        // Allocate closure object: [header(16)] [fn_id(8)] [env_ptr(8)]
        let closure_size = i64_ty.const_int(32, false); // 16 header + 8 fn_id + 8 env_ptr
        let type_tag = context.i32_type().const_zero();
        let closure_alloc = builder
            .build_direct_call(
                arc.arc_alloc,
                &[closure_size.into(), type_tag.into()],
                "closure_alloc",
            )
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let closure_ptr = closure_alloc
            .try_as_basic_value()
            .basic()
            .expect("alloc returns ptr")
            .into_pointer_value();

        // Store fn_ptr at offset 16 — the actual LLVM function pointer, cast to i64
        let fn_ptr_val = if let Some(&thunk_fn) = function_map.get(&thunk_fn_id.0) {
            // Convert function pointer to i64
            builder
                .build_ptr_to_int(
                    thunk_fn.as_global_value().as_pointer_value(),
                    i64_ty,
                    "fn_ptr_i64",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
        } else {
            i64_ty.const_zero()
        };
        let fn_ptr_field = unsafe {
            builder
                .build_gep(
                    context.i8_type(),
                    closure_ptr,
                    &[i64_ty.const_int(16, false)],
                    "fn_ptr_field",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
        };
        builder
            .build_store(fn_ptr_field, fn_ptr_val)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // Store env_ptr at offset 24
        let env_ptr_field = unsafe {
            builder
                .build_gep(
                    context.i8_type(),
                    closure_ptr,
                    &[i64_ty.const_int(24, false)],
                    "env_ptr_field",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
        };
        builder
            .build_store(env_ptr_field, env_ptr_val)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // Store closure pointer as i64 in the register
        let ptr_as_i64 = builder
            .build_ptr_to_int(closure_ptr, i64_ty, "closure_i64")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        store(builder, register_allocas, dst.0, ptr_as_i64)?;
    }

    Instruction::ClosureCall(dst, closure_reg, args) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());

        // Load closure pointer
        let closure_ptr = load_ptr(context, builder, register_allocas, closure_reg.0)?;

        // Load env_ptr from offset 24
        let env_ptr_field = unsafe {
            builder
                .build_gep(
                    context.i8_type(),
                    closure_ptr,
                    &[i64_ty.const_int(24, false)],
                    "env_ptr_field",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
        };
        let env_ptr = builder
            .build_load(i64_ty, env_ptr_field, "env_ptr")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // Load fn_ptr from offset 16 (stored as i64, convert to function pointer)
        let fn_ptr_field = unsafe {
            builder
                .build_gep(
                    context.i8_type(),
                    closure_ptr,
                    &[i64_ty.const_int(16, false)],
                    "fn_ptr_field",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
        };
        let fn_ptr_as_i64 = builder
            .build_load(i64_ty, fn_ptr_field, "fn_ptr_i64")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
            .into_int_value();
        let fn_ptr = builder
            .build_int_to_ptr(fn_ptr_as_i64, ptr_ty, "fn_ptr")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // Build call arguments: env_ptr first, then the closure's args
        let mut call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> = vec![env_ptr.into()];
        for arg_reg in args {
            let val = load_i64(context, builder, register_allocas, arg_reg.0)?;
            call_args.push(val.into());
        }

        // Build function type for indirect call: all i64 params, i64 return
        let param_types: Vec<BasicMetadataTypeEnum<'ctx>> =
            (0..call_args.len()).map(|_| i64_ty.into()).collect();
        let fn_type = i64_ty.fn_type(&param_types, false);

        // Indirect call through function pointer
        let call_result = builder
            .build_indirect_call(fn_type, fn_ptr, &call_args, "closure_call")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        if let Some(ret_val) = call_result.try_as_basic_value().basic() {
            if let BasicValueEnum::IntValue(iv) = ret_val {
                store(builder, register_allocas, dst.0, iv)?;
            }
        } else {
            store(builder, register_allocas, dst.0, i64_ty.const_zero())?;
        }
    }

    // ---- Dynamic Array operations ----

    Instruction::DynArrayAlloc(dst, _type_id) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        // module is passed as parameter
        let array_alloc = module
            .get_function("__nudl_array_alloc")
            .unwrap_or_else(|| {
                let fn_ty = ptr_ty.fn_type(&[], false);
                module.add_function(
                    "__nudl_array_alloc",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let call_result = builder
            .build_direct_call(array_alloc, &[], "array_alloc")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let ptr = call_result
            .try_as_basic_value()
            .basic()
            .expect("array_alloc returns ptr")
            .into_pointer_value();
        let ptr_as_i64 = builder
            .build_ptr_to_int(ptr, i64_ty, "arr_ptr_i64")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        store(builder, register_allocas, dst.0, ptr_as_i64)?;
    }

    Instruction::DynArrayPush(arr_reg, val_reg) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        // module is passed as parameter
        let array_push = module
            .get_function("__nudl_array_push")
            .unwrap_or_else(|| {
                let fn_ty = context.void_type().fn_type(&[ptr_ty.into(), i64_ty.into()], false);
                module.add_function(
                    "__nudl_array_push",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let arr_ptr = load_ptr(context, builder, register_allocas, arr_reg.0)?;
        let val = load_i64(context, builder, register_allocas, val_reg.0)?;
        builder
            .build_direct_call(array_push, &[arr_ptr.into(), val.into()], "")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    }

    Instruction::DynArrayPop(dst, arr_reg) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        // module is passed as parameter
        let array_pop = module
            .get_function("__nudl_array_pop")
            .unwrap_or_else(|| {
                let fn_ty = i64_ty.fn_type(&[ptr_ty.into()], false);
                module.add_function(
                    "__nudl_array_pop",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let arr_ptr = load_ptr(context, builder, register_allocas, arr_reg.0)?;
        let call_result = builder
            .build_direct_call(array_pop, &[arr_ptr.into()], "pop_val")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let val = call_result
            .try_as_basic_value()
            .basic()
            .expect("pop returns i64")
            .into_int_value();
        store(builder, register_allocas, dst.0, val)?;
    }

    Instruction::DynArrayLen(dst, arr_reg) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        // module is passed as parameter
        let array_len = module
            .get_function("__nudl_array_len")
            .unwrap_or_else(|| {
                let fn_ty = i64_ty.fn_type(&[ptr_ty.into()], false);
                module.add_function(
                    "__nudl_array_len",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let arr_ptr = load_ptr(context, builder, register_allocas, arr_reg.0)?;
        let call_result = builder
            .build_direct_call(array_len, &[arr_ptr.into()], "arr_len")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let val = call_result
            .try_as_basic_value()
            .basic()
            .expect("len returns i64")
            .into_int_value();
        store(builder, register_allocas, dst.0, val)?;
    }

    Instruction::DynArrayGet(dst, arr_reg, idx_reg) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        let array_get = module
            .get_function("__nudl_array_get")
            .unwrap_or_else(|| {
                let fn_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);
                module.add_function(
                    "__nudl_array_get",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let arr_ptr = load_ptr(context, builder, register_allocas, arr_reg.0)?;
        let idx = load_i64(context, builder, register_allocas, idx_reg.0)?;
        let call_result = builder
            .build_direct_call(array_get, &[arr_ptr.into(), idx.into()], "array_get")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let val = call_result
            .try_as_basic_value()
            .basic()
            .expect("array_get returns i64")
            .into_int_value();
        store(builder, register_allocas, dst.0, val)?;
    }

    Instruction::DynArraySet(arr_reg, idx_reg, val_reg) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        let array_set = module
            .get_function("__nudl_array_set")
            .unwrap_or_else(|| {
                let fn_ty = context.void_type().fn_type(&[ptr_ty.into(), i64_ty.into(), i64_ty.into()], false);
                module.add_function(
                    "__nudl_array_set",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let arr_ptr = load_ptr(context, builder, register_allocas, arr_reg.0)?;
        let idx = load_i64(context, builder, register_allocas, idx_reg.0)?;
        let val = load_i64(context, builder, register_allocas, val_reg.0)?;
        builder
            .build_direct_call(array_set, &[arr_ptr.into(), idx.into(), val.into()], "array_set")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    }

    // ---- Map operations ----

    Instruction::MapAlloc(dst, _type_id) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        // module is passed as parameter
        let map_alloc = module
            .get_function("__nudl_map_alloc")
            .unwrap_or_else(|| {
                let fn_ty = ptr_ty.fn_type(&[], false);
                module.add_function(
                    "__nudl_map_alloc",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let call_result = builder
            .build_direct_call(map_alloc, &[], "map_alloc")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let ptr = call_result
            .try_as_basic_value()
            .basic()
            .expect("map_alloc returns ptr")
            .into_pointer_value();
        let ptr_as_i64 = builder
            .build_ptr_to_int(ptr, i64_ty, "map_ptr_i64")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        store(builder, register_allocas, dst.0, ptr_as_i64)?;
    }

    Instruction::MapInsert(map_reg, key_reg, val_reg) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        // module is passed as parameter
        let map_insert = module
            .get_function("__nudl_map_insert")
            .unwrap_or_else(|| {
                let fn_ty = context.void_type().fn_type(
                    &[ptr_ty.into(), i64_ty.into(), i64_ty.into()],
                    false,
                );
                module.add_function(
                    "__nudl_map_insert",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let map_ptr = load_ptr(context, builder, register_allocas, map_reg.0)?;
        let key = load_i64(context, builder, register_allocas, key_reg.0)?;
        let val = load_i64(context, builder, register_allocas, val_reg.0)?;
        builder
            .build_direct_call(map_insert, &[map_ptr.into(), key.into(), val.into()], "")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    }

    Instruction::MapGet(dst, map_reg, key_reg) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        // module is passed as parameter
        // __nudl_map_get returns value; third param is pointer to 'found' flag
        let map_get = module
            .get_function("__nudl_map_get")
            .unwrap_or_else(|| {
                let fn_ty = i64_ty.fn_type(
                    &[ptr_ty.into(), i64_ty.into(), ptr_ty.into()],
                    false,
                );
                module.add_function(
                    "__nudl_map_get",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let map_ptr = load_ptr(context, builder, register_allocas, map_reg.0)?;
        let key = load_i64(context, builder, register_allocas, key_reg.0)?;
        // Allocate a local for the 'found' flag
        let found_alloca = builder
            .build_alloca(i64_ty, "found")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let call_result = builder
            .build_direct_call(
                map_get,
                &[map_ptr.into(), key.into(), found_alloca.into()],
                "map_get_val",
            )
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let val = call_result
            .try_as_basic_value()
            .basic()
            .expect("map_get returns i64")
            .into_int_value();
        store(builder, register_allocas, dst.0, val)?;
    }

    Instruction::MapLen(dst, map_reg) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        // module is passed as parameter
        let map_len = module
            .get_function("__nudl_map_len")
            .unwrap_or_else(|| {
                let fn_ty = i64_ty.fn_type(&[ptr_ty.into()], false);
                module.add_function(
                    "__nudl_map_len",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let map_ptr = load_ptr(context, builder, register_allocas, map_reg.0)?;
        let call_result = builder
            .build_direct_call(map_len, &[map_ptr.into()], "map_len")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let val = call_result
            .try_as_basic_value()
            .basic()
            .expect("map_len returns i64")
            .into_int_value();
        store(builder, register_allocas, dst.0, val)?;
    }

    Instruction::MapContains(dst, map_reg, key_reg) => {
        let i64_ty = context.i64_type();
        let ptr_ty = context.ptr_type(AddressSpace::default());
        // module is passed as parameter
        let map_contains = module
            .get_function("__nudl_map_contains")
            .unwrap_or_else(|| {
                let fn_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);
                module.add_function(
                    "__nudl_map_contains",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
        let map_ptr = load_ptr(context, builder, register_allocas, map_reg.0)?;
        let key = load_i64(context, builder, register_allocas, key_reg.0)?;
        let call_result = builder
            .build_direct_call(map_contains, &[map_ptr.into(), key.into()], "map_contains")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let val = call_result
            .try_as_basic_value()
            .basic()
            .expect("map_contains returns i64")
            .into_int_value();
        store(builder, register_allocas, dst.0, val)?;
    }

    }
    Ok(())
}

