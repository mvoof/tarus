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
                        // A Rust file links only to its frontend counterparts;
                        // Rust-to-Rust references are the language server's own
                        // job, not ours.
                        if !is_current_rust {
                            rust_targets.push(target.clone());
                        }
                    } else {
                        frontend_targets.push(target.clone());
                    }
                }

                // The limit caps how many links one lens row offers, so it is
                // measured across every category at once. Counting each category
                // on its own let a line show `limit` Rust links *and* `limit`
                // frontend ones, which is what the setting exists to prevent.
                let link_count = distinct_files(&rust_targets)
                    + distinct_files(&frontend_targets)
                    + same_file_targets.len();
                let summarise = link_count > limit;

                push_file_lenses(
                    &mut result,
                    my_loc.range,
                    rust_targets,
                    summarise,
                    "rust ref",
                );
                push_file_lenses(
                    &mut result,
                    my_loc.range,
                    frontend_targets,
                    summarise,
                    "reference",
                );
                push_same_file_lens(&mut result, my_loc.range, same_file_targets, summarise);
            }
        }

        result
    }
}

/// Number of distinct files a set of locations points at — one link is offered
/// per file, not per location.
fn distinct_files(targets: &[LocationInfo]) -> usize {
    targets
        .iter()
        .map(|t| &t.path)
        .collect::<std::collections::HashSet<_>>()
        .len()
}

/// Pluralise a summary label against the count it is shown with.
fn summary_title(count: usize, label: &str) -> String {
    if count == 1 {
        format!("{count} {label}")
    } else {
        format!("{count} {label}s")
    }
}

/// Lens for references living in the file being viewed.
///
/// Naming the file would be tautological here, so each target is offered by line
/// number instead.
fn push_same_file_lens(
    result: &mut Vec<(Range, String, Vec<LocationInfo>)>,
    range: Range,
    mut targets: Vec<LocationInfo>,
    summarise: bool,
) {
    if targets.is_empty() {
        return;
    }

    targets.sort_by_key(|t| (t.range.start.line, t.range.start.character));

    if summarise {
        result.push((range, format!("{} in this file", targets.len()), targets));

        return;
    }

    for target in targets {
        // Editors count lines from one.
        let title = format!("Go to line {}", target.range.start.line + 1);

        result.push((range, title, vec![target]));
    }
}

fn push_file_lenses(
    result: &mut Vec<(Range, String, Vec<LocationInfo>)>,
    range: Range,
    targets: Vec<LocationInfo>,
    summarise: bool,
    summary_label: &str,
) {
    if targets.is_empty() {
        return;
    }

    let mut files_map: HashMap<PathBuf, Vec<LocationInfo>> = HashMap::new();
    for t in &targets {
        files_map.entry(t.path.clone()).or_default().push(t.clone());
    }

    if summarise {
        let title = summary_title(files_map.len(), summary_label);

        result.push((range, title, targets));

        return;
    }

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
}
