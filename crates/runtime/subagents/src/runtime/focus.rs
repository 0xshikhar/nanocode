use super::{AgentDirectoryEntry, AgentSummary, MessageReceipt, SubagentControl};
use crate::model::{AgentId, MessagePriority, MessagePurpose};

impl SubagentControl {
    pub async fn send_user_message(
        &self,
        root_session_id: &str,
        id: AgentId,
        priority: MessagePriority,
        body: impl Into<String>,
    ) -> std::io::Result<MessageReceipt> {
        self.require_root(root_session_id).await?;
        self.registry.await_restored(root_session_id).await?;
        self.registry
            .deliver_message(
                root_session_id,
                id,
                priority,
                MessagePurpose::Coordinate,
                None,
                body.into(),
                true,
            )
            .await
    }

    pub async fn interrupt(
        &self,
        root_session_id: &str,
        id: AgentId,
    ) -> std::io::Result<Vec<AgentSummary>> {
        self.require_root(root_session_id).await?;
        self.registry.interrupt(root_session_id, id).await
    }

    pub async fn agents(&self, root_session_id: &str) -> std::io::Result<Vec<AgentDirectoryEntry>> {
        self.require_root(root_session_id).await?;
        self.registry.directory(root_session_id, true, false).await
    }

    pub async fn session_id(&self, root_session_id: &str, id: AgentId) -> std::io::Result<String> {
        self.require_root(root_session_id).await?;
        self.registry.child_session_id(root_session_id, id).await
    }

    async fn require_root(&self, root_session_id: &str) -> std::io::Result<()> {
        if self.registry.is_root_session(root_session_id).await {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "{root_session_id} is not a root session with subagents"
            )))
        }
    }
}
