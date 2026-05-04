//! Language detection and query routing

/// Query files embedded at compile time
pub(super) const RUST_QUERY: &str = include_str!("../queries/rust.scm");
pub(super) const TS_QUERY: &str = include_str!("../queries/typescript.scm");
pub(super) const JS_QUERY: &str = include_str!("../queries/javascript.scm");

/// Supported language types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LangType {
    Rust,
    TypeScript,
    JavaScript,
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
            "ts" | "tsx" => Some(Self::TypeScript),
            "js" | "jsx" => Some(Self::JavaScript),
            "vue" => Some(Self::Vue),
            "svelte" => Some(Self::Svelte),
            _ => None,
        }
    }
}

/// Get the query string for a given language
pub(super) fn get_query_source(lang: LangType) -> &'static str {
    match lang {
        LangType::Rust => RUST_QUERY,
        LangType::TypeScript | LangType::Vue | LangType::Svelte | LangType::Angular => TS_QUERY,
        LangType::JavaScript => JS_QUERY,
    }
}

/// Check if TypeScript file contains Angular decorators
pub(super) fn is_angular_file(content: &str) -> bool {
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
