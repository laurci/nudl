use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::request::{GotoImplementationParams, GotoImplementationResponse};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use nudl_ast::ast::{InterfaceMethodDef, Item};
use nudl_ast::lexer::Lexer;
use nudl_ast::parser::Parser;
use nudl_bc::checker::{Checker, FunctionSig};
use nudl_bc::symbol_table::{SymbolKind, SymbolTable};
use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::package;
use nudl_core::source::SourceMap;
use nudl_core::span::{FileId, Span};
use nudl_core::types::{TypeId, TypeInterner};

use crate::diagnostics::convert_diagnostics;
use crate::handlers;
use crate::imports;

/// Exported symbol from a project file (for import suggestions).
#[derive(Debug, Clone)]
pub struct ExportedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub is_pub: bool,
    pub import_path: Vec<String>,
}

/// Cached check result for a single file.
pub struct FileCheckResult {
    pub symbol_table: SymbolTable,
    pub types: TypeInterner,
    pub functions: HashMap<String, FunctionSig>,
    pub structs: HashMap<String, TypeId>,
    pub enums: HashMap<String, TypeId>,
    pub interfaces: HashMap<String, TypeId>,
    pub interface_impls: HashMap<String, Vec<String>>,
    pub interface_method_defs: HashMap<String, Vec<InterfaceMethodDef>>,
    pub item_def_spans: HashMap<String, Span>,
    pub source_map: SourceMap,
    pub file_id: FileId,
    pub diagnostics: Vec<Diagnostic>,
    pub project_symbols: HashMap<Url, Vec<ExportedSymbol>>,
}

pub struct ServerState {
    /// Open file contents
    pub documents: HashMap<Url, String>,
    /// Cached check results per file
    pub file_cache: HashMap<Url, FileCheckResult>,
    /// File → its imports (as file paths)
    pub import_graph: HashMap<Url, HashSet<Url>>,
    /// File → files that import it (reverse dependency)
    pub reverse_imports: HashMap<Url, HashSet<Url>>,
    /// Project-wide exported symbols
    pub project_symbols: HashMap<Url, Vec<ExportedSymbol>>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            file_cache: HashMap::new(),
            import_graph: HashMap::new(),
            reverse_imports: HashMap::new(),
            project_symbols: HashMap::new(),
        }
    }
}

pub struct NudlLanguageServer {
    pub client: Client,
    pub state: Mutex<ServerState>,
}

impl NudlLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Mutex::new(ServerState::new()),
        }
    }

    /// Check a document and return the check result + diagnostics.
    pub fn check_document(&self, uri: &Url, content: &str) -> FileCheckResult {
        let mut source_map = SourceMap::new();
        let mut diagnostics = DiagnosticBag::new();

        let file_id = source_map.add_file(uri.path().into(), content.to_string());

        let (tokens, lex_diags) = Lexer::new(content, file_id).tokenize();
        diagnostics.merge(lex_diags);

        let mut imported_uris = HashSet::new();
        let mut symbol_table = SymbolTable::new();
        let mut types = TypeInterner::new();
        let mut functions = HashMap::new();
        let mut structs = HashMap::new();
        let mut enums = HashMap::new();
        let mut interfaces = HashMap::new();
        let mut interface_impls = HashMap::new();
        let mut interface_method_defs = HashMap::new();
        let mut item_def_spans = HashMap::new();

        if !diagnostics.has_errors() {
            let (module, parse_diags) = Parser::new(tokens).parse_module();
            diagnostics.merge(parse_diags);

            if !diagnostics.has_errors() {
                // Resolve imports and auto-import prelude
                let source_path = PathBuf::from(uri.path());
                let (module, imported_paths) = imports::resolve_imports(
                    module,
                    &source_path,
                    &mut source_map,
                    &mut diagnostics,
                );

                // Build import URIs for the import graph
                for path in &imported_paths {
                    if let Ok(import_uri) = Url::from_file_path(path) {
                        imported_uris.insert(import_uri);
                    }
                }

                if !diagnostics.has_errors() {
                    // Determine if this file needs a main function
                    let require_main = Self::is_entry_file(&source_path);

                    let (checked, check_diags) =
                        Checker::new().require_main(require_main).check(&module);
                    diagnostics.merge(check_diags);

                    symbol_table = checked.symbol_table;
                    types = checked.types;
                    functions = checked.functions;
                    structs = checked.structs;
                    enums = checked.enums;
                    interfaces = checked.interfaces;
                    interface_impls = checked.interface_impls;
                    interface_method_defs = checked.interface_method_defs;
                    item_def_spans = checked.item_def_spans;
                }
            }
        }

        let lsp_diagnostics = convert_diagnostics(&diagnostics, &source_map, file_id);

        // Update import graph
        {
            let mut state = self.state.lock().unwrap();
            // Clear old reverse imports for this file
            if let Some(old_imports) = state.import_graph.get(uri) {
                for old_import in old_imports.clone() {
                    if let Some(reverse) = state.reverse_imports.get_mut(&old_import) {
                        reverse.remove(uri);
                    }
                }
            }
            // Set new imports
            state
                .import_graph
                .insert(uri.clone(), imported_uris.clone());
            // Update reverse imports
            for import_uri in &imported_uris {
                state
                    .reverse_imports
                    .entry(import_uri.clone())
                    .or_default()
                    .insert(uri.clone());
            }
        }

        // Get project symbols for import suggestions
        let project_symbols = {
            let state = self.state.lock().unwrap();
            state.project_symbols.clone()
        };

        FileCheckResult {
            symbol_table,
            types,
            functions,
            structs,
            enums,
            interfaces,
            interface_impls,
            interface_method_defs,
            item_def_spans,
            source_map,
            file_id,
            diagnostics: lsp_diagnostics,
            project_symbols,
        }
    }

    /// Check whether the given file is an entry point of a nudl package.
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
                false
            }
            None => true,
        }
    }

    /// Get all transitive reverse dependencies (files that import this file).
    fn get_dependents(&self, uri: &Url) -> HashSet<Url> {
        let state = self.state.lock().unwrap();
        let mut visited = HashSet::new();
        let mut queue = vec![uri.clone()];
        while let Some(current) = queue.pop() {
            if let Some(importers) = state.reverse_imports.get(&current) {
                for importer in importers {
                    if visited.insert(importer.clone()) {
                        queue.push(importer.clone());
                    }
                }
            }
        }
        visited
    }

    /// Scan project .nudl files and build the project-wide symbol index.
    pub fn scan_project_symbols(&self) {
        // Find project root by looking for nudl.toml
        let state = self.state.lock().unwrap();
        let uris: Vec<Url> = state.documents.keys().cloned().collect();
        drop(state);

        if uris.is_empty() {
            return;
        }

        // Find all .nudl files in the project directory
        let first_path = uris.first().and_then(|u| {
            let p = PathBuf::from(u.path());
            p.parent().map(|p| p.to_path_buf())
        });
        let project_dir = match first_path {
            Some(dir) => {
                // Walk up to find nudl.toml or use the dir
                let mut d = dir.clone();
                loop {
                    if d.join("nudl.toml").exists() {
                        break d;
                    }
                    if !d.pop() {
                        break dir;
                    }
                }
            }
            None => return,
        };

        let mut new_symbols: HashMap<Url, Vec<ExportedSymbol>> = HashMap::new();

        // Walk directory for .nudl files
        if let Ok(entries) = walkdir(&project_dir) {
            for path in entries {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let uri = match Url::from_file_path(&path) {
                        Ok(u) => u,
                        Err(_) => continue,
                    };

                    // Compute the import path from the project directory
                    let rel = path.strip_prefix(&project_dir).unwrap_or(&path);
                    let import_path: Vec<String> = rel
                        .with_extension("")
                        .components()
                        .filter_map(|c| c.as_os_str().to_str().map(String::from))
                        .collect();

                    // Quick parse to extract top-level items
                    let file_id = FileId(0);
                    let (tokens, _) = Lexer::new(&content, file_id).tokenize();
                    let (module, _) = Parser::new(tokens).parse_module();

                    let mut symbols = Vec::new();
                    for item in &module.items {
                        match &item.node {
                            Item::FnDef { name, is_pub, .. } => {
                                symbols.push(ExportedSymbol {
                                    name: name.clone(),
                                    kind: SymbolKind::Function,
                                    is_pub: *is_pub,
                                    import_path: import_path.clone(),
                                });
                            }
                            Item::StructDef { name, is_pub, .. } => {
                                symbols.push(ExportedSymbol {
                                    name: name.clone(),
                                    kind: SymbolKind::Struct,
                                    is_pub: *is_pub,
                                    import_path: import_path.clone(),
                                });
                            }
                            Item::EnumDef { name, is_pub, .. } => {
                                symbols.push(ExportedSymbol {
                                    name: name.clone(),
                                    kind: SymbolKind::Enum,
                                    is_pub: *is_pub,
                                    import_path: import_path.clone(),
                                });
                            }
                            Item::InterfaceDef { name, is_pub, .. } => {
                                symbols.push(ExportedSymbol {
                                    name: name.clone(),
                                    kind: SymbolKind::Interface,
                                    is_pub: *is_pub,
                                    import_path: import_path.clone(),
                                });
                            }
                            _ => {}
                        }
                    }

                    if !symbols.is_empty() {
                        new_symbols.insert(uri, symbols);
                    }
                }
            }
        }

        let mut state = self.state.lock().unwrap();
        state.project_symbols = new_symbols;
    }
}

/// Simple recursive directory walker for .nudl files.
fn walkdir(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden dirs, target, tmp, etc.
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.') || name == "target" || name == "tmp" {
                    continue;
                }
                results.extend(walkdir(&path)?);
            } else if path.extension().and_then(|e| e.to_str()) == Some("nudl") {
                results.push(path);
            }
        }
    }
    Ok(results)
}

#[tower_lsp::async_trait]
impl LanguageServer for NudlLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into(), ":".into()]),
                    ..Default::default()
                }),
                references_provider: Some(OneOf::Left(true)),
                implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
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

        // Scan project symbols on startup
        self.scan_project_symbols();
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let content = params.text_document.text.clone();

        {
            let mut state = self.state.lock().unwrap();
            state.documents.insert(uri.clone(), content.clone());
        }

        let result = self.check_document(&uri, &content);
        let diagnostics = result.diagnostics.clone();

        {
            let mut state = self.state.lock().unwrap();
            state.file_cache.insert(uri.clone(), result);
        }

        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();

        if let Some(change) = params.content_changes.into_iter().last() {
            let content = change.text;

            {
                let mut state = self.state.lock().unwrap();
                state.documents.insert(uri.clone(), content.clone());
            }

            // Re-check the changed file
            let result = self.check_document(&uri, &content);
            let diagnostics = result.diagnostics.clone();
            let has_check_data =
                !result.symbol_table.definitions.is_empty() || !result.functions.is_empty();

            {
                let mut state = self.state.lock().unwrap();
                if has_check_data {
                    // Full check succeeded — replace the cache entirely
                    state.file_cache.insert(uri.clone(), result);
                } else {
                    // Parse/check failed — keep old result for completions but update diagnostics
                    if let Some(cached) = state.file_cache.get_mut(&uri) {
                        cached.diagnostics = diagnostics.clone();
                    } else {
                        state.file_cache.insert(uri.clone(), result);
                    }
                }
            }

            self.client
                .publish_diagnostics(uri.clone(), diagnostics, None)
                .await;

            // Cross-file invalidation: re-check dependent files
            let dependents = self.get_dependents(&uri);
            for dep_uri in dependents {
                let content = {
                    let state = self.state.lock().unwrap();
                    state.documents.get(&dep_uri).cloned()
                };
                if let Some(content) = content {
                    let dep_result = self.check_document(&dep_uri, &content);
                    let dep_diagnostics = dep_result.diagnostics.clone();

                    {
                        let mut state = self.state.lock().unwrap();
                        state.file_cache.insert(dep_uri.clone(), dep_result);
                    }

                    self.client
                        .publish_diagnostics(dep_uri, dep_diagnostics, None)
                        .await;
                }
            }
        }
    }

    async fn did_save(&self, _params: DidSaveTextDocumentParams) {
        // Re-scan project symbols on save
        self.scan_project_symbols();
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;

        {
            let mut state = self.state.lock().unwrap();
            state.documents.remove(&uri);
            state.file_cache.remove(&uri);
        }

        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let state = self.state.lock().unwrap();
        if let Some(result) = state.file_cache.get(&uri) {
            Ok(handlers::handle_goto_definition(result, position))
        } else {
            Ok(None)
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let state = self.state.lock().unwrap();
        if let Some(result) = state.file_cache.get(&uri) {
            Ok(handlers::handle_hover(result, position))
        } else {
            Ok(None)
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        let state = self.state.lock().unwrap();
        if let Some(result) = state.file_cache.get(&uri) {
            Ok(handlers::handle_references(
                result,
                position,
                include_declaration,
                &state.file_cache,
            ))
        } else {
            Ok(None)
        }
    }

    async fn goto_implementation(
        &self,
        params: GotoImplementationParams,
    ) -> Result<Option<GotoImplementationResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let state = self.state.lock().unwrap();
        if let Some(result) = state.file_cache.get(&uri) {
            Ok(handlers::handle_goto_implementation(
                result,
                position,
                &state.file_cache,
            ))
        } else {
            Ok(None)
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let trigger_char = params
            .context
            .as_ref()
            .and_then(|ctx| ctx.trigger_character.as_deref());

        let state = self.state.lock().unwrap();
        if let Some(result) = state.file_cache.get(&uri) {
            let live_source = state.documents.get(&uri).map(|s| s.as_str());
            Ok(handlers::handle_completion(
                result,
                position,
                trigger_char,
                live_source,
            ))
        } else {
            Ok(None)
        }
    }
}
