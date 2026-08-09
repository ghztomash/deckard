//! Pollable operation progress without per-item callbacks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Shared progress for one operation stage.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    inner: Arc<ProgressInner>,
}

#[derive(Debug, Default)]
struct ProgressInner {
    done: AtomicUsize,
    total: AtomicUsize,
    total_known: AtomicBool,
}

/// A point-in-time progress snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgressSnapshot {
    /// Number of completed items.
    pub done: usize,
    /// Total item count, when the operation can determine it.
    pub total: Option<usize>,
}

impl Progress {
    /// Creates progress for a new operation stage.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the latest progress values without blocking the worker.
    #[must_use]
    pub fn snapshot(&self) -> ProgressSnapshot {
        let total = self
            .inner
            .total_known
            .load(Ordering::Acquire)
            .then(|| self.inner.total.load(Ordering::Relaxed));
        ProgressSnapshot {
            done: self.inner.done.load(Ordering::Relaxed),
            total,
        }
    }

    pub(crate) fn publish_total(&self, total: usize) {
        if self.inner.total_known.load(Ordering::Acquire) {
            return;
        }
        self.inner.total.store(total, Ordering::Relaxed);
        self.inner.total_known.store(true, Ordering::Release);
    }

    pub(crate) fn advance(&self) {
        self.inner.done.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::{Progress, ProgressSnapshot};

    #[test]
    fn new_progress_is_empty() {
        let progress = Progress::new();

        assert_eq!(
            progress.snapshot(),
            ProgressSnapshot {
                done: 0,
                total: None,
            }
        );
    }

    #[test]
    fn total_is_published_once() {
        let progress = Progress::new();

        progress.publish_total(17);
        progress.publish_total(99);

        assert_eq!(progress.snapshot().total, Some(17));
    }

    #[test]
    fn concurrent_advances_are_counted() {
        let progress = Progress::new();

        std::thread::scope(|scope| {
            for _ in 0..4 {
                let progress = progress.clone();
                scope.spawn(move || {
                    for _ in 0..1_000 {
                        progress.advance();
                    }
                });
            }
        });

        assert_eq!(progress.snapshot().done, 4_000);
    }
}
