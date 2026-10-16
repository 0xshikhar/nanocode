use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum EventError {
    #[error("failed to encode agent event")]
    Encode(#[source] serde_json::Error),

    #[error("failed to write agent event")]
    Write(#[source] std::io::Error),

    #[error("agent event stream closed before the turn emitted a terminal event")]
    ClosedBeforeTerminal,

    #[error("agent event protocol version mismatch: expected {expected}, received {actual}")]
    ProtocolVersionMismatch { expected: u32, actual: u32 },

    #[error("agent event request identity mismatch: expected {expected}, received {actual}")]
    RequestIdMismatch {
        expected: Arc<str>,

        actual: Arc<str>,
    },

    #[error("agent event sequence did not advance: minimum {expected}, received {actual}")]
    SequenceMismatch { expected: u64, actual: u64 },

    #[error("agent event sequence is exhausted")]
    SequenceExhausted,

    #[error("agent turn event was published after its terminal event")]
    TurnAlreadyTerminal,
}
