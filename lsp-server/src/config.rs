//! Configuration management for the LSP server

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tower_lsp_server::lsp_types::{ConfigurationItem, ConfigurationParams, MessageType};
use tower_lsp_server::Client;

use crate::indexer::ProjectIndex;

/// Load and apply configuration settings from the client
pub async fn load_configuration(
    client: &Client,
    is_developer_mode_active: &Arc<AtomicBool>,
    project_index: &Arc<ProjectIndex>,
) {
    let ext_config_request = ConfigurationParams {
        items: vec![
            ConfigurationItem {
                scope_uri: None,
                section: Some("tarus.developerMode".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("tarus.referenceLimit".to_string()),
            },
        ],
    };

    let Ok(response) = client.configuration(ext_config_request.items).await else {
        return;
    };

    let mut iter = response.into_iter();

    // Handle developerMode
    if let Some(settings) = iter.next() {
        if let Some(is_enabled) = settings.as_bool() {
            is_developer_mode_active.store(is_enabled, Ordering::Relaxed);

            client
                .log_message(
                    MessageType::INFO,
                    &format!("Developer Mode initialized to: {is_enabled}"),
                )
                .await;
        }
    }

    // Handle referenceLimit
    if let Some(settings) = iter.next() {
        if let Some(limit) = settings.as_u64() {
            project_index.reference_limit.store(
                usize::try_from(limit).unwrap_or(usize::MAX),
                Ordering::Relaxed,
            );

            client
                .log_message(
                    MessageType::INFO,
                    &format!("Reference Limit initialized to: {limit}"),
                )
                .await;
        }
    }
}
