//! Code Lens capability - shows reference counts above symbols

use crate::indexer::ProjectIndex;
use serde_json::json;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{CodeLens, CodeLensParams, Uri};
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
        .filter_map(|(range, title, targets)| {
            // Convert targets to locations format expected by VS Code
            let locations: Vec<_> = targets
                .iter()
                .filter_map(|target| {
                    let target_uri = Uri::from_file_path(&target.path)?;
                    Some(json!({
                        "uri": target_uri.to_string(),
                        "range": {
                            "start": {
                                "line": target.range.start.line,
                                "character": target.range.start.character
                            },
                            "end": {
                                "line": target.range.end.line,
                                "character": target.range.end.character
                            }
                        }
                    }))
                })
                .collect();

            if locations.is_empty() {
                return None;
            }

            // Create command arguments: (uriStr, pos, locs)
            let arguments = Some(vec![
                json!(uri.to_string()),
                json!({
                    "line": range.start.line,
                    "character": range.start.character
                }),
                json!(locations),
            ]);

            Some(CodeLens {
                range,
                command: Some(tower_lsp_server::lsp_types::Command {
                    title,
                    command: "tarus.show_references".to_string(),
                    arguments,
                }),
                data: None,
            })
        })
        .collect();

    if lenses.is_empty() {
        return None;
    }

    Some(lenses)
}
