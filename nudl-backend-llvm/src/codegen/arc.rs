use super::*;

/// Map a nudl PrimitiveType to the corresponding C-sized LLVM type.
pub(super) fn primitive_to_c_type<'ctx>(
    p: &nudl_core::types::PrimitiveType,
    context: &'ctx Context,
) -> inkwell::types::BasicTypeEnum<'ctx> {
    use nudl_core::types::PrimitiveType;
    match p {
        PrimitiveType::I8 | PrimitiveType::U8 | PrimitiveType::Bool => context.i8_type().into(),
        PrimitiveType::I16 | PrimitiveType::U16 => context.i16_type().into(),
        PrimitiveType::I32 | PrimitiveType::U32 | PrimitiveType::Char => context.i32_type().into(),
        PrimitiveType::I64 | PrimitiveType::U64 | PrimitiveType::Unit => context.i64_type().into(),
        PrimitiveType::F32 => context.f32_type().into(),
        PrimitiveType::F64 => context.f64_type().into(),
    }
}

/// Build a C-compatible LLVM struct type from extern struct fields.
pub(super) fn build_c_struct_type<'ctx>(
    context: &'ctx Context,
    types: &TypeInterner,
    fields: &[(String, nudl_core::types::TypeId)],
) -> inkwell::types::StructType<'ctx> {
    let field_types: Vec<inkwell::types::BasicTypeEnum<'ctx>> = fields
        .iter()
        .map(|(_, fty)| match types.resolve(*fty) {
            TypeKind::Primitive(p) => primitive_to_c_type(p, context),
            TypeKind::Struct {
                fields: inner_fields,
                is_extern: true,
                ..
            } => build_c_struct_type(context, types, inner_fields).into(),
            TypeKind::FixedArray { element, length } => {
                let elem_kind = types.resolve(*element);
                let c_elem = match elem_kind {
                    TypeKind::Primitive(p) => primitive_to_c_type(p, context),
                    _ => context.i64_type().into(),
                };
                match c_elem {
                    inkwell::types::BasicTypeEnum::IntType(t) => {
                        t.array_type(*length as u32).into()
                    }
                    inkwell::types::BasicTypeEnum::FloatType(t) => {
                        t.array_type(*length as u32).into()
                    }
                    _ => context.i64_type().array_type(*length as u32).into(),
                }
            }
            _ => context.i64_type().into(),
        })
        .collect();
    context.struct_type(&field_types, false)
}

/// Compute the byte size and alignment of a nudl type when laid out as a C type.
fn c_type_size_align(types: &TypeInterner, type_id: nudl_core::types::TypeId) -> (u64, u64) {
    use nudl_core::types::PrimitiveType;
    match types.resolve(type_id) {
        TypeKind::Primitive(p) => match p {
            PrimitiveType::I8 | PrimitiveType::U8 | PrimitiveType::Bool => (1, 1),
            PrimitiveType::I16 | PrimitiveType::U16 => (2, 2),
            PrimitiveType::I32 | PrimitiveType::U32 | PrimitiveType::Char => (4, 4),
            PrimitiveType::I64 | PrimitiveType::U64 => (8, 8),
            PrimitiveType::F32 => (4, 4),
            PrimitiveType::F64 => (8, 8),
            PrimitiveType::Unit => (0, 1),
        },
        TypeKind::Struct {
            fields,
            is_extern: true,
            ..
        } => c_struct_size_align(types, fields),
        TypeKind::FixedArray { element, length } => {
            let (elem_sz, elem_align) = c_type_size_align(types, *element);
            (elem_sz * (*length as u64), elem_align)
        }
        _ => (8, 8),
    }
}

/// Compute the byte size and alignment of a C struct from its fields.
fn c_struct_size_align(
    types: &TypeInterner,
    fields: &[(String, nudl_core::types::TypeId)],
) -> (u64, u64) {
    let mut offset = 0u64;
    let mut max_align = 1u64;
    for (_, fty) in fields {
        let (sz, align) = c_type_size_align(types, *fty);
        offset = (offset + align - 1) & !(align - 1);
        offset += sz;
        max_align = max_align.max(align);
    }
    let total = (offset + max_align - 1) & !(max_align - 1);
    (total, max_align)
}

/// Get the ABI-coerced integer type for passing an extern struct by value.
/// On ARM64/x86_64, small structs (≤8 bytes) are coerced to an integer of the
/// same size for register passing. This matches Clang's ABI lowering.
pub(super) fn extern_struct_abi_type<'ctx>(
    context: &'ctx Context,
    types: &TypeInterner,
    fields: &[(String, nudl_core::types::TypeId)],
) -> inkwell::types::BasicTypeEnum<'ctx> {
    let (size, _) = c_struct_size_align(types, fields);
    match size {
        0 | 1 => context.i8_type().into(),
        2 => context.i16_type().into(),
        3..=4 => context.i32_type().into(),
        5..=8 => context.i64_type().into(),
        // For structs > 8 bytes, fall back to the LLVM struct type.
        // This handles up to 16-byte structs passed in two registers.
        _ => build_c_struct_type(context, types, fields).into(),
    }
}

/// Get the C-compatible LLVM type for a TypeId (handles extern struct, FixedArray, primitives).
/// For extern structs, returns the ABI-coerced integer type for correct parameter/return passing.
pub(super) fn type_to_c_llvm_type<'ctx>(
    context: &'ctx Context,
    types: &TypeInterner,
    type_id: nudl_core::types::TypeId,
) -> Option<inkwell::types::BasicTypeEnum<'ctx>> {
    match types.resolve(type_id) {
        TypeKind::Struct {
            fields,
            is_extern: true,
            ..
        } => Some(extern_struct_abi_type(context, types, fields)),
        TypeKind::FixedArray { element, length } => {
            let elem_kind = types.resolve(*element);
            let c_elem = match elem_kind {
                TypeKind::Primitive(p) => primitive_to_c_type(p, context),
                _ => context.i64_type().into(),
            };
            Some(match c_elem {
                inkwell::types::BasicTypeEnum::IntType(t) => t.array_type(*length as u32).into(),
                inkwell::types::BasicTypeEnum::FloatType(t) => t.array_type(*length as u32).into(),
                _ => context.i64_type().array_type(*length as u32).into(),
            })
        }
        _ => None,
    }
}

/// Build LLVM param types for a function based on its param layout.
/// When `is_extern` is true, FixedArray and extern struct params use C-compatible types
/// instead of i64 pointers to ARC objects.
pub(super) fn build_llvm_param_types<'ctx>(
    func: &Function,
    types: &TypeInterner,
    context: &'ctx Context,
    is_extern: bool,
) -> Vec<BasicMetadataTypeEnum<'ctx>> {
    let mut params = Vec::new();
    for (_name, type_id) in &func.params {
        let kind = types.resolve(*type_id);
        match kind {
            TypeKind::String => {
                if is_extern {
                    // Extern: pass as raw ptr (caller must use cstr())
                    params.push(context.ptr_type(AddressSpace::default()).into());
                } else {
                    params.push(context.ptr_type(AddressSpace::default()).into());
                    params.push(context.i64_type().into());
                }
            }
            TypeKind::Primitive(p) if p.is_float() => {
                params.push(context.f64_type().into());
            }
            TypeKind::Struct {
                fields,
                is_extern: true,
                ..
            } if is_extern => {
                // Extern struct param: use ABI-coerced integer type
                let abi_ty = extern_struct_abi_type(context, types, fields);
                params.push(abi_ty.into());
            }
            TypeKind::FixedArray { element, length } if is_extern => {
                // Extern: use C-compatible packed array type [N x elem_type]
                let elem_kind = types.resolve(*element);
                let llvm_elem_ty = match elem_kind {
                    TypeKind::Primitive(p) => primitive_to_c_type(p, context),
                    _ => context.i64_type().into(),
                };
                let array_ty = match llvm_elem_ty {
                    inkwell::types::BasicTypeEnum::IntType(t) => {
                        t.array_type(*length as u32).into()
                    }
                    inkwell::types::BasicTypeEnum::FloatType(t) => {
                        t.array_type(*length as u32).into()
                    }
                    _ => context.i64_type().into(),
                };
                params.push(array_ty);
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

    // __nudl_arc_release_slow_nodrop(ptr) -> void  (fast path: no drop handler)
    let release_slow_nodrop_ty = void_ty.fn_type(&[ptr_ty.into()], false);
    let arc_release_slow_nodrop = module.add_function(
        "__nudl_arc_release_slow_nodrop",
        release_slow_nodrop_ty,
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
    // if strong == 0:
    //   if drop_fn == null: call __nudl_arc_release_slow_nodrop(ptr)
    //   else: call __nudl_arc_release_slow(ptr, drop_fn)
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
        let check_drop = context.append_basic_block(arc_release, "check_drop");
        let call_slow = context.append_basic_block(arc_release, "call_slow");
        let call_slow_nodrop = context.append_basic_block(arc_release, "call_slow_nodrop");
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

        // check_zero: if new_strong == 0, check drop handler
        b.position_at_end(check_zero);
        let zero = i32_ty.const_zero();
        let is_zero = b
            .build_int_compare(inkwell::IntPredicate::EQ, new_strong, zero, "is_zero")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_conditional_branch(is_zero, check_drop, done)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // check_drop: if drop_fn is null, use fast nodrop path
        b.position_at_end(check_drop);
        let drop_is_null = b
            .build_is_null(drop_fn_val, "drop_is_null")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_conditional_branch(drop_is_null, call_slow_nodrop, call_slow)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;

        // call_slow_nodrop: call __nudl_arc_release_slow_nodrop(ptr)
        b.position_at_end(call_slow_nodrop);
        b.build_direct_call(arc_release_slow_nodrop, &[obj_ptr.into()], "")
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        b.build_unconditional_branch(done)
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
        arc_release_slow_nodrop,
        arc_overflow_abort,
        arc_retain,
        arc_release,
    })
}

/// Declare string builtin runtime functions as external symbols.
pub(super) fn declare_string_builtins<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
) -> StringBuiltins<'ctx> {
    let ptr_ty = context.ptr_type(AddressSpace::default());
    let i64_ty = context.i64_type();
    let f64_ty = context.f64_type();
    let ext = Some(inkwell::module::Linkage::External);

    // __nudl_str_concat(ptr, i64, ptr, i64) -> ptr
    let concat_ty = ptr_ty.fn_type(
        &[ptr_ty.into(), i64_ty.into(), ptr_ty.into(), i64_ty.into()],
        false,
    );
    let str_concat = module.add_function("__nudl_str_concat", concat_ty, ext);

    // __nudl_i64_to_str(i64) -> ptr
    let i64_to_str_ty = ptr_ty.fn_type(&[i64_ty.into()], false);
    let i64_to_str = module.add_function("__nudl_i64_to_str", i64_to_str_ty, ext);

    // __nudl_f64_to_str(f64) -> ptr
    let f64_to_str_ty = ptr_ty.fn_type(&[f64_ty.into()], false);
    let f64_to_str = module.add_function("__nudl_f64_to_str", f64_to_str_ty, ext);

    // __nudl_bool_to_str(i64) -> ptr
    let bool_to_str_ty = ptr_ty.fn_type(&[i64_ty.into()], false);
    let bool_to_str = module.add_function("__nudl_bool_to_str", bool_to_str_ty, ext);

    // __nudl_char_to_str(i64) -> ptr
    let char_to_str_ty = ptr_ty.fn_type(&[i64_ty.into()], false);
    let char_to_str = module.add_function("__nudl_char_to_str", char_to_str_ty, ext);

    // String operation builtins

    // __nudl_str_substr(ptr, i64, i64, i64) -> ptr  (str_ptr, str_len, start, end)
    let substr_ty = ptr_ty.fn_type(
        &[ptr_ty.into(), i64_ty.into(), i64_ty.into(), i64_ty.into()],
        false,
    );
    let str_substr = module.add_function("__nudl_str_substr", substr_ty, ext);

    // __nudl_str_indexof(ptr, i64, ptr, i64) -> i64  (h_ptr, h_len, n_ptr, n_len)
    let indexof_ty = i64_ty.fn_type(
        &[ptr_ty.into(), i64_ty.into(), ptr_ty.into(), i64_ty.into()],
        false,
    );
    let str_indexof = module.add_function("__nudl_str_indexof", indexof_ty, ext);

    // __nudl_str_trim(ptr, i64) -> ptr
    let trim_ty = ptr_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);
    let str_trim = module.add_function("__nudl_str_trim", trim_ty, ext);

    // __nudl_str_contains(ptr, i64, ptr, i64) -> i64
    let contains_ty = i64_ty.fn_type(
        &[ptr_ty.into(), i64_ty.into(), ptr_ty.into(), i64_ty.into()],
        false,
    );
    let str_contains = module.add_function("__nudl_str_contains", contains_ty, ext);

    // __nudl_str_starts_with(ptr, i64, ptr, i64) -> i64
    let starts_with_ty = i64_ty.fn_type(
        &[ptr_ty.into(), i64_ty.into(), ptr_ty.into(), i64_ty.into()],
        false,
    );
    let str_starts_with = module.add_function("__nudl_str_starts_with", starts_with_ty, ext);

    // __nudl_str_ends_with(ptr, i64, ptr, i64) -> i64
    let ends_with_ty = i64_ty.fn_type(
        &[ptr_ty.into(), i64_ty.into(), ptr_ty.into(), i64_ty.into()],
        false,
    );
    let str_ends_with = module.add_function("__nudl_str_ends_with", ends_with_ty, ext);

    // __nudl_str_to_upper(ptr, i64) -> ptr
    let to_upper_ty = ptr_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);
    let str_to_upper = module.add_function("__nudl_str_to_upper", to_upper_ty, ext);

    // __nudl_str_to_lower(ptr, i64) -> ptr
    let to_lower_ty = ptr_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);
    let str_to_lower = module.add_function("__nudl_str_to_lower", to_lower_ty, ext);

    // __nudl_str_replace(ptr, i64, ptr, i64, ptr, i64) -> ptr
    let replace_ty = ptr_ty.fn_type(
        &[
            ptr_ty.into(),
            i64_ty.into(),
            ptr_ty.into(),
            i64_ty.into(),
            ptr_ty.into(),
            i64_ty.into(),
        ],
        false,
    );
    let str_replace = module.add_function("__nudl_str_replace", replace_ty, ext);

    // __nudl_str_repeat(ptr, i64, i64) -> ptr
    let repeat_ty = ptr_ty.fn_type(&[ptr_ty.into(), i64_ty.into(), i64_ty.into()], false);
    let str_repeat = module.add_function("__nudl_str_repeat", repeat_ty, ext);

    StringBuiltins {
        str_concat,
        i64_to_str,
        f64_to_str,
        bool_to_str,
        char_to_str,
        str_substr,
        str_indexof,
        str_trim,
        str_contains,
        str_starts_with,
        str_ends_with,
        str_to_upper,
        str_to_lower,
        str_replace,
        str_repeat,
    }
}
