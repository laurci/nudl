use std::collections::HashSet;
use std::path::{Path, PathBuf};

use nudl_ast::ast::{Item, Module};
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

/// Resolve an import path to a file path.
/// Searches:
/// 1. Relative to the source file's directory
/// 2. In the nudl-std directory
fn resolve_import_path(
    source_dir: &Path,
    import_path: &[String],
    std_root: &Path,
) -> Option<PathBuf> {
    // Build the relative path from import segments
    let mut rel_path = PathBuf::new();
    for (i, segment) in import_path.iter().enumerate() {
        if i == import_path.len() - 1 {
            // Last segment: try as a file
            rel_path.push(format!("{}.nudl", segment));
        } else {
            rel_path.push(segment);
        }
    }

    // 1. Try relative to source directory
    let candidate = source_dir.join(&rel_path);
    if candidate.exists() {
        return Some(candidate);
    }

    // 2. Try relative to source directory as directory/lib.nudl
    let mut dir_path = PathBuf::new();
    for segment in import_path {
        dir_path.push(segment);
    }
    let candidate = source_dir.join(&dir_path).join("lib.nudl");
    if candidate.exists() {
        return Some(candidate);
    }

    // 3. Try in nudl-std directory
    let std_candidate = std_root.join(&rel_path);
    if std_candidate.exists() {
        return Some(std_candidate);
    }

    // 4. Try nudl-std directory as directory/lib.nudl
    let std_candidate = std_root.join(&dir_path).join("lib.nudl");
    if std_candidate.exists() {
        return Some(std_candidate);
    }

    // 5. For "std" prefix, try nudl-std directly
    if import_path.first().map(|s| s.as_str()) == Some("std") && import_path.len() >= 2 {
        let mut std_rel = PathBuf::new();
        for (i, segment) in import_path[1..].iter().enumerate() {
            if i == import_path.len() - 2 {
                std_rel.push(format!("{}.nudl", segment));
            } else {
                std_rel.push(segment);
            }
        }
        let candidate = std_root.join(&std_rel);
        if candidate.exists() {
            return Some(candidate);
        }

        let mut dir_path = PathBuf::new();
        for segment in &import_path[1..] {
            dir_path.push(segment);
        }
        let candidate = std_root.join(&dir_path).join("lib.nudl");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

/// Default nudl-std path, baked in at compile time from the workspace root.
const DEFAULT_STD_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../nudl-std");

/// Resolve the nudl-std directory. Uses the explicit override if provided,
/// then checks `NUDL_STD_PATH` env var, then falls back to the compile-time default.
fn find_std_root(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Ok(path) = std::env::var("NUDL_STD_PATH") {
        return PathBuf::from(path);
    }
    PathBuf::from(DEFAULT_STD_PATH)
}

/// Parse a file and return its module, accumulating diagnostics.
fn parse_file(
    path: &Path,
    source_map: &mut SourceMap,
    diagnostics: &mut DiagnosticBag,
) -> Option<Module> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not read '{}': {}", path.display(), e);
            return None;
        }
    };

    let file_id = source_map.add_file(path.to_path_buf(), content.clone());

    let (tokens, lex_diags) = Lexer::new(&content, file_id).tokenize();
    diagnostics.merge(lex_diags);
    if diagnostics.has_errors() {
        return None;
    }

    let (module, parse_diags) = Parser::new(tokens).parse_module();
    diagnostics.merge(parse_diags);
    if diagnostics.has_errors() {
        return None;
    }

    Some(module)
}

/// Resolve and merge imports from a module.
/// Returns the merged module with imported items prepended.
/// Automatically injects the prelude unless the source IS the prelude.
fn resolve_imports(
    module: Module,
    source_path: &Path,
    std_path: Option<&Path>,
    source_map: &mut SourceMap,
    diagnostics: &mut DiagnosticBag,
) -> Module {
    let source_dir = source_path.parent().unwrap_or(Path::new("."));
    let std_root = find_std_root(std_path);
    let mut merged_items = Vec::new();
    let mut imported_files: HashSet<PathBuf> = HashSet::new();

    // Auto-import prelude unless the source file IS the prelude
    let prelude_path = std_root.join("prelude.nudl");
    let is_prelude = source_path
        .canonicalize()
        .ok()
        .and_then(|sp| prelude_path.canonicalize().ok().map(|pp| sp == pp))
        .unwrap_or(false);

    if !is_prelude && prelude_path.exists() {
        imported_files.insert(prelude_path.clone());
        if let Some(prelude_module) = parse_file(&prelude_path, source_map, diagnostics) {
            for imp_item in prelude_module.items {
                if !matches!(&imp_item.node, Item::Import { .. }) {
                    merged_items.push(imp_item);
                }
            }
        }
    }

    // Process user imports
    for item in &module.items {
        if let Item::Import { path, .. } = &item.node {
            if let Some(import_path) = resolve_import_path(source_dir, path, &std_root) {
                if imported_files.contains(&import_path) {
                    continue; // Skip duplicate imports
                }
                imported_files.insert(import_path.clone());

                if let Some(imported_module) = parse_file(&import_path, source_map, diagnostics) {
                    // Add all non-import items from the imported module
                    for imp_item in imported_module.items {
                        if !matches!(&imp_item.node, Item::Import { .. }) {
                            merged_items.push(imp_item);
                        }
                    }
                }
            }
            // Silently skip unresolved imports - the stdlib prelude will be auto-loaded
        }
    }

    // Add all original items (including imports, which checker will skip)
    merged_items.extend(module.items);

    Module {
        items: merged_items,
    }
}

pub fn check(source_path: &Path, std_path: Option<&Path>, dump: &DumpOptions) -> PipelineResult {
    let mut source_map = SourceMap::new();
    let mut diagnostics = DiagnosticBag::new();

    let module = match parse_file(source_path, &mut source_map, &mut diagnostics) {
        Some(m) => m,
        None => {
            return PipelineResult {
                source_map,
                diagnostics,
            };
        }
    };

    let module = resolve_imports(
        module,
        source_path,
        std_path,
        &mut source_map,
        &mut diagnostics,
    );
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
    std_path: Option<&Path>,
    release: bool,
    dump: &DumpOptions,
) -> CompileResult {
    let mut source_map = SourceMap::new();
    let mut diagnostics = DiagnosticBag::new();

    let module = match parse_file(source_path, &mut source_map, &mut diagnostics) {
        Some(m) => m,
        None => {
            return CompileResult {
                source_map,
                diagnostics,
                success: false,
            };
        }
    };

    let module = resolve_imports(
        module,
        source_path,
        std_path,
        &mut source_map,
        &mut diagnostics,
    );
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

pub fn run_vm(source_path: &Path, std_path: Option<&Path>, dump: &DumpOptions) -> CompileResult {
    let mut source_map = SourceMap::new();
    let mut diagnostics = DiagnosticBag::new();

    let module = match parse_file(source_path, &mut source_map, &mut diagnostics) {
        Some(m) => m,
        None => {
            return CompileResult {
                source_map,
                diagnostics,
                success: false,
            };
        }
    };

    let module = resolve_imports(
        module,
        source_path,
        std_path,
        &mut source_map,
        &mut diagnostics,
    );
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
