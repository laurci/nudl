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
            let mut param_types = build_llvm_param_types(func, types, context, true);

            // Check if return type is a large extern struct (needs sret)
            let ret_is_large_extern_struct = match types.resolve(func.return_type) {
                TypeKind::Struct {
                    fields,
                    is_extern: true,
                    ..
                } => extern_struct_is_large(types, fields),
                _ => false,
            };

            let fn_type = if ret_is_large_extern_struct {
                // Large extern struct return: use void return + sret pointer param
                let ptr_ty = context.ptr_type(AddressSpace::default());
                param_types.insert(0, ptr_ty.into());
                context.void_type().fn_type(&param_types, false)
            } else if ret_is_string {
                // Extern string-returning functions return a raw ptr (ARC heap string)
                context
                    .ptr_type(AddressSpace::default())
                    .fn_type(&param_types, false)
            } else if ret_is_float {
                // Use actual C float type (f32 or f64) for extern functions
                let ret_float_ty = match types.resolve(func.return_type) {
                    TypeKind::Primitive(nudl_core::types::PrimitiveType::F32) => {
                        context.f32_type()
                    }
                    _ => context.f64_type(),
                };
                ret_float_ty.fn_type(&param_types, false)
            } else if let Some(c_ret_ty) = type_to_c_llvm_type(context, types, func.return_type) {
                // Extern struct or fixed array return type
                c_ret_ty.fn_type(&param_types, false)
            } else {
                context.i64_type().fn_type(&param_types, false)
            };
            let ext_name = func.extern_symbol.as_deref().unwrap_or("unknown_extern");
            let fn_val =
                module.add_function(ext_name, fn_type, Some(inkwell::module::Linkage::External));

            // Add sret attribute to the first parameter if large struct return
            if ret_is_large_extern_struct {
                if let TypeKind::Struct {
                    fields,
                    is_extern: true,
                    ..
                } = types.resolve(func.return_type)
                {
                    let c_struct_ty = build_c_struct_type(context, types, fields);
                    let sret_attr = context.create_type_attribute(
                        inkwell::attributes::Attribute::get_named_enum_kind_id("sret"),
                        c_struct_ty.as_any_type_enum(),
                    );
                    fn_val.add_attribute(inkwell::attributes::AttributeLoc::Param(0), sret_attr);
                }
            }

            // Note: On ARM64, large struct parameters are passed indirectly by
            // passing a pointer in a register (not via byval). Clang does NOT use
            // byval on AArch64. The pack_extern_struct_for_ffi function already
            // allocates a copy and returns a pointer, so we just declare the param
            // as `ptr` (which build_llvm_param_types already does for large structs).

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

    // Generate per-struct drop functions only for structs that implement Drop
    // (i.e., have a `drop(self)` method — mangled as `{StructName}__drop` in the IR).
    // Structs without a drop method get nullptr as their drop handler, which the
    // runtime uses to dispatch to a fast path (no indirect call overhead).
    let mut drop_fns: HashMap<nudl_core::types::TypeId, FunctionValue<'ctx>> = HashMap::new();
    {
        // Build a map from struct name → IR function for drop methods.
        let mut drop_methods: HashMap<String, &Function> = HashMap::new();
        for func in &program.functions {
            let name_str = program.interner.resolve(func.name);
            if let Some(struct_name) = name_str.strip_suffix("__drop") {
                // Verify it's a method with exactly one param (self)
                if func.params.len() == 1 {
                    drop_methods.insert(struct_name.to_string(), func);
                }
            }
        }

        for (type_id, kind) in types.iter_types() {
            if let TypeKind::Struct {
                name, is_extern, ..
            } = kind
            {
                if *is_extern {
                    continue;
                }
                // Only register a drop function if the struct has a drop(self) method
                if let Some(ir_func) = drop_methods.get(name) {
                    if let Some(fn_val) = function_map.get(&ir_func.id.0) {
                        drop_fns.insert(type_id, *fn_val);
                    }
                }
            }
        }
    }

    // Generate drop functions for DynArray types that free the data buffer
    // and release reference-typed elements via __nudl_array_destroy.
    {
        let ptr_ty = context.ptr_type(AddressSpace::default());
        let i64_ty = context.i64_type();
        let void_ty = context.void_type();
        let drop_fn_ty = void_ty.fn_type(&[ptr_ty.into()], false);

        // Declare __nudl_array_destroy(ptr, i64, ptr) -> void
        //   arg0: array ARC object pointer
        //   arg1: is_ref_elem (1 if elements are reference types, 0 otherwise)
        //   arg2: elem_drop function pointer (NULL if no custom Drop)
        let array_destroy_ty =
            void_ty.fn_type(&[ptr_ty.into(), i64_ty.into(), ptr_ty.into()], false);
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

            // Determine is_ref_elem flag and element drop function
            let is_ref = types.is_reference_type(*element);
            let is_ref_val = i64_ty.const_int(if is_ref { 1 } else { 0 }, false);
            let elem_drop: inkwell::values::PointerValue = if is_ref {
                if let Some(dfn) = drop_fns.get(element) {
                    dfn.as_global_value().as_pointer_value()
                } else {
                    ptr_ty.const_null()
                }
            } else {
                ptr_ty.const_null()
            };

            b.build_direct_call(
                array_destroy,
                &[arr_ptr.into(), is_ref_val.into(), elem_drop.into()],
                "",
            )
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
