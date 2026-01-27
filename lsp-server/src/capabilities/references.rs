//! Find References capability
//!
//! Handles Shift+F12 to find all references

use crate::indexer::ProjectIndex;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{Location, ReferenceParams, Uri};
use tower_lsp_server::UriExt;

/// Handle find references request (pure function)
pub fn handle_references(
    params: ReferenceParams,
    project_index: &ProjectIndex,
) -> Option<Vec<Location>> {
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let path_cow = uri.to_file_path()?;
    let path: PathBuf = path_cow.to_path_buf();

    // Find the key under the cursor
    if let Some((key, _)) = project_index.get_key_at_position(&path, position) {
        let refs = project_index.get_locations(key.entity, &key.name);

        let locations: Vec<Location> = refs
            .iter()
            .filter_map(|r| {
                let uri = Uri::from_file_path(&r.path)?;
                Some(Location {
                    uri,
                    range: r.range,
                })
            })
            .collect();

        return Some(locations);
    }

    None
}
