//! LSP Server library - exports modules for testing

#![warn(clippy::all, clippy::pedantic)]

pub mod bindings_reader;
pub mod capabilities;
pub mod config_reader;
pub mod file_processor;
pub mod indexer;
pub mod rust_type_extractor;
pub mod scanner;
pub mod syntax;
pub mod tree_parser;
pub mod ts_tree_utils;
pub mod utils;
