use tower_lsp::{LspService, Server};

use fluent_lsp::Backend;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::build(Backend::new)
        .custom_method("$/setTrace", Backend::set_trace)
        .finish();
    Server::new(stdin, stdout, socket).serve(service).await;
}
