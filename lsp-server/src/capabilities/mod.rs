//! LSP Server Capabilities
//!
//! This module contains all LSP capability implementations.
//! Each capability is in its own module with full implementation.

pub mod code_actions;
pub mod code_lens;
pub mod commands;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod hover;
pub mod references;
pub mod symbols;

use std::path::PathBuf;
use tower_lsp_server::ls_types::{
    CodeActionProviderCapability, CodeLensOptions, CompletionOptions, HoverProviderCapability,
    OneOf, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, Uri,
};

/// Convert LSP URI to `PathBuf`
///
/// Helper to avoid repetitive `uri.to_file_path()?.into_owned()` pattern
#[must_use]
pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    uri.to_file_path().map(std::borrow::Cow::into_owned)
}

/// Build the LSP server capabilities configuration
#[must_use]
pub fn build_server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(false),
        }),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec!["\"".to_string(), "'".to_string()]),
            resolve_provider: Some(false),
            ..Default::default()
        }),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                ..Default::default()
            },
        )),
        execute_command_provider: Some(tower_lsp_server::ls_types::ExecuteCommandOptions {
            commands: vec!["tarus.syncTypes".to_string()],
            work_done_progress_options:
                tower_lsp_server::ls_types::WorkDoneProgressOptions::default(),
        }),
        ..Default::default()
    }
}
