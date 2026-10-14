use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Clone)]
pub(super) struct Capacity {
    state: Arc<Mutex<CapacityState>>,
    revision: watch::Sender<u64>,
}

struct CapacityState {
    active: usize,
    limit: usize,
}

pub(super) struct TurnCapacity {
    state: Arc<Mutex<CapacityState>>,
    revision: watch::Sender<u64>,
}

impl Capacity {
    pub(super) fn new(limit: usize) -> Self {
        let (revision, _) = watch::channel(0);
        Self {
            state: Arc::new(Mutex::new(CapacityState { active: 0, limit })),
            revision,
        }
    }

    pub(super) fn reserve(&self) -> std::io::Result<TurnCapacity> {
        let mut state = self
            .state
            .lock()
            .expect("subagent capacity lock should not be poisoned");
        if state.limit != usize::MAX && state.active >= state.limit {
            return Err(std::io::Error::other(format!(
                "sub-agent concurrency limit of {} has been reached; try delegation again later",
                state.limit
            )));
        }
        state.active += 1;
        drop(state);

        Ok(TurnCapacity {
            state: Arc::clone(&self.state),
            revision: self.revision.clone(),
        })
    }

    pub(super) fn reserve_many(&self, count: usize) -> std::io::Result<Vec<TurnCapacity>> {
        let mut state = self
            .state
            .lock()
            .expect("subagent capacity lock should not be poisoned");
        if state.limit != usize::MAX && count > state.limit.saturating_sub(state.active) {
            return Err(std::io::Error::other(format!(
                "sub-agent concurrency limit of {} has been reached; try delegation again later",
                state.limit
            )));
        }
        state.active += count;
        drop(state);

        Ok((0..count)
            .map(|_| TurnCapacity {
                state: Arc::clone(&self.state),
                revision: self.revision.clone(),
            })
            .collect())
    }

    pub(super) fn set_limit(&self, limit: usize) {
        self.state
            .lock()
            .expect("subagent capacity lock should not be poisoned")
            .limit = limit;
        self.changed();
    }

    pub(super) fn subscribe(&self) -> watch::Receiver<u64> {
        self.revision.subscribe()
    }

    fn changed(&self) {
        self.revision.send_modify(|revision| {
            *revision = revision.wrapping_add(1);
        });
    }
}

impl Drop for TurnCapacity {
    fn drop(&mut self) {
        self.state
            .lock()
            .expect("subagent capacity lock should not be poisoned")
            .active -= 1;
        self.revision.send_modify(|revision| {
            *revision = revision.wrapping_add(1);
        });
    }
}
