//! Language detection and query routing

use std::sync::LazyLock;
use tree_sitter::{Language, Query};

/// Query files embedded at compile time
const RUST_QUERY: &str = include_str!("../queries/rust.scm");
const TS_QUERY: &str = concat!(
    include_str!("../queries/typescript.scm"),
    "\n",
    include_str!("../queries/typescript_type_assertion.scm"),
);
const TSX_QUERY: &str = include_str!("../queries/typescript.scm");
const JS_QUERY: &str = include_str!("../queries/javascript.scm");

/// Supported language types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LangType {
    Rust,
    TypeScript,
    TypeScriptJsx,
    JavaScript,
    JavaScriptJsx,
    Vue,
    Svelte,
    Angular,
}

impl LangType {
    /// Get language type from file extension
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::TypeScriptJsx),
            "js" => Some(Self::JavaScript),
            "jsx" => Some(Self::JavaScriptJsx),
            "vue" => Some(Self::Vue),
            "svelte" => Some(Self::Svelte),
            _ => None,
        }
    }
}

// Compiled once per process: compiling the TypeScript query takes ~75 ms, longer
// than parsing most files, and every file of a scan needs it.
static RUST_COMPILED_QUERY: LazyLock<Query> =
    LazyLock::new(|| compile(&tree_sitter_rust::LANGUAGE.into(), RUST_QUERY));

static TS_COMPILED_QUERY: LazyLock<Query> = LazyLock::new(|| {
    compile(
        &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        TS_QUERY,
    )
});

static TSX_COMPILED_QUERY: LazyLock<Query> =
    LazyLock::new(|| compile(&tree_sitter_typescript::LANGUAGE_TSX.into(), TSX_QUERY));

static JS_COMPILED_QUERY: LazyLock<Query> =
    LazyLock::new(|| compile(&tree_sitter_javascript::LANGUAGE.into(), JS_QUERY));

fn compile(language: &Language, source: &str) -> Query {
    Query::new(language, source).expect("embedded query compiles")
}

/// The compiled query for a language
pub(crate) fn compiled_query(lang: LangType) -> &'static Query {
    match lang {
        LangType::Rust => &RUST_COMPILED_QUERY,
        LangType::TypeScript | LangType::Vue | LangType::Svelte | LangType::Angular => {
            &TS_COMPILED_QUERY
        }
        LangType::TypeScriptJsx => &TSX_COMPILED_QUERY,
        LangType::JavaScript | LangType::JavaScriptJsx => &JS_COMPILED_QUERY,
    }
}

/// Check if TypeScript file contains Angular decorators
pub(crate) fn is_angular_file(content: &str) -> bool {
    const ANGULAR_DECORATORS: &[&str] = &[
        "@Component(",
        "@Injectable(",
        "@NgModule(",
        "@Directive(",
        "@Pipe(",
    ];

    ANGULAR_DECORATORS
        .iter()
        .any(|decorator| content.contains(decorator))
}
