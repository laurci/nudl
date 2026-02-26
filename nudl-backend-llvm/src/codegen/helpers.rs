use super::*;

pub(super) fn store<'ctx, V: inkwell::values::BasicValue<'ctx>>(
    builder: &Builder<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    reg: u32,
    val: V,
) -> Result<(), BackendError> {
    builder
        .build_store(register_allocas[&reg], val)
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    Ok(())
}

pub(super) fn load_i64<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    ssa_reg: u32,
) -> Result<inkwell::values::IntValue<'ctx>, BackendError> {
    let alloca = register_allocas[&ssa_reg];
    let val = builder
        .build_load(context.i64_type(), alloca, &format!("load_r{}", ssa_reg))
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    Ok(val.into_int_value())
}

/// Load a register as a pointer (registers store pointers as i64, so we inttoptr).
pub(super) fn load_ptr<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    ssa_reg: u32,
) -> Result<PointerValue<'ctx>, BackendError> {
    let i64_val = load_i64(context, builder, register_allocas, ssa_reg)?;
    let ptr = builder
        .build_int_to_ptr(
            i64_val,
            context.ptr_type(AddressSpace::default()),
            &format!("i64_to_ptr_r{}", ssa_reg),
        )
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    Ok(ptr)
}

pub(super) fn load_f64<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    ssa_reg: u32,
) -> Result<inkwell::values::FloatValue<'ctx>, BackendError> {
    let alloca = register_allocas[&ssa_reg];
    let val = builder
        .build_load(context.f64_type(), alloca, &format!("load_f_r{}", ssa_reg))
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    Ok(val.into_float_value())
}

pub(super) fn load_binop<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    lhs: u32,
    rhs: u32,
) -> Result<
    (
        inkwell::values::IntValue<'ctx>,
        inkwell::values::IntValue<'ctx>,
    ),
    BackendError,
> {
    let lv = load_i64(context, builder, register_allocas, lhs)?;
    let rv = load_i64(context, builder, register_allocas, rhs)?;
    Ok((lv, rv))
}

pub(super) fn load_float_binop<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    lhs: u32,
    rhs: u32,
) -> Result<
    (
        inkwell::values::FloatValue<'ctx>,
        inkwell::values::FloatValue<'ctx>,
    ),
    BackendError,
> {
    let lv = load_f64(context, builder, register_allocas, lhs)?;
    let rv = load_f64(context, builder, register_allocas, rhs)?;
    Ok((lv, rv))
}

pub(super) fn emit_fcmp<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    dst: u32,
    lhs: u32,
    rhs: u32,
    pred: FloatPredicate,
) -> Result<(), BackendError> {
    let (lv, rv) = load_float_binop(context, builder, register_allocas, lhs, rhs)?;
    let cmp = builder
        .build_float_compare(pred, lv, rv, "fcmp")
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    let result = builder
        .build_int_z_extend(cmp, context.i64_type(), "zext")
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    store(builder, register_allocas, dst, result)?;
    Ok(())
}

pub(super) fn emit_icmp<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    dst: u32,
    lhs: u32,
    rhs: u32,
    pred: inkwell::IntPredicate,
) -> Result<(), BackendError> {
    let (lv, rv) = load_binop(context, builder, register_allocas, lhs, rhs)?;
    let cmp = builder
        .build_int_compare(pred, lv, rv, "cmp")
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    let result = builder
        .build_int_z_extend(cmp, context.i64_type(), "zext")
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    store(builder, register_allocas, dst, result)?;
    Ok(())
}

pub(super) fn gep_string_ptr<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    global: &GlobalValue<'ctx>,
    len: u64,
) -> Result<PointerValue<'ctx>, BackendError> {
    let zero = context.i32_type().const_zero();
    let ptr = unsafe {
        builder
            .build_gep(
                context.i8_type().array_type(len as u32),
                global.as_pointer_value(),
                &[zero, zero],
                "str_ptr",
            )
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
    };
    Ok(ptr)
}
