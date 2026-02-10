//! Configuration management for the LSP server

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tower_lsp_server::ls_types::{ConfigurationItem, ConfigurationParams, MessageType};
use tower_lsp_server::Client;

use crate::indexer::ProjectIndex;
use crate::typegen::TypegenConfig;

/// Load and apply configuration settings from the client
pub async fn load_configuration(
    client: &Client,
    is_developer_mode_active: &Arc<AtomicBool>,
    project_index: &Arc<ProjectIndex>,
    typegen_config: &Arc<tokio::sync::RwLock<TypegenConfig>>,
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
            ConfigurationItem {
                scope_uri: None,
                section: Some("tarus.dtsOutputPath".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("tarus.strictTypeSafety".to_string()),
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

    // Handle dtsOutputPath and strictTypeSafety
    let dts_path = iter.next().and_then(|v| v.as_str().map(String::from));
    let strict_mode = iter.next().and_then(|v| v.as_bool());

    {
        let mut config = typegen_config.write().await;
        if let Some(path) = dts_path {
            if !path.is_empty() {
                config.dts_output_path = Some(path.clone());
                client
                    .log_message(
                        MessageType::INFO,
                        &format!("DTS output path set to: {path}"),
                    )
                    .await;
            }
        }

        if let Some(strict) = strict_mode {
            config.strict_type_safety = strict;
            client
                .log_message(MessageType::INFO, &format!("Strict type safety: {strict}"))
                .await;
        }
    }
}
