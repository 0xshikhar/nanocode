use std::sync::Arc;

use nanocode_agent::{
    AgentEvents, BackendRegistration, BackendRequest, ExecutionPolicyDisposition, Nanocodex,
    NanocodexError, Result as AgentResult, SessionInfo, backend::BackendFuture,
};

use crate::{CheckpointFormat, DurableSession, Error, SessionRecord, SharedStore, StoreError};

pub trait DurableAgentExt: Sized {
    fn durability(self, state: DurableSession) -> impl Future<Output = AgentResult<Self>>;
}

pub trait DurableBackendRegistration: BackendRegistration {
    fn checkpoint_format(&self) -> Option<Arc<dyn CheckpointFormat>> {
        None
    }

    fn build_durable(
        &self,
        _request: BackendRequest,
        _state: DurableSession,
    ) -> BackendFuture<AgentResult<(Nanocodex, AgentEvents)>> {
        Box::pin(async {
            Err(NanocodexError::UnsupportedCapability {
                capability: "durability",
            })
        })
    }
}

#[derive(Clone)]
pub struct Branches {
    pub store: SharedStore,
    pub record: Option<SessionRecord>,
}

impl Branches {
    pub fn child_record(&self, child: &SessionInfo) -> AgentResult<SessionRecord> {
        let parent = self.record.as_ref().ok_or_else(|| {
            NanocodexError::InvalidExecutionPolicy(
                "a durable session without a recorded model cannot persist forks; open it through SessionStore"
                    .to_owned(),
            )
        })?;
        Ok(parent
            .derive(child.session_id.clone(), child.lineage.origin)
            .with_lineage(child.lineage.clone()))
    }
}

#[must_use]
pub fn agent_error(error: Error) -> NanocodexError {
    if matches!(error, Error::SteerQueueFull) {
        return NanocodexError::SteerQueueFull;
    }
    if matches!(
        error,
        Error::SteerConflict { .. } | Error::SteerWithdrawn { .. }
    ) {
        return NanocodexError::InvalidRequest(error.to_string());
    }
    let disposition = match &error {
        Error::Store(StoreError::NotCommitted(_))
        | Error::OperationBlocked { .. }
        | Error::OperationActive { .. } => ExecutionPolicyDisposition::Retry,
        Error::Store(StoreError::Fenced | StoreError::Conflict { .. } | StoreError::Backend(_))
        | Error::ModelOwnerFenced
        | Error::DriverStopped => ExecutionPolicyDisposition::Reopen,
        _ => ExecutionPolicyDisposition::Fatal,
    };
    NanocodexError::execution_policy_with_disposition("durability", disposition, error)
}
