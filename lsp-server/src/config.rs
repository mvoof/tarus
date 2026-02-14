//! Configuration management for the LSP server

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tower_lsp_server::ls_types::{ConfigurationItem, ConfigurationParams, MessageType};
use tower_lsp_server::Client;

use crate::bindings_reader::BindingsConfig;
use crate::indexer::ProjectIndex;

/// Load and apply configuration settings from the client
#[allow(clippy::too_many_lines)]
pub async fn load_configuration(
    client: &Client,
    is_developer_mode_active: &Arc<AtomicBool>,
    project_index: &Arc<ProjectIndex>,
    bindings_config: &Arc<tokio::sync::RwLock<BindingsConfig>>,
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
                section: Some("tarus.typeBindingsPaths".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("tarus.typeSafetyEnabled".to_string()),
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

    // Handle typeBindingsPaths and typeSafetyEnabled
    let bindings_paths = iter.next().and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|val| val.as_str().map(String::from))
                .collect::<Vec<String>>()
        })
    });
    let safety_enabled = iter.next().and_then(|v| v.as_bool());

    {
        let mut config = bindings_config.write().await;
        if let Some(paths) = bindings_paths {
            config.type_bindings_paths = Some(paths.clone());
            client
                .log_message(
                    MessageType::INFO,
                    &format!("Type bindings paths set to: {paths:?}"),
                )
                .await;
        }

        if let Some(enabled) = safety_enabled {
            config.type_safety_enabled = enabled;
            client
                .log_message(
                    MessageType::INFO,
                    &format!("Type safety enabled: {enabled}"),
                )
                .await;
        }
    }
}
