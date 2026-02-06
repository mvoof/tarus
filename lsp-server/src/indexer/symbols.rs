//! Symbol generation and reporting utilities

use super::{IndexKey, LocationInfo};
use crate::syntax::{Behavior, EntityType};
use dashmap::DashMap;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{Location, SymbolInformation, SymbolKind, Uri};
use tower_lsp_server::UriExt;

/// Get document symbols for outline view
pub fn get_document_symbols(
    path: &PathBuf,
    file_map: &DashMap<PathBuf, Vec<IndexKey>>,
    map: &DashMap<IndexKey, Vec<LocationInfo>>,
) -> Vec<SymbolInformation> {
    let mut symbols = Vec::new();

    let Some(keys) = file_map.get(path) else {
        return symbols;
    };

    let Some(uri) = Uri::from_file_path(path) else {
        return symbols;
    };

    for key in keys.value() {
        if let Some(locations) = map.get(key) {
            for loc in locations.iter().filter(|l| l.path == *path) {
                let kind = match key.entity {
                    EntityType::Command => SymbolKind::FUNCTION,
                    EntityType::Event => SymbolKind::EVENT,
                    EntityType::Struct => SymbolKind::STRUCT,
                    EntityType::Enum => SymbolKind::ENUM,
                    EntityType::Interface => SymbolKind::INTERFACE,
                };

                // Use behavior terms
                let behavior_label = match loc.behavior {
                    Behavior::Definition => "command",
                    Behavior::Call => "invoke",
                    Behavior::Emit => "emit",
                    Behavior::Listen => "listen",
                };

                #[allow(deprecated)]
                symbols.push(SymbolInformation {
                    name: format!("{} ({})", key.name, behavior_label),
                    kind,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: uri.clone(),
                        range: loc.range,
                    },
                    container_name: Some(format!("{:?}", key.entity)),
                });
            }
        }
    }

    symbols.sort_by_key(|s| s.location.range.start.line);
    symbols
}

/// Search workspace symbols by query (Ctrl+T)
pub fn search_workspace_symbols(
    query: &str,
    map: &DashMap<IndexKey, Vec<LocationInfo>>,
) -> Vec<SymbolInformation> {
    let mut symbols = Vec::new();
    let query_lower = query.to_lowercase();

    for entry in map {
        let key = entry.key();

        // Filter by query (substring match)
        if !query.is_empty() && !key.name.to_lowercase().contains(&query_lower) {
            continue;
        }

        for loc in entry.value() {
            let Some(uri) = Uri::from_file_path(&loc.path) else {
                continue;
            };

            let kind = match key.entity {
                EntityType::Command => SymbolKind::FUNCTION,
                EntityType::Event => SymbolKind::EVENT,
                EntityType::Struct => SymbolKind::STRUCT,
                EntityType::Enum => SymbolKind::ENUM,
                EntityType::Interface => SymbolKind::INTERFACE,
            };

            let behavior_label = match loc.behavior {
                Behavior::Definition => "command",
                Behavior::Call => "invoke",
                Behavior::Emit => "emit",
                Behavior::Listen => "listen",
            };

            #[allow(deprecated)]
            symbols.push(SymbolInformation {
                name: format!("{} ({})", key.name, behavior_label),
                kind,
                tags: None,
                deprecated: None,
                location: Location {
                    uri,
                    range: loc.range,
                },
                container_name: Some(format!("{:?}", key.entity)),
            });
        }
    }

    // Limit results
    symbols.truncate(100);
    symbols
}

/// Generates a report only for a specific file (delta update)
pub fn file_report(
    path: &PathBuf,
    file_map: &DashMap<PathBuf, Vec<IndexKey>>,
    map: &DashMap<IndexKey, Vec<LocationInfo>>,
) -> String {
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let mut report_message = String::new();

    let _ = writeln!(report_message, "\n📝 === UPDATE REPORT: {filename} ===");

    let Some(keys) = file_map.get(path) else {
        return format!("📝 File update: {filename:?} (No Tarus keys found)");
    };

    let keys_clone: Vec<IndexKey> = keys.value().clone();

    report_message.push_str("\n🔑 === 1. MAIN KEY INDEX (Delta Subset) ===\n");

    let mut delta_map: HashMap<IndexKey, Vec<LocationInfo>> = HashMap::new();

    for key in &keys_clone {
        if let Some(locs) = map.get(key) {
            let file_locs: Vec<LocationInfo> =
                locs.iter().filter(|l| l.path == *path).cloned().collect();

            if !file_locs.is_empty() {
                delta_map.insert(key.clone(), file_locs);
            }
        }
    }

    if delta_map.is_empty() {
        report_message.push_str("   (Map subset is empty)\n");
    } else {
        let _ = writeln!(report_message, "{delta_map:#?}");
    }

    report_message.push_str("\n📄 === 2. REVERSE FILE MAP (Delta Subset) ===\n");

    let mut delta_file_map: HashMap<PathBuf, Vec<IndexKey>> = HashMap::new();
    delta_file_map.insert(path.clone(), keys_clone);

    let _ = writeln!(report_message, "{delta_file_map:#?}");

    report_message
}

/// Creates a readable report of the index contents
pub fn technical_report(
    map: &DashMap<IndexKey, Vec<LocationInfo>>,
    file_map: &DashMap<PathBuf, Vec<IndexKey>>,
) -> String {
    let mut report_message = String::from("\n💾 === TECHNICAL INDEX DUMP ===\n");

    if map.is_empty() {
        report_message.push_str("   (Storage is Empty)\n");

        return report_message;
    }

    report_message.push_str("\n\n🔑 === 1. MAIN KEY INDEX (map) ===\n");
    report_message.push_str("   [Key -> List of ALL Locations]\n");

    let _ = writeln!(report_message, "{map:#?}");

    report_message.push_str("\n\n📄 === 2. REVERSE FILE MAP (file_map) ===\n");
    report_message.push_str("   [FilePath -> List of ALL Keys in that file]\n");

    if file_map.is_empty() {
        report_message.push_str("   (File Map is Empty)\n");
    } else {
        let _ = writeln!(report_message, "{file_map:#?}");
    }

    report_message
}
