use super::*;

/// ARC object header size in bytes (refcount: i64, type tag: i64)
pub(super) const ARC_HEADER_SIZE: u64 = 16;
/// Size of each field/element slot in bytes (everything is i64-width)
pub(super) const FIELD_SIZE: u64 = 8;
/// Offset of string length field within heap string object (after ARC header)
pub(super) const STRING_LEN_OFFSET: u64 = 16;
/// Offset of string data within heap string object (after ARC header + length)
pub(super) const STRING_DATA_OFFSET: u64 = 24;

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
    string_builtins: &StringBuiltins<'ctx>,
    drop_fns: &HashMap<nudl_core::types::TypeId, FunctionValue<'ctx>>,
) -> Result<(), BackendError> {
    // Macro for arithmetic ops with float/int dispatch
    macro_rules! emit_arith {
        ($dst:expr, $lhs:expr, $rhs:expr, $float_op:ident, $fname:expr, $int_op:ident, $iname:expr) => {{
            if is_float_register(func, $dst.0, types) {
                let (lv, rv) =
                    load_float_binop(context, builder, register_allocas, $lhs.0, $rhs.0)?;
                let r = builder
                    .$float_op(lv, rv, $fname)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, $dst.0, r)?;
            } else {
                let (lv, rv) = load_binop(context, builder, register_allocas, $lhs.0, $rhs.0)?;
                let r = builder
                    .$int_op(lv, rv, $iname)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                store(builder, register_allocas, $dst.0, r)?;
            }
        }};
    }

    // Macro for comparison ops with float/int dispatch
    macro_rules! emit_cmp {
        ($dst:expr, $lhs:expr, $rhs:expr, $fpred:expr, $ipred:expr) => {{
            if is_float_register(func, $lhs.0, types) {
                emit_fcmp(
                    context,
                    builder,
                    register_allocas,
                    $dst.0,
                    $lhs.0,
                    $rhs.0,
                    $fpred,
                )?;
            } else {
                emit_icmp(
                    context,
                    builder,
                    register_allocas,
                    $dst.0,
                    $lhs.0,
                    $rhs.0,
                    $ipred,
                )?;
            }
        }};
    }

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
                _ => {
                    // Fallback: load from companion alloca (e.g., after Copy)
                    if let Some(&len_al) = str_len_allocas.get(&src.0) {
                        builder
                            .build_load(context.i64_type(), len_al, "str_alloca_len")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?
                            .into_int_value()
                    } else {
                        context.i64_type().const_zero()
                    }
                }
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

        Instruction::StringCharAt(dst, str_reg, idx_reg) => {
            let i8_ty = context.i8_type();
            let i64_ty = context.i64_type();
            let ptr_ty = context.ptr_type(AddressSpace::default());

            // Load string (ptr, len)
            let (str_ptr_val, str_len_val) = match reg_string_info.get(&str_reg.0) {
                Some(RegStringInfo::StringLiteral(idx)) => {
                    let (global, len) = &string_constants[*idx as usize];
                    let ptr = gep_string_ptr(context, builder, global, *len)?;
                    let len_val = i64_ty.const_int(*len, false);
                    (ptr, len_val)
                }
                Some(RegStringInfo::StringParam(ptr_alloca, len_alloca)) => {
                    let ptr = builder
                        .build_load(ptr_ty, *ptr_alloca, "str_ptr")
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                        .into_pointer_value();
                    let len = builder
                        .build_load(i64_ty, *len_alloca, "str_len")
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                        .into_int_value();
                    (ptr, len)
                }
                _ => {
                    if let (Some(&ptr_al), Some(&len_al)) = (
                        str_ptr_allocas.get(&str_reg.0),
                        str_len_allocas.get(&str_reg.0),
                    ) {
                        let ptr = builder
                            .build_load(ptr_ty, ptr_al, "str_ptr")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?
                            .into_pointer_value();
                        let len = builder
                            .build_load(i64_ty, len_al, "str_len")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?
                            .into_int_value();
                        (ptr, len)
                    } else {
                        (ptr_ty.const_null(), i64_ty.const_zero())
                    }
                }
            };

            let idx_val = load_i64(context, builder, register_allocas, idx_reg.0)?;

            // Bounds check: if idx < 0 || idx >= len, abort
            let current_fn = builder.get_insert_block().unwrap().get_parent().unwrap();
            let ok_block = context.append_basic_block(current_fn, "str_idx_ok");
            let abort_block = context.append_basic_block(current_fn, "str_idx_oob");

            // Check idx >= 0 && idx < len
            let zero = i64_ty.const_zero();
            let ge_zero = builder
                .build_int_compare(inkwell::IntPredicate::SGE, idx_val, zero, "ge_zero")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let lt_len = builder
                .build_int_compare(inkwell::IntPredicate::SLT, idx_val, str_len_val, "lt_len")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            let in_bounds = builder
                .build_and(ge_zero, lt_len, "in_bounds")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            builder
                .build_conditional_branch(in_bounds, ok_block, abort_block)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            // OOB: call abort
            builder.position_at_end(abort_block);
            let abort_fn = module.get_function("abort").unwrap_or_else(|| {
                let fn_ty = context.void_type().fn_type(&[], false);
                module.add_function("abort", fn_ty, Some(inkwell::module::Linkage::External))
            });
            builder
                .build_direct_call(abort_fn, &[], "")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            builder
                .build_unreachable()
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            // OK: GEP + load byte, zero-extend to i64
            builder.position_at_end(ok_block);
            let char_ptr = unsafe {
                builder
                    .build_gep(i8_ty, str_ptr_val, &[idx_val], "char_ptr")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            };
            let byte_val = builder
                .build_load(i8_ty, char_ptr, "char_byte")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?
                .into_int_value();
            let char_val = builder
                .build_int_z_extend(byte_val, i64_ty, "char_i64")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            store(builder, register_allocas, dst.0, char_val)?;
        }

        // Arithmetic
        Instruction::Add(dst, lhs, rhs) => {
            emit_arith!(dst, lhs, rhs, build_float_add, "fadd", build_int_add, "add");
        }
        Instruction::Sub(dst, lhs, rhs) => {
            emit_arith!(dst, lhs, rhs, build_float_sub, "fsub", build_int_sub, "sub");
        }
        Instruction::Mul(dst, lhs, rhs) => {
            emit_arith!(dst, lhs, rhs, build_float_mul, "fmul", build_int_mul, "mul");
        }
        Instruction::Div(dst, lhs, rhs) => {
            emit_arith!(
                dst,
                lhs,
                rhs,
                build_float_div,
                "fdiv",
                build_int_signed_div,
                "sdiv"
            );
        }
        Instruction::Mod(dst, lhs, rhs) => {
            emit_arith!(
                dst,
                lhs,
                rhs,
                build_float_rem,
                "frem",
                build_int_signed_rem,
                "srem"
            );
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
            emit_cmp!(
                dst,
                lhs,
                rhs,
                FloatPredicate::OEQ,
                inkwell::IntPredicate::EQ
            );
        }
        Instruction::Ne(dst, lhs, rhs) => {
            emit_cmp!(
                dst,
                lhs,
                rhs,
                FloatPredicate::ONE,
                inkwell::IntPredicate::NE
            );
        }
        Instruction::Lt(dst, lhs, rhs) => {
            emit_cmp!(
                dst,
                lhs,
                rhs,
                FloatPredicate::OLT,
                inkwell::IntPredicate::SLT
            );
        }
        Instruction::Le(dst, lhs, rhs) => {
            emit_cmp!(
                dst,
                lhs,
                rhs,
                FloatPredicate::OLE,
                inkwell::IntPredicate::SLE
            );
        }
        Instruction::Gt(dst, lhs, rhs) => {
            emit_cmp!(
                dst,
                lhs,
                rhs,
                FloatPredicate::OGT,
                inkwell::IntPredicate::SGT
            );
        }
        Instruction::Ge(dst, lhs, rhs) => {
            emit_cmp!(
                dst,
                lhs,
                rhs,
                FloatPredicate::OGE,
                inkwell::IntPredicate::SGE
            );
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
                arc,
                string_builtins,
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
            let byte_offset = ARC_HEADER_SIZE + (*offset as u64) * FIELD_SIZE;
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
            let byte_offset = ARC_HEADER_SIZE + (*offset as u64) * FIELD_SIZE;
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
            let ptr_ty = context.ptr_type(AddressSpace::default());
            let drop_fn_ptr = if let Some(tid) = type_id {
                if let Some(dfn) = drop_fns.get(tid) {
                    dfn.as_global_value().as_pointer_value()
                } else {
                    ptr_ty.const_null()
                }
            } else {
                ptr_ty.const_null()
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
                // String elements need heap copies (same as DynArrayPush)
                let val_is_string = reg_string_info.contains_key(&elem_reg.0);
                let val = if val_is_string {
                    let (s_ptr, s_len) = load_string_arg(
                        context,
                        builder,
                        reg_string_info,
                        string_constants,
                        str_ptr_allocas,
                        str_len_allocas,
                        elem_reg.0,
                    )?;
                    let i64_ty = context.i64_type();
                    let ptr_ty = context.ptr_type(AddressSpace::default());
                    let str_alloc_fn =
                        module.get_function("__nudl_str_alloc").unwrap_or_else(|| {
                            let fn_ty = ptr_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);
                            module.add_function(
                                "__nudl_str_alloc",
                                fn_ty,
                                Some(inkwell::module::Linkage::External),
                            )
                        });
                    let call_result = builder
                        .build_direct_call(
                            str_alloc_fn,
                            &[s_ptr.into(), s_len.into()],
                            "str_heap_copy",
                        )
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                    let heap_ptr = call_result
                        .try_as_basic_value()
                        .basic()
                        .expect("str_alloc returns ptr")
                        .into_pointer_value();
                    builder
                        .build_ptr_to_int(heap_ptr, context.i64_type(), "str_heap_i64")
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                } else {
                    load_i64(context, builder, register_allocas, elem_reg.0)?
                };
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
            let byte_offset = ARC_HEADER_SIZE + (*offset as u64) * FIELD_SIZE;
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

            // If the field type is string, extract (data_ptr, len) from the heap string pointer
            let dst_ty = func.register_types.get(dst.0 as usize).copied();
            let is_string = dst_ty.is_some_and(|ty| matches!(types.resolve(ty), TypeKind::String));
            if is_string {
                let ptr_ty = context.ptr_type(AddressSpace::default());
                let heap_ptr = builder
                    .build_int_to_ptr(val, ptr_ty, "str_heap_ptr")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
                store_string_result(
                    context,
                    builder,
                    str_ptr_allocas,
                    str_len_allocas,
                    register_allocas,
                    reg_string_info,
                    dst.0,
                    data_ptr,
                    len_val,
                )?;
            }
        }
        Instruction::TupleStore(ptr_reg, offset, src) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, ptr_reg.0)?;
            let byte_offset = ARC_HEADER_SIZE + (*offset as u64) * FIELD_SIZE;
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
            // String values need heap copies (same as DynArrayPush)
            let val_is_string = reg_string_info.contains_key(&src.0);
            let val = if val_is_string {
                let (s_ptr, s_len) = load_string_arg(
                    context,
                    builder,
                    reg_string_info,
                    string_constants,
                    str_ptr_allocas,
                    str_len_allocas,
                    src.0,
                )?;
                let i64_ty = context.i64_type();
                let ptr_ty = context.ptr_type(AddressSpace::default());
                let str_alloc_fn = module.get_function("__nudl_str_alloc").unwrap_or_else(|| {
                    let fn_ty = ptr_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);
                    module.add_function(
                        "__nudl_str_alloc",
                        fn_ty,
                        Some(inkwell::module::Linkage::External),
                    )
                });
                let call_result = builder
                    .build_direct_call(str_alloc_fn, &[s_ptr.into(), s_len.into()], "str_heap_copy")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                let heap_ptr = call_result
                    .try_as_basic_value()
                    .basic()
                    .expect("str_alloc returns ptr")
                    .into_pointer_value();
                builder
                    .build_ptr_to_int(heap_ptr, context.i64_type(), "str_heap_i64")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            } else {
                load_i64(context, builder, register_allocas, src.0)?
            };
            builder
                .build_store(field_ptr, val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
        Instruction::IndexLoad(dst, ptr_reg, idx_reg, elem_type) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, ptr_reg.0)?;
            let idx_val = load_i64(context, builder, register_allocas, idx_reg.0)?;
            // Compute byte offset: 16 (header) + idx * 8
            let eight = context.i64_type().const_int(FIELD_SIZE, false);
            let sixteen = context.i64_type().const_int(ARC_HEADER_SIZE, false);
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

            // If element is a string, extract (ptr, len) from the heap string object.
            if matches!(types.resolve(*elem_type), TypeKind::String) {
                let ptr_ty = context.ptr_type(AddressSpace::default());
                let heap_ptr = builder
                    .build_int_to_ptr(val, ptr_ty, "str_heap_ptr")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
                store_string_result(
                    context,
                    builder,
                    str_ptr_allocas,
                    str_len_allocas,
                    register_allocas,
                    reg_string_info,
                    dst.0,
                    data_ptr,
                    len_val,
                )?;
            }
        }
        Instruction::IndexStore(ptr_reg, idx_reg, src) => {
            let obj_ptr = load_ptr(context, builder, register_allocas, ptr_reg.0)?;
            let idx_val = load_i64(context, builder, register_allocas, idx_reg.0)?;
            let eight = context.i64_type().const_int(FIELD_SIZE, false);
            let sixteen = context.i64_type().const_int(ARC_HEADER_SIZE, false);
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
            // Store fn_ptr at ARC_HEADER_SIZE (first field after header)
            let fn_ptr_field = unsafe {
                builder
                    .build_gep(
                        context.i8_type(),
                        closure_ptr,
                        &[i64_ty.const_int(ARC_HEADER_SIZE, false)],
                        "fn_ptr_field",
                    )
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            };
            builder
                .build_store(fn_ptr_field, fn_ptr_val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            // Store env_ptr at ARC_HEADER_SIZE + FIELD_SIZE (second field)
            let env_ptr_field = unsafe {
                builder
                    .build_gep(
                        context.i8_type(),
                        closure_ptr,
                        &[i64_ty.const_int(ARC_HEADER_SIZE + FIELD_SIZE, false)],
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

        Instruction::ClosureCall(dst, closure_reg, args, cl_param_types, cl_ret_type) => {
            let i64_ty = context.i64_type();
            let ptr_ty = context.ptr_type(AddressSpace::default());

            // Load closure pointer
            let closure_ptr = load_ptr(context, builder, register_allocas, closure_reg.0)?;

            // Load env_ptr from ARC_HEADER_SIZE + FIELD_SIZE (second field)
            let env_ptr_field = unsafe {
                builder
                    .build_gep(
                        context.i8_type(),
                        closure_ptr,
                        &[i64_ty.const_int(ARC_HEADER_SIZE + FIELD_SIZE, false)],
                        "env_ptr_field",
                    )
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            };
            let env_ptr = builder
                .build_load(i64_ty, env_ptr_field, "env_ptr")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            // Load fn_ptr from ARC_HEADER_SIZE (first field, stored as i64, convert to function pointer)
            let fn_ptr_field = unsafe {
                builder
                    .build_gep(
                        context.i8_type(),
                        closure_ptr,
                        &[i64_ty.const_int(ARC_HEADER_SIZE, false)],
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
            // String args must be expanded to (ptr, len) pairs.
            let mut call_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
                vec![env_ptr.into()];
            let mut llvm_param_types: Vec<BasicMetadataTypeEnum<'ctx>> = vec![i64_ty.into()];

            for (i, arg_reg) in args.iter().enumerate() {
                let is_string = i < cl_param_types.len()
                    && matches!(
                        types.resolve(cl_param_types[i]),
                        nudl_core::types::TypeKind::String
                    );
                if is_string {
                    let (s_ptr, s_len) = load_string_arg(
                        context,
                        builder,
                        reg_string_info,
                        string_constants,
                        str_ptr_allocas,
                        str_len_allocas,
                        arg_reg.0,
                    )?;
                    call_args.push(s_ptr.into());
                    call_args.push(s_len.into());
                    llvm_param_types.push(ptr_ty.into());
                    llvm_param_types.push(i64_ty.into());
                } else {
                    let val = load_i64(context, builder, register_allocas, arg_reg.0)?;
                    call_args.push(val.into());
                    llvm_param_types.push(i64_ty.into());
                }
            }

            // Check if closure returns a string
            let returns_string = matches!(
                types.resolve(*cl_ret_type),
                nudl_core::types::TypeKind::String
            );

            let fn_type = if returns_string {
                let str_ret_ty = context.struct_type(&[ptr_ty.into(), i64_ty.into()], false);
                str_ret_ty.fn_type(&llvm_param_types, false)
            } else {
                i64_ty.fn_type(&llvm_param_types, false)
            };

            // Indirect call through function pointer
            let call_result = builder
                .build_indirect_call(fn_type, fn_ptr, &call_args, "closure_call")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            if returns_string {
                if let Some(ret_val) = call_result.try_as_basic_value().basic() {
                    let struct_val = ret_val.into_struct_value();
                    let data_ptr = builder
                        .build_extract_value(struct_val, 0, "cl_str_ptr")
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                        .into_pointer_value();
                    let len_val = builder
                        .build_extract_value(struct_val, 1, "cl_str_len")
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?
                        .into_int_value();
                    store_string_result(
                        context,
                        builder,
                        str_ptr_allocas,
                        str_len_allocas,
                        register_allocas,
                        reg_string_info,
                        dst.0,
                        data_ptr,
                        len_val,
                    )?;
                }
            } else if let Some(ret_val) = call_result.try_as_basic_value().basic() {
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
            let array_push = module.get_function("__nudl_array_push").unwrap_or_else(|| {
                let fn_ty = context
                    .void_type()
                    .fn_type(&[ptr_ty.into(), i64_ty.into()], false);
                module.add_function(
                    "__nudl_array_push",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
            let arr_ptr = load_ptr(context, builder, register_allocas, arr_reg.0)?;

            // Check if the value being pushed is a string.
            // String literals don't have a heap pointer in register_allocas,
            // so we need to create a heap copy via __nudl_str_alloc.
            let val_is_string = reg_string_info.contains_key(&val_reg.0);
            let val = if val_is_string {
                let (s_ptr, s_len) = load_string_arg(
                    context,
                    builder,
                    reg_string_info,
                    string_constants,
                    str_ptr_allocas,
                    str_len_allocas,
                    val_reg.0,
                )?;
                let str_alloc = module.get_function("__nudl_str_alloc").unwrap_or_else(|| {
                    let fn_ty = ptr_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);
                    module.add_function(
                        "__nudl_str_alloc",
                        fn_ty,
                        Some(inkwell::module::Linkage::External),
                    )
                });
                let call_result = builder
                    .build_direct_call(str_alloc, &[s_ptr.into(), s_len.into()], "str_heap_copy")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                let heap_ptr = call_result
                    .try_as_basic_value()
                    .basic()
                    .expect("str_alloc returns ptr")
                    .into_pointer_value();
                builder
                    .build_ptr_to_int(heap_ptr, i64_ty, "str_heap_i64")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            } else {
                load_i64(context, builder, register_allocas, val_reg.0)?
            };
            builder
                .build_direct_call(array_push, &[arr_ptr.into(), val.into()], "")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }

        Instruction::DynArrayPop(dst, arr_reg) => {
            let i64_ty = context.i64_type();
            let ptr_ty = context.ptr_type(AddressSpace::default());
            let array_pop = module.get_function("__nudl_array_pop").unwrap_or_else(|| {
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

            // If element is a string, extract (ptr, len) from the heap string object.
            let dst_ty = func.register_types.get(dst.0 as usize).copied();
            let is_string = dst_ty.is_some_and(|ty| matches!(types.resolve(ty), TypeKind::String));
            if is_string {
                let heap_ptr = builder
                    .build_int_to_ptr(val, ptr_ty, "str_heap_ptr")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
                store_string_result(
                    context,
                    builder,
                    str_ptr_allocas,
                    str_len_allocas,
                    register_allocas,
                    reg_string_info,
                    dst.0,
                    data_ptr,
                    len_val,
                )?;
            }
        }

        Instruction::DynArrayLen(dst, arr_reg) => {
            let i64_ty = context.i64_type();
            let ptr_ty = context.ptr_type(AddressSpace::default());
            // module is passed as parameter
            let array_len = module.get_function("__nudl_array_len").unwrap_or_else(|| {
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
            let array_get = module.get_function("__nudl_array_get").unwrap_or_else(|| {
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

            // If the element type is string, the i64 value is a heap string pointer.
            // Extract (data_ptr, len) so string operations work correctly.
            let dst_ty = func.register_types.get(dst.0 as usize).copied();
            let is_string = dst_ty.is_some_and(|ty| matches!(types.resolve(ty), TypeKind::String));
            if is_string {
                let heap_ptr = builder
                    .build_int_to_ptr(val, ptr_ty, "str_heap_ptr")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                let (data_ptr, len_val) = extract_heap_string(context, builder, heap_ptr)?;
                store_string_result(
                    context,
                    builder,
                    str_ptr_allocas,
                    str_len_allocas,
                    register_allocas,
                    reg_string_info,
                    dst.0,
                    data_ptr,
                    len_val,
                )?;
            }
        }

        Instruction::DynArraySet(arr_reg, idx_reg, val_reg) => {
            let i64_ty = context.i64_type();
            let ptr_ty = context.ptr_type(AddressSpace::default());
            let array_set = module.get_function("__nudl_array_set").unwrap_or_else(|| {
                let fn_ty = context
                    .void_type()
                    .fn_type(&[ptr_ty.into(), i64_ty.into(), i64_ty.into()], false);
                module.add_function(
                    "__nudl_array_set",
                    fn_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });
            let arr_ptr = load_ptr(context, builder, register_allocas, arr_reg.0)?;
            let idx = load_i64(context, builder, register_allocas, idx_reg.0)?;

            // Check if the value being set is a string — same as DynArrayPush.
            // String literals need a heap copy via __nudl_str_alloc.
            let val_is_string = reg_string_info.contains_key(&val_reg.0);
            let val = if val_is_string {
                let (s_ptr, s_len) = load_string_arg(
                    context,
                    builder,
                    reg_string_info,
                    string_constants,
                    str_ptr_allocas,
                    str_len_allocas,
                    val_reg.0,
                )?;
                let str_alloc = module.get_function("__nudl_str_alloc").unwrap_or_else(|| {
                    let fn_ty = ptr_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);
                    module.add_function(
                        "__nudl_str_alloc",
                        fn_ty,
                        Some(inkwell::module::Linkage::External),
                    )
                });
                let call_result = builder
                    .build_direct_call(str_alloc, &[s_ptr.into(), s_len.into()], "str_heap_copy")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                let heap_ptr = call_result
                    .try_as_basic_value()
                    .basic()
                    .expect("str_alloc returns ptr")
                    .into_pointer_value();
                builder
                    .build_ptr_to_int(heap_ptr, i64_ty, "str_heap_i64")
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?
            } else {
                load_i64(context, builder, register_allocas, val_reg.0)?
            };
            builder
                .build_direct_call(
                    array_set,
                    &[arr_ptr.into(), idx.into(), val.into()],
                    "array_set",
                )
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }

        // ---- Map operations ----
        Instruction::MapAlloc(dst, _type_id) => {
            let i64_ty = context.i64_type();
            let ptr_ty = context.ptr_type(AddressSpace::default());
            // module is passed as parameter
            let map_alloc = module.get_function("__nudl_map_alloc").unwrap_or_else(|| {
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
            let map_insert = module.get_function("__nudl_map_insert").unwrap_or_else(|| {
                let fn_ty = context
                    .void_type()
                    .fn_type(&[ptr_ty.into(), i64_ty.into(), i64_ty.into()], false);
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
            let map_get = module.get_function("__nudl_map_get").unwrap_or_else(|| {
                let fn_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into(), ptr_ty.into()], false);
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
            let map_len = module.get_function("__nudl_map_len").unwrap_or_else(|| {
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
