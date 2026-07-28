//! `CodeLens` data preparation

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tower_lsp_server::lsp_types::Range;

use super::types::{IndexKey, LocationInfo};
use super::ProjectIndex;

impl ProjectIndex {
    /// Preparing data for `CodeLens`
    pub fn get_lens_data(&self, path: &Path) -> Vec<(Range, String, Vec<LocationInfo>)> {
        let mut result = Vec::new();

        let Some(keys) = self.file_map.get(&path.to_path_buf()) else {
            return result;
        };

        let mut processed_keys: HashSet<&IndexKey> = HashSet::new();

        for key in keys.value() {
            if !processed_keys.insert(key) {
                continue;
            }

            let Some(all_locations) = self.map.get(key) else {
                continue;
            };

            let current_file_locations: Vec<&LocationInfo> =
                all_locations.iter().filter(|l| l.path == path).collect();

            let is_current_rust = path.extension().and_then(|s| s.to_str()) == Some("rs");
            let limit = self.reference_limit.load(Ordering::Relaxed);

            for my_loc in current_file_locations {
                // Only the lens's own location is excluded, never its whole file:
                // the other side of an event is just as hard to find a hundred lines
                // down as it is in a neighbouring file.
                let mut rust_targets = Vec::new();
                let mut frontend_targets = Vec::new();
                let mut same_file_targets = Vec::new();

                for target in all_locations.iter() {
                    if target.path == path {
                        if target.range != my_loc.range {
                            same_file_targets.push(target.clone());
                        }
                    } else if target.path.extension().and_then(|s| s.to_str()) == Some("rs") {
                        rust_targets.push(target.clone());
                    } else {
                        frontend_targets.push(target.clone());
                    }
                }

                if !is_current_rust {
                    push_file_lenses(&mut result, my_loc.range, rust_targets, limit, "rust refs");
                }

                push_file_lenses(
                    &mut result,
                    my_loc.range,
                    frontend_targets,
                    limit,
                    "references",
                );
                push_same_file_lens(&mut result, my_loc.range, same_file_targets);
            }
        }

        result
    }
}

/// Lens for references living in the file being viewed.
///
/// Naming the file would be tautological here, so a single target shows the line
/// to jump to and several are summarised by count.
fn push_same_file_lens(
    result: &mut Vec<(Range, String, Vec<LocationInfo>)>,
    range: Range,
    mut targets: Vec<LocationInfo>,
) {
    if targets.is_empty() {
        return;
    }

    targets.sort_by_key(|t| (t.range.start.line, t.range.start.character));

    let title = if targets.len() == 1 {
        // Editors count lines from one.
        format!("Go to line {}", targets[0].range.start.line + 1)
    } else {
        format!("{} in this file", targets.len())
    };

    result.push((range, title, targets));
}

fn push_file_lenses(
    result: &mut Vec<(Range, String, Vec<LocationInfo>)>,
    range: Range,
    targets: Vec<LocationInfo>,
    limit: usize,
    summary_label: &str,
) {
    if targets.is_empty() {
        return;
    }

    let mut files_map: HashMap<PathBuf, Vec<LocationInfo>> = HashMap::new();
    for t in &targets {
        files_map.entry(t.path.clone()).or_default().push(t.clone());
    }

    if files_map.len() <= limit {
        let mut sorted_files: Vec<_> = files_map.into_iter().collect();
        sorted_files.sort_by(|a, b| a.0.cmp(&b.0));

        for (fpath, locs) in sorted_files {
            let fname = fpath
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            result.push((range, format!("Go to {fname}"), locs));
        }
    } else {
        result.push((
            range,
            format!("{} {}", targets.len(), summary_label),
            targets,
        ));
    }
}
