//! Debug reports and introspection

use super::types::LocationInfo;
use super::ProjectIndex;
use crate::syntax::{Behavior, EntityType};
use std::fmt::Write as _;
use std::path::Path;

/// Distinct names in the index, by kind
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NameCounts {
    pub commands: usize,
    pub events: usize,
}

impl ProjectIndex {
    /// How many distinct command and event names the index holds
    #[must_use]
    pub fn name_counts(&self) -> NameCounts {
        let mut counts = NameCounts::default();

        for entry in &self.map {
            match entry.key().entity {
                EntityType::Command => counts.commands += 1,
                EntityType::Event => counts.events += 1,
            }
        }

        counts
    }

    /// The whole index for `TARUS: Dump Index`: every command and event by name,
    /// each with the places it is defined, called, emitted or listened to.
    /// `display_path` writes a file's path, relative to the workspace.
    pub fn index_report(&self, display_path: &dyn Fn(&Path) -> String) -> String {
        if self.map.is_empty() {
            return "Index: empty".to_string();
        }

        let counts = self.name_counts();
        let mut report = format!(
            "Index: {} commands, {} events, {} files\n",
            counts.commands,
            counts.events,
            self.file_map.len()
        );

        for (entity, heading) in [
            (EntityType::Command, "Commands"),
            (EntityType::Event, "Events"),
        ] {
            let mut names: Vec<(String, Vec<LocationInfo>)> = self
                .map
                .iter()
                .filter(|entry| entry.key().entity == entity)
                .map(|entry| (entry.key().name.clone(), entry.value().clone()))
                .collect();

            if names.is_empty() {
                continue;
            }

            names.sort_by(|left, right| left.0.cmp(&right.0));
            let _ = write!(report, "\n{heading}\n");

            for (name, mut locations) in names {
                locations.sort_by_key(|location| {
                    (
                        behavior_rank(location.behavior),
                        display_path(&location.path),
                        location.range.start.line,
                        location.range.start.character,
                    )
                });

                let _ = writeln!(report, "  {name} ({})", locations.len());

                for location in &locations {
                    let _ = writeln!(
                        report,
                        "    {:<11} {}:{}:{}",
                        behavior_label(location.behavior),
                        display_path(&location.path),
                        location.range.start.line + 1,
                        location.range.start.character + 1
                    );
                }
            }
        }

        report
    }
}

/// Definitions first, then the places that use them
fn behavior_rank(behavior: Behavior) -> u8 {
    match behavior {
        Behavior::Definition => 0,
        Behavior::Call | Behavior::SpectaCall => 1,
        Behavior::Emit => 2,
        Behavior::Listen => 3,
    }
}

fn behavior_label(behavior: Behavior) -> &'static str {
    match behavior {
        Behavior::Definition => "definition",
        Behavior::Call => "call",
        Behavior::SpectaCall => "specta call",
        Behavior::Emit => "emit",
        Behavior::Listen => "listen",
    }
}
