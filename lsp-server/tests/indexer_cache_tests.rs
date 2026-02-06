//! Additional indexer tests for caching and reporting functionality
//! These tests ensure the refactoring doesn't break existing behavior

mod common_paths;

use common_paths::test_path;
use lsp_server::indexer::{FileIndex, Finding, ProjectIndex};
use lsp_server::syntax::{Behavior, EntityType};
use tower_lsp_server::lsp_types::{Position, Range};

fn create_test_finding_with_range(
    key: &str,
    entity: EntityType,
    behavior: Behavior,
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
) -> Finding {
    Finding {
        key: key.to_string(),
        entity,
        behavior,
        range: Range {
            start: Position {
                line: start_line,
                character: start_char,
            },
            end: Position {
                line: end_line,
                character: end_char,
            },
        },
        parameters: None,
        return_type: None,
        fields: None,
        attributes: None,
    }
}

#[test]
fn test_lens_data_caching() {
    let index = ProjectIndex::new();
    let backend_path = test_path("backend.rs");
    let frontend_path = test_path("frontend.ts");

    // Add backend command definition
    index.add_file(FileIndex {
        path: backend_path.clone(),
        findings: vec![create_test_finding_with_range(
            "greet",
            EntityType::Command,
            Behavior::Definition,
            5,
            0,
            10,
            0,
        )],
    });

    // Add frontend call (creates cross-reference)
    index.add_file(FileIndex {
        path: frontend_path.clone(),
        findings: vec![create_test_finding_with_range(
            "greet",
            EntityType::Command,
            Behavior::Call,
            2,
            0,
            2,
            10,
        )],
    });

    // First call - builds cache for backend (which has references now)
    let lens_data_1 = index.get_lens_data(&backend_path);
    assert!(
        !lens_data_1.is_empty(),
        "Should have lens data for backend with call"
    );

    // Second call - should use cache (same result)
    let lens_data_2 = index.get_lens_data(&backend_path);
    assert_eq!(lens_data_1.len(), lens_data_2.len());
}

#[test]
fn test_position_index_caching() {
    let index = ProjectIndex::new();
    let path = test_path("test.ts");

    let file_index = FileIndex {
        path: path.clone(),
        findings: vec![create_test_finding_with_range(
            "greet",
            EntityType::Command,
            Behavior::Call,
            5,
            10,
            5,
            15,
        )],
    };

    index.add_file(file_index);

    let pos = Position {
        line: 5,
        character: 12,
    };

    // First call - builds cache
    let result1 = index.get_key_at_position(&path, pos);
    assert!(result1.is_some());

    // Second call - should use cache
    let result2 = index.get_key_at_position(&path, pos);
    assert!(result2.is_some());

    let (key1, _) = result1.unwrap();
    let (key2, _) = result2.unwrap();
    assert_eq!(key1.name, key2.name);
}

#[test]
fn test_cache_invalidation_on_file_update() {
    let index = ProjectIndex::new();
    let backend_path = test_path("backend.rs");
    let frontend_path = test_path("frontend.ts");

    // Add initial backend file
    index.add_file(FileIndex {
        path: backend_path.clone(),
        findings: vec![create_test_finding_with_range(
            "old_command",
            EntityType::Command,
            Behavior::Definition,
            0,
            0,
            1,
            0,
        )],
    });

    // Add frontend call to create references
    index.add_file(FileIndex {
        path: frontend_path.clone(),
        findings: vec![create_test_finding_with_range(
            "old_command",
            EntityType::Command,
            Behavior::Call,
            0,
            0,
            0,
            10,
        )],
    });

    // Get initial lens data (builds cache)
    let initial_lens = index.get_lens_data(&backend_path);
    let initial_len = initial_lens.len();

    // Update backend file with new command
    index.add_file(FileIndex {
        path: backend_path.clone(),
        findings: vec![create_test_finding_with_range(
            "new_command",
            EntityType::Command,
            Behavior::Definition,
            0,
            0,
            1,
            0,
        )],
    });

    // Cache should be invalidated - verify by checking lens data changes
    let updated_lens = index.get_lens_data(&backend_path);
    // After update with different command, lens data should be different
    // (old_command calls are gone, no refs to new_command yet)
    assert!(updated_lens.len() != initial_len || updated_lens.is_empty());
}

#[test]
fn test_technical_report() {
    let index = ProjectIndex::new();

    // Add some data
    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![
            create_test_finding_with_range(
                "command1",
                EntityType::Command,
                Behavior::Definition,
                0,
                0,
                1,
                0,
            ),
            create_test_finding_with_range("event1", EntityType::Event, Behavior::Emit, 2, 0, 3, 0),
        ],
    });

    let report = index.technical_report();

    // Report should contain useful information (just verify it's not empty and has some structure)
    assert!(!report.is_empty(), "Technical report should not be empty");
    assert!(
        report.len() > 10,
        "Report should contain meaningful content"
    );
}

#[test]
fn test_file_report() {
    let index = ProjectIndex::new();
    let path = test_path("test.rs");

    index.add_file(FileIndex {
        path: path.clone(),
        findings: vec![create_test_finding_with_range(
            "my_command",
            EntityType::Command,
            Behavior::Definition,
            0,
            0,
            1,
            0,
        )],
    });

    let report = index.file_report(&path);

    // Report should contain file-specific information
    assert!(!report.is_empty());
}

#[test]
fn test_get_all_names_with_caching() {
    let index = ProjectIndex::new();

    index.add_file(FileIndex {
        path: test_path("commands.rs"),
        findings: vec![
            create_test_finding_with_range(
                "cmd1",
                EntityType::Command,
                Behavior::Definition,
                0,
                0,
                1,
                0,
            ),
            create_test_finding_with_range(
                "cmd2",
                EntityType::Command,
                Behavior::Definition,
                2,
                0,
                3,
                0,
            ),
        ],
    });

    // First call - builds cache
    let names1 = index.get_all_names(EntityType::Command);
    assert_eq!(names1.len(), 2);

    // Second call - uses cache
    let names2 = index.get_all_names(EntityType::Command);
    assert_eq!(names2.len(), 2);
    // Verify both calls return same data
    assert_eq!(names1.len(), names2.len());
    for (name1, _) in &names1 {
        assert!(names2.iter().any(|(name2, _)| name1 == name2));
    }
}

#[test]
fn test_multiple_positions_same_file() {
    let index = ProjectIndex::new();
    let path = test_path("commands.rs");

    // Add file with multiple commands at different positions
    index.add_file(FileIndex {
        path: path.clone(),
        findings: vec![
            create_test_finding_with_range(
                "cmd1",
                EntityType::Command,
                Behavior::Definition,
                5,
                0,
                5,
                10,
            ),
            create_test_finding_with_range(
                "cmd2",
                EntityType::Command,
                Behavior::Definition,
                15,
                0,
                15,
                10,
            ),
        ],
    });

    // Test position in first command
    let result1 = index.get_key_at_position(
        &path,
        Position {
            line: 5,
            character: 5,
        },
    );
    assert!(result1.is_some());
    assert_eq!(result1.unwrap().0.name, "cmd1");

    // Test position in second command
    let result2 = index.get_key_at_position(
        &path,
        Position {
            line: 15,
            character: 5,
        },
    );
    assert!(result2.is_some());
    assert_eq!(result2.unwrap().0.name, "cmd2");

    // Test position outside any command
    let result3 = index.get_key_at_position(
        &path,
        Position {
            line: 10,
            character: 0,
        },
    );
    assert!(result3.is_none());
}
