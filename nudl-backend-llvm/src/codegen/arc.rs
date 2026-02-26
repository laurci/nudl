use super::*;

/// Build LLVM param types for a function based on its param layout.
pub(super) fn build_llvm_param_types<'ctx>(
    func: &Function,
    types: &TypeInterner,
    context: &'ctx Context,
) -> Vec<BasicMetadataTypeEnum<'ctx>> {
    let mut params = Vec::new();
    for (_name, type_id) in &func.params {
        let kind = types.resolve(*type_id);
        match kind {
            TypeKind::String => {
                params.push(context.ptr_type(AddressSpace::default()).into());
                params.push(context.i64_type().into());
            }
            TypeKind::Primitive(p) if p.is_float() => {
                params.push(context.f64_type().into());
            }
            _ => {
                params.push(context.i64_type().into());
            }
        }
    }
    params
}

/// Check if a register holds a float type.
pub(super) fn is_float_register(func: &Function, reg: u32, types: &TypeInterner) -> bool {
    if let Some(type_id) = func.register_types.get(reg as usize) {
        matches!(types.resolve(*type_id), TypeKind::Primitive(p) if p.is_float())
    } else {
        false
    }
}

/// Emit ARC intrinsic declarations and inline functions into the module.
pub(super) fn emit_arc_intrinsics<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
) -> Result<ArcIntrinsics<'ctx>, BackendError> {
    let ptr_ty = context.ptr_type(AddressSpace::default());
    let i32_ty = context.i32_type();
    let i64_ty = context.i64_type();
    let void_ty = context.void_type();

    // ARC header struct type: { i32 strong, i32 weak, i32 type_tag, i32 padding }
    let header_ty = context.struct_type(
        &[i32_ty.into(), i32_ty.into(), i32_ty.into(), i32_ty.into()],
        false,
    );

    // --- Declare external C runtime symbols ---

    // __nudl_arc_alloc(u64 total_size, u32 type_tag) -> ptr
    let alloc_ty = ptr_ty.fn_type(&[i64_ty.into(), i32_ty.into()], false);
    let arc_alloc = module.add_function(
        "__nudl_arc_alloc",
        alloc_ty,
        Some(inkwell::module::Linkage::External),
    );

    // __nudl_arc_release_slow(ptr, void(*drop_fn)(ptr)) -> void
    let release_slow_ty = void_ty.fn_type(&[ptr_ty.into(), ptr_ty.into()], false);
    let arc_release_slow = module.add_function(
        "__nudl_arc_release_slow",
        release_slow_ty,
        Some(inkwell::module::Linkage::External),
    );

    // __nudl_arc_overflow_abort() -> void [noreturn]
    let abort_ty = void_ty.fn_type(&[], false);
    let arc_overflow_abort = module.add_function(
        "__nudl_arc_overflow_abort",
        abort_ty,
        Some(inkwell::module::Linkage::External),
    );
    arc_overflow_abort.add_attribute(
        inkwell::attributes::AttributeLoc::Function,
        context.create_enum_attribute(
            inkwell::attributes::Attribute::get_named_enum_kind_id("noreturn"),
            0,
        ),
    );

    // --- Emit __nudl_drop_noop(ptr) -> void ---
    let noop_ty = void_ty.fn_type(&[ptr_ty.into()], false);
    let drop_noop = module.add_function(
        "__nudl_drop_noop",
        noop_ty,
        Some(inkwell::module::Linkage::Internal),
    );
    {
        let bb = context.append_basic_block(drop_noop, "entry");
        let b = context.create_builder();
        b.position_at_end(bb);
        b.build_return(None)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    }

    // --- Emit __nudl_arc_retain(ptr) -> void  [alwaysinline] ---
    //
    // if ptr == null: return
    // strong = load i32 from ptr+0
    // if strong == UINT32_MAX: call overflow_abort
    // strong++
    // store strong to ptr+0
    // return
    let retain_ty = void_ty.fn_type(&[ptr_ty.into()], false);
    let arc_retain = module.add_function(
        "__nudl_arc_retain",
        retain_ty,
        Some(inkwell::module::Linkage::Internal),
    );
    arc_retain.add_attribute(
        inkwell::attributes::AttributeLoc::Function,
        context.create_enum_attribute(
            inkwell::attributes::Attribute::get_named_enum_kind_id("alwaysinline"),
            0,
        ),
    );
    {
        let b = context.create_builder();
        let entry = context.append_basic_block(arc_retain, "entry");
        let do_retain = context.append_basic_block(arc_retain, "do_retain");
        let overflow = context.append_basic_block(arc_retain, "overflow");
        let inc = context.append_basic_block(arc_retain, "inc");
        let done = context.append_basic_block(arc_retain, "done");

        // entry: null check
        b.position_at_end(entry);
        let obj_ptr = arc_retain.get_nth_param(0).unwrap().into_pointer_value();
        let is_null = b
            .build_is_null(obj_ptr, "is_null")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_conditional_branch(is_null, done, do_retain)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // do_retain: load strong_count
        b.position_at_end(do_retain);
        let strong_ptr = b
            .build_struct_gep(header_ty, obj_ptr, 0, "strong_ptr")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let strong = b
            .build_load(i32_ty, strong_ptr, "strong")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
            .into_int_value();
        let max_val = i32_ty.const_int(u32::MAX as u64, false);
        let is_max = b
            .build_int_compare(inkwell::IntPredicate::EQ, strong, max_val, "is_max")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_conditional_branch(is_max, overflow, inc)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // overflow: abort
        b.position_at_end(overflow);
        b.build_direct_call(arc_overflow_abort, &[], "")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_unreachable()
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // inc: increment and store
        b.position_at_end(inc);
        let one = i32_ty.const_int(1, false);
        let new_strong = b
            .build_int_add(strong, one, "new_strong")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_store(strong_ptr, new_strong)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_unconditional_branch(done)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // done
        b.position_at_end(done);
        b.build_return(None)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    }

    // --- Emit __nudl_arc_release(ptr, drop_fn) -> void  [alwaysinline] ---
    //
    // if ptr == null: return
    // strong = load i32 from ptr+0
    // strong--
    // store strong to ptr+0
    // if strong == 0: call __nudl_arc_release_slow(ptr, drop_fn)
    // return
    let release_ty = void_ty.fn_type(&[ptr_ty.into(), ptr_ty.into()], false);
    let arc_release = module.add_function(
        "__nudl_arc_release",
        release_ty,
        Some(inkwell::module::Linkage::Internal),
    );
    arc_release.add_attribute(
        inkwell::attributes::AttributeLoc::Function,
        context.create_enum_attribute(
            inkwell::attributes::Attribute::get_named_enum_kind_id("alwaysinline"),
            0,
        ),
    );
    {
        let b = context.create_builder();
        let entry = context.append_basic_block(arc_release, "entry");
        let do_release = context.append_basic_block(arc_release, "do_release");
        let check_zero = context.append_basic_block(arc_release, "check_zero");
        let call_slow = context.append_basic_block(arc_release, "call_slow");
        let done = context.append_basic_block(arc_release, "done");

        // entry: null check
        b.position_at_end(entry);
        let obj_ptr = arc_release.get_nth_param(0).unwrap().into_pointer_value();
        let drop_fn_val = arc_release.get_nth_param(1).unwrap().into_pointer_value();
        let is_null = b
            .build_is_null(obj_ptr, "is_null")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_conditional_branch(is_null, done, do_release)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // do_release: decrement strong
        b.position_at_end(do_release);
        let strong_ptr = b
            .build_struct_gep(header_ty, obj_ptr, 0, "strong_ptr")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let strong = b
            .build_load(i32_ty, strong_ptr, "strong")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?
            .into_int_value();
        let one = i32_ty.const_int(1, false);
        let new_strong = b
            .build_int_sub(strong, one, "new_strong")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_store(strong_ptr, new_strong)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_unconditional_branch(check_zero)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // check_zero: if new_strong == 0, call slow path
        b.position_at_end(check_zero);
        let zero = i32_ty.const_zero();
        let is_zero = b
            .build_int_compare(inkwell::IntPredicate::EQ, new_strong, zero, "is_zero")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_conditional_branch(is_zero, call_slow, done)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // call_slow: call __nudl_arc_release_slow(ptr, drop_fn)
        b.position_at_end(call_slow);
        b.build_direct_call(arc_release_slow, &[obj_ptr.into(), drop_fn_val.into()], "")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_unconditional_branch(done)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // done
        b.position_at_end(done);
        b.build_return(None)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    }

    Ok(ArcIntrinsics {
        arc_alloc,
        arc_release_slow,
        arc_overflow_abort,
        arc_retain,
        arc_release,
        drop_noop,
    })
}
