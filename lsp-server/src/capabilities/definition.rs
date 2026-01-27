//! Go to Definition capability
//!
//! Handles F12 navigation between Tauri commands/events

use crate::indexer::{LocationInfo, ProjectIndex};
use crate::syntax::Behavior;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{
    GotoDefinitionParams, GotoDefinitionResponse, LocationLink, Uri,
};
use tower_lsp_server::UriExt;

/// Handle go to definition request (pure function)
pub fn handle_goto_definition(
    params: GotoDefinitionParams,
    project_index: &ProjectIndex,
) -> Option<GotoDefinitionResponse> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let path_cow = uri.to_file_path()?;
    let path: PathBuf = path_cow.to_path_buf();

    if let Some((key, origin_loc)) = project_index.get_key_at_position(&path, position) {
        let all_refs = project_index.get_locations(key.entity, &key.name);

        let targets: Vec<&LocationInfo> = all_refs
            .iter()
            .filter(|target| {
                // Exclude the current location
                if target.range == origin_loc.range && target.path == origin_loc.path {
                    return false;
                }

                match origin_loc.behavior {
                    // If on Definition (Rust) -> Look for Call (JS/TS)
                    Behavior::Definition => target.behavior == Behavior::Call,
                    // If on Call (JS/TS) -> Search for Definition (Rust)
                    Behavior::Call => target.behavior == Behavior::Definition,
                    // If on Emit -> Search for Listen
                    Behavior::Emit => target.behavior == Behavior::Listen,
                    // If on Listen -> Search for Emit
                    Behavior::Listen => target.behavior == Behavior::Emit,
                }
            })
            .collect();

        if targets.is_empty() {
            return None;
        }

        let links: Vec<LocationLink> = targets
            .into_iter()
            .filter_map(|target| {
                let target_uri = Uri::from_file_path(&target.path)?;

                Some(LocationLink {
                    origin_selection_range: Some(origin_loc.range),
                    target_uri,
                    target_range: target.range,
                    target_selection_range: target.range,
                })
            })
            .collect();

        return Some(GotoDefinitionResponse::Link(links));
    }

    None
}
