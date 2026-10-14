use nanocode_durability::{DurableSession, Result};

use crate::{durability::context, execution::ExecutionContinuation, session::SessionSnapshot};

pub trait DurableSessionExt {
    fn agent_snapshot(&self) -> impl Future<Output = Result<Option<SessionSnapshot>>>;

    fn agent_continuation(
        &self,
        operation_id: &str,
    ) -> impl Future<Output = Result<Option<ExecutionContinuation>>>;
}

impl DurableSessionExt for DurableSession {
    async fn agent_snapshot(&self) -> Result<Option<SessionSnapshot>> {
        match self.latest_checkpoint().await? {
            Some(payload) => context::load_snapshot(self.into(), payload).await.map(Some),
            None => Ok(None),
        }
    }

    async fn agent_continuation(
        &self,
        operation_id: &str,
    ) -> Result<Option<ExecutionContinuation>> {
        let state = self.state().await?;
        match state
            .operation(operation_id)
            .and_then(|operation| operation.continuation.as_ref())
        {
            Some(reference) => {
                let payload = self.resolve(reference).await?;
                context::load_continuation(self.into(), payload)
                    .await
                    .map(|(value, _)| Some(value))
            }
            None => Ok(None),
        }
    }
}
