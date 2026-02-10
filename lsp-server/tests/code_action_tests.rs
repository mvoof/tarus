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

    let actions = handle_code_action(&params, &index, Some(&temp_dir));

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

#[test]
fn test_wrapper_generation() {
    let index = create_mock_project_index();
    let ts_path = PathBuf::from("/tmp/src/components/MyComp.ts");
    let rust_path = PathBuf::from("/tmp/src-tauri/src/main.rs");

    let key = IndexKey {
        name: "existing_cmd".to_string(),
        entity: EntityType::Command,
    };

    // Add Definition
    let def_loc = LocationInfo {
        path: rust_path.clone(),
        range: Range::default(),
        behavior: Behavior::Definition,
        parameters: Some(vec![
            Parameter {
                name: "id".to_string(),
                type_name: "i32".to_string(),
            },
            Parameter {
                name: "name".to_string(),
                type_name: "String".to_string(),
            },
        ]),
        return_type: Some("Result<String, String>".to_string()),
        fields: None,
        attributes: None,
    };

    // Add Call usage
    let call_loc = LocationInfo {
        path: ts_path.clone(),
        range: Range {
            start: Position {
                line: 5,
                character: 0,
            },
            end: Position {
                line: 5,
                character: 10,
            },
        },
        behavior: Behavior::Call,
        parameters: None,
        return_type: None,
        fields: None,
        attributes: None,
    };

    index
        .map
        .insert(key.clone(), vec![def_loc, call_loc.clone()]);
    index.file_map.insert(ts_path.clone(), vec![key]);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: Uri::from_file_path(&ts_path).unwrap(),
        },
        range: Range {
            start: Position {
                line: 5,
                character: 0,
            },
            end: Position {
                line: 5,
                character: 10,
            },
        },
        context: CodeActionContext::default(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    // Create workspace with TS file candidate
    let temp_dir = std::env::temp_dir().join("tarus_test_actions_2");
    let _ = std::fs::remove_dir_all(&temp_dir);
    let src_dir = temp_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    // create api.ts
    std::fs::write(src_dir.join("api.ts"), "// API file").unwrap();

    let actions = handle_code_action(&params, &index, Some(&temp_dir));

    assert!(actions.is_some(), "Should return actions");
    let actions = actions.unwrap();
    let wrapper_action = actions.iter().find(|a| {
        if let CodeActionOrCommand::CodeAction(ca) = a {
            ca.title.contains("Generate wrapper 'existingCmd'")
        } else {
            false
        }
    });

    assert!(
        wrapper_action.is_some(),
        "Generate wrapper action not found"
    );

    if let CodeActionOrCommand::CodeAction(ca) = wrapper_action.unwrap() {
        assert!(ca.title.contains("in api.ts"));
        let edit = ca.edit.as_ref().unwrap();
        let change = &edit.document_changes.as_ref().unwrap();
        if let DocumentChanges::Edits(edits) = change {
            // Edits should target api.ts
            let uri = &edits[0].text_document.uri;
            assert!(uri.path().as_str().ends_with("api.ts"));

            let text_edit = &edits[0].edits[0]; // OneOf::Left
            if let OneOf::Left(te) = text_edit {
                println!("Wrapper: {}", te.new_text);
                assert!(te.new_text.contains(
                    "export async function existingCmd(id: number, name: string): Promise<string>"
                ));
                assert!(te
                    .new_text
                    .contains("return await invoke('existing_cmd', { id, name });"));
            }
        }
    }
}
