mod diagnostics;
mod handlers;
mod imports;
mod server;

use tower_lsp::{LspService, Server};

use server::NudlLanguageServer;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| NudlLanguageServer::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
