use std::path::Path;

use nudl_ast::lexer::Lexer;
use nudl_ast::parser::Parser;
use nudl_backend_llvm::codegen;
use nudl_bc::checker::Checker;
use nudl_bc::lower::Lowerer;
use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::source::SourceMap;
use nudl_vm::Vm;

use super::fmt_ast::fmt_ast;
use super::fmt_ir::fmt_ir;
use super::{CompileResult, DumpOptions, PipelineResult};

pub fn check(source_path: &Path, dump: &DumpOptions) -> PipelineResult {
    let mut source_map = SourceMap::new();
    let mut diagnostics = DiagnosticBag::new();

    let content = match std::fs::read_to_string(source_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not read '{}': {}", source_path.display(), e);
            return PipelineResult {
                source_map,
                diagnostics,
            };
        }
    };

    let file_id = source_map.add_file(source_path.to_path_buf(), content.clone());

    let (tokens, lex_diags) = Lexer::new(&content, file_id).tokenize();
    diagnostics.merge(lex_diags);
    if diagnostics.has_errors() {
        return PipelineResult {
            source_map,
            diagnostics,
        };
    }

    let (module, parse_diags) = Parser::new(tokens).parse_module();
    diagnostics.merge(parse_diags);
    if diagnostics.has_errors() {
        return PipelineResult {
            source_map,
            diagnostics,
        };
    }

    if dump.dump_ast {
        eprintln!("{}", fmt_ast(&module));
    }

    let (checked, check_diags) = Checker::new().check(&module);
    diagnostics.merge(check_diags);

    if dump.dump_ir && !diagnostics.has_errors() {
        let program = Lowerer::new(checked).lower(&module);
        eprintln!("{}", fmt_ir(&program));
    }

    PipelineResult {
        source_map,
        diagnostics,
    }
}

pub fn build(
    source_path: &Path,
    output_path: &Path,
    release: bool,
    dump: &DumpOptions,
) -> CompileResult {
    let mut source_map = SourceMap::new();
    let mut diagnostics = DiagnosticBag::new();

    let content = match std::fs::read_to_string(source_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not read '{}': {}", source_path.display(), e);
            return CompileResult {
                source_map,
                diagnostics,
                success: false,
            };
        }
    };

    let file_id = source_map.add_file(source_path.to_path_buf(), content.clone());

    let (tokens, lex_diags) = Lexer::new(&content, file_id).tokenize();
    diagnostics.merge(lex_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    let (module, parse_diags) = Parser::new(tokens).parse_module();
    diagnostics.merge(parse_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    if dump.dump_ast {
        eprintln!("{}", fmt_ast(&module));
    }

    let (checked, check_diags) = Checker::new().check(&module);
    diagnostics.merge(check_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    let mut program = Lowerer::new(checked).lower(&module);
    program.source_map = Some(source_map);

    if dump.dump_ir {
        eprintln!("{}", fmt_ir(&program));
    }

    if dump.dump_llvm_ir {
        match codegen::compile_to_llvm_ir(&program) {
            Ok(ir) => eprintln!("{}", ir),
            Err(e) => eprintln!("error generating LLVM IR: {}", e),
        }
    }

    if dump.dump_asm {
        match codegen::compile_to_asm_text(&program, release) {
            Ok(asm) => eprintln!("{}", asm),
            Err(e) => eprintln!("error generating assembly: {}", e),
        }
    }

    let result = codegen::compile_to_executable(&program, output_path, release);
    let source_map = program.source_map.unwrap_or_default();
    match result {
        Ok(()) => CompileResult {
            source_map,
            diagnostics,
            success: true,
        },
        Err(e) => {
            eprintln!("error: {}", e);
            CompileResult {
                source_map,
                diagnostics,
                success: false,
            }
        }
    }
}

pub fn run_vm(source_path: &Path, dump: &DumpOptions) -> CompileResult {
    let mut source_map = SourceMap::new();
    let mut diagnostics = DiagnosticBag::new();

    let content = match std::fs::read_to_string(source_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not read '{}': {}", source_path.display(), e);
            return CompileResult {
                source_map,
                diagnostics,
                success: false,
            };
        }
    };

    let file_id = source_map.add_file(source_path.to_path_buf(), content.clone());

    let (tokens, lex_diags) = Lexer::new(&content, file_id).tokenize();
    diagnostics.merge(lex_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    let (module, parse_diags) = Parser::new(tokens).parse_module();
    diagnostics.merge(parse_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    if dump.dump_ast {
        eprintln!("{}", fmt_ast(&module));
    }

    let (checked, check_diags) = Checker::new().check(&module);
    diagnostics.merge(check_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    let program = Lowerer::new(checked).lower(&module);

    if dump.dump_ir {
        eprintln!("{}", fmt_ir(&program));
    }

    let mut vm = Vm::new();
    match vm.run(&program) {
        Ok(_) => CompileResult {
            source_map,
            diagnostics,
            success: true,
        },
        Err(e) => {
            eprintln!("vm error: {}", e);
            CompileResult {
                source_map,
                diagnostics,
                success: false,
            }
        }
    }
}
