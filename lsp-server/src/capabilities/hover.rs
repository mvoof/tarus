//! Hover capability - shows detailed tooltip with usage statistics

use crate::indexer::{LocationInfo, ProjectIndex};
use crate::syntax::{Behavior, EntityType};
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind};
use tower_lsp_server::UriExt;

/// Get file icon and filename for display purposes
fn file_icon_and_name(path: &Path) -> (&'static str, &str) {
    let icon = if path.extension().is_some_and(|e| e == "rs") {
        "🦀"
    } else {
        "⚡️"
    };

    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    (icon, name)
}

#[allow(clippy::too_many_lines)]
/// Handle hover request (pure function)
pub fn handle_hover(params: HoverParams, project_index: &ProjectIndex) -> Option<Hover> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let path_cow = uri.to_file_path()?;
    let path: PathBuf = path_cow.to_path_buf();

    if let Some((key, origin_loc)) = project_index.get_key_at_position(&path, position) {
        let locations = project_index.get_locations(key.entity, &key.name);

        if locations.is_empty() {
            return None;
        }

        // Get diagnostic info for warnings
        let info = project_index.get_diagnostic_info(&key);

        // Count by behavior type
        let calls_count = locations
            .iter()
            .filter(|l| matches!(l.behavior, Behavior::Call))
            .count();

        let emits_count = locations
            .iter()
            .filter(|l| matches!(l.behavior, Behavior::Emit))
            .count();

        let listens_count = locations
            .iter()
            .filter(|l| matches!(l.behavior, Behavior::Listen))
            .count();

        let definitions_count = locations
            .iter()
            .filter(|l| matches!(l.behavior, Behavior::Definition))
            .count();

        let (definitions, references): (Vec<&LocationInfo>, Vec<&LocationInfo>) =
            locations.iter().partition(|l| match key.entity {
                EntityType::Event => l.behavior == Behavior::Listen,
                EntityType::Command
                | EntityType::Struct
                | EntityType::Enum
                | EntityType::Interface => l.behavior == Behavior::Definition,
            });

        // Create Markdown Text
        let mut md_text = String::new();

        // Header with emoji
        let icon = match key.entity {
            EntityType::Command => "⚙️",
            EntityType::Event => "📡",
            EntityType::Struct => "📦",
            EntityType::Enum => "🔢",
            EntityType::Interface => "📄",
        };

        let _ = write!(md_text, "### {} {:?}: `{}`\n\n", icon, key.entity, key.name);

        // Definitions Section
        if !definitions.is_empty() {
            md_text.push_str("**Definition:**\n");

            for def in &definitions {
                let (file_icon, filename) = file_icon_and_name(&def.path);

                let _ = writeln!(
                    md_text,
                    "- {} `{}:{}`",
                    file_icon,
                    filename,
                    def.range.start.line + 1
                );
            }

            md_text.push('\n');
        }

        // Reference count breakdown
        let total_refs = locations.len();
        let _ = writeln!(md_text, "**References ({total_refs} total)**");

        if key.entity == EntityType::Command {
            if definitions_count > 0 {
                let _ = writeln!(md_text, "- 🦀 {definitions_count} definition(s)");
            }

            if calls_count > 0 {
                let _ = writeln!(md_text, "- ⚡ {calls_count} call(s)");
            }
        } else {
            if emits_count > 0 {
                let _ = writeln!(md_text, "- 📤 {emits_count} emit(s)");
            }

            if listens_count > 0 {
                let _ = writeln!(md_text, "- 👂 {listens_count} listener(s)");
            }
        }

        md_text.push('\n');

        // Sample references (first 5)
        if !references.is_empty() {
            md_text.push_str("**Sample References:**\n");

            for (i, rf) in references.iter().enumerate() {
                if i >= 5 {
                    let _ = writeln!(md_text, "- *...and {} more*", references.len() - 5);
                    break;
                }

                let (file_icon, filename) = file_icon_and_name(&rf.path);
                let behavior_badge = format!("{:?}", rf.behavior).to_uppercase();

                let _ = writeln!(
                    md_text,
                    "- {} `[{}] {}:{}`",
                    file_icon,
                    behavior_badge,
                    filename,
                    rf.range.start.line + 1
                );
            }

            md_text.push('\n');
        }

        // Add warnings/tips based on diagnostic info
        if key.entity == EntityType::Command && !info.has_definition {
            md_text.push_str("⚠️ *No backend implementation found*\n");
        } else if key.entity == EntityType::Command && !info.has_calls {
            md_text.push_str("💡 *Defined but never called in frontend*\n");
        } else if key.entity == EntityType::Event && !info.has_emitters {
            md_text.push_str("💡 *Event listened for but never emitted*\n");
        } else if key.entity == EntityType::Event && !info.has_listeners {
            md_text.push_str("💡 *Event emitted but no listeners found*\n");
        }

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: md_text,
            }),
            range: Some(origin_loc.range),
        });
    }

    None
}
