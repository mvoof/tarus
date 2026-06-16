//! LSP Server library - exports modules for testing

#![warn(clippy::all, clippy::pedantic)]

pub mod capabilities;
pub mod constants;
pub mod discovery;
pub mod file_processor;
pub mod indexer;
pub mod syntax;
pub mod tree_parser;
pub mod utils;

// Public re-exports for backward compatibility and test access
pub use crate::discovery::config_reader;
pub use crate::discovery::scanner;
pub use crate::tree_parser::bindings_reader;
pub use crate::tree_parser::rust_attr;
pub use crate::tree_parser::rust_type_extractor;
pub use crate::utils::ts_tree_utils;
