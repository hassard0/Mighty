//! tower-lsp `LanguageServer` implementation. Entry point: [`run_stdio`].

use crate::completion;
use crate::definition;
use crate::diagnostics as diag_module;
use crate::docs::{apply_change, DocStore};
use crate::hover;
use std::sync::Arc;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::{
    notification::PublishDiagnostics, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams, MessageType,
    OneOf, Position, PublishDiagnosticsParams, Range, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};

/// Public LSP backend — holds the [`DocStore`] and a handle to the
/// client (for diagnostic notifications).
pub struct Backend {
    pub client: Client,
    pub docs: Arc<DocStore>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            docs: Arc::new(DocStore::new()),
        }
    }

    async fn publish(&self, params: PublishDiagnosticsParams) {
        self.client
            .send_notification::<PublishDiagnostics>(params)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _params: InitializeParams) -> LspResult<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    ..Default::default()
                }),
                document_formatting_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "sdust-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "sdust-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let text = params.text_document.text;
        let version = params.text_document.version;
        let doc = self.docs.open(uri.clone(), text, version);
        let publish = diag_module::build_publish(uri, &doc);
        self.publish(publish).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let version = params.text_document.version;
        let current = self.docs.get(&uri);
        let (mut source, mut line_index) = match current {
            Some(d) => (d.source.clone(), d.line_index.clone()),
            None => (String::new(), crate::line_index::LineIndex::new("")),
        };
        for change in &params.content_changes {
            match apply_change(&source, &line_index, change) {
                Some(new_src) => {
                    line_index = crate::line_index::LineIndex::new(&new_src);
                    source = new_src;
                }
                None => {
                    // Bad range — replace with the change text as a
                    // pessimistic fallback (treats it as full sync).
                    source = change.text.clone();
                    line_index = crate::line_index::LineIndex::new(&source);
                }
            }
        }
        let doc = self.docs.update(uri.clone(), source, version);
        let publish = diag_module::build_publish(uri, &doc);
        self.publish(publish).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.docs.close(&uri);
        let publish = PublishDiagnosticsParams {
            uri,
            diagnostics: vec![],
            version: None,
        };
        self.publish(publish).await;
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        Ok(hover::hover(&doc, pos))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        Ok(definition::definition(uri, &doc, pos))
    }

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        Ok(completion::complete(&doc, pos))
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        let formatted = sdust_fmt::format(doc.parsed.green.clone());
        if formatted == doc.source {
            return Ok(Some(vec![]));
        }
        // Whole-document replacement: range covers from (0,0) to the
        // end-of-buffer position.
        let (end_line, end_char) = doc
            .line_index
            .offset_to_position(&doc.source, doc.line_index.len());
        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: end_line,
                character: end_char,
            },
        };
        Ok(Some(vec![TextEdit {
            range,
            new_text: formatted,
        }]))
    }
}

/// Convenience: run a tower-lsp server over stdio. Used by
/// `sdust lsp` (see `crates/sdust-cli/src/cmd/lsp.rs`).
pub fn run_stdio() -> i32 {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("sdust lsp: failed to start tokio runtime: {}", e);
            return 1;
        }
    };
    rt.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = LspService::new(Backend::new);
        Server::new(stdin, stdout, socket).serve(service).await;
    });
    0
}
