mod compile;
mod fmt_ast;
mod fmt_ir;

use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::source::SourceMap;

pub use compile::*;

#[derive(Default)]
pub struct DumpOptions {
    pub dump_ast: bool,
    pub dump_ir: bool,
    pub dump_asm: bool,
    pub dump_llvm_ir: bool,
}

pub struct PipelineResult {
    pub source_map: SourceMap,
    pub diagnostics: DiagnosticBag,
}

pub struct CompileResult {
    pub source_map: SourceMap,
    pub diagnostics: DiagnosticBag,
    pub success: bool,
}
