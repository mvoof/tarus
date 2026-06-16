//! LSP Server library - exports modules for testing

#![warn(clippy::all, clippy::pedantic)]

pub mod capabilities;
pub mod constants;
pub mod discovery;
pub mod indexer;
pub mod parser;
pub mod pipeline;
pub mod syntax;
pub mod utils;

// Backward-compatible aliases for test files
pub use crate::discovery::config_reader;
pub use crate::discovery::scanner;
pub use crate::parser as tree_parser;
pub use crate::parser::rust::attrs as rust_attr;
pub use crate::parser::rust::types as rust_type_extractor;
pub use crate::parser::typescript::bindings as bindings_reader;
pub use crate::pipeline as file_processor;
pub use crate::utils::ts_tree_utils;
