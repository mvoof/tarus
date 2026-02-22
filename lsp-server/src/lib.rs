//! LSP Server library - exports modules for testing

#![warn(clippy::all, clippy::pedantic)]

pub mod bindings_reader_v2;
pub mod capabilities;
pub mod file_processor;
pub mod indexer;
pub mod scanner;
pub mod syntax;
pub mod tree_parser;
