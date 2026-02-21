use serde_json::Value;
use std::path::PathBuf;
use tower_lsp_server::jsonrpc::{Error, Result};
use tower_lsp_server::ls_types::ExecuteCommandParams;

use crate::bindings_reader::{self, BindingsConfig};
use crate::indexer::ProjectIndex;

/// # Errors
/// Returns an error if command execution fails.
pub fn handle_execute_command(
    params: &ExecuteCommandParams,
    project_index: &ProjectIndex,
    roots: &[PathBuf],
    bindings_config: &BindingsConfig,
) -> Result<Option<Value>> {
    match params.command.as_str() {
        "tarus.syncTypes" => {
            if let Some(root) = roots.first() {
                // Reload external bindings from ts-rs, tauri-specta, tauri-typegen
                let result =
                    bindings_reader::load_all_bindings(root, bindings_config, project_index, true);

                // Return error if any files failed to load
                if !result.errors.is_empty() {
                    return Err(Error::internal_error());
                }

                Ok(None)
            } else {
                Err(Error::invalid_params("No workspace root found".to_string()))
            }
        }
        _ => Err(Error::method_not_found()),
    }
}
