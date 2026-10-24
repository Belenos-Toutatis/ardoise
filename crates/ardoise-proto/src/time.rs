//! The tablet clock format (protocol.md §4.4): six BCD bytes `yy mm dd hh mm ss`, UTC.
//!
//! `19 03 15 03 22 16` is 2019-03-15 03:22:16 UTC. The year has two digits, read as 2000 to
//! 2099. The same format is used by SET_TIME / GET_TIME and by the timestamp of a stored page
//! (`cf` file-info reply).

use std::fmt;

/// Unix time of 2000-01-01T00:00:00Z, the earliest [`BcdTime`].
const MIN_UNIX: i64 = 946_684_800;
/// Unix time of 2099-12-31T23:59:59Z, the latest [`BcdTime`].
const MAX_UNIX: i64 = 4_102_444_799;
const SECONDS_PER_DAY: i64 = 86_400;

/// A validated UTC date and time between 2000-01-01 00:00:00 and 2099-12-31 23:59:59, with
/// one-second resolution. Ordering is chronological.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BcdTime {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

/// Failure while building or decoding a [`BcdTime`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimeError {
    /// Not exactly six bytes.
    #[error("a BCD time has 6 bytes, got {0}")]
    Length(usize),
    /// A byte whose nibbles are not both decimal digits.
    #[error("byte {index} ({value:02x}) is not a BCD pair")]
    InvalidBcd {
        /// Position of the byte (0 = year).
        index: usize,
        /// Offending byte.
        value: u8,
    },
    /// Digits that do not form a real date and time between 2000 and 2099.
    #[error(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} is not a valid date and time"
    )]
    InvalidDate {
        /// Year.
        year: u16,
        /// Month.
        month: u8,
        /// Day of the month.
        day: u8,
        /// Hour.
        hour: u8,
        /// Minute.
        minute: u8,
        /// Second.
        second: u8,
    },
    /// A unix time before 2000 or after 2099.
    #[error("unix time {0} is outside 2000-01-01 to 2099-12-31")]
    OutOfRange(i64),
}

impl BcdTime {
    /// Earliest representable time, 2000-01-01T00:00:00Z.
    pub const MIN: Self = Self {
        year: 2000,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
    };
    /// Latest representable time, 2099-12-31T23:59:59Z.
    pub const MAX: Self = Self {
        year: 2099,
        month: 12,
        day: 31,
        hour: 23,
        minute: 59,
        second: 59,
    };

    /// Validates a calendar date and time (UTC). Leap seconds are not accepted.
    pub fn new(
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> Result<Self, TimeError> {
        let valid = (2000..=2099).contains(&year)
            && (1..=12).contains(&month)
            && day >= 1
            && day <= days_in_month(year, month)
            && hour < 24
            && minute < 60
            && second < 60;
        if valid {
            Ok(Self {
                year,
                month,
                day,
                hour,
                minute,
                second,
            })
        } else {
            Err(TimeError::InvalidDate {
                year,
                month,
                day,
                hour,
                minute,
                second,
            })
        }
    }

    /// Decodes the six wire bytes `yy mm dd hh mm ss`.
    pub fn from_bcd(bytes: [u8; 6]) -> Result<Self, TimeError> {
        let mut digits = [0u8; 6];
        for (index, (digit, value)) in digits.iter_mut().zip(bytes).enumerate() {
            *digit = from_bcd_pair(value).ok_or(TimeError::InvalidBcd { index, value })?;
        }
        let [yy, month, day, hour, minute, second] = digits;
        Self::new(2000 + u16::from(yy), month, day, hour, minute, second)
    }

    /// Like [`BcdTime::from_bcd`] for a slice, which must hold exactly six bytes.
    pub fn from_bcd_slice(bytes: &[u8]) -> Result<Self, TimeError> {
        let array = <[u8; 6]>::try_from(bytes).map_err(|_| TimeError::Length(bytes.len()))?;
        Self::from_bcd(array)
    }

    /// The six wire bytes `yy mm dd hh mm ss`.
    pub fn to_bcd(self) -> [u8; 6] {
        // The year is 2000..=2099 by construction, so `yy` is 0..=99.
        let yy = u8::try_from(self.year - 2000).unwrap_or(0);
        [
            yy,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
        ]
        .map(to_bcd_pair)
    }

    /// Converts seconds since 1970-01-01T00:00:00Z.
    pub fn from_unix(seconds: i64) -> Result<Self, TimeError> {
        if !(MIN_UNIX..=MAX_UNIX).contains(&seconds) {
            return Err(TimeError::OutOfRange(seconds));
        }
        let days = seconds.div_euclid(SECONDS_PER_DAY);
        let secs = seconds.rem_euclid(SECONDS_PER_DAY);
        let (year, month, day) = civil_from_days(days);
        let out_of_range = || TimeError::OutOfRange(seconds);
        Self::new(
            u16::try_from(year).map_err(|_| out_of_range())?,
            u8::try_from(month).map_err(|_| out_of_range())?,
            u8::try_from(day).map_err(|_| out_of_range())?,
            u8::try_from(secs / 3600).map_err(|_| out_of_range())?,
            u8::try_from(secs / 60 % 60).map_err(|_| out_of_range())?,
            u8::try_from(secs % 60).map_err(|_| out_of_range())?,
        )
    }

    /// Seconds since 1970-01-01T00:00:00Z.
    pub fn to_unix(self) -> i64 {
        let days = days_from_civil(
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        );
        days * SECONDS_PER_DAY
            + i64::from(self.hour) * 3600
            + i64::from(self.minute) * 60
            + i64::from(self.second)
    }

    /// Year, 2000 to 2099.
    pub fn year(self) -> u16 {
        self.year
    }

    /// Month, 1 to 12.
    pub fn month(self) -> u8 {
        self.month
    }

    /// Day of the month, 1 to 31.
    pub fn day(self) -> u8 {
        self.day
    }

    /// Hour, 0 to 23.
    pub fn hour(self) -> u8 {
        self.hour
    }

    /// Minute, 0 to 59.
    pub fn minute(self) -> u8 {
        self.minute
    }

    /// Second, 0 to 59.
    pub fn second(self) -> u8 {
        self.second
    }
}

/// RFC 3339, e.g. `2019-03-15T03:22:16Z`.
impl fmt::Display for BcdTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

fn from_bcd_pair(byte: u8) -> Option<u8> {
    let (high, low) = (byte >> 4, byte & 0x0f);
    (high <= 9 && low <= 9).then_some(high * 10 + low)
}

fn to_bcd_pair(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

fn is_leap_year(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
///
/// The year is shifted to start in March, so the leap day is the last day of the shifted
/// year; a 400-year era has exactly 146 097 days.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    // 719 468 days separate 0000-03-01 from 1970-01-01.
    era * 146_097 + day_of_era - 719_468
}

/// Inverse of [`days_from_civil`]: `(year, month, day)` of a day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_issue_117_example() {
        let time = BcdTime::from_bcd([0x19, 0x03, 0x15, 0x03, 0x22, 0x16]).unwrap();
        assert_eq!(time, BcdTime::new(2019, 3, 15, 3, 22, 16).unwrap());
        assert_eq!(time.to_string(), "2019-03-15T03:22:16Z");
        assert_eq!(time.to_unix(), 1_552_620_136);
        assert_eq!(time.to_bcd(), [0x19, 0x03, 0x15, 0x03, 0x22, 0x16]);
        assert_eq!(BcdTime::from_unix(1_552_620_136).unwrap(), time);
    }

    #[test]
    fn range_limits() {
        assert_eq!(BcdTime::MIN.to_unix(), MIN_UNIX);
        assert_eq!(BcdTime::MAX.to_unix(), MAX_UNIX);
        assert_eq!(BcdTime::from_unix(MIN_UNIX).unwrap(), BcdTime::MIN);
        assert_eq!(
            BcdTime::from_unix(MAX_UNIX).unwrap(),
            BcdTime::new(2099, 12, 31, 23, 59, 59).unwrap()
        );
        assert_eq!(
            BcdTime::from_bcd([0x99, 0x12, 0x31, 0x23, 0x59, 0x59]).unwrap(),
            BcdTime::MAX
        );
        assert_eq!(BcdTime::MAX.to_bcd(), [0x99, 0x12, 0x31, 0x23, 0x59, 0x59]);
        assert_eq!(BcdTime::MIN.to_bcd(), [0x00, 0x01, 0x01, 0x00, 0x00, 0x00]);
        assert_eq!(
            BcdTime::from_unix(MIN_UNIX - 1),
            Err(TimeError::OutOfRange(MIN_UNIX - 1))
        );
        assert_eq!(
            BcdTime::from_unix(MAX_UNIX + 1),
            Err(TimeError::OutOfRange(MAX_UNIX + 1))
        );
        assert!(BcdTime::from_unix(i64::MIN).is_err());
        assert!(BcdTime::from_unix(i64::MAX).is_err());
    }

    #[test]
    fn leap_days() {
        let leap = BcdTime::from_bcd([0x24, 0x02, 0x29, 0x12, 0x00, 0x00]).unwrap();
        assert_eq!(leap.to_unix(), 1_709_208_000);
        assert_eq!(BcdTime::from_unix(1_709_208_000).unwrap(), leap);
        // 2000 is a leap year (divisible by 400), 2023 and 2100-like years are not.
        assert!(BcdTime::new(2000, 2, 29, 0, 0, 0).is_ok());
        assert!(BcdTime::new(2023, 2, 29, 0, 0, 0).is_err());
        assert!(BcdTime::from_bcd([0x23, 0x02, 0x29, 0x00, 0x00, 0x00]).is_err());
        // The day after 2024-02-29 is 2024-03-01.
        assert_eq!(
            BcdTime::from_unix(leap.to_unix() + 12 * 3600).unwrap(),
            BcdTime::new(2024, 3, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn every_day_round_trips_through_unix_time() {
        let mut seconds = MIN_UNIX;
        let mut previous: Option<BcdTime> = None;
        while seconds <= MAX_UNIX {
            let time = BcdTime::from_unix(seconds).unwrap();
            assert_eq!(time.to_unix(), seconds);
            assert_eq!(BcdTime::from_bcd(time.to_bcd()).unwrap(), time);
            if let Some(previous) = previous {
                assert!(previous < time);
            }
            previous = Some(time);
            seconds += SECONDS_PER_DAY + 3_661;
        }
    }

    #[test]
    fn rejects_invalid_bcd_nibbles() {
        assert_eq!(
            BcdTime::from_bcd([0x1a, 0x03, 0x15, 0x03, 0x22, 0x16]),
            Err(TimeError::InvalidBcd {
                index: 0,
                value: 0x1a
            })
        );
        assert_eq!(
            BcdTime::from_bcd([0x19, 0x03, 0x15, 0x03, 0xa2, 0x16]),
            Err(TimeError::InvalidBcd {
                index: 4,
                value: 0xa2
            })
        );
    }

    #[test]
    fn rejects_impossible_dates_and_times() {
        for bytes in [
            [0x19, 0x00, 0x15, 0x03, 0x22, 0x16], // month 0
            [0x19, 0x13, 0x15, 0x03, 0x22, 0x16], // month 13
            [0x19, 0x04, 0x31, 0x03, 0x22, 0x16], // 31 April
            [0x19, 0x03, 0x00, 0x03, 0x22, 0x16], // day 0
            [0x19, 0x03, 0x15, 0x24, 0x22, 0x16], // hour 24
            [0x19, 0x03, 0x15, 0x03, 0x60, 0x16], // minute 60
            [0x19, 0x03, 0x15, 0x03, 0x22, 0x60], // second 60
        ] {
            assert!(
                matches!(BcdTime::from_bcd(bytes), Err(TimeError::InvalidDate { .. })),
                "{bytes:02x?}"
            );
        }
        assert!(BcdTime::new(1999, 12, 31, 0, 0, 0).is_err());
        assert!(BcdTime::new(2100, 1, 1, 0, 0, 0).is_err());
    }

    #[test]
    fn slice_length_is_checked() {
        assert_eq!(
            BcdTime::from_bcd_slice(&[0x19, 0x03]),
            Err(TimeError::Length(2))
        );
        assert!(BcdTime::from_bcd_slice(&[0x19, 0x03, 0x15, 0x03, 0x22, 0x16]).is_ok());
    }
}
