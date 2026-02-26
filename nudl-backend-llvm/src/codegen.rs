use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType};
use inkwell::values::{BasicValueEnum, FunctionValue, GlobalValue, PointerValue};
use inkwell::debug_info::{
    AsDIScope, DICompileUnit, DIFlags, DIFlagsConstants, DWARFEmissionKind, DWARFSourceLanguage,
    DebugInfoBuilder,
};
use inkwell::AddressSpace;
use inkwell::OptimizationLevel;

use inkwell::FloatPredicate;

use nudl_bc::ir::*;
use nudl_core::source::SourceMap;
use nudl_core::types::{TypeInterner, TypeKind};

/// Embedded pre-compiled runtime object file (built by build.rs).
const RUNTIME_OBJ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/nudl_rt.o"));

#[derive(Debug)]
pub enum BackendError {
    LlvmError(String),
    IoError(std::io::Error),
    LinkError(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::LlvmError(msg) => write!(f, "LLVM error: {}", msg),
            BackendError::IoError(e) => write!(f, "I/O error: {}", e),
            BackendError::LinkError(msg) => write!(f, "link error: {}", msg),
        }
    }
}

impl std::error::Error for BackendError {}

impl From<std::io::Error> for BackendError {
    fn from(e: std::io::Error) -> Self {
        BackendError::IoError(e)
    }
}

/// Tracks what an SSA register holds so StringPtr/StringLen can resolve correctly.
#[derive(Debug, Clone)]
enum RegStringInfo {
    /// Holds a string literal (index into string_constants)
    StringLiteral(u32),
    /// Holds a string parameter (ptr_alloca, len_alloca).
    /// Uses 'static because the allocas outlive the function (owned by Context).
    StringParam(PointerValue<'static>, PointerValue<'static>),
}

/// Maps parameter index to LLVM param layout accounting for string pairs.
struct ParamLayout {
    /// For each SSA param index: (first LLVM param index, count of LLVM params used)
    entries: Vec<(u32, u32)>,
}

impl ParamLayout {
    fn compute(func: &Function, types: &TypeInterner) -> Self {
        let mut entries = Vec::new();
        let mut llvm_param = 0u32;
        for (_name, type_id) in &func.params {
            let kind = types.resolve(*type_id);
            match kind {
                TypeKind::String => {
                    entries.push((llvm_param, 2)); // ptr, len
                    llvm_param += 2;
                }
                TypeKind::Struct { .. } => {
                    // Structs are heap-allocated pointers — single i64
                    entries.push((llvm_param, 1));
                    llvm_param += 1;
                }
                _ => {
                    entries.push((llvm_param, 1));
                    llvm_param += 1;
                }
            }
        }
        ParamLayout { entries }
    }
}

/// Build LLVM param types for a function based on its param layout.
fn build_llvm_param_types<'ctx>(
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
fn is_float_register(func: &Function, reg: u32, types: &TypeInterner) -> bool {
    if let Some(type_id) = func.register_types.get(reg as usize) {
        matches!(types.resolve(*type_id), TypeKind::Primitive(p) if p.is_float())
    } else {
        false
    }
}

// Safety: these extend lifetimes of LLVM values. Safe because values are valid for
// the Context's lifetime and we never use them after dropping the Context.
unsafe fn extend_ptr_lifetime<'a>(v: PointerValue<'_>) -> PointerValue<'a> {
    unsafe { std::mem::transmute(v) }
}

/// LLVM function references for ARC runtime intrinsics.
#[allow(dead_code)]
struct ArcIntrinsics<'ctx> {
    /// extern: __nudl_arc_alloc(u64, u32) -> ptr
    arc_alloc: FunctionValue<'ctx>,
    /// extern: __nudl_arc_release_slow(ptr, fn_ptr) -> void
    arc_release_slow: FunctionValue<'ctx>,
    /// extern: __nudl_arc_overflow_abort() -> void [noreturn]
    arc_overflow_abort: FunctionValue<'ctx>,
    /// inline: __nudl_arc_retain(ptr) -> void
    arc_retain: FunctionValue<'ctx>,
    /// inline: __nudl_arc_release(ptr, drop_fn) -> void
    arc_release: FunctionValue<'ctx>,
    /// inline: __nudl_drop_noop(ptr) -> void
    drop_noop: FunctionValue<'ctx>,
}

/// Emit ARC intrinsic declarations and inline functions into the module.
fn emit_arc_intrinsics<'ctx>(
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
        b.build_direct_call(
            arc_release_slow,
            &[obj_ptr.into(), drop_fn_val.into()],
            "",
        )
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

// --- Public API ---

/// Compile a program to an executable binary at the given output path.
pub fn compile_to_executable(
    program: &Program,
    output: &Path,
    optimized: bool,
) -> Result<(), BackendError> {
    let context = Context::create();
    let module = build_module(&context, program, optimized)?;

    let opt = if optimized {
        OptimizationLevel::Aggressive
    } else {
        OptimizationLevel::None
    };
    let target_machine = create_target_machine(opt)?;

    let obj_path = output.with_extension("o");
    target_machine
        .write_to_file(&module, FileType::Object, &obj_path)
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;

    // Write embedded runtime .o to temp file for linking
    let rt_obj_path = output.with_file_name("nudl_rt.o");
    std::fs::write(&rt_obj_path, RUNTIME_OBJ)?;

    link(&obj_path, &rt_obj_path, output)?;

    // Generate .dSYM bundle on macOS (must happen before .o is deleted)
    if cfg!(target_os = "macos") {
        let _ = Command::new("dsymutil").arg(output).status();
    }

    let _ = std::fs::remove_file(&obj_path);
    let _ = std::fs::remove_file(&rt_obj_path);

    Ok(())
}

/// Compile a program and return the LLVM IR as a string.
pub fn compile_to_llvm_ir(program: &Program) -> Result<String, BackendError> {
    let context = Context::create();
    let module = build_module(&context, program, false)?;
    Ok(module.print_to_string().to_string())
}

/// Compile a program and return native assembly text.
pub fn compile_to_asm_text(
    program: &Program,
    optimized: bool,
) -> Result<String, BackendError> {
    let context = Context::create();
    let module = build_module(&context, program, optimized)?;

    let opt = if optimized {
        OptimizationLevel::Aggressive
    } else {
        OptimizationLevel::None
    };
    let target_machine = create_target_machine(opt)?;

    let buf = target_machine
        .write_to_memory_buffer(&module, FileType::Assembly)
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;

    let asm = std::str::from_utf8(buf.as_slice())
        .map_err(|e| BackendError::LlvmError(e.to_string()))?
        .to_string();

    Ok(asm)
}

// --- Internal ---

fn create_target_machine(opt_level: OptimizationLevel) -> Result<TargetMachine, BackendError> {
    Target::initialize_all(&InitializationConfig::default());

    let target_triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&target_triple)
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;

    target
        .create_target_machine(
            &target_triple,
            "generic",
            "",
            opt_level,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| BackendError::LlvmError("failed to create target machine".into()))
}

fn build_module<'ctx>(
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
        DWARFEmissionKind::Full,
        0,
        false,
        false,
        "",
        "",
    );

    // Emit ARC intrinsics (inline retain/release + extern declarations)
    let arc = emit_arc_intrinsics(context, &module)?;

    // Emit string constants as globals
    for (i, s) in program.string_constants.iter().enumerate() {
        let bytes = s.as_bytes();
        let global = context.const_string(bytes, false);
        let global_val = module.add_global(
            context.i8_type().array_type(bytes.len() as u32),
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

        if func.is_extern {
            let param_types = build_llvm_param_types(func, types, context);
            let fn_type = if ret_is_float {
                context.f64_type().fn_type(&param_types, false)
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
            let param_types = build_llvm_param_types(func, types, context);
            let fn_type = if ret_is_float {
                context.f64_type().fn_type(&param_types, false)
            } else {
                context.i64_type().fn_type(&param_types, false)
            };
            let func_name = program.interner.resolve(func.name);
            let fn_val =
                module.add_function(&format!("__func_{}", func_name), fn_type, None);
            function_map.insert(func.id.0, fn_val);
        }
    }

    // Generate per-struct drop functions that log "dropping <Name>\n"
    let mut drop_fns: HashMap<nudl_core::types::TypeId, FunctionValue<'ctx>> = HashMap::new();
    {
        let ptr_ty = context.ptr_type(AddressSpace::default());
        let void_ty = context.void_type();
        let drop_fn_ty = void_ty.fn_type(&[ptr_ty.into()], false);

        // Find the extern write function in the module
        let write_fn = module.get_function("write");

        for (type_id, kind) in types.iter_types() {
            if let TypeKind::Struct { name, .. } = kind {
                let msg = format!("dropping {}\n", name);
                let msg_bytes = msg.as_bytes();

                // Create a global string for the drop message
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

                let drop_fn = module.add_function(
                    &format!("__nudl_drop_{}", name),
                    drop_fn_ty,
                    Some(inkwell::module::Linkage::Internal),
                );
                let bb = context.append_basic_block(drop_fn, "entry");
                let b = context.create_builder();
                b.position_at_end(bb);

                // Call write(1, msg_ptr, msg_len) if write is available
                // Note: extern write has LLVM signature (i64, i64, i64) -> i64
                if let Some(write_fn) = write_fn {
                    let i64_ty = context.i64_type();
                    let fd = i64_ty.const_int(1, false);
                    let msg_ptr_raw = b.build_ptr_to_int(
                        global_val.as_pointer_value(),
                        i64_ty,
                        "msg_ptr_int",
                    ).map_err(|e| BackendError::LlvmError(e.to_string()))?;
                    let msg_len = i64_ty.const_int(msg_bytes.len() as u64, false);
                    b.build_direct_call(write_fn, &[fd.into(), msg_ptr_raw.into(), msg_len.into()], "")
                        .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                }

                b.build_return(None)
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;

                drop_fns.insert(type_id, drop_fn);
            }
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
            program,
            func,
            &function_map,
            &string_constants,
            &mut reg_string_info,
            types,
            &arc,
            &drop_fns,
            is_entry,
            optimized,
            &dibuilder,
            &compile_unit,
            source_map,
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

fn emit_function<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    program: &Program,
    func: &Function,
    function_map: &HashMap<u32, FunctionValue<'ctx>>,
    string_constants: &[(GlobalValue<'ctx>, u64)],
    reg_string_info: &mut HashMap<u32, RegStringInfo>,
    types: &TypeInterner,
    arc: &ArcIntrinsics<'ctx>,
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

    let subroutine_type = dibuilder.create_subroutine_type(
        compile_unit.get_file(),
        None,
        &[],
        DIFlags::PUBLIC,
    );

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
    let default_debug_loc = dibuilder.create_debug_location(
        context,
        func_line.max(1),
        0,
        di_scope,
        None,
    );
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
                RegStringInfo::StringParam(
                    unsafe { extend_ptr_lifetime(ptr_alloca) },
                    unsafe { extend_ptr_lifetime(len_alloca) },
                ),
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
                    let loc = dibuilder.create_debug_location(
                        context,
                        line,
                        col,
                        di_scope,
                        None,
                    );
                    builder.set_current_debug_location(loc);
                }
            }
            emit_instruction(
                inst,
                program,
                func,
                context,
                builder,
                &register_allocas,
                &str_ptr_allocas,
                &str_len_allocas,
                reg_string_info,
                string_constants,
                function_map,
                types,
                arc,
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
                        let loc = dibuilder.create_debug_location(
                            context,
                            line,
                            col,
                            di_scope,
                            None,
                        );
                        builder.set_current_debug_location(loc);
                        // Emit a dummy store for the debugger stop, but NOT
                        // to the return register — that would clobber the value.
                        let ret_reg_idx = match &block.terminator {
                            Terminator::Return(r) => Some(r.0),
                            _ => None,
                        };
                        // Pick a register that isn't the return register for the dummy store.
                        let dummy_idx = if ret_reg_idx == Some(0) { None } else { Some(0u32) };
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
                let epilogue_loc = dibuilder.create_debug_location(
                    context,
                    0,
                    0,
                    di_scope,
                    None,
                );
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

fn emit_instruction<'ctx>(
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
                emit_fcmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, FloatPredicate::OEQ)?;
            } else {
                emit_icmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, inkwell::IntPredicate::EQ)?;
            }
        }
        Instruction::Ne(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, FloatPredicate::ONE)?;
            } else {
                emit_icmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, inkwell::IntPredicate::NE)?;
            }
        }
        Instruction::Lt(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, FloatPredicate::OLT)?;
            } else {
                emit_icmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, inkwell::IntPredicate::SLT)?;
            }
        }
        Instruction::Le(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, FloatPredicate::OLE)?;
            } else {
                emit_icmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, inkwell::IntPredicate::SLE)?;
            }
        }
        Instruction::Gt(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, FloatPredicate::OGT)?;
            } else {
                emit_icmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, inkwell::IntPredicate::SGT)?;
            }
        }
        Instruction::Ge(dst, lhs, rhs) => {
            if is_float_register(func, lhs.0, types) {
                emit_fcmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, FloatPredicate::OGE)?;
            } else {
                emit_icmp(context, builder, register_allocas, dst.0, lhs.0, rhs.0, inkwell::IntPredicate::SGE)?;
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
            let total_size = context.i64_type().const_int(header_size + field_size, false);
            let type_tag = context.i32_type().const_int(type_id.0 as u64, false);
            let call_result = builder
                .build_direct_call(arc.arc_alloc, &[total_size.into(), type_tag.into()], "alloc")
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
        Instruction::TupleAlloc(dst, type_id, elements) | Instruction::FixedArrayAlloc(dst, type_id, elements) => {
            // Allocate: header (16 bytes) + elements (8 bytes each)
            let header_size = 16u64;
            let field_size = elements.len() as u64 * 8;
            let total_size = context.i64_type().const_int(header_size + field_size, false);
            let type_tag = context.i32_type().const_int(type_id.0 as u64, false);
            let call_result = builder
                .build_direct_call(arc.arc_alloc, &[total_size.into(), type_tag.into()], "alloc")
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
                    .build_gep(
                        context.i8_type(),
                        obj_ptr,
                        &[byte_offset],
                        "index_ptr",
                    )
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
                    .build_gep(
                        context.i8_type(),
                        obj_ptr,
                        &[byte_offset],
                        "index_ptr",
                    )
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

fn emit_call<'ctx>(
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
    result_reg: &Register,
    func_ref: &FunctionRef,
    args: &[Register],
) -> Result<(), BackendError> {
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
        FunctionRef::Builtin(_) => (None, None),
    };

    let callee_fn = match callee_fn_val {
        Some(f) => f,
        None => return Ok(()),
    };

    let callee_func =
        callee_func_id.and_then(|id| program.functions.iter().find(|f| f.id.0 == id));
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
                match reg_string_info.get(&arg_reg.0) {
                    Some(RegStringInfo::StringLiteral(idx)) => {
                        let (global, len) = &string_constants[*idx as usize];
                        let ptr = gep_string_ptr(context, builder, global, *len)?;
                        let len_val = context.i64_type().const_int(*len, false);
                        llvm_args.push(ptr.into());
                        llvm_args.push(len_val.into());
                    }
                    Some(RegStringInfo::StringParam(ptr_alloca, len_alloca)) => {
                        let ptr_alloca = *ptr_alloca;
                        let len_alloca = *len_alloca;
                        let ptr = builder
                            .build_load(
                                context.ptr_type(AddressSpace::default()),
                                ptr_alloca,
                                "str_param_ptr",
                            )
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        let len = builder
                            .build_load(context.i64_type(), len_alloca, "str_param_len")
                            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
                        llvm_args.push(ptr);
                        llvm_args.push(len);
                    }
                    _ => {
                        // Fallback: load from companion allocas (handles control-flow
                        // cases where reg_string_info was overwritten by another branch)
                        if let (Some(&ptr_al), Some(&len_al)) = (
                            str_ptr_allocas.get(&arg_reg.0),
                            str_len_allocas.get(&arg_reg.0),
                        ) {
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
                            llvm_args.push(ptr);
                            llvm_args.push(len);
                        } else {
                            llvm_args.push(
                                context
                                    .ptr_type(AddressSpace::default())
                                    .const_null()
                                    .into(),
                            );
                            llvm_args.push(context.i64_type().const_zero().into());
                        }
                    }
                }
            } else {
                // Check if the callee param is float
                let param_is_float = callee_func
                    .and_then(|cf| cf.params.get(i))
                    .map(|(_, pty)| matches!(types.resolve(*pty), TypeKind::Primitive(p) if p.is_float()))
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

    // Store result
    if let Some(ret_val) = call_result.try_as_basic_value().basic() {
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

fn emit_terminator<'ctx>(
    term: &Terminator,
    func: &Function,
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    block_map: &HashMap<u32, inkwell::basic_block::BasicBlock<'ctx>>,
    register_allocas: &HashMap<u32, PointerValue<'ctx>>,
    types: &TypeInterner,
    is_entry: bool,
) -> Result<(), BackendError> {
    match term {
        Terminator::Return(ret_reg) => {
            if is_entry {
                let zero = context.i32_type().const_zero();
                builder
                    .build_return(Some(&zero))
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            } else if is_float_register(func, ret_reg.0, types) {
                let val = load_f64(context, builder, register_allocas, ret_reg.0)?;
                builder
                    .build_return(Some(&val))
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            } else {
                let val = load_i64(context, builder, register_allocas, ret_reg.0)?;
                builder
                    .build_return(Some(&val))
                    .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            }
        }
        Terminator::Jump(target) => {
            builder
                .build_unconditional_branch(block_map[&target.0])
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
        Terminator::Branch(cond, then_block, else_block) => {
            let cond_val = load_i64(context, builder, register_allocas, cond.0)?;
            let zero = context.i64_type().const_zero();
            let cmp = builder
                .build_int_compare(inkwell::IntPredicate::NE, cond_val, zero, "branch_cond")
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
            builder
                .build_conditional_branch(cmp, block_map[&then_block.0], block_map[&else_block.0])
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
        Terminator::Unreachable => {
            builder
                .build_unreachable()
                .map_err(|e| BackendError::LlvmError(e.to_string()))?;
        }
    }
    Ok(())
}

// --- Helpers ---

fn store<'ctx, V: inkwell::values::BasicValue<'ctx>>(
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

fn load_i64<'ctx>(
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
fn load_ptr<'ctx>(
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

fn load_f64<'ctx>(
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

fn load_binop<'ctx>(
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

fn load_float_binop<'ctx>(
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

fn emit_fcmp<'ctx>(
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

fn emit_icmp<'ctx>(
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

fn gep_string_ptr<'ctx>(
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

fn link(obj_path: &Path, rt_obj_path: &Path, output: &Path) -> Result<(), BackendError> {
    let mut cmd = Command::new("cc");
    cmd.arg("-g")
        .arg("-o")
        .arg(output)
        .arg(obj_path)
        .arg(rt_obj_path);

    if cfg!(target_os = "macos") {
        cmd.arg("-lSystem");
    } else {
        cmd.arg("-lc");
    }

    let status = cmd.status()?;

    if !status.success() {
        return Err(BackendError::LinkError(format!(
            "linker exited with status: {}",
            status
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    use nudl_ast::lexer::Lexer;
    use nudl_ast::parser::Parser;
    use nudl_bc::checker::Checker;
    use nudl_bc::lower::Lowerer;
    use nudl_core::span::FileId;

    fn compile_source(source: &str) -> Program {
        let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
        let (module, _) = Parser::new(tokens).parse_module();
        let (checked, diags) = Checker::new().check(&module);
        assert!(
            !diags.has_errors(),
            "checker errors: {:?}",
            diags.reports()
        );
        Lowerer::new(checked).lower(&module)
    }

    fn compile_and_run(source: &str) -> (String, bool) {
        let program = compile_source(source);
        let output = std::env::temp_dir().join("nudl_llvm_test");
        compile_to_executable(&program, &output, false).expect("compilation failed");

        assert!(output.exists(), "output binary should exist");

        let result = Command::new(&output)
            .output()
            .expect("failed to run binary");

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let success = result.status.success();

        let _ = std::fs::remove_file(&output);

        (stdout, success)
    }

    #[test]
    fn compile_hello_world() {
        let (stdout, success) = compile_and_run(
            r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}

fn print(s: string) {
    write(1, __str_ptr(s), __str_len(s));
}

fn println(s: string) {
    print(s);
    print("\n");
}

fn main() {
    println("Hello, world!");
}
"#,
        );

        assert_eq!(stdout, "Hello, world!\n");
        assert!(success, "binary should exit with 0");
    }

    #[test]
    fn compile_with_arithmetic() {
        let program = compile_source(
            r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
fn main() {
    let x = add(10, 20);
}
"#,
        );
        let ir = compile_to_llvm_ir(&program).expect("IR generation failed");
        assert!(ir.contains("define i32 @main()"));
        assert!(ir.contains("add"));
    }

    #[test]
    fn compile_with_if_else() {
        let program = compile_source(
            r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}

fn print(s: string) {
    write(1, __str_ptr(s), __str_len(s));
}

fn main() {
    let x = 1;
    if x == 1 {
        print("yes\n");
    } else {
        print("no\n");
    }
}
"#,
        );
        let ir = compile_to_llvm_ir(&program).expect("IR generation failed");
        assert!(ir.contains("br i1"));
    }

    #[test]
    fn emit_llvm_ir() {
        let program = compile_source(
            r#"
fn main() {
    let x = 42;
}
"#,
        );
        let ir = compile_to_llvm_ir(&program).expect("IR generation failed");
        assert!(ir.contains("define i32 @main()"));
    }

    #[test]
    fn emit_asm() {
        let program = compile_source(
            r#"
fn main() {
    let x = 42;
}
"#,
        );
        let asm = compile_to_asm_text(&program, false).expect("ASM generation failed");
        assert!(!asm.is_empty());
        assert!(asm.contains("main"));
    }
}
