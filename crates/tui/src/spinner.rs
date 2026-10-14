use std::time::{Duration, Instant};

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Clone, Copy, Debug)]
pub struct Spinner {
    started_at: Instant,
    interval: Duration,
}

impl Spinner {
    #[must_use]
    pub const fn new(started_at: Instant, interval: Duration) -> Self {
        Self {
            started_at,
            interval,
        }
    }

    #[must_use]
    pub const fn started_at(&self) -> Instant {
        self.started_at
    }

    #[must_use]
    pub fn frame(&self, now: Instant) -> &'static str {
        FRAMES[self.tick(now) % FRAMES.len()]
    }

    #[must_use]
    pub fn next_deadline(&self, now: Instant) -> Instant {
        let next = u32::try_from(self.tick(now) + 1).unwrap_or(u32::MAX);
        self.started_at + self.interval * next
    }

    fn tick(&self, now: Instant) -> usize {
        let elapsed = now.saturating_duration_since(self.started_at);
        usize::try_from(elapsed.as_nanos() / self.interval.as_nanos().max(1)).unwrap_or(usize::MAX)
    }
}

#[must_use]
pub fn elapsed_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}
