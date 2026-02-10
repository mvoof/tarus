use serde_json::Value;
use std::path::PathBuf;
use tower_lsp_server::jsonrpc::{Error, Result};
use tower_lsp_server::ls_types::ExecuteCommandParams;

use crate::indexer::ProjectIndex;
use crate::typegen::{self, TypegenConfig};

pub fn handle_execute_command(
    params: ExecuteCommandParams,
    project_index: &ProjectIndex,
    roots: &[PathBuf],
    typegen_config: &TypegenConfig,
) -> Result<Option<Value>> {
    match params.command.as_str() {
        "tarus.syncTypes" => {
            if let Some(root) = roots.first() {
                if let Err(e) =
                    typegen::write_types_file_with_config(project_index, root, typegen_config)
                {
                    return Err(Error::internal_error()); // Or custom error with message
                }
                // Return success
                Ok(None)
            } else {
                Err(Error::invalid_params("No workspace root found".to_string()))
            }
        }
        _ => Err(Error::method_not_found()),
    }
}
