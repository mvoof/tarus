use serde_json::Value;
use std::path::PathBuf;
use tower_lsp_server::ls_types::ExecuteCommandParams;

use crate::indexer::ProjectIndex;

pub fn handle_execute_command(
    _params: &ExecuteCommandParams,
    _project_index: &ProjectIndex,
    _roots: &[PathBuf],
) -> Option<Value> {
    // All commands are no-ops: type checking uses live Rust source directly
    None
}
