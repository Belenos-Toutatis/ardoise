//! The virtual clock of the simulator and of the replay (docs/design/device-stack.md,
//! "Virtual time").
//!
//! Time only moves when a simulated transport says so: a `recv` that finds nothing due within
//! its timeout jumps the clock by that timeout, and an event due later than now moves the
//! clock to its due time. A 30 s live session therefore runs in milliseconds of real time and
//! a timeout elapses instantly, while every timestamp stays exactly reproducible.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ardoise_device::transport::Clock;

/// Unix time of the simulator's wall-clock origin: 2026-09-01T08:00:00Z. It is a constant so
/// that simulated sessions, the names of their logs and the times they send to the tablet are
/// the same at every run.
pub const SIM_EPOCH_UNIX: u64 = 1_788_249_600;

/// The simulator's wall-clock origin as a [`SystemTime`].
pub fn sim_epoch() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(SIM_EPOCH_UNIX)
}

/// A clock that only moves when told to. Shared (behind an `Arc`) by a connector, its
/// transports and the code that drives them.
#[derive(Debug)]
pub struct VirtualClock {
    origin: SystemTime,
    /// Monotonic time since the origin, in nanoseconds.
    nanos: AtomicU64,
}

impl VirtualClock {
    /// A clock at monotonic 0, whose wall clock starts at [`SIM_EPOCH_UNIX`].
    pub fn new() -> Self {
        Self::with_origin(sim_epoch())
    }

    /// A clock at monotonic 0 whose wall clock starts at `origin` (the replay uses the creation
    /// time of the log it replays).
    pub fn with_origin(origin: SystemTime) -> Self {
        Self {
            origin,
            nanos: AtomicU64::new(0),
        }
    }

    /// Wall-clock time at monotonic 0.
    pub fn origin(&self) -> SystemTime {
        self.origin
    }

    /// Time elapsed since the origin.
    pub fn monotonic(&self) -> Duration {
        Duration::from_nanos(self.nanos.load(Ordering::SeqCst))
    }

    /// Current wall-clock time: the origin plus [`VirtualClock::monotonic`].
    pub fn now(&self) -> SystemTime {
        self.origin + self.monotonic()
    }

    /// Moves the clock forward by `by`.
    pub fn advance(&self, by: Duration) {
        let by = nanos(by);
        // The closure always returns `Some`, so the update cannot fail.
        let _ = self
            .nanos
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |now| {
                Some(now.saturating_add(by))
            });
    }

    /// Moves the clock to `time` if it is later than now; the clock never goes back.
    pub fn advance_to(&self, time: Duration) {
        self.nanos.fetch_max(nanos(time), Ordering::SeqCst);
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for VirtualClock {
    fn monotonic(&self) -> Duration {
        VirtualClock::monotonic(self)
    }

    fn now(&self) -> SystemTime {
        VirtualClock::now(self)
    }
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// Whole unix seconds of `time` (negative before 1970).
pub(crate) fn unix_seconds(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(elapsed) => i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// Milliseconds of `duration`, saturating.
pub(crate) fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_the_fixed_origin_and_only_moves_forward() {
        let clock = VirtualClock::new();
        assert_eq!(clock.monotonic(), Duration::ZERO);
        assert_eq!(unix_seconds(clock.now()), 1_788_249_600);
        clock.advance(Duration::from_millis(1500));
        assert_eq!(clock.monotonic(), Duration::from_millis(1500));
        clock.advance_to(Duration::from_millis(1000));
        assert_eq!(clock.monotonic(), Duration::from_millis(1500), "never back");
        clock.advance_to(Duration::from_secs(2));
        assert_eq!(clock.monotonic(), Duration::from_secs(2));
        assert_eq!(unix_seconds(clock.now()), 1_788_249_602);
    }

    #[test]
    fn epoch_is_the_first_of_september_2026() {
        let date = chrono::DateTime::<chrono::Utc>::from(sim_epoch());
        assert_eq!(date.to_rfc3339(), "2026-09-01T08:00:00+00:00");
    }
}
