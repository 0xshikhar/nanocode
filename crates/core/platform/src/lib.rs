#[cfg(target_family = "wasm")]
mod host;
#[cfg(target_family = "wasm")]
pub use host::{Host, Instant, LocalFuture, install, spawn_local};

#[cfg(not(target_family = "wasm"))]
pub use std::time::Instant;
pub use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(not(target_family = "wasm"))]
pub use tokio::time::sleep;

#[cfg(not(target_family = "wasm"))]
pub fn system_time() -> SystemTime {
    SystemTime::now()
}

#[cfg(target_family = "wasm")]
pub fn system_time() -> SystemTime {
    host::current().system_time()
}

#[cfg(target_family = "wasm")]
pub fn sleep(duration: Duration) -> LocalFuture<'static, ()> {
    host::current().sleep(duration)
}

#[cfg(not(target_family = "wasm"))]
pub fn fill_random(bytes: &mut [u8]) -> std::io::Result<()> {
    getrandom::fill(bytes).map_err(std::io::Error::other)
}

#[cfg(target_family = "wasm")]
pub fn fill_random(bytes: &mut [u8]) -> std::io::Result<()> {
    host::current().fill_random(bytes)
}

#[cfg(not(target_family = "wasm"))]
pub fn uuid_v4() -> uuid::Uuid {
    uuid::Uuid::new_v4()
}

#[cfg(target_family = "wasm")]
pub fn uuid_v4() -> uuid::Uuid {
    let mut bytes = [0; 16];
    fill_random(&mut bytes).expect("the host could not provide UUID randomness");
    uuid::Builder::from_random_bytes(bytes).into_uuid()
}

#[cfg(not(target_family = "wasm"))]
pub fn uuid_v7() -> uuid::Uuid {
    uuid::Uuid::now_v7()
}

#[cfg(target_family = "wasm")]
pub fn uuid_v7() -> uuid::Uuid {
    let millis = system_time()
        .duration_since(UNIX_EPOCH)
        .expect("the host clock predates the Unix epoch")
        .as_millis();
    assert!(
        millis < 1 << 48,
        "the host clock exceeds the UUIDv7 timestamp range"
    );
    let mut bytes = [0; 10];
    fill_random(&mut bytes).expect("the host could not provide UUID randomness");
    uuid::Builder::from_unix_timestamp_millis(millis as u64, &bytes).into_uuid()
}
