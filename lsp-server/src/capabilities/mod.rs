//! LSP Server Capabilities
//! 
//! This module contains all LSP capability implementations.
//! Each capability is in its own module with full implementation.

pub mod diagnostics;
pub mod definition;
pub mod references;
pub mod hover;
pub mod code_lens;
pub mod symbols;
pub mod completion;
pub mod code_actions;

use tower_lsp_server::lsp_types::{
    CodeActionProviderCapability, CodeLensOptions, CompletionOptions,
    HoverProviderCapability, OneOf, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions,
};

/// Build the LSP server capabilities configuration
#[must_use] pub fn build_server_capabilities() -> ServerCapabilities {
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
        ..Default::default()
    }
}
