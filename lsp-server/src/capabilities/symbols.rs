//! Document and Workspace Symbol capabilities

use crate::indexer::ProjectIndex;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{
    DocumentSymbolParams, DocumentSymbolResponse, OneOf, SymbolInformation,
    WorkspaceSymbolParams,
};
use tower_lsp_server::UriExt;

/// Handle document symbol request (pure function)
pub fn handle_document_symbol(
    params: DocumentSymbolParams,
    project_index: &ProjectIndex,
) -> Option<DocumentSymbolResponse> {
    let uri = params.text_document.uri;

    let path_cow = uri.to_file_path()?;
    let path: PathBuf = path_cow.to_path_buf();
    let symbols = project_index.get_document_symbols(&path);

    if symbols.is_empty() {
        return None;
    }

    Some(DocumentSymbolResponse::Flat(symbols))
}

/// Handle workspace symbol request (pure function)
pub fn handle_workspace_symbol(
    params: &WorkspaceSymbolParams,
    project_index: &ProjectIndex,
) -> Option<OneOf<Vec<SymbolInformation>, Vec<tower_lsp_server::lsp_types::WorkspaceSymbol>>> {
    let symbols = project_index.search_workspace_symbols(&params.query);

    if symbols.is_empty() {
        return None;
    }

    Some(OneOf::Left(symbols))
}
