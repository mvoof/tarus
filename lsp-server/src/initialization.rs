//! Server initialization and background indexing

use crate::capabilities::diagnostics;
use crate::file_processor;
use crate::indexer::ProjectIndex;
use crate::scanner::scan_workspace_files;
use crate::typegen;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tower_lsp_server::lsp_types::{MessageType, Uri};
use tower_lsp_server::{Client, UriExt};

/// Spawn background indexing task for multiple roots
pub fn spawn_background_indexing(
    roots: &[PathBuf],
    project_index: Arc<ProjectIndex>,
    client: Client,
    is_dev_mode: Arc<AtomicBool>,
) {
    let roots_for_scan = roots.to_owned();
    let primary_root = roots.first().cloned();

    tokio::spawn(async move {
        client
            .log_message(MessageType::INFO, "🚀 Starting background indexing...")
            .await;

        let mut all_files = Vec::new();
        for root in roots_for_scan {
            let files = tokio::task::spawn_blocking(move || scan_workspace_files(&root))
                .await
                .unwrap_or_default();
            all_files.extend(files);
        }

        for path in all_files {
            file_processor::process_file_index(path, &project_index);
        }

        // Generate TypeScript type definitions (use primary root)
        if let Some(root) = primary_root {
            if let Err(e) = typegen::write_types_file(&project_index, &root) {
                client
                    .log_message(
                        MessageType::WARNING,
                        format!("Failed to generate type definitions: {e}"),
                    )
                    .await;
            } else {
                client
                    .log_message(MessageType::INFO, "📝 Generated tauri-commands.d.ts")
                    .await;
            }
        }

        // Publish diagnostics for all indexed files
        for entry in &project_index.file_map {
            let path = entry.key().clone();

            if let Some(uri) = Uri::from_file_path(&path) {
                let diagnostics = diagnostics::compute_file_diagnostics(&path, &project_index);
                client.publish_diagnostics(uri, diagnostics, None).await;
            }
        }

        // Report about the indexing process
        let report = project_index.technical_report();

        if is_dev_mode.load(Ordering::Relaxed) {
            client.log_message(MessageType::INFO, report).await;
        }

        client
            .log_message(MessageType::INFO, "🏁 Indexing complete".to_string())
            .await;
    });
}