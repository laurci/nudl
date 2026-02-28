mod arc;
mod call;
mod function;
mod helpers;
mod instruction;
mod module_builder;
mod target;
mod terminator;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use inkwell::AddressSpace;
use inkwell::OptimizationLevel;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::debug_info::{
    AsDIScope, DICompileUnit, DIFile, DIFlags, DIFlagsConstants, DWARFEmissionKind,
    DWARFSourceLanguage, DebugInfoBuilder,
};
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType};
use inkwell::values::{BasicValueEnum, FunctionValue, GlobalValue, PointerValue};

use inkwell::FloatPredicate;

use nudl_bc::ir::*;
use nudl_core::source::SourceMap;
use nudl_core::types::{TypeInterner, TypeKind};

use arc::*;
use call::*;
use function::*;
use helpers::*;
use instruction::*;
use module_builder::*;
use target::*;
use terminator::*;

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
pub(super) enum RegStringInfo {
    StringLiteral(u32),
    StringParam(PointerValue<'static>, PointerValue<'static>),
}

/// Maps parameter index to LLVM param layout accounting for string pairs.
pub(super) struct ParamLayout {
    pub(super) entries: Vec<(u32, u32)>,
}

impl ParamLayout {
    pub(super) fn compute(func: &Function, types: &TypeInterner) -> Self {
        let mut entries = Vec::new();
        let mut llvm_param = 0u32;
        for (_name, type_id) in &func.params {
            let kind = types.resolve(*type_id);
            match kind {
                TypeKind::String => {
                    entries.push((llvm_param, 2));
                    llvm_param += 2;
                }
                TypeKind::Struct { .. } => {
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

/// LLVM function references for ARC runtime intrinsics.
#[allow(dead_code)]
pub(super) struct ArcIntrinsics<'ctx> {
    pub(super) arc_alloc: FunctionValue<'ctx>,
    pub(super) arc_release_slow: FunctionValue<'ctx>,
    pub(super) arc_overflow_abort: FunctionValue<'ctx>,
    pub(super) arc_retain: FunctionValue<'ctx>,
    pub(super) arc_release: FunctionValue<'ctx>,
    pub(super) drop_noop: FunctionValue<'ctx>,
}

/// LLVM function references for string runtime builtins.
pub(super) struct StringBuiltins<'ctx> {
    pub(super) str_concat: FunctionValue<'ctx>,
    pub(super) i64_to_str: FunctionValue<'ctx>,
    pub(super) f64_to_str: FunctionValue<'ctx>,
    pub(super) bool_to_str: FunctionValue<'ctx>,
    pub(super) char_to_str: FunctionValue<'ctx>,
    // String operations
    pub(super) str_substr: FunctionValue<'ctx>,
    pub(super) str_indexof: FunctionValue<'ctx>,
    pub(super) str_trim: FunctionValue<'ctx>,
    pub(super) str_contains: FunctionValue<'ctx>,
    pub(super) str_starts_with: FunctionValue<'ctx>,
    pub(super) str_ends_with: FunctionValue<'ctx>,
    pub(super) str_to_upper: FunctionValue<'ctx>,
    pub(super) str_to_lower: FunctionValue<'ctx>,
    pub(super) str_replace: FunctionValue<'ctx>,
    pub(super) str_repeat: FunctionValue<'ctx>,
}

// Safety: these extend lifetimes of LLVM values. Safe because values are valid for
// the Context's lifetime and we never use them after dropping the Context.
pub(super) unsafe fn extend_ptr_lifetime<'a>(v: PointerValue<'_>) -> PointerValue<'a> {
    unsafe { std::mem::transmute(v) }
}

// --- Public API ---

/// Compile a program to an executable binary at the given output path.
/// When `optimized` is true, runs the full LLVM `-O3` pass pipeline.
/// When `native` is true, targets the host CPU (like `-march=native`).
pub fn compile_to_executable(
    program: &Program,
    output: &Path,
    optimized: bool,
    native: bool,
) -> Result<(), BackendError> {
    let context = Context::create();
    let module = build_module(&context, program, optimized)?;

    let opt = if optimized {
        OptimizationLevel::Aggressive
    } else {
        OptimizationLevel::None
    };
    let target_machine = create_target_machine(opt, native)?;

    // Run LLVM optimization passes in release mode
    if optimized {
        let pass_options = PassBuilderOptions::create();
        pass_options.set_loop_vectorization(true);
        pass_options.set_loop_slp_vectorization(true);
        pass_options.set_loop_unrolling(true);
        pass_options.set_loop_interleaving(true);
        pass_options.set_merge_functions(true);
        module
            .run_passes("default<O3>", &target_machine, pass_options)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    }

    let obj_path = output.with_extension("o");
    target_machine
        .write_to_file(&module, FileType::Object, &obj_path)
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;

    // Write embedded runtime .o to temp file for linking
    let rt_obj_path = output.with_file_name("nudl_rt.o");
    std::fs::write(&rt_obj_path, RUNTIME_OBJ)?;

    link(&obj_path, &rt_obj_path, output)?;

    // Generate .dSYM bundle on macOS (must happen before .o is deleted)
    if cfg!(target_os = "macos") && !optimized {
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
    native: bool,
) -> Result<String, BackendError> {
    let context = Context::create();
    let module = build_module(&context, program, optimized)?;

    let opt = if optimized {
        OptimizationLevel::Aggressive
    } else {
        OptimizationLevel::None
    };
    let target_machine = create_target_machine(opt, native)?;

    // Run LLVM optimization passes in release mode
    if optimized {
        let pass_options = PassBuilderOptions::create();
        pass_options.set_loop_vectorization(true);
        pass_options.set_loop_slp_vectorization(true);
        pass_options.set_loop_unrolling(true);
        pass_options.set_loop_interleaving(true);
        pass_options.set_merge_functions(true);
        module
            .run_passes("default<O3>", &target_machine, pass_options)
            .map_err(|e| BackendError::LlvmError(e.to_string()))?;
    }

    let buf = target_machine
        .write_to_memory_buffer(&module, FileType::Assembly)
        .map_err(|e| BackendError::LlvmError(e.to_string()))?;

    let asm = std::str::from_utf8(buf.as_slice())
        .map_err(|e| BackendError::LlvmError(e.to_string()))?
        .to_string();

    Ok(asm)
}
