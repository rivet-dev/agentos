//! Per-VM coalescer for guest filesystem mutations, feeding the
//! `filesystem.changed` structured wire event.
//!
//! Mutation handlers mark the parent directory of every successful write-side
//! operation; the stdio loop drains due trackers on its existing tick and emits
//! one VM-scoped event per flush window. Coalescing is lossy by design: past
//! `MAX_FS_CHANGED_DIRS` distinct directories in one window the tracker latches
//! `overflow` and consumers treat the whole tree as changed, so the event stays
//! truthful without an unbounded set. That degraded-but-honest representation is
//! why overflow is not a hard error.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use agentos_bridge::queue_tracker::{register_queue, QueueGauge, TrackedLimit};

/// Distinct dirty directories buffered per VM per flush window. Raise by
/// recompiling; overflow collapses to a whole-tree invalidation rather than
/// dropping changes.
pub(crate) const MAX_FS_CHANGED_DIRS: usize = 64;

/// Trailing-edge flush delay from the first mark in a window. Bounds the event
/// rate to ~3/s per VM under sustained guest churn.
pub(crate) const FS_CHANGED_FLUSH_INTERVAL: Duration = Duration::from_millis(300);

/// One coalesced flush: the directories whose direct entries changed, and
/// whether the window overflowed (consumers must treat everything as changed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FsChangeFlush {
    pub(crate) dirs: Vec<String>,
    pub(crate) overflow: bool,
}

struct FsChangeState {
    dirs: BTreeSet<String>,
    overflow: bool,
    first_marked_at: Option<Instant>,
}

pub(crate) struct FsChangeTracker {
    inner: Mutex<FsChangeState>,
    gauge: Arc<QueueGauge>,
}

impl FsChangeTracker {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(FsChangeState {
                dirs: BTreeSet::new(),
                overflow: false,
                first_marked_at: None,
            }),
            gauge: register_queue(TrackedLimit::FsChangedDirtyDirs, MAX_FS_CHANGED_DIRS),
        }
    }

    /// Record a mutation of the entry at `path`: its parent directory's listing
    /// is now stale.
    pub(crate) fn mark(&self, path: &str, now: Instant) {
        self.insert(parent_dir(path), now);
    }

    /// Record removal or rename of the entry at `path`. Beyond the parent, the
    /// path itself is marked: if it was a directory, queries against its own
    /// listing must also go stale.
    pub(crate) fn mark_removed(&self, path: &str, now: Instant) {
        self.insert(parent_dir(path), now);
        self.insert(normalize_dir(path), now);
    }

    fn insert(&self, dir: String, now: Instant) {
        let mut state = self.inner.lock().expect("fs change tracker poisoned");
        if state.first_marked_at.is_none() {
            state.first_marked_at = Some(now);
        }
        if state.overflow {
            return;
        }
        state.dirs.insert(dir);
        self.gauge.observe_depth(state.dirs.len());
        if state.dirs.len() >= MAX_FS_CHANGED_DIRS {
            tracing::warn!(
                limit = "fs_changed_dirty_dirs",
                capacity = MAX_FS_CHANGED_DIRS,
                "fs change window overflowed; collapsing to whole-tree invalidation"
            );
            state.dirs.clear();
            state.overflow = true;
        }
    }

    /// Drain and reset the tracker if a flush window elapsed. Returns `None`
    /// while empty or before the window closes; `now` is a parameter so tests
    /// stay sleep-free.
    pub(crate) fn take_due(&self, now: Instant) -> Option<FsChangeFlush> {
        let mut state = self.inner.lock().expect("fs change tracker poisoned");
        let first = state.first_marked_at?;
        if now.duration_since(first) < FS_CHANGED_FLUSH_INTERVAL {
            return None;
        }
        let dirs: Vec<String> = std::mem::take(&mut state.dirs).into_iter().collect();
        let overflow = std::mem::take(&mut state.overflow);
        state.first_marked_at = None;
        self.gauge.observe_depth(0);
        if dirs.is_empty() && !overflow {
            return None;
        }
        Some(FsChangeFlush { dirs, overflow })
    }
}

/// Parent directory of a normalized absolute guest path (`/a/b.txt` → `/a`,
/// `/a` → `/`, `/` → `/`). Trailing slashes are stripped first so `/a/b/`
/// resolves like `/a/b`.
pub(crate) fn parent_dir(path: &str) -> String {
    let trimmed = normalize_dir(path);
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(idx) => trimmed[..idx].to_owned(),
    }
}

fn normalize_dir(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flush_deadline(start: Instant) -> Instant {
        start + FS_CHANGED_FLUSH_INTERVAL
    }

    #[test]
    fn parent_dir_handles_root_and_trailing_slashes() {
        assert_eq!(parent_dir("/a/b.txt"), "/a");
        assert_eq!(parent_dir("/a"), "/");
        assert_eq!(parent_dir("/a/b/"), "/a");
        assert_eq!(parent_dir("/"), "/");
    }

    #[test]
    fn marks_dedupe_and_flush_resets() {
        let tracker = FsChangeTracker::new();
        let start = Instant::now();
        tracker.mark("/tmp/a.txt", start);
        tracker.mark("/tmp/b.txt", start);
        assert!(tracker.take_due(start).is_none(), "window still open");
        let flush = tracker
            .take_due(flush_deadline(start))
            .expect("due after the window");
        assert_eq!(flush.dirs, vec!["/tmp".to_owned()]);
        assert!(!flush.overflow);
        assert!(tracker.take_due(flush_deadline(start)).is_none(), "reset");
    }

    #[test]
    fn removed_dir_marks_itself_and_parent() {
        let tracker = FsChangeTracker::new();
        let start = Instant::now();
        tracker.mark_removed("/tmp/dir", start);
        let flush = tracker.take_due(flush_deadline(start)).expect("due");
        assert_eq!(flush.dirs, vec!["/tmp".to_owned(), "/tmp/dir".to_owned()]);
    }

    #[test]
    fn cap_latches_overflow_and_collapses_dirs() {
        let tracker = FsChangeTracker::new();
        let start = Instant::now();
        for i in 0..(MAX_FS_CHANGED_DIRS + 5) {
            tracker.mark(&format!("/d{i}/f"), start);
        }
        let flush = tracker.take_due(flush_deadline(start)).expect("due");
        assert!(flush.overflow);
        assert!(flush.dirs.is_empty());
    }
}
