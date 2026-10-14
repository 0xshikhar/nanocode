//! Receipt-time server advice. Monotonic deadlines stay process-local; only
//! validated Unix milliseconds may cross a host or durable boundary.
use std::time::Duration;
#[cfg(not(target_family = "wasm"))]
use tokio::time::Instant;
#[cfg(target_family = "wasm")]
use web_time::Instant;
use web_time::{SystemTime, UNIX_EPOCH};

/// Process-local clock samples captured before queueing or observer work.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RetryReceipt {
    wall: SystemTime,
    monotonic: Instant,
}
impl RetryReceipt {
    pub(crate) fn now() -> Self {
        let wall = SystemTime::now();
        Self {
            wall,
            monotonic: Instant::now(),
        }
    }
}

/// The earliest time advised by a server for a follow-up request.
/// This is intentionally not serializable. Hosted adapters send an absolute
/// wall-clock deadline and reconstruct the remaining interval exactly once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryAfter {
    deadline: Instant,
    deadline_unix_ms: u64,
}
impl RetryAfter {
    /// Captures a validated interval at receipt time.
    #[must_use]
    pub fn from_delay(delay: Duration) -> Option<Self> {
        Self::from_delay_received(delay, RetryReceipt::now())
    }
    /// Parses nonnegative decimal seconds or an HTTP-date at receipt time.
    /// Decimal seconds retain compatibility with existing hosted/provider advice.
    #[must_use]
    pub fn from_header(value: &str) -> Option<Self> {
        Self::from_header_received(value, RetryReceipt::now())
    }
    /// Reconstructs advice crossing a host/durable boundary, without restarting
    /// its original interval. Values must fit JavaScript's exact integer range.
    #[must_use]
    pub fn from_unix_ms(deadline_unix_ms: u64) -> Option<Self> {
        Self::from_unix_ms_at(deadline_unix_ms, SystemTime::now(), Instant::now())
    }
    /// Returns a portable wall-clock representation, never a persisted Instant.
    #[must_use]
    pub const fn deadline_unix_ms(self) -> u64 {
        self.deadline_unix_ms
    }
    /// Returns the remaining server-requested delay, or zero when expired.
    #[must_use]
    pub fn remaining_delay(self) -> Duration {
        self.remaining_at(Instant::now())
    }
    pub(crate) fn from_delay_received(delay: Duration, received: RetryReceipt) -> Option<Self> {
        Self::capture(delay, received.wall, received.monotonic)
    }
    pub(crate) fn from_header_received(value: &str, received: RetryReceipt) -> Option<Self> {
        Self::from_header_at(value, received.wall, received.monotonic)
    }
    fn remaining_at(self, now: Instant) -> Duration {
        self.deadline.saturating_duration_since(now)
    }
    fn capture(delay: Duration, wall: SystemTime, received: Instant) -> Option<Self> {
        let wall_ns = wall.duration_since(UNIX_EPOCH).ok()?.as_nanos();
        // Round the complete deadline up, never shorten fractional advice at
        // the JS/WASM boundary by truncating receipt-time submilliseconds.
        let deadline_ms = wall_ns
            .checked_add(delay.as_nanos())?
            .checked_add(999_999)?
            / 1_000_000;
        let deadline_unix_ms = u64::try_from(deadline_ms).ok()?;
        if deadline_unix_ms > 9_007_199_254_740_991 {
            return None;
        }
        Some(Self {
            deadline: received.checked_add(delay)?,
            deadline_unix_ms,
        })
    }
    fn from_unix_ms_at(ms: u64, wall: SystemTime, received: Instant) -> Option<Self> {
        if ms > 9_007_199_254_740_991 {
            return None;
        }
        let target = UNIX_EPOCH.checked_add(Duration::from_millis(ms))?;
        Some(Self {
            deadline: received.checked_add(target.duration_since(wall).unwrap_or_default())?,
            deadline_unix_ms: ms,
        })
    }
    fn from_header_at(value: &str, wall: SystemTime, received: Instant) -> Option<Self> {
        let value = value.trim_matches([' ', '\t']);
        let digits =
            |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
        let numeric = value.split_once('.').map_or_else(
            || digits(value),
            |(whole, fraction)| (whole.is_empty() || digits(whole)) && digits(fraction),
        );
        let delay = if numeric {
            Duration::try_from_secs_f64(value.parse().ok()?).ok()?
        } else {
            let date = httpdate::parse_http_date(value).ok()?;
            let since_epoch = date.duration_since(std::time::UNIX_EPOCH).ok()?;
            UNIX_EPOCH
                .checked_add(since_epoch)?
                .duration_since(wall)
                .unwrap_or_default()
        };
        Self::capture(delay, wall, received)
    }
}
