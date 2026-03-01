use std::collections::HashSet;
use std::path::{Path, PathBuf};

use nudl_ast::ast::{Item, Module};
use nudl_ast::lexer::Lexer;
use nudl_ast::parser::Parser;
use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::source::SourceMap;

/// Default nudl-std path, baked in at compile time from the workspace root.
const DEFAULT_STD_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../nudl-std");

/// Resolve the nudl-std directory. Checks `NUDL_STD_PATH` env var first,
/// then falls back to the compile-time embedded path.
pub fn find_std_root() -> PathBuf {
    if let Ok(path) = std::env::var("NUDL_STD_PATH") {
        return PathBuf::from(path);
    }
    PathBuf::from(DEFAULT_STD_PATH)
}

pub fn resolve_import_path(
    source_dir: &Path,
    import_path: &[String],
    std_root: &Path,
) -> Option<PathBuf> {
    let mut rel_path = PathBuf::new();
    for (i, segment) in import_path.iter().enumerate() {
        if i == import_path.len() - 1 {
            rel_path.push(format!("{}.nudl", segment));
        } else {
            rel_path.push(segment);
        }
    }

    // 1. Relative to source directory
    let candidate = source_dir.join(&rel_path);
    if candidate.exists() {
        return Some(candidate);
    }

    // 2. As directory/lib.nudl relative to source
    let mut dir_path = PathBuf::new();
    for segment in import_path {
        dir_path.push(segment);
    }
    let candidate = source_dir.join(&dir_path).join("lib.nudl");
    if candidate.exists() {
        return Some(candidate);
    }

    // 3. In nudl-std directory
    let std_candidate = std_root.join(&rel_path);
    if std_candidate.exists() {
        return Some(std_candidate);
    }

    // 4. nudl-std directory as directory/lib.nudl
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

pub fn parse_file(
    path: &Path,
    source_map: &mut SourceMap,
    diagnostics: &mut DiagnosticBag,
) -> Option<Module> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return None,
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

/// Resolve imports for a module. Returns the merged module and the list of
/// imported file paths (for building the import graph).
pub fn resolve_imports(
    module: Module,
    source_path: &Path,
    source_map: &mut SourceMap,
    diagnostics: &mut DiagnosticBag,
) -> (Module, Vec<PathBuf>) {
    let source_dir = source_path.parent().unwrap_or(Path::new("."));
    let std_root = find_std_root();
    let mut merged_items = Vec::new();
    let mut imported_files: HashSet<PathBuf> = HashSet::new();
    let mut imported_paths: Vec<PathBuf> = Vec::new();

    // Auto-import prelude unless the source file IS the prelude
    let prelude_path = std_root.join("prelude.nudl");
    let is_prelude = source_path
        .canonicalize()
        .ok()
        .and_then(|sp| prelude_path.canonicalize().ok().map(|pp| sp == pp))
        .unwrap_or(false);

    if !is_prelude && prelude_path.exists() {
        imported_files.insert(prelude_path.clone());
        // Use a separate DiagnosticBag for prelude parsing so prelude errors
        // don't block user code checking
        let mut prelude_diags = DiagnosticBag::new();
        if let Some(prelude_module) = parse_file(&prelude_path, source_map, &mut prelude_diags) {
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
                    continue;
                }
                imported_files.insert(import_path.clone());
                imported_paths.push(import_path.clone());

                if let Some(imported_module) = parse_file(&import_path, source_map, diagnostics) {
                    for imp_item in imported_module.items {
                        if !matches!(&imp_item.node, Item::Import { .. }) {
                            merged_items.push(imp_item);
                        }
                    }
                }
            }
        }
    }

    merged_items.extend(module.items);

    (
        Module {
            items: merged_items,
        },
        imported_paths,
    )
}
