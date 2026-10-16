//! Ordering and coalescing for work the server runs in the background: diagnostics
//! publishes that can finish out of order, and re-index jobs that can pile up.

use dashmap::{DashMap, DashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Keeps an older diagnostics result from being sent over a newer one.
///
/// Each computation takes a number as it starts ([`Self::start`]): one that starts
/// later reads an index at least as new. A result is sent only if no result with a
/// higher number was sent for its file already.
#[derive(Debug, Default)]
pub struct PublishOrder {
    next: AtomicU64,
    last_sent: DashMap<PathBuf, u64>,
    // Held from deciding to send one file's diagnostics until they are sent.
    send_lock: tokio::sync::Mutex<()>,
}

impl PublishOrder {
    /// Number a computation; call it right before reading the index.
    #[must_use]
    pub fn start(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst)
    }

    /// Run `send` for `file`'s result numbered `sequence`, unless a later-numbered
    /// result was sent for it already. Returns whether it ran.
    pub async fn send_if_newest<F, Fut>(&self, file: &Path, sequence: u64, send: F) -> bool
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = ()>,
    {
        let _guard = self.send_lock.lock().await;

        if self
            .last_sent
            .get(file)
            .is_some_and(|last| *last > sequence)
        {
            return false;
        }

        self.last_sent.insert(file.to_path_buf(), sequence);
        send().await;

        true
    }
}

/// Files whose re-index job is queued but has not read their text yet.
///
/// A job reads the file's text when it runs, so a second one queued behind it
/// would only index the same text again.
#[derive(Debug, Default)]
pub struct ReindexQueue {
    queued: DashSet<PathBuf>,
}

impl ReindexQueue {
    /// Queue a job for `path`: false when one queued earlier has not started yet,
    /// and will index the newest text anyway.
    #[must_use]
    pub fn try_queue(&self, path: &Path) -> bool {
        self.queued.insert(path.to_path_buf())
    }

    /// The job for `path` is about to read its text: an edit from here on needs a
    /// job of its own.
    pub fn started(&self, path: &Path) {
        self.queued.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[tokio::test]
    async fn test_an_older_result_is_not_sent_after_a_newer_one() {
        let publishes = PublishOrder::default();
        let file = Path::new("app.ts");
        let count = AtomicUsize::new(0);
        let publish = || async {
            count.fetch_add(1, Ordering::SeqCst);
        };

        let earlier = publishes.start();
        let later = publishes.start();

        assert!(publishes.send_if_newest(file, later, publish).await);
        assert!(!publishes.send_if_newest(file, earlier, publish).await);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_results_are_ordered_per_file() {
        let publishes = PublishOrder::default();
        let publish = || async {};

        let earlier = publishes.start();
        let later = publishes.start();

        assert!(
            publishes
                .send_if_newest(Path::new("a.ts"), later, publish)
                .await
        );
        assert!(
            publishes
                .send_if_newest(Path::new("b.ts"), earlier, publish)
                .await
        );
    }

    #[tokio::test]
    async fn test_a_newer_result_waits_for_an_older_one_mid_send() {
        // Decided to go out, the older result is still being sent when the newer one
        // arrives: the newer one must go out after it, not be overwritten by it.
        let publishes = PublishOrder::default();
        let file = Path::new("app.ts");
        let sent = std::sync::Mutex::new(Vec::new());
        let release = tokio::sync::Notify::new();

        let earlier = publishes.start();
        let later = publishes.start();

        let older_send = publishes.send_if_newest(file, earlier, || async {
            release.notified().await;
            sent.lock().unwrap().push(earlier);
        });
        let newer_send = async {
            tokio::task::yield_now().await;
            let newer = publishes.send_if_newest(file, later, || async {
                sent.lock().unwrap().push(later);
            });
            release.notify_one();
            newer.await
        };

        let (older_ran, newer_ran) = tokio::join!(older_send, newer_send);

        assert!(older_ran && newer_ran);
        assert_eq!(*sent.lock().unwrap(), vec![earlier, later]);
    }

    #[test]
    fn test_a_job_queued_behind_a_waiting_one_is_skipped() {
        let queue = ReindexQueue::default();
        let file = Path::new("app.ts");

        assert!(queue.try_queue(file));
        assert!(!queue.try_queue(file));
        assert!(queue.try_queue(Path::new("other.ts")));
    }

    #[test]
    fn test_an_edit_after_the_job_started_queues_a_new_one() {
        let queue = ReindexQueue::default();
        let file = Path::new("app.ts");

        assert!(queue.try_queue(file));
        queue.started(file);

        assert!(queue.try_queue(file));
    }
}
