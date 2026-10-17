//! Timings of the startup scan, run by hand: they print numbers, they assert nothing.
//!
//! ```bash
//! cargo test --release --manifest-path lsp-server/Cargo.toml --test indexing_profile_tests -- --ignored --nocapture
//! TARUS_PROFILE_ROOT=/path/to/tauri-app cargo test --release ... -- --ignored --nocapture
//! ```

use lsp_server::indexer::ProjectIndex;
use lsp_server::pipeline;
use lsp_server::scanner::scan_workspace_files;
use std::time::{Duration, Instant};
use tree_sitter::{Language, Query};

const QUERY_COMPILE_RUNS: u32 = 5;

const TS_QUERY: &str = concat!(
    include_str!("../src/queries/typescript.scm"),
    "\n",
    include_str!("../src/queries/typescript_type_assertion.scm"),
);

fn average_compile_time(language: &Language, source: &str) -> Duration {
    let started = Instant::now();

    for _ in 0..QUERY_COMPILE_RUNS {
        Query::new(language, source).expect("query compiles");
    }

    started.elapsed() / QUERY_COMPILE_RUNS
}

#[test]
#[ignore = "profiling, run by hand"]
fn profile_query_compilation() {
    let cases: [(&str, Language, &str); 4] = [
        (
            "typescript.scm (TS)",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            TS_QUERY,
        ),
        (
            "typescript.scm (TSX)",
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            include_str!("../src/queries/typescript.scm"),
        ),
        (
            "javascript.scm",
            tree_sitter_javascript::LANGUAGE.into(),
            include_str!("../src/queries/javascript.scm"),
        ),
        (
            "rust.scm",
            tree_sitter_rust::LANGUAGE.into(),
            include_str!("../src/queries/rust.scm"),
        ),
    ];

    for (name, language, source) in cases {
        println!(
            "{name}: {:?} per compile",
            average_compile_time(&language, source)
        );
    }
}

#[test]
#[ignore = "profiling, run by hand"]
fn profile_startup_scan() {
    let Ok(root) = std::env::var("TARUS_PROFILE_ROOT") else {
        println!("TARUS_PROFILE_ROOT is not set, nothing to profile");

        return;
    };

    let started = Instant::now();
    let files = scan_workspace_files(std::path::Path::new(&root), &[]).files;
    println!("scan: {} files in {:?}", files.len(), started.elapsed());

    let contents: Vec<_> = files
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok().map(|text| (path, text)))
        .collect();

    let project_index = ProjectIndex::new();

    let started = Instant::now();

    for (path, content) in &contents {
        pipeline::collect_constants_from_file(path, content, &project_index);
    }

    println!("pass 1 (constants, helpers): {:?}", started.elapsed());

    let started = Instant::now();

    for (path, content) in &contents {
        pipeline::process_file_content(path, content, &project_index);
    }

    println!("pass 2 (full parse): {:?}", started.elapsed());
}
