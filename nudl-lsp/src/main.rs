use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use nudl_ast::ast::{Item, Module};
use nudl_ast::lexer::Lexer;
use nudl_ast::parser::Parser;
use nudl_bc::checker::Checker;
use nudl_core::diagnostic::{DiagnosticBag, DiagnosticReport, Severity};
use nudl_core::package;
use nudl_core::source::SourceMap;
use nudl_core::span::FileId;

struct NudlLanguageServer {
    client: Client,
    documents: Mutex<HashMap<Url, String>>,
}

impl NudlLanguageServer {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
        }
    }

    fn check_document(&self, uri: &Url, content: &str) -> Vec<Diagnostic> {
        let mut source_map = SourceMap::new();
        let mut diagnostics = DiagnosticBag::new();

        let file_id = source_map.add_file(uri.path().into(), content.to_string());

        let (tokens, lex_diags) = Lexer::new(content, file_id).tokenize();
        diagnostics.merge(lex_diags);

        if !diagnostics.has_errors() {
            let (module, parse_diags) = Parser::new(tokens).parse_module();
            diagnostics.merge(parse_diags);

            if !diagnostics.has_errors() {
                // Resolve imports and auto-import prelude
                let source_path = PathBuf::from(uri.path());
                let module =
                    resolve_imports(module, &source_path, &mut source_map, &mut diagnostics);

                if !diagnostics.has_errors() {
                    // Determine if this file needs a main function
                    let require_main = Self::is_entry_file(&source_path);

                    let (_checked, check_diags) =
                        Checker::new().require_main(require_main).check(&module);
                    diagnostics.merge(check_diags);
                }
            }
        }

        convert_diagnostics(&diagnostics, &source_map, file_id)
    }

    /// Check whether the given file is an entry point of a nudl package.
    /// If a nudl.toml exists and this file matches any [[bin]] target, returns true.
    /// If no nudl.toml exists, returns true (standalone files need main).
    fn is_entry_file(file_path: &Path) -> bool {
        let file_dir = file_path.parent().unwrap_or(Path::new("."));
        match package::discover_package(file_dir) {
            Some((config, package_dir)) => {
                let canonical_file = file_path.canonicalize().ok();
                for bin in &config.bin {
                    let bin_path = config.resolve_bin_path(bin, &package_dir);
                    let matches = match (&canonical_file, bin_path.canonicalize().ok()) {
                        (Some(a), Some(b)) => a == &b,
                        _ => file_path == bin_path,
                    };
                    if matches {
                        return true;
                    }
                }
                // File is not a bin target — it's a library/module, no main required
                false
            }
            // No package file found — standalone file, require main
            None => true,
        }
    }
}

// --- Import Resolution (mirrors nudl-cli pipeline) ---

/// Default nudl-std path, baked in at compile time from the workspace root.
const DEFAULT_STD_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../nudl-std");

/// Resolve the nudl-std directory. Checks `NUDL_STD_PATH` env var first,
/// then falls back to the compile-time embedded path.
fn find_std_root() -> PathBuf {
    if let Ok(path) = std::env::var("NUDL_STD_PATH") {
        return PathBuf::from(path);
    }
    PathBuf::from(DEFAULT_STD_PATH)
}

fn resolve_import_path(
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

fn parse_file(
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

fn resolve_imports(
    module: Module,
    source_path: &Path,
    source_map: &mut SourceMap,
    diagnostics: &mut DiagnosticBag,
) -> Module {
    let source_dir = source_path.parent().unwrap_or(Path::new("."));
    let std_root = find_std_root();
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

    Module {
        items: merged_items,
    }
}

// --- Diagnostic conversion ---

fn convert_diagnostics(
    bag: &DiagnosticBag,
    source_map: &SourceMap,
    file_id: FileId,
) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    for report in bag.reports() {
        // Only show diagnostics from the current file
        let has_label_in_file = report
            .labels
            .iter()
            .any(|label| label.span.file_id == file_id);
        if !has_label_in_file && !report.labels.is_empty() {
            continue;
        }

        let severity = match report.info.severity {
            Severity::Error => Some(DiagnosticSeverity::ERROR),
            Severity::Warning => Some(DiagnosticSeverity::WARNING),
            Severity::Info => Some(DiagnosticSeverity::INFORMATION),
        };

        let range = report_range(report, source_map, file_id);

        result.push(Diagnostic {
            range,
            severity,
            code: Some(NumberOrString::Number(report.info.code as i32)),
            source: Some("nudl".into()),
            message: report.message.clone(),
            ..Default::default()
        });
    }

    result
}

fn report_range(report: &DiagnosticReport, source_map: &SourceMap, file_id: FileId) -> Range {
    if let Some(label) = report.labels.first() {
        let span = label.span;
        if span.file_id == file_id {
            let file = source_map.get_file(span.file_id);

            if span.is_empty() {
                let offset = span.start.min(file.content.len().saturating_sub(1) as u32);
                let (line, col) = file.line_col(offset);
                let pos = Position::new(line - 1, col - 1);
                return Range {
                    start: pos,
                    end: pos,
                };
            }

            let (start_line, start_col) = file.line_col(span.start);
            let (end_line, end_col) = file.line_col(span.end.min(file.content.len() as u32));
            return Range {
                start: Position::new(start_line - 1, start_col - 1),
                end: Position::new(end_line - 1, end_col - 1),
            };
        }
    }

    Range {
        start: Position::new(0, 0),
        end: Position::new(0, 0),
    }
}

// --- LSP Protocol Implementation ---

#[tower_lsp::async_trait]
impl LanguageServer for NudlLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "nudl-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "nudl language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let content = params.text_document.text.clone();

        {
            let mut docs = self.documents.lock().unwrap();
            docs.insert(uri.clone(), content.clone());
        }

        let diagnostics = self.check_document(&uri, &content);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();

        if let Some(change) = params.content_changes.into_iter().last() {
            let content = change.text;

            {
                let mut docs = self.documents.lock().unwrap();
                docs.insert(uri.clone(), content.clone());
            }

            let diagnostics = self.check_document(&uri, &content);
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;

        {
            let mut docs = self.documents.lock().unwrap();
            docs.remove(&uri);
        }

        self.client.publish_diagnostics(uri, vec![], None).await;
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| NudlLanguageServer::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
