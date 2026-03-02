//! Hover capability - shows detailed tooltip with usage statistics

use crate::indexer::{LocationInfo, ProjectIndex};
use crate::syntax::{Behavior, EntityType};
use std::fmt::Write as _;
use tower_lsp_server::lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind};
use tower_lsp_server::UriExt;

/// Handle hover request (pure function)
pub fn handle_hover(params: HoverParams, project_index: &ProjectIndex) -> Option<Hover> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let path_cow = uri.to_file_path()?;
    let path = &path_cow;

    if let Some((key, origin_loc)) = project_index.get_key_at_position(path, position) {
        let locations = project_index.get_locations(key.entity, &key.name);

        if locations.is_empty() {
            return None;
        }

        let mut md_text = String::new();
        let icon = match key.entity {
            EntityType::Command => "⚙️",
            EntityType::Event => "📡",
        };

        let _ = write!(md_text, "### {} {:?}: `{}`\n\n", icon, key.entity, key.name);

        // Command return type
        if key.entity == EntityType::Command {
            if let Some(schema) = project_index.get_schema(&key.name) {
                let _ = writeln!(md_text, "**Returns:** `{}`\n", schema.return_type);
            }
        }

        // Definitions Section
        push_definitions_section(&mut md_text, key.entity, &locations);

        // Reference count breakdown
        push_reference_summary(&mut md_text, key.entity, &locations);

        // Sample references
        push_sample_references(&mut md_text, key.entity, &locations);

        // Add warnings/tips
        push_diagnostic_tips(&mut md_text, key.entity, &key.name, project_index);

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

fn push_definitions_section(md_text: &mut String, entity: EntityType, locations: &[LocationInfo]) {
    let definitions: Vec<&LocationInfo> = locations
        .iter()
        .filter(|l| match entity {
            EntityType::Command => l.behavior == Behavior::Definition,
            EntityType::Event => l.behavior == Behavior::Listen,
        })
        .collect();

    if !definitions.is_empty() {
        md_text.push_str("**Definition:**\n");

        for def in &definitions {
            let file_icon = if def.path.extension().is_some_and(|e| e == "rs") {
                "🦀"
            } else {
                "⚡️"
            };

            let filename = def.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

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
}

fn push_reference_summary(md_text: &mut String, entity: EntityType, locations: &[LocationInfo]) {
    let total_refs = locations.len();
    let _ = writeln!(md_text, "**References ({total_refs} total)**");

    if entity == EntityType::Command {
        let definitions_count = locations
            .iter()
            .filter(|l| l.behavior == Behavior::Definition)
            .count();
        let calls_count = locations
            .iter()
            .filter(|l| matches!(l.behavior, Behavior::Call | Behavior::SpectaCall))
            .count();

        if definitions_count > 0 {
            let _ = writeln!(md_text, "- 🦀 {definitions_count} definition(s)");
        }
        if calls_count > 0 {
            let _ = writeln!(md_text, "- ⚡ {calls_count} call(s)");
        }
    } else {
        let emits_count = locations
            .iter()
            .filter(|l| matches!(l.behavior, Behavior::Emit))
            .count();
        let listens_count = locations
            .iter()
            .filter(|l| matches!(l.behavior, Behavior::Listen))
            .count();

        if emits_count > 0 {
            let _ = writeln!(md_text, "- 📤 {emits_count} emit(s)");
        }
        if listens_count > 0 {
            let _ = writeln!(md_text, "- 👂 {listens_count} listener(s)");
        }
    }

    md_text.push('\n');
}

fn push_sample_references(md_text: &mut String, entity: EntityType, locations: &[LocationInfo]) {
    let references: Vec<&LocationInfo> = locations
        .iter()
        .filter(|l| match entity {
            EntityType::Command => l.behavior != Behavior::Definition,
            EntityType::Event => l.behavior != Behavior::Listen,
        })
        .collect();

    if !references.is_empty() {
        md_text.push_str("**Sample References:**\n");
        for (i, rf) in references.iter().enumerate() {
            if i >= 5 {
                let _ = writeln!(md_text, "- *...and {} more*", references.len() - 5);
                break;
            }

            let file_icon = if rf.path.extension().is_some_and(|e| e == "rs") {
                "🦀"
            } else {
                "⚡️"
            };

            let filename = rf.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            let mut behavior_badge = format!("{:?}", rf.behavior).to_uppercase();

            if let Some(count) = rf.call_arg_count {
                let _ = write!(behavior_badge, " ({count} ARGS)");
            }

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
}

fn push_diagnostic_tips(
    md_text: &mut String,
    entity: EntityType,
    name: &str,
    project_index: &ProjectIndex,
) {
    let key = crate::indexer::IndexKey {
        entity,
        name: name.to_string(),
    };
    let info = project_index.get_diagnostic_info(&key);

    if entity == EntityType::Command && !info.has_definition {
        md_text.push_str("⚠️ *No backend implementation found*\n");
    } else if entity == EntityType::Command && !info.has_calls {
        md_text.push_str("💡 *Defined but never called in frontend*\n");
    } else if entity == EntityType::Event && !info.has_emitters {
        md_text.push_str("💡 *Event listened for but never emitted*\n");
    } else if entity == EntityType::Event && !info.has_listeners {
        md_text.push_str("💡 *Event emitted but no listeners found*\n");
    }
}
