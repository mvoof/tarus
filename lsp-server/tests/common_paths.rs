//! Common test utilities for paths

use std::path::PathBuf;

/// Create a test PathBuf with a given extension
pub fn test_path(filename: &str) -> PathBuf {
    PathBuf::from(filename)
}
