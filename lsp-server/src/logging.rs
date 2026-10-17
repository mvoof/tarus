//! The server's output channel: what it shows and how a line reads.
//!
//! `info`, `warn` and `error` are always shown; `debug` only in developer mode
//! (`tarus.developerMode`). The editor prefixes every line with its level and time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tower_lsp_server::lsp_types::{MessageType, Position, Uri};
use tower_lsp_server::{Client, UriExt};

/// Width of the request kind column, so request lines line up
const REQUEST_KIND_WIDTH: usize = 11;

/// Writes to the editor's output channel for this server.
#[derive(Debug, Clone)]
pub struct Log {
    client: Client,
    is_debug: Arc<AtomicBool>,
    /// Paths are shown relative to it once it is known.
    root: Arc<OnceLock<PathBuf>>,
}

impl Log {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            is_debug: Arc::new(AtomicBool::new(false)),
            root: Arc::new(OnceLock::new()),
        }
    }

    pub fn set_debug(&self, is_enabled: bool) {
        self.is_debug.store(is_enabled, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_debug(&self) -> bool {
        self.is_debug.load(Ordering::Relaxed)
    }

    pub fn set_root(&self, root: PathBuf) {
        let _ = self.root.set(root);
    }

    pub async fn info(&self, message: impl Into<String>) {
        self.client
            .log_message(MessageType::INFO, message.into())
            .await;
    }

    pub async fn warn(&self, message: impl Into<String>) {
        self.client
            .log_message(MessageType::WARNING, message.into())
            .await;
    }

    pub async fn error(&self, message: impl Into<String>) {
        self.client
            .log_message(MessageType::ERROR, message.into())
            .await;
    }

    /// Shown in developer mode only; `message` is not even built otherwise.
    pub async fn debug(&self, message: impl FnOnce() -> String) {
        if self.is_debug() {
            self.client.log_message(MessageType::LOG, message()).await;
        }
    }

    /// One request line: `CodeLens    src/app.ts → 6 lenses (3 ms)`.
    pub async fn request(&self, kind: &str, target: &str, outcome: &str, started: Instant) {
        self.debug(|| {
            format!(
                "{kind:<REQUEST_KIND_WIDTH$} {target} → {outcome} ({})",
                format_duration(started.elapsed())
            )
        })
        .await;
    }

    /// `path` relative to the workspace, with `/` separators.
    #[must_use]
    pub fn path(&self, path: &Path) -> String {
        let relative = self
            .root
            .get()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path);

        relative.to_string_lossy().replace('\\', "/")
    }

    /// `uri` as a workspace path, with a 1-based `:line:column` when given.
    #[must_use]
    pub fn location(&self, uri: &Uri, position: Option<Position>) -> String {
        let file = uri
            .to_file_path()
            .map_or_else(|| uri.as_str().to_string(), |path| self.path(&path));

        match position {
            Some(position) => format!("{file}:{}:{}", position.line + 1, position.character + 1),
            None => file,
        }
    }
}

/// A request's result: `none`, or `count` with `noun` (`6 lenses`).
#[must_use]
pub fn outcome(count: Option<usize>, noun: &str) -> String {
    match count {
        Some(count) => format!("{count} {noun}"),
        None => "none".to_string(),
    }
}

/// `3 ms` under a second, `1.6 s` from there.
#[must_use]
pub fn format_duration(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{} ms", duration.as_millis())
    } else {
        format!("{:.1} s", duration.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_in_ms_then_seconds() {
        assert_eq!(format_duration(Duration::from_micros(2_400)), "2 ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999 ms");
        assert_eq!(format_duration(Duration::from_millis(1_640)), "1.6 s");
    }

    #[test]
    fn an_outcome_is_a_count_or_none() {
        assert_eq!(outcome(Some(6), "lenses"), "6 lenses");
        assert_eq!(outcome(Some(0), "lenses"), "0 lenses");
        assert_eq!(outcome(None, "lenses"), "none");
    }
}
