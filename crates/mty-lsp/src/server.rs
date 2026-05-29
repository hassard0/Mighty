//! tower-lsp `LanguageServer` implementation. Entry point: [`run_stdio`].

use crate::code_actions;
use crate::completion;
use crate::definition;
use crate::diagnostics as diag_module;
use crate::docs::{apply_change, DocStore};
use crate::hover;
use crate::inlay_hints;
use crate::rename as rename_mod;
use crate::semantic_tokens;
use crate::signature_help as sig_help;
use crate::workspace::{uri_to_path, WorkspaceRegistry};
use std::sync::Arc;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::{
    notification::PublishDiagnostics, CodeActionOptions, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CompletionOptions, CompletionParams,
    CompletionResponse, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams, InlayHint,
    InlayHintParams, InlayHintServerCapabilities, MessageType, OneOf, Position,
    PrepareRenameResponse, PublishDiagnosticsParams, Range, RenameOptions, RenameParams,
    SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensRangeParams, SemanticTokensRangeResult, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo, SignatureHelp,
    SignatureHelpOptions, SignatureHelpParams, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, WorkDoneProgressOptions,
    WorkspaceEdit, WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};

/// Public LSP backend — holds the [`DocStore`], the workspace
/// registry (v0.8), and a handle to the client (for diagnostic
/// notifications).
pub struct Backend {
    pub client: Client,
    pub docs: Arc<DocStore>,
    pub workspaces: Arc<WorkspaceRegistry>,
    /// v0.34 T2: per-session CodeAction tunables. Updated from the
    /// client's `initializationOptions` JSON in `initialize`.
    pub code_action_config: Arc<std::sync::RwLock<code_actions::CodeActionConfig>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            docs: Arc::new(DocStore::new()),
            workspaces: Arc::new(WorkspaceRegistry::new()),
            code_action_config: Arc::new(std::sync::RwLock::new(
                code_actions::CodeActionConfig::default(),
            )),
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
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        // v0.8: seed the workspace registry from the workspaceFolders
        // the client supplied at startup so cross-file rename has an
        // index ready before the first request.
        if let Some(folders) = params.workspace_folders {
            for f in folders {
                if let Some(p) = uri_to_path(&f.uri) {
                    self.workspaces.add_folder(p);
                }
            }
        } else if let Some(uri) = params.root_uri.as_ref() {
            if let Some(p) = uri_to_path(uri) {
                self.workspaces.add_folder(p);
            }
        }
        // v0.34 T2: read `mighty.codeAction.confidenceThreshold` from
        // the client's `initializationOptions` so the user can opt
        // into a wider (or narrower) suggestion list.
        if let Some(opts) = &params.initialization_options {
            let cfg = code_actions::CodeActionConfig::from_initialization_options(opts);
            if let Ok(mut g) = self.code_action_config.write() {
                *g = cfg;
            }
        }
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
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        // v0.34 T2: advertise every kind the envelope-
                        // driven actions can return so editors that
                        // filter on `only` (refactor menus, source
                        // actions) see them.
                        code_action_kinds: Some(code_actions::supported_code_action_kinds()),
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                        resolve_provider: Some(false),
                    },
                )),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![",".to_string()]),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            legend: semantic_tokens::legend(),
                            range: Some(true),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
                    tower_lsp::lsp_types::InlayHintOptions {
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                        resolve_provider: Some(false),
                    },
                ))),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "mty-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "mty-lsp initialized (v0.5)")
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
        // v0.8: keep the workspace index in sync so cross-file rename
        // sees the unsaved buffer.
        self.workspaces.update_open(&uri, doc.clone());
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
        self.workspaces.update_open(&uri, doc.clone());
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
        let formatted = mty_fmt::format(doc.parsed.green.clone());
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

    // ---------- v0.5 capabilities ----------

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokensResult>> {
        let Some(doc) = self.docs.get(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(Some(semantic_tokens::full(&doc)))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> LspResult<Option<SemanticTokensRangeResult>> {
        let Some(doc) = self.docs.get(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(Some(semantic_tokens::range(&doc, params.range)))
    }

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri.clone();
        let pos = params.text_document_position.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        rename_mod::rename_with_workspace(uri, &doc, pos, &params.new_name, Some(&self.workspaces))
            .map(Some)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> LspResult<Option<PrepareRenameResponse>> {
        let Some(doc) = self.docs.get(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(rename_mod::prepare(&doc, params.position))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let Some(doc) = self.docs.get(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(Some(inlay_hints::inlay_hints(&doc, params.range)))
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.clone();
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        let cfg = self
            .code_action_config
            .read()
            .map(|g| *g)
            .unwrap_or_default();
        // v0.35 T3 — honor `context.only` so editors that send a
        // `source.fixAll.mighty` filter get the bulk-apply action.
        Ok(Some(code_actions::code_actions_with_filter(
            &uri,
            &doc,
            params.range,
            &params.context.diagnostics,
            params.context.only.as_deref(),
            cfg,
        )))
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> LspResult<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        Ok(sig_help::signature_help(&doc, pos))
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        // v0.8: add scanned indexes for newly-added folders, drop
        // indexes for removed folders.
        for added in &params.event.added {
            if let Some(p) = uri_to_path(&added.uri) {
                self.workspaces.add_folder(p);
            }
        }
        for removed in &params.event.removed {
            if let Some(p) = uri_to_path(&removed.uri) {
                self.workspaces.remove_folder(&p);
            }
        }
        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "workspace folders changed (+{} / -{}); cross-file index refreshed",
                    params.event.added.len(),
                    params.event.removed.len()
                ),
            )
            .await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        // v0.8: refresh the workspace index for each changed file so
        // cross-file rename / go-to-def stays consistent.
        for change in &params.changes {
            match change.typ {
                tower_lsp::lsp_types::FileChangeType::DELETED => {
                    self.workspaces.drop_uri(&change.uri);
                }
                tower_lsp::lsp_types::FileChangeType::CREATED
                | tower_lsp::lsp_types::FileChangeType::CHANGED => {
                    self.workspaces.refresh_from_disk(&change.uri);
                }
                _ => {}
            }
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("watched file changed: {}", change.uri),
                )
                .await;
        }
    }
}

/// Convenience: run a tower-lsp server over stdio. Used by
/// `mty lsp` (see `crates/mty-cli/src/cmd/lsp.rs`).
pub fn run_stdio() -> i32 {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("mty lsp: failed to start tokio runtime: {}", e);
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
