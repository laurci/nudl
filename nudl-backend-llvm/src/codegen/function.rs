use super::*;

pub(super) fn emit_function<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    module: &Module<'ctx>,
    program: &Program,
    func: &Function,
    function_map: &HashMap<u32, FunctionValue<'ctx>>,
    string_constants: &[(GlobalValue<'ctx>, u64)],
    reg_string_info: &mut HashMap<u32, RegStringInfo>,
    types: &TypeInterner,
    arc: &ArcIntrinsics<'ctx>,
    string_builtins: &StringBuiltins<'ctx>,
    drop_fns: &HashMap<nudl_core::types::TypeId, FunctionValue<'ctx>>,
    is_entry: bool,
    optimized: bool,
    dibuilder: &DebugInfoBuilder<'ctx>,
    compile_unit: &DICompileUnit<'ctx>,
    source_map: Option<&SourceMap>,
) -> Result<(), BackendError> {
    let fn_val = function_map[&func.id.0];
    let layout = ParamLayout::compute(func, types);

    // Create debug info for this function
    let func_line = if let Some(sm) = source_map {
        if !func.span.is_empty() {
            let (_, line, _) = sm.span_to_location(func.span);
            line
        } else {
            0
        }
    } else {
        0
    };

    let subroutine_type =
        dibuilder.create_subroutine_type(compile_unit.get_file(), None, &[], DIFlags::PUBLIC);

    let func_name = program.interner.resolve(func.name);
    let di_subprogram = dibuilder.create_function(
        compile_unit.as_debug_info_scope(),
        func_name,
        None,
        compile_unit.get_file(),
        func_line,
        subroutine_type,
        true,
        true,
        func_line,
        DIFlags::PUBLIC,
        false,
    );
    fn_val.set_subprogram(di_subprogram);
    let di_scope = di_subprogram.as_debug_info_scope();

    // Set default debug location to the function's start line
    let default_debug_loc =
        dibuilder.create_debug_location(context, func_line.max(1), 0, di_scope, None);
    builder.set_current_debug_location(default_debug_loc);

    // Entry block for allocas
    let alloca_block = context.append_basic_block(fn_val, "entry");
    builder.position_at_end(alloca_block);
    builder.set_current_debug_location(default_debug_loc);

    reg_string_info.clear();

    // Create allocas for all SSA registers
    let mut register_allocas: HashMap<u32, PointerValue<'ctx>> = HashMap::new();
    let mut str_ptr_allocas: HashMap<u32, PointerValue<'ctx>> = HashMap::new();
    let mut str_len_allocas: HashMap<u32, PointerValue<'ctx>> = HashMap::new();
    for r in 0..func.register_count {
        let alloca_ty = if is_float_register(func, r, types) {
            context.f64_type().as_basic_type_enum()
        } else {
            context.i64_type().as_basic_type_enum()
        };
        let alloca = builder
            .build_alloca(alloca_ty, &format!("r{}", r))
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        register_allocas.insert(r, alloca);
        // Companion allocas for string ptr/len — used when string values
        // flow through control flow (if-else branches, loops).
        let ptr_alloca = builder
            .build_alloca(
                context.ptr_type(AddressSpace::default()),
                &format!("r{}_str_ptr", r),
            )
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        let len_alloca = builder
            .build_alloca(context.i64_type(), &format!("r{}_str_len", r))
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        str_ptr_allocas.insert(r, ptr_alloca);
        str_len_allocas.insert(r, len_alloca);
    }

    // Store incoming parameters
    for (param_idx, &(first_llvm, count)) in layout.entries.iter().enumerate() {
        if count == 2 {
            // String param: store ptr/len into the pre-allocated companion allocas
            let ptr_alloca = str_ptr_allocas[&(param_idx as u32)];
            let len_alloca = str_len_allocas[&(param_idx as u32)];

            let ptr_param = fn_val.get_nth_param(first_llvm).unwrap();
            let len_param = fn_val.get_nth_param(first_llvm + 1).unwrap();

            builder
                .build_store(ptr_alloca, ptr_param)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            builder
                .build_store(len_alloca, len_param)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;

            reg_string_info.insert(
                param_idx as u32,
                RegStringInfo::StringParam(unsafe { extend_ptr_lifetime(ptr_alloca) }, unsafe {
                    extend_ptr_lifetime(len_alloca)
                }),
            );
        } else {
            let param_val = fn_val.get_nth_param(first_llvm).unwrap();
            let alloca = register_allocas[&(param_idx as u32)];
            // Float params arrive as f64 values, stored directly into f64 allocas
            builder
                .build_store(alloca, param_val)
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
    }

    // Create basic blocks for each IR block
    let mut block_map: HashMap<u32, inkwell::basic_block::BasicBlock<'ctx>> = HashMap::new();
    for block in &func.blocks {
        let bb = context.append_basic_block(fn_val, &format!("b{}", block.id.0));
        block_map.insert(block.id.0, bb);
    }

    // Jump from alloca block to first IR block
    if let Some(first_block) = func.blocks.first() {
        let first_bb = block_map[&first_block.id.0];
        builder
            .build_unconditional_branch(first_bb)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    }

    // Emit each basic block
    for block in &func.blocks {
        let bb = block_map[&block.id.0];
        builder.position_at_end(bb);
        builder.set_current_debug_location(default_debug_loc);

        for (inst, span) in block.instructions.iter().zip(block.spans.iter()) {
            if let Some(sm) = source_map {
                if !span.is_empty() {
                    let (_, line, col) = sm.span_to_location(*span);
                    let loc = dibuilder.create_debug_location(context, line, col, di_scope, None);
                    builder.set_current_debug_location(loc);
                }
            }
            emit_instruction(
                inst,
                program,
                func,
                context,
                builder,
                module,
                &register_allocas,
                &str_ptr_allocas,
                &str_len_allocas,
                reg_string_info,
                string_constants,
                function_map,
                types,
                arc,
                string_builtins,
                drop_fns,
            )?;
        }

        // For return terminators: emit a dummy store at the closing `}` line
        // so the debugger stops there, then set line 0 on the actual `ret` so
        // it executes without an extra stop.
        if matches!(&block.terminator, Terminator::Return(_)) {
            if !optimized {
                // Debug builds: emit a dummy store at the closing `}` so the
                // debugger has a source-level stop, then line 0 on the ret.
                if let Some(sm) = source_map {
                    if !func.span.is_empty() && func.span.end > 0 {
                        let file = sm.get_file(func.span.file_id);
                        let (line, col) = file.line_col(func.span.end.saturating_sub(1));
                        let loc =
                            dibuilder.create_debug_location(context, line, col, di_scope, None);
                        builder.set_current_debug_location(loc);
                        // Emit a dummy store for the debugger stop, but NOT
                        // to the return register — that would clobber the value.
                        let ret_reg_idx = match &block.terminator {
                            Terminator::Return(r) => Some(r.0),
                            _ => None,
                        };
                        // Pick a register that isn't the return register for the dummy store.
                        let dummy_idx = if ret_reg_idx == Some(0) {
                            None
                        } else {
                            Some(0u32)
                        };
                        if let Some(idx) = dummy_idx {
                            if let Some(alloca) = register_allocas.get(&idx) {
                                if is_float_register(func, idx, types) {
                                    let zero = context.f64_type().const_zero();
                                    let _ = builder.build_store(*alloca, zero);
                                } else {
                                    let zero = context.i64_type().const_zero();
                                    let _ = builder.build_store(*alloca, zero);
                                }
                            }
                        }
                    }
                }
                let epilogue_loc = dibuilder.create_debug_location(context, 0, 0, di_scope, None);
                builder.set_current_debug_location(epilogue_loc);
            }
        }

        emit_terminator(
            &block.terminator,
            func,
            context,
            builder,
            &block_map,
            &register_allocas,
            types,
            is_entry,
        )?;
    }

    Ok(())
}
