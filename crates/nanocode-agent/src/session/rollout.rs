use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
/// Stable identity and file location of a recorded Nanocodex thread.
#[derive(Clone, Debug)]
pub struct RolloutInfo {
    thread_id: String,
    path: PathBuf,
    committed_bytes: Arc<AtomicU64>,
}

impl PartialEq for RolloutInfo {
    fn eq(&self, other: &Self) -> bool {
        self.thread_id == other.thread_id && self.path == other.path
    }
}
impl Eq for RolloutInfo {}

impl RolloutInfo {
    /// Creates shared rollout metadata with a writer-owned committed byte counter.
    #[doc(hidden)]
    pub fn new(thread_id: String, path: PathBuf, committed_bytes: Arc<AtomicU64>) -> Self {
        Self {
            thread_id,
            path,
            committed_bytes,
        }
    }

    /// Exclusive byte boundary of successfully flushed, complete rollout records.
    #[must_use]
    pub fn committed_bytes(&self) -> u64 {
        self.committed_bytes.load(Ordering::Acquire)
    }

    /// UUID accepted by `codex resume` and `codex exec resume`.
    #[must_use]
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    /// Codex-compatible JSONL rollout path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}
