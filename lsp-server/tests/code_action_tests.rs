use lsp_server::capabilities::code_actions::handle_code_action;
use lsp_server::indexer::{IndexKey, LocationInfo, Parameter, ProjectIndex};
use lsp_server::syntax::{Behavior, EntityType};
use std::path::PathBuf;
use tower_lsp_server::ls_types::{
    CodeActionContext, CodeActionOrCommand, CodeActionParams, DocumentChanges, OneOf, Position,
    Range, TextDocumentIdentifier, Uri,
};

fn create_mock_project_index() -> ProjectIndex {
    ProjectIndex::default()
}

#[test]
fn test_undefined_command_action() {
    let index = create_mock_project_index();
    let path = PathBuf::from("/tmp/test.ts");
    let key = IndexKey {
        name: "my_missing_cmd".to_string(),
        entity: EntityType::Command,
    };

    // Add call site to index
    let loc = LocationInfo {
        path: path.clone(),
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 10,
            },
        },
        behavior: Behavior::Call,
        parameters: Some(vec![Parameter {
            name: "args".to_string(),
            type_name: "{ id: number, valid: boolean }".to_string(),
        }]),
        return_type: None,
        fields: None,
        attributes: None,
    };

    index.map.insert(key.clone(), vec![loc]);
    index.file_map.insert(path.clone(), vec![key]);

    // Create params
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: Uri::from_file_path(&path).unwrap(),
        },
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 10,
            },
        },
        context: CodeActionContext::default(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    // Create temp workspace structure
    let temp_dir = std::env::temp_dir().join("tarus_test_actions_1");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("src-tauri/src")).unwrap();
    std::fs::write(temp_dir.join("src-tauri/tauri.conf.json"), "{}").unwrap();
    std::fs::write(temp_dir.join("src-tauri/src/main.rs"), "fn main() {}").unwrap();

    // handle_code_action now expects the pre-computed src-tauri directory
    let src_tauri_dir = temp_dir.join("src-tauri");
    let actions = handle_code_action(&params, &index, Some(&src_tauri_dir));

    assert!(actions.is_some(), "Should return actions");

    let actions = actions.unwrap();

    assert!(!actions.is_empty(), "Should have at least one action");

    let create_action = actions.iter().find(|a| {
        if let CodeActionOrCommand::CodeAction(ca) = a {
            ca.title.contains("Create Rust command 'my_missing_cmd'")
        } else {
            false
        }
    });

    assert!(
        create_action.is_some(),
        "Create Rust command action not found"
    );

    if let CodeActionOrCommand::CodeAction(ca) = create_action.unwrap() {
        let edit = ca.edit.as_ref().unwrap();
        let change = &edit.document_changes.as_ref().unwrap();

        if let DocumentChanges::Edits(edits) = change {
            let text_edit = &edits[0].edits[0]; // OneOf::Left
            if let OneOf::Left(te) = text_edit {
                println!("New Text: {}", te.new_text);

                assert!(
                    te.new_text
                        .contains("fn my_missing_cmd(id: i64, valid: bool)"),
                    "Generated signature incorrect: {}",
                    te.new_text
                );
            }
        }
    }
}
