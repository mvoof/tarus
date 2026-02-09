//! Query helpers for tree-sitter parsing
//!
//! This module provides utilities to simplify working with tree-sitter queries,
//! particularly for managing capture indices.

use std::collections::HashMap;
use tree_sitter::{Query, QueryCapture};

/// Helper for managing query capture indices
///
/// This struct provides a convenient way to retrieve capture indices by name,
/// reducing boilerplate code in parsers.
#[derive(Debug)]
pub struct CaptureIndices {
    indices: HashMap<String, Option<u32>>,
}

impl CaptureIndices {
    /// Create a new `CaptureIndices` from a query and list of capture names
    ///
    /// # Arguments
    ///
    /// * `query` - The tree-sitter query to extract indices from
    /// * `names` - Slice of capture names to look up
    ///
    /// # Example
    ///
    /// ```ignore
    /// let indices = CaptureIndices::from_query(&query, &[
    ///     "command_name",
    ///     "command_params",
    ///     "command_return_type",
    /// ]);
    /// ```
    #[must_use]
    pub fn from_query(query: &Query, names: &[&str]) -> Self {
        let mut indices = HashMap::new();
        for name in names {
            indices.insert((*name).to_string(), query.capture_index_for_name(name));
        }
        Self { indices }
    }

    /// Get the capture index for a given name
    ///
    /// Returns `None` if the capture name doesn't exist in the query
    #[must_use]
    pub fn get(&self, name: &str) -> Option<u32> {
        self.indices.get(name).copied().flatten()
    }

    /// Find the first capture in a list that matches the given index
    ///
    /// This is a common pattern when processing query matches
    #[must_use]
    pub fn find_capture<'a>(
        &self,
        captures: &'a [QueryCapture<'a>],
        name: &str,
    ) -> Option<&'a QueryCapture<'a>> {
        let idx = self.get(name)?;
        captures.iter().find(|c| c.index == idx)
    }

    /// Find all captures in a list that match the given index
    #[must_use]
    pub fn find_captures<'a>(
        &self,
        captures: &'a [QueryCapture<'a>],
        name: &str,
    ) -> Vec<&'a QueryCapture<'a>> {
        let Some(idx) = self.get(name) else {
            return Vec::new();
        };
        captures.iter().filter(|c| c.index == idx).collect()
    }
}

