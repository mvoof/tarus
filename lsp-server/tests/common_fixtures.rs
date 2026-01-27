//! Common test utilities for loading fixtures

use std::path::Path;

/// Load a fixture file from the fixtures directory
pub fn load_fixture(relative_path: &str) -> String {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let fixture_path = fixtures_dir.join(relative_path);
    std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("Failed to load fixture {:?}: {}", fixture_path, e))
}
