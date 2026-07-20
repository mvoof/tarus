//! Language detection and query routing

/// Query files embedded at compile time
pub(crate) const RUST_QUERY: &str = include_str!("../queries/rust.scm");
pub(crate) const TS_QUERY: &str = include_str!("../queries/typescript.scm");
pub(crate) const JS_QUERY: &str = include_str!("../queries/javascript.scm");

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

/// Get the query string for a given language
pub(crate) fn get_query_source(lang: LangType) -> &'static str {
    match lang {
        LangType::Rust => RUST_QUERY,
        LangType::TypeScript
        | LangType::TypeScriptJsx
        | LangType::Vue
        | LangType::Svelte
        | LangType::Angular => TS_QUERY,
        LangType::JavaScript | LangType::JavaScriptJsx => JS_QUERY,
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
