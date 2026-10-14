use std::{fmt, future::Future, pin::Pin, sync::Arc};

use nanocode_agent::{HarnessFamily, SessionCheckpoint, session::TranscriptItem};
use serde_json::Value;

use crate::{EncodedPayload, Error, PayloadReader, Result, SessionRecord};

#[cfg(not(target_family = "wasm"))]
pub type FormatFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
#[cfg(target_family = "wasm")]
pub type FormatFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub struct AdmittedTurn<'a> {
    pub input: &'a Value,
    pub steers: usize,
}

pub trait CheckpointFormat: Send + Sync {
    fn family(&self) -> HarnessFamily;

    fn owns(&self, checkpoint: &Value) -> bool;

    fn hydrate<'a>(
        &'a self,
        _reader: PayloadReader<'a>,
        stored: Value,
    ) -> FormatFuture<'a, Result<Value>> {
        Box::pin(async move { Ok(stored) })
    }

    fn encode(&self, checkpoint: Value) -> Result<EncodedPayload> {
        EncodedPayload::encode(&checkpoint)
    }

    fn session_checkpoint(
        &self,
        record: &SessionRecord,
        checkpoint: Value,
    ) -> Result<SessionCheckpoint>;

    fn transcript(&self, checkpoint: &Value, turns: &[AdmittedTurn<'_>]) -> Vec<TranscriptItem>;
}

#[derive(Clone, Default)]
pub(crate) struct Formats(Vec<Arc<dyn CheckpointFormat>>);

impl Formats {
    pub(crate) fn with(&self, format: Arc<dyn CheckpointFormat>) -> Self {
        let mut formats: Vec<_> = self
            .0
            .iter()
            .filter(|known| known.family() != format.family())
            .cloned()
            .collect();
        formats.push(format);
        Self(formats)
    }

    pub(crate) fn family(&self, family: HarnessFamily) -> Result<&dyn CheckpointFormat> {
        self.0
            .iter()
            .find(|format| format.family() == family)
            .map(AsRef::as_ref)
            .ok_or_else(|| Error::UnsupportedFamily(family.to_string()))
    }

    pub(crate) fn owner(&self, checkpoint: &Value) -> Result<&dyn CheckpointFormat> {
        self.0
            .iter()
            .find(|format| format.owns(checkpoint))
            .map(AsRef::as_ref)
            .ok_or_else(|| {
                Error::InvalidState("no registered checkpoint format owns this checkpoint".into())
            })
    }
}

impl fmt::Debug for Formats {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.0.iter().map(|format| format.family()))
            .finish()
    }
}
