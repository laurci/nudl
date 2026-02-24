use std::collections::HashMap;
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use nudl_ast::lexer::Lexer;
use nudl_ast::parser::Parser;
use nudl_bc::checker::Checker;
use nudl_core::diagnostic::{DiagnosticBag, DiagnosticReport, Severity};
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
                let (_checked, check_diags) = Checker::new().check(&module);
                diagnostics.merge(check_diags);
            }
        }

        convert_diagnostics(&diagnostics, &source_map, file_id)
    }
}

fn convert_diagnostics(
    bag: &DiagnosticBag,
    source_map: &SourceMap,
    file_id: FileId,
) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    for report in bag.reports() {
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
                // Empty span (e.g. EOF token) — point to the end of the file.
                // Clamp to last valid offset so line_col doesn't go out of bounds.
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
                // line_col returns 1-based, LSP uses 0-based
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

        // Full sync: take the last change (which is the entire content)
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

        // Clear diagnostics
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
