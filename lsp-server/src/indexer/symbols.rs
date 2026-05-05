//! Document and workspace symbol search

use crate::syntax::{Behavior, EntityType};
use std::path::Path;
use tower_lsp_server::lsp_types::{Location, SymbolInformation, SymbolKind, Uri};
use tower_lsp_server::UriExt;

use super::types::{IndexKey, LocationInfo};
use super::ProjectIndex;

fn behavior_label(behavior: Behavior) -> &'static str {
    match behavior {
        Behavior::Definition => "command",
        Behavior::Call => "invoke",
        Behavior::SpectaCall => "commands",
        Behavior::Emit => "emit",
        Behavior::Listen => "listen",
    }
}

// `deprecated` field is deprecated in favor of `tags`, but it's still a required
// field in the `SymbolInformation` struct in this version of `lsp-types`.
#[allow(deprecated)]
fn make_symbol_info(key: &IndexKey, loc: &LocationInfo, uri: Uri) -> SymbolInformation {
    let kind = match key.entity {
        EntityType::Command => SymbolKind::FUNCTION,
        EntityType::Event => SymbolKind::EVENT,
    };
    SymbolInformation {
        name: format!("{} ({})", key.name, behavior_label(loc.behavior)),
        kind,
        tags: None,
        deprecated: None,
        location: Location {
            uri,
            range: loc.range,
        },
        container_name: Some(format!("{:?}", key.entity)),
    }
}

impl ProjectIndex {
    /// Get document symbols for outline view
    pub fn get_document_symbols(&self, path: &Path) -> Vec<SymbolInformation> {
        let mut symbols = Vec::new();

        let Some(keys) = self.file_map.get(&path.to_path_buf()) else {
            return symbols;
        };

        let Some(uri) = Uri::from_file_path(path) else {
            return symbols;
        };

        for key in keys.value() {
            if let Some(locations) = self.map.get(key) {
                for loc in locations.iter().filter(|l| l.path == path) {
                    symbols.push(make_symbol_info(key, loc, uri.clone()));
                }
            }
        }

        symbols.sort_by_key(|s| s.location.range.start.line);
        symbols
    }

    /// Search workspace symbols by query (Ctrl+T)
    pub fn search_workspace_symbols(&self, query: &str) -> Vec<SymbolInformation> {
        let mut symbols = Vec::new();
        let query_lower = query.to_lowercase();

        for entry in &self.map {
            let key = entry.key();

            if !query.is_empty() && !key.name.to_lowercase().contains(&query_lower) {
                continue;
            }

            for loc in entry.value() {
                let Some(uri) = Uri::from_file_path(&loc.path) else {
                    continue;
                };
                symbols.push(make_symbol_info(key, loc, uri));
            }
        }

        symbols.truncate(100);
        symbols
    }
}
