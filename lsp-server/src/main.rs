#![warn(clippy::all, clippy::pedantic)]

use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

mod indexer;
mod parser_backend;
mod parser_frontend;
mod scanner;
mod syntax;

use crate::indexer::{LocationInfo, ProjectIndex};
use scanner::{is_tauri_project, scan_workspace_files};
use std::sync::atomic::{AtomicBool, Ordering};
use syntax::{load_syntax, Behavior, CommandSyntax, EntityType};

async fn process_file_index(
    path: PathBuf,
    command_syntax: &CommandSyntax,
    project_index: &Arc<ProjectIndex>,
) -> Option<usize> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    if !["rs", "ts", "tsx", "js", "jsx", "vue"].contains(&ext) {
        return None;
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return None,
    };

    let file_index = match ext {
        "rs" => parser_backend::parse(&path, &content, &command_syntax.backend),
        _ => parser_frontend::parse(&path, &content, &command_syntax.frontend),
    };

    let count = file_index.findings.len();

    project_index.add_file(file_index);

    Some(count)
}

#[derive(Debug)]
struct Backend {
    client: Client,
    syntax_config_path: PathBuf,
    command_syntax: OnceCell<CommandSyntax>,
    workspace_root: OnceCell<PathBuf>,
    project_index: Arc<ProjectIndex>,
    is_developer_mode_active: Arc<AtomicBool>,
}

impl Backend {
    /// Helper: Checks if the server is fully initialized (Config loaded)
    fn is_ready(&self) -> bool {
        self.command_syntax.get().is_some()
    }

    async fn on_change(&self, path: PathBuf) {
        let command_syntax = match self.command_syntax.get() {
            Some(s) => s,
            None => return,
        };

        if let Some(count) =
            process_file_index(path.clone(), command_syntax, &self.project_index).await
        {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!(
                        "🔄 Updated index for {:?} ({} findings)",
                        path.file_name().unwrap_or_default(),
                        count
                    ),
                )
                .await;
        }
    }

    async fn log_dev_info(&self, message: &str) {
        if self.is_developer_mode_active.load(Ordering::Relaxed) {
            self.client.log_message(MessageType::INFO, message).await;
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let root_path = if let Some(root_uri) = params.root_uri {
            root_uri.to_file_path().ok()
        } else {
            #[allow(deprecated)]
            params.root_path.map(PathBuf::from)
        };

        let mut is_tauri = false;

        if let Some(root) = root_path {
            if is_tauri_project(&root) {
                is_tauri = true;

                let _ = self.workspace_root.set(root.clone());

                match load_syntax(&self.syntax_config_path) {
                    Ok(syntax) => {
                        let _ = self.command_syntax.set(syntax);

                        self.client
                            .log_message(
                                MessageType::INFO,
                                "✅ Tauri project detected. Config loaded.",
                            )
                            .await;
                    }

                    Err(e) => {
                        self.client
                            .log_message(
                                MessageType::ERROR,
                                format!("❌ Failed to load syntax config: {}", e),
                            )
                            .await;

                        is_tauri = false;
                    }
                }
            }
        }

        if !is_tauri {
            return Ok(InitializeResult {
                capabilities: ServerCapabilities::default(),
                server_info: None,
            });
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),

                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        open_close: Some(true),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let command_syntax = match self.command_syntax.get() {
            Some(s) => s,
            None => return,
        };

        let ext_config_request = ConfigurationParams {
            items: vec![ConfigurationItem {
                scope_uri: None,
                section: Some("tarus.developerMode".to_string()),
            }],
        };

        if let Ok(response) = self
            .client
            .configuration(vec![ext_config_request.items[0].clone()])
            .await
        {
            if let Some(settings) = response.into_iter().next() {
                if let Some(is_enabled) = settings.as_bool() {
                    self.is_developer_mode_active
                        .store(is_enabled, Ordering::Relaxed);
                    self.client
                        .log_message(
                            MessageType::INFO,
                            &format!("Developer Mode initialized to: {}", is_enabled),
                        )
                        .await;
                }
            }
        }

        let root = match self.workspace_root.get() {
            Some(r) => r,
            None => return,
        };

        let root_clone = root.clone();
        let command_syntax_clone = command_syntax.clone();
        let project_index_clone = self.project_index.clone();
        let client_clone = self.client.clone();

        tokio::spawn(async move {
            client_clone
                .log_message(MessageType::INFO, "🚀 Starting background indexing...")
                .await;

            let files = tokio::task::spawn_blocking(move || scan_workspace_files(&root_clone))
                .await
                .unwrap_or_default();

            let mut total_findings = 0;

            for path in files {
                if let Some(count) =
                    process_file_index(path, &command_syntax_clone, &project_index_clone).await
                {
                    total_findings += count;
                }
            }

            // Report about the indexing process
            let report = project_index_clone.technical_report();
            client_clone.log_message(MessageType::INFO, report).await;

            client_clone
                .log_message(
                    MessageType::INFO,
                    format!("🏁 Indexing complete. Found {} references.", total_findings),
                )
                .await;
        });
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        self.log_dev_info(&format!(
            "➡️ Request: Definition at {:?} line: {}, char: {}",
            uri, position.line, position.character
        ))
        .await;

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => {
                self.client
                    .log_message(MessageType::ERROR, "❌ Failed to convert URI to path")
                    .await;
                return Ok(None);
            }
        };

        if let Some((key, origin_loc)) = self.project_index.get_key_at_position(&path, position) {
            self.log_dev_info(&format!(
                "✅ Found key under cursor: {:?} '{}' (Type: {:?})",
                key.entity, key.name, origin_loc.behavior
            ))
            .await;

            let all_refs = self.project_index.get_locations(key.entity, &key.name);

            let targets: Vec<&LocationInfo> = all_refs
                .iter()
                .filter(|target| {
                    // Exclude the current file
                    if target.range == origin_loc.range && target.path == origin_loc.path {
                        return false;
                    }

                    match origin_loc.behavior {
                        // If on Definition (Rust) -> Look for Call (JS)
                        Behavior::Definition => target.behavior == Behavior::Call,
                        // If on Call (JS) -> Search for Definition (Rust)
                        Behavior::Call => target.behavior == Behavior::Definition,

                        // If on Emit -> Search for Listen
                        Behavior::Emit => target.behavior == Behavior::Listen,
                        // If on Listen -> Search for Emit
                        Behavior::Listen => target.behavior == Behavior::Emit,
                    }
                })
                .collect();

            self.log_dev_info(&format!(
                "🔎 Found {} targets (smart portal)",
                targets.len()
            ))
            .await;

            if targets.is_empty() {
                return Ok(None);
            }

            let links: Vec<LocationLink> = targets
                .into_iter()
                .filter_map(|target| {
                    let target_uri = Url::from_file_path(&target.path).ok()?;

                    Some(LocationLink {
                        origin_selection_range: Some(origin_loc.range),

                        target_uri,
                        target_range: target.range,
                        target_selection_range: target.range,
                    })
                })
                .collect();

            return Ok(Some(GotoDefinitionResponse::Link(links)));
        } else {
            self.log_dev_info("⚠️ No key found at this position. Check ranges!")
                .await;

            if let Some(keys) = self.project_index.file_map.get(&path) {
                self.log_dev_info(&format!("ℹ️ Keys in this file: {:?}", keys.len()))
                    .await;
            }
        }

        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        // Find the key under the cursor
        if let Some((key, _)) = self.project_index.get_key_at_position(&path, position) {
            self.log_dev_info(&format!("🔎 Finding references for: {:?}", key))
                .await;

            let refs = self.project_index.get_locations(key.entity, &key.name);

            let locations: Vec<Location> = refs
                .iter()
                .filter_map(|r| {
                    let uri = Url::from_file_path(&r.path).ok()?;
                    Some(Location {
                        uri,
                        range: r.range,
                    })
                })
                .collect();

            return Ok(Some(locations));
        }

        Ok(None)
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = params.text_document.uri;
        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        let lens_data = self.project_index.get_lens_data(&path);

        if lens_data.is_empty() {
            return Ok(None);
        }

        let mut lenses = Vec::new();

        for (range, title, targets) in lens_data {
            // Converting LocationInfo to LSP Location
            let lsp_locations: Vec<Location> = targets
                .iter()
                .filter_map(|t| {
                    let target_uri = Url::from_file_path(&t.path).ok()?;
                    Some(Location {
                        uri: target_uri,
                        range: t.range,
                    })
                })
                .collect();

            if lsp_locations.is_empty() {
                continue;
            }

            // Generate a click command
            // editor.action.showReferences takes 3 arguments:
            // [Uri (where to open), Position (where the arrow points), List<Location> (results)]
            let arguments = vec![
                json!(uri.to_string()), // pass the URI as a string!
                json!(range.start),
                json!(lsp_locations),
            ];

            let command = Command {
                title,
                command: "tarus.show_references".to_string(),
                arguments: Some(arguments),
            };

            lenses.push(CodeLens {
                range,
                command: Some(command),
                data: None,
            });
        }

        Ok(Some(lenses))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        if let Some((key, origin_loc)) = self.project_index.get_key_at_position(&path, position) {
            let locations = self.project_index.get_locations(key.entity, &key.name);

            if locations.is_empty() {
                return Ok(None);
            }

            let (definitions, references): (Vec<&LocationInfo>, Vec<&LocationInfo>) =
                locations.iter().partition(|l| match key.entity {
                    EntityType::Command => l.behavior == Behavior::Definition,
                    EntityType::Event => l.behavior == Behavior::Listen,
                });

            // Create Markdown Text
            let mut md_text = String::new();

            // Header
            let icon = match key.entity {
                EntityType::Command => "🔧",
                EntityType::Event => "📡",
            };

            md_text.push_str(&format!(
                "### {} {:?}: `{}`\n\n",
                icon, key.entity, key.name
            ));

            // Definitions Section
            if !definitions.is_empty() {
                md_text.push_str("Definitions:\n");

                for def in definitions {
                    let file_icon = if def.path.extension().map_or(false, |e| e == "rs") {
                        "🦀"
                    } else {
                        "⚡️"
                    };

                    let filename = def.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

                    md_text.push_str(&format!(
                        "* {} `{} : {}`\n",
                        file_icon,
                        filename,
                        def.range.start.line + 1
                    ));
                }

                md_text.push_str("\n");
            }

            // Links Section
            if !references.is_empty() {
                md_text.push_str(&format!("**References ({})**:\n", references.len()));
                for (i, rf) in references.iter().enumerate() {
                    if i >= 5 {
                        md_text.push_str(&format!("* *...and {} more*\n", references.len() - 5));
                        break;
                    }

                    let file_icon = if rf.path.extension().map_or(false, |e| e == "rs") {
                        "🦀"
                    } else {
                        "⚡️"
                    };

                    let filename = rf.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                    let behavior_badge = format!("{:?}", rf.behavior).to_uppercase();

                    md_text.push_str(&format!(
                        "* {} `[{}] {} : {}`\n",
                        file_icon,
                        behavior_badge,
                        filename,
                        rf.range.start.line + 1
                    ));
                }
            }

            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: md_text,
                }),

                range: Some(origin_loc.range),
            }));
        }

        Ok(None)
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if !self.is_ready() {
            return;
        }

        if let Ok(path) = params.text_document.uri.to_file_path() {
            self.on_change(path).await;
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let exe_path = std::env::current_exe().expect("Failed to get exe path");
    let exe_dir = exe_path.parent().expect("Failed to get exe dir");
    let config_path = exe_dir.join("command_syntax.json");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let project_index = Arc::new(ProjectIndex::new());

    let initial_dev_mode_state = Arc::new(AtomicBool::new(false));

    let (service, socket) = LspService::new(|client| Backend {
        client,
        syntax_config_path: config_path,
        command_syntax: OnceCell::new(),
        workspace_root: OnceCell::new(),
        project_index,
        is_developer_mode_active: initial_dev_mode_state.clone(),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
