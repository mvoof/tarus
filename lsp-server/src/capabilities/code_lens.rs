//! Code Lens capability - shows reference counts above symbols

use crate::indexer::ProjectIndex;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{CodeLens, CodeLensParams};
use tower_lsp_server::UriExt;

/// Handle code lens request (pure function)
pub fn handle_code_lens(
    params: CodeLensParams,
    project_index: &ProjectIndex,
) -> Option<Vec<CodeLens>> {
    let uri = params.text_document.uri;

    let path_cow = uri.to_file_path()?;
    let path: PathBuf = path_cow.to_path_buf();
    let lens_data = project_index.get_lens_data(&path);

    if lens_data.is_empty() {
        return None;
    }

    let lenses: Vec<CodeLens> = lens_data
        .into_iter()
        .map(|(range, title, _targets)| CodeLens {
            range,
            command: Some(tower_lsp_server::lsp_types::Command {
                title,
                command: "tarus.show_references".to_string(),
                arguments: None,
            }),
            data: None,
        })
        .collect();

    Some(lenses)
}
