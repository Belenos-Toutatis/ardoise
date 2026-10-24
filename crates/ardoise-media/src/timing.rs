//! Media Foundation time arithmetic: every timestamp and duration is an `i64` count of
//! 100-nanosecond units ("hns").

use std::time::Duration;

/// 100-ns units per second.
pub(crate) const HNS_PER_SECOND: u64 = 10_000_000;

/// `Duration` to 100-ns units (truncated; saturates far beyond any real video length).
pub(crate) fn duration_to_hns(duration: Duration) -> i64 {
    i64::try_from(duration.as_nanos() / 100).unwrap_or(i64::MAX)
}

/// 100-ns units to `Duration` (negative values count as zero).
pub(crate) fn hns_to_duration(hns: i64) -> Duration {
    let hns = u64::try_from(hns).unwrap_or(0);
    Duration::from_nanos(hns.saturating_mul(100))
}

/// Start time of audio frame number `frames` at `rate` frames per second.
///
/// Computed from the total frame count rather than by adding chunk durations, so timestamps
/// never drift whatever the chunk sizes are.
pub(crate) fn frames_to_hns(frames: u64, rate: u32) -> i64 {
    if rate == 0 {
        return 0;
    }
    let hns = u128::from(frames) * u128::from(HNS_PER_SECOND) / u128::from(rate);
    i64::try_from(hns).unwrap_or(i64::MAX)
}

/// Nominal duration of one video frame at `frame_rate` frames per second.
pub(crate) fn frame_period_hns(frame_rate: u32) -> i64 {
    frames_to_hns(1, frame_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_durations() {
        assert_eq!(duration_to_hns(Duration::from_secs(3)), 30_000_000);
        assert_eq!(duration_to_hns(Duration::from_nanos(150)), 1);
        assert_eq!(hns_to_duration(30_000_000), Duration::from_secs(3));
        assert_eq!(hns_to_duration(-5), Duration::ZERO);
    }

    #[test]
    fn audio_timestamps_do_not_drift() {
        // 1 s of 44.1 kHz audio in odd chunks ends exactly at 1 s.
        let mut written = 0u64;
        let mut previous_end = 0i64;
        for chunk in [1000u64, 1023, 4410, 7, 37_660] {
            let start = frames_to_hns(written, 44_100);
            assert_eq!(start, previous_end, "chunks must be contiguous");
            written += chunk;
            previous_end = frames_to_hns(written, 44_100);
        }
        assert_eq!(written, 44_100);
        assert_eq!(previous_end, 10_000_000);
    }

    #[test]
    fn frame_period() {
        assert_eq!(frame_period_hns(30), 333_333);
        assert_eq!(frame_period_hns(25), 400_000);
        assert_eq!(frames_to_hns(90, 30), 30_000_000);
        assert_eq!(frames_to_hns(5, 0), 0);
    }
}
