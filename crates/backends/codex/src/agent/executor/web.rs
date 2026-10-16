use std::sync::Arc;

use crate::client::__private::ModelConfig;

use crate::Result;

pub(crate) type ServiceFactory = Arc<dyn Fn(Arc<ModelConfig>) -> super::ResponsesClientBox>;

pub(crate) trait AgentSend {}

impl<T> AgentSend for T {}

pub(crate) trait AgentFactory {}

impl<T> AgentFactory for T {}

pub(crate) fn spawn_driver(
    driver: crate::agent::execution::ExecutionFuture<'static, ()>,
) -> Result<()> {
    nanocode_platform::spawn_local(driver);
    Ok(())
}
