//! tower-lsp `LanguageServer` implementation. Entry point: [`run_stdio`].

use crate::code_actions;
use crate::completion;
use crate::definition;
use crate::diagnostics as diag_module;
use crate::docs::{apply_change, DocStore};
use crate::document_symbols;
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
    DocumentFormattingParams, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InitializedParams, InlayHint, InlayHintParams, InlayHintServerCapabilities,
    MessageType, OneOf, Position, PrepareRenameResponse, PublishDiagnosticsParams, Range,
    RenameOptions, RenameParams, SemanticTokensDeltaParams, SemanticTokensFullDeltaResult,
    SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensRangeParams, SemanticTokensRangeResult, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    SignatureHelp, SignatureHelpOptions, SignatureHelpParams, TextDocumentPositionParams,
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
    /// v0.46 T5: whether the client advertised
    /// `textDocument.definition.linkSupport = true`. When `true`, the
    /// `goto_definition` handler emits the structured
    /// `LocationLink[]` (carrying `originSelectionRange` +
    /// `targetSelectionRange`); when `false`, it downgrades to the
    /// legacy `Location` scalar so v0.2-vintage clients still parse the
    /// payload.
    pub link_support: Arc<std::sync::RwLock<bool>>,
    /// v0.47 T5: whether the client advertised
    /// `workspace.workspaceEdit.documentChanges = true`. When `true`,
    /// rename + codeAction emit the versioned
    /// `documentChanges: Vec<TextDocumentEdit>` shape (so the editor
    /// can refuse stale edits); when `false`, both surfaces fall back
    /// to the legacy `changes: HashMap<Url, Vec<TextEdit>>` shape that
    /// v0.46-vintage IDE L31 clients consume.
    pub document_changes_support: Arc<std::sync::RwLock<bool>>,
    /// v0.47 T5: per-buffer semanticTokens delta cache. Maps
    /// `(uri, version)` → `(result_id, encoded tokens)`. On a delta
    /// request the server diffs the new tokens against the entry whose
    /// `result_id` matches `previous_result_id`; on a miss it returns
    /// the full token array with a fresh `result_id` and stores that
    /// snapshot for the next call.
    ///
    /// Bounded so misbehaving clients can't grow the cache without
    /// limit — see [`SEMANTIC_TOKENS_CACHE_LIMIT`].
    pub semantic_tokens_cache: Arc<std::sync::RwLock<semantic_tokens::DeltaCache>>,
}

/// Server-wide cap on the semanticTokens delta cache. Older snapshots
/// are evicted FIFO when the map exceeds this size. Sized to comfortably
/// hold a few hundred open files without unbounded growth on a long-
/// running session.
pub const SEMANTIC_TOKENS_CACHE_LIMIT: usize = 512;

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            docs: Arc::new(DocStore::new()),
            workspaces: Arc::new(WorkspaceRegistry::new()),
            code_action_config: Arc::new(std::sync::RwLock::new(
                code_actions::CodeActionConfig::default(),
            )),
            // Default to `true` so clients that don't advertise
            // capabilities at all (and modern editors that handle both
            // shapes) get the richer Link form.
            link_support: Arc::new(std::sync::RwLock::new(true)),
            // Default to `true`: modern editors (3.16+) all support
            // documentChanges. v0.46-vintage clients that don't will
            // advertise the absence explicitly during `initialize`.
            document_changes_support: Arc::new(std::sync::RwLock::new(true)),
            semantic_tokens_cache: Arc::new(std::sync::RwLock::new(
                semantic_tokens::DeltaCache::with_capacity(SEMANTIC_TOKENS_CACHE_LIMIT),
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
        // v0.46 T5: capability negotiation for `textDocument.definition.linkSupport`.
        // Defaults to `true` so capability-omitting clients still see
        // the richer Link shape; a client that explicitly advertises
        // `linkSupport = false` falls back to the legacy `Location`
        // scalar form.
        if let Some(client_caps) = params.capabilities.text_document.as_ref() {
            if let Some(def_cap) = client_caps.definition.as_ref() {
                let advertised = def_cap.link_support.unwrap_or(true);
                if let Ok(mut g) = self.link_support.write() {
                    *g = advertised;
                }
            }
        }
        // v0.47 T5: capability negotiation for
        // `workspace.workspaceEdit.documentChanges`. Defaults to `true`
        // so 3.16+ clients (the modern majority) see the versioned
        // shape automatically; v0.46-vintage IDE L31 will keep working
        // because it explicitly advertises `documentChanges: false`
        // and the server downgrades to the legacy `changes` map.
        if let Some(ws_caps) = params.capabilities.workspace.as_ref() {
            if let Some(we_caps) = ws_caps.workspace_edit.as_ref() {
                let advertised = we_caps.document_changes.unwrap_or(true);
                if let Ok(mut g) = self.document_changes_support.write() {
                    *g = advertised;
                }
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
                document_symbol_provider: Some(OneOf::Left(true)),
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
                            // v0.47 T5: advertise `full = { delta: true }`
                            // so clients know to send
                            // `textDocument/semanticTokens/full/delta`
                            // requests with a `previous_result_id`.
                            full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
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
        // v0.47 T5: drop the semanticTokens delta cache entries for
        // this URI — the client won't be sending a /delta against
        // them again, and keeping them only delays eviction of newer
        // entries.
        if let Ok(mut cache) = self.semantic_tokens_cache.write() {
            cache.drop_uri(&uri);
        }
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
        let resp = definition::definition(uri, &doc, pos);
        // v0.46 T5 — back-compat downgrade for clients that don't
        // advertise `textDocument.definition.linkSupport`.
        let link_support = self.link_support.read().map(|g| *g).unwrap_or(true);
        let resp = match resp {
            Some(GotoDefinitionResponse::Link(links)) if !link_support => {
                let locations: Vec<_> = links
                    .into_iter()
                    .map(|l| tower_lsp::lsp_types::Location {
                        uri: l.target_uri,
                        range: l.target_range,
                    })
                    .collect();
                if locations.len() == 1 {
                    Some(GotoDefinitionResponse::Scalar(
                        locations.into_iter().next().unwrap(),
                    ))
                } else {
                    Some(GotoDefinitionResponse::Array(locations))
                }
            }
            other => other,
        };
        Ok(resp)
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
        let uri = params.text_document.uri.clone();
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        // v0.47 T5: store the snapshot in the delta cache so a
        // subsequent /full/delta request can diff against it.
        let Ok(mut cache) = self.semantic_tokens_cache.write() else {
            return Ok(Some(semantic_tokens::full(&doc)));
        };
        Ok(Some(semantic_tokens::full_with_cache(&uri, &doc, &mut cache)))
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

    async fn semantic_tokens_full_delta(
        &self,
        params: SemanticTokensDeltaParams,
    ) -> LspResult<Option<SemanticTokensFullDeltaResult>> {
        let uri = params.text_document.uri.clone();
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        let Ok(mut cache) = self.semantic_tokens_cache.write() else {
            // Cache poisoned — fall back to a full response so the
            // client can resync.
            let result = match semantic_tokens::full(&doc) {
                SemanticTokensResult::Tokens(t) => SemanticTokensFullDeltaResult::Tokens(t),
                SemanticTokensResult::Partial(_) => {
                    return Ok(None);
                }
            };
            return Ok(Some(result));
        };
        Ok(Some(semantic_tokens::full_delta(
            &uri,
            &doc,
            &params.previous_result_id,
            &mut cache,
        )))
    }

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri.clone();
        let pos = params.text_document_position.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        let dc_support = self
            .document_changes_support
            .read()
            .map(|g| *g)
            .unwrap_or(true);
        rename_mod::rename_with_caps(
            uri,
            &doc,
            pos,
            &params.new_name,
            Some(&self.workspaces),
            dc_support,
        )
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
        let dc_support = self
            .document_changes_support
            .read()
            .map(|g| *g)
            .unwrap_or(true);
        let caps = code_actions::WorkspaceEditCaps {
            document_changes: dc_support,
        };
        // v0.35 T3 — honor `context.only` so editors that send a
        // `source.fixAll.mighty` filter get the bulk-apply action.
        // v0.47 T5 — pass `WorkspaceEditCaps` so the returned
        // `WorkspaceEdit`s use the versioned `documentChanges` shape
        // when the client advertises support.
        Ok(Some(code_actions::code_actions_with_filter_caps(
            &uri,
            &doc,
            params.range,
            &params.context.diagnostics,
            params.context.only.as_deref(),
            cfg,
            caps,
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

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let Some(doc) = self.docs.get(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(document_symbols::document_symbols(&doc))
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
