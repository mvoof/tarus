//! Common test utilities and helpers

use std::path::{Path, PathBuf};

/// Load a fixture file from the fixtures directory
#[allow(dead_code)]
pub fn load_fixture(relative_path: &str) -> String {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/common/fixtures");
    let fixture_path = fixtures_dir.join(relative_path);
    std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("Failed to load fixture {:?}: {}", fixture_path, e))
}

/// Create a test PathBuf with a given extension
pub fn test_path(filename: &str) -> PathBuf {
    PathBuf::from(filename)
}
