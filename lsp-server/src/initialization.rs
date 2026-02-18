//! Server initialization and background indexing

use crate::bindings_reader::{load_all_bindings, BindingsConfig};
use crate::capabilities::diagnostics;
use crate::file_processor;
use crate::indexer::ProjectIndex;
use crate::scanner::scan_workspace_files;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tower_lsp_server::ls_types::{MessageType, Uri};
use tower_lsp_server::Client;

/// Spawn background indexing task for multiple roots
pub fn spawn_background_indexing(
    roots: &[PathBuf],
    project_index: Arc<ProjectIndex>,
    client: Client,
    is_dev_mode: Arc<AtomicBool>,
    bindings_config: BindingsConfig,
) {
    let roots_for_scan = roots.to_owned();
    let primary_root = roots.first().cloned();

    tokio::spawn(async move {
        client
            .log_message(MessageType::INFO, "🚀 Starting background indexing...")
            .await;

        // Load external bindings FIRST (Specta/Typegen)
        // This populates the bindings registry before workspace scan
        if let Some(ref root) = primary_root {
            load_external_bindings(root, &project_index, &bindings_config, &client).await;
        }

        // THEN scan workspace files (bindings files will be skipped)
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

/// Load external bindings files (tauri-specta / tauri-plugin-typegen)
pub async fn load_external_bindings(
    project_root: &Path,
    project_index: &ProjectIndex,
    bindings_config: &BindingsConfig,
    client: &Client,
) {
    let result = load_all_bindings(project_root, bindings_config, project_index, false);

    // Log successfully loaded files
    if result.loaded > 0 {
        client
            .log_message(
                MessageType::INFO,
                format!("📦 Loaded {} bindings file(s)", result.loaded),
            )
            .await;
    }

    // Log any errors
    for (path, error) in &result.errors {
        client
            .log_message(
                MessageType::WARNING,
                format!("Failed to load bindings from {}: {}", path.display(), error),
            )
            .await;
    }
}
