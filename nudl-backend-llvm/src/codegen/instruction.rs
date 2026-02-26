use super::*;

pub(super) fn emit_instruction<'ctx>(
    inst: &Instruction,
    program: &Program,
    func: &Function,
    context: &'ctx Context,
    builder: &Builder<'ctx>,
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
            // Header (16 bytes) + fields (8 bytes each for struct types)
            let header_size = 16u64;
            let field_size = match types.resolve(*type_id) {
                TypeKind::Struct { fields, .. } => fields.len() as u64 * 8,
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
    }
    Ok(())
}
