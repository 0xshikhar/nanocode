use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_util::future::{AbortHandle, Abortable};
use nanocode_platform::Instant;
use tokio::sync::oneshot;

#[cfg(not(target_family = "wasm"))]
pub(crate) trait TaskSend: Send {}
#[cfg(not(target_family = "wasm"))]
impl<T: Send> TaskSend for T {}

#[cfg(target_family = "wasm")]
pub(crate) trait TaskSend {}
#[cfg(target_family = "wasm")]
impl<T> TaskSend for T {}

#[derive(Debug)]
pub(crate) struct TaskError;

impl std::fmt::Display for TaskError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("task was aborted")
    }
}

pub(crate) struct Task<T> {
    result: oneshot::Receiver<T>,
    abort: AbortHandle,
}

#[cfg(target_family = "wasm")]
pub(crate) struct TaskAbortGuard(AbortHandle);

#[cfg(target_family = "wasm")]
impl Drop for TaskAbortGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl<T> Task<T> {
    #[cfg(target_family = "wasm")]
    pub(crate) fn abort_on_drop(&self) -> TaskAbortGuard {
        TaskAbortGuard(self.abort.clone())
    }

    pub(crate) fn abort(&self) {
        self.abort.abort();
    }
}

impl<T> Future for Task<T> {
    type Output = Result<T, TaskError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.result)
            .poll(context)
            .map(|result| result.map_err(|_| TaskError))
    }
}

pub(crate) fn spawn<T, F>(future: F) -> Task<T>
where
    T: TaskSend + 'static,
    F: Future<Output = T> + TaskSend + 'static,
{
    let (send, result) = oneshot::channel();
    let (abort, registration) = AbortHandle::new_pair();
    spawn_detached(async move {
        let _ = Abortable::new(
            async move {
                let output = future.await;
                let _ = send.send(output);
            },
            registration,
        )
        .await;
    });
    Task { result, abort }
}

pub(crate) async fn timeout_at<T, F>(deadline: Instant, future: F) -> Result<T, ()>
where
    F: Future<Output = T>,
{
    let delay = deadline.saturating_duration_since(Instant::now());
    futures_util::pin_mut!(future);
    let timeout = sleep(delay);
    futures_util::pin_mut!(timeout);
    match futures_util::future::select(future, timeout).await {
        futures_util::future::Either::Left((output, _)) => Ok(output),
        futures_util::future::Either::Right(((), _)) => Err(()),
    }
}

#[cfg(not(target_family = "wasm"))]
fn spawn_detached<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    drop(tokio::spawn(future));
}

#[cfg(target_family = "wasm")]
fn spawn_detached<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    nanocode_platform::spawn_local(future);
}

#[cfg(not(target_family = "wasm"))]
pub(crate) async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(target_family = "wasm")]
pub(crate) async fn sleep(duration: Duration) {
    nanocode_platform::sleep(duration).await;
}
