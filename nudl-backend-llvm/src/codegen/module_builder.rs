use super::*;

pub(super) fn build_module<'ctx>(
    context: &'ctx Context,
    program: &Program,
    optimized: bool,
) -> Result<Module<'ctx>, BackendError> {
    let module = context.create_module("nudl");
    let builder = context.create_builder();

    let types = &program.types;
    let mut function_map: HashMap<u32, FunctionValue<'ctx>> = HashMap::new();
    let mut string_constants: Vec<(GlobalValue<'ctx>, u64)> = Vec::new();
    let mut reg_string_info: HashMap<u32, RegStringInfo> = HashMap::new();

    // Set up debug info
    let source_map = program.source_map.as_ref();
    let (src_filename, src_directory) = if let Some(sm) = source_map {
        let file = sm.get_file(nudl_core::span::FileId(0));
        let path = &file.path;
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown.nudl".to_string());
        let directory = path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".to_string());
        (filename, directory)
    } else {
        ("unknown.nudl".to_string(), ".".to_string())
    };

    let debug_metadata_version = context.i32_type().const_int(3, false);
    module.add_basic_value_flag(
        "Debug Info Version",
        inkwell::module::FlagBehavior::Warning,
        debug_metadata_version,
    );

    let emission_kind = if optimized {
        DWARFEmissionKind::None
    } else {
        DWARFEmissionKind::Full
    };

    let (dibuilder, compile_unit) = module.create_debug_info_builder(
        true,
        DWARFSourceLanguage::C,
        &src_filename,
        &src_directory,
        "nudl",
        optimized,
        "",
        0,
        "",
        emission_kind,
        0,
        false,
        false,
        "",
        "",
    );

    // Emit ARC intrinsics (inline retain/release + extern declarations)
    let arc = emit_arc_intrinsics(context, &module)?;

    // Declare string builtin runtime functions
    let string_builtins = declare_string_builtins(context, &module);

    // Emit string constants as globals (null-terminated for C FFI compatibility)
    for (i, s) in program.string_constants.iter().enumerate() {
        let bytes = s.as_bytes();
        let global = context.const_string(bytes, true); // adds null terminator
        let global_val = module.add_global(
            context.i8_type().array_type(bytes.len() as u32 + 1),
            Some(AddressSpace::default()),
            &format!(".str.{}", i),
        );
        global_val.set_initializer(&global);
        global_val.set_constant(true);
        global_val.set_unnamed_addr(true);
        global_val.set_linkage(inkwell::module::Linkage::Private);
        string_constants.push((global_val, bytes.len() as u64));
    }

    // Declare all functions
    for func in &program.functions {
        let is_entry = program.entry_function == Some(func.id);

        let ret_is_float = matches!(
            types.resolve(func.return_type),
            TypeKind::Primitive(p) if p.is_float()
        );
        let ret_is_string = matches!(types.resolve(func.return_type), TypeKind::String);

        if func.is_extern {
            let param_types = build_llvm_param_types(func, types, context, true);
            let fn_type = if ret_is_string {
                // Extern string-returning functions return a raw ptr (ARC heap string)
                context
                    .ptr_type(AddressSpace::default())
                    .fn_type(&param_types, false)
            } else if ret_is_float {
                context.f64_type().fn_type(&param_types, false)
            } else if let Some(c_ret_ty) = type_to_c_llvm_type(context, types, func.return_type) {
                // Extern struct or fixed array return type
                c_ret_ty.fn_type(&param_types, false)
            } else {
                context.i64_type().fn_type(&param_types, false)
            };
            let ext_name = func.extern_symbol.as_deref().unwrap_or("unknown_extern");
            let fn_val =
                module.add_function(ext_name, fn_type, Some(inkwell::module::Linkage::External));
            function_map.insert(func.id.0, fn_val);
        } else if is_entry {
            let fn_type = context.i32_type().fn_type(&[], false);
            let fn_val = module.add_function("main", fn_type, None);
            function_map.insert(func.id.0, fn_val);
        } else {
            let param_types = build_llvm_param_types(func, types, context, false);
            let fn_type = if ret_is_string {
                // String-returning functions return {ptr, i64} struct
                let ptr_ty = context.ptr_type(AddressSpace::default());
                let i64_ty = context.i64_type();
                let struct_ty = context.struct_type(&[ptr_ty.into(), i64_ty.into()], false);
                struct_ty.fn_type(&param_types, false)
            } else if ret_is_float {
                context.f64_type().fn_type(&param_types, false)
            } else {
                context.i64_type().fn_type(&param_types, false)
            };
            let func_name = program.interner.resolve(func.name);
            let fn_val = module.add_function(&format!("__func_{}", func_name), fn_type, None);
            function_map.insert(func.id.0, fn_val);
        }
    }

    // Generate per-struct drop functions.
    // In debug builds, they log "dropping <Name>\n" via write(1, ...).
    // In release builds, they are empty stubs (no I/O overhead).
    let mut drop_fns: HashMap<nudl_core::types::TypeId, FunctionValue<'ctx>> = HashMap::new();
    {
        let ptr_ty = context.ptr_type(AddressSpace::default());
        let void_ty = context.void_type();
        let drop_fn_ty = void_ty.fn_type(&[ptr_ty.into()], false);

        // Find the extern write function in the module (only needed for debug drops)
        let write_fn = if !optimized {
            module.get_function("write")
        } else {
            None
        };

        for (type_id, kind) in types.iter_types() {
            if let TypeKind::Struct {
                name, is_extern, ..
            } = kind
            {
                // Skip drop function for extern structs (no ARC management)
                if *is_extern {
                    continue;
                }
                let drop_fn = module.add_function(
                    &format!("__nudl_drop_{}", name),
                    drop_fn_ty,
                    Some(inkwell::module::Linkage::Internal),
                );
                let bb = context.append_basic_block(drop_fn, "entry");
                let b = context.create_builder();
                b.position_at_end(bb);

                // Only emit drop logging in debug builds
                if let Some(write_fn) = write_fn {
                    let msg = format!("dropping {}\n", name);
                    let msg_bytes = msg.as_bytes();

                    let global = context.const_string(msg_bytes, false);
                    let global_val = module.add_global(
                        context.i8_type().array_type(msg_bytes.len() as u32),
                        Some(AddressSpace::default()),
                        &format!(".drop_msg.{}", name),
                    );
                    global_val.set_initializer(&global);
                    global_val.set_constant(true);
                    global_val.set_unnamed_addr(true);
                    global_val.set_linkage(inkwell::module::Linkage::Private);

                    // Note: extern write has LLVM signature (i64, i64, i64) -> i64
                    let i64_ty = context.i64_type();
                    let fd = i64_ty.const_int(1, false);
                    let msg_ptr_raw = b
                        .build_ptr_to_int(global_val.as_pointer_value(), i64_ty, "msg_ptr_int")
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                    let msg_len = i64_ty.const_int(msg_bytes.len() as u64, false);
                    b.build_direct_call(
                        write_fn,
                        &[fd.into(), msg_ptr_raw.into(), msg_len.into()],
                        "",
                    )
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                }

                b.build_return(None)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;

                drop_fns.insert(type_id, drop_fn);
            }
        }
    }

    // Generate drop functions for DynArray types that free the data buffer
    // and release reference-typed elements via __nudl_array_destroy.
    {
        let ptr_ty = context.ptr_type(AddressSpace::default());
        let void_ty = context.void_type();
        let drop_fn_ty = void_ty.fn_type(&[ptr_ty.into()], false);

        // Declare __nudl_array_destroy(ptr, ptr) -> void
        let array_destroy_ty = void_ty.fn_type(&[ptr_ty.into(), ptr_ty.into()], false);
        let array_destroy = module
            .get_function("__nudl_array_destroy")
            .unwrap_or_else(|| {
                module.add_function(
                    "__nudl_array_destroy",
                    array_destroy_ty,
                    Some(inkwell::module::Linkage::External),
                )
            });

        // Collect DynArray types first to avoid borrow issues
        let dyn_array_types: Vec<(nudl_core::types::TypeId, nudl_core::types::TypeId)> = types
            .iter_types()
            .filter_map(|(type_id, kind)| {
                if let TypeKind::DynamicArray { element } = kind {
                    Some((type_id, *element))
                } else {
                    None
                }
            })
            .collect();

        for (type_id, element) in &dyn_array_types {
            let drop_fn = module.add_function(
                &format!("__nudl_drop_arr_{}", type_id.0),
                drop_fn_ty,
                Some(inkwell::module::Linkage::Internal),
            );
            let bb = context.append_basic_block(drop_fn, "entry");
            let b = context.create_builder();
            b.position_at_end(bb);

            let arr_ptr = drop_fn.get_nth_param(0).unwrap().into_pointer_value();

            // Determine element drop function:
            // - Reference-typed element with existing drop_fn → use that
            // - Reference-typed element without specific drop → use drop_noop
            // - Value-typed element → NULL (skip element release)
            let elem_drop: inkwell::values::PointerValue = if types.is_reference_type(*element) {
                if let Some(dfn) = drop_fns.get(element) {
                    dfn.as_global_value().as_pointer_value()
                } else {
                    arc.drop_noop.as_global_value().as_pointer_value()
                }
            } else {
                ptr_ty.const_null()
            };

            b.build_direct_call(array_destroy, &[arr_ptr.into(), elem_drop.into()], "")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            b.build_return(None)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            drop_fns.insert(*type_id, drop_fn);
        }
    }

    // Build per-file DIFile map so functions from different source files
    // get correct file references in DWARF debug info.
    let mut di_files: HashMap<u32, DIFile<'ctx>> = HashMap::new();
    if let Some(sm) = source_map {
        for i in 0..sm.file_count() {
            let file = sm.get_file(nudl_core::span::FileId(i as u32));
            let filename = file
                .path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "unknown.nudl".to_string());
            let directory = file
                .path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| ".".to_string());
            di_files.insert(i as u32, dibuilder.create_file(&filename, &directory));
        }
    }

    // Emit function bodies
    for func in &program.functions {
        if func.is_extern {
            continue;
        }
        let is_entry = program.entry_function == Some(func.id);
        emit_function(
            context,
            &builder,
            &module,
            program,
            func,
            &function_map,
            &string_constants,
            &mut reg_string_info,
            types,
            &arc,
            &string_builtins,
            &drop_fns,
            is_entry,
            optimized,
            &dibuilder,
            &compile_unit,
            source_map,
            &di_files,
        )?;
    }

    dibuilder.finalize();

    if let Err(msg) = module.verify() {
        return Err(BackendError::LlvmError(format!(
            "Module verification failed: {}",
            msg.to_string()
        )));
    }

    Ok(module)
}
