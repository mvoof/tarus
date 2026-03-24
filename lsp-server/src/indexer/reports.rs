//! Debug reports and introspection

use super::types::IndexKey;
use super::ProjectIndex;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::types::LocationInfo;

impl ProjectIndex {
    /// Generates a report only for a specific file (delta update)
    pub fn file_report(&self, path: &Path) -> String {
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let mut report_message = String::new();

        let _ = writeln!(report_message, "\n📝 === UPDATE REPORT: {filename} ===");

        let Some(keys) = self.file_map.get(&path.to_path_buf()) else {
            return format!("📝 File update: {filename:?} (No Tarus keys found)");
        };

        let keys_clone: Vec<IndexKey> = keys.value().clone();

        report_message.push_str("\n🔑 === 1. MAIN KEY INDEX (Delta Subset) ===\n");

        let mut delta_map: HashMap<IndexKey, Vec<LocationInfo>> = HashMap::new();

        for key in &keys_clone {
            if let Some(locs) = self.map.get(key) {
                let file_locs: Vec<LocationInfo> =
                    locs.iter().filter(|l| l.path == path).cloned().collect();

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
        delta_file_map.insert(path.to_path_buf(), keys_clone);

        let _ = writeln!(report_message, "{delta_file_map:#?}");

        report_message
    }

    /// Creates a readable report of the index contents
    pub fn technical_report(&self) -> String {
        let mut report_message = String::from("\n💾 === TECHNICAL INDEX DUMP ===\n");

        if self.map.is_empty() {
            report_message.push_str("   (Storage is Empty)\n");

            return report_message;
        }

        report_message.push_str("\n\n🔑 === 1. MAIN KEY INDEX (map) ===\n");
        report_message.push_str("   [Key -> List of ALL Locations]\n");

        let _ = writeln!(report_message, "{:#?}", self.map);

        report_message.push_str("\n\n📄 === 2. REVERSE FILE MAP (file_map) ===\n");
        report_message.push_str("   [FilePath -> List of ALL Keys in that file]\n");

        if self.file_map.is_empty() {
            report_message.push_str("   (File Map is Empty)\n");
        } else {
            let _ = writeln!(report_message, "{:#?}", self.file_map);
        }

        report_message
    }
}
