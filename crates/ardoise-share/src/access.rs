//! Who may read the event stream (`docs/design/lan-share.md`, "Access: code, token, attempts"):
//! the 6-character code, the session tokens it gives, and the limit of wrong codes per address.
//!
//! Time is passed in (`now`) so that the tests can move the clock.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// The 31 unambiguous symbols of a code (no `0 O 1 I L`).
pub(crate) const ALPHABET: &[u8; 31] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
/// Characters in a code.
pub(crate) const CODE_LEN: usize = 6;
/// Wrong codes allowed per address within [`WINDOW`].
pub(crate) const MAX_WRONG: usize = 5;
/// How long a wrong code counts.
pub(crate) const WINDOW: Duration = Duration::from_secs(5 * 60);
/// Session tokens kept (the oldest is forgotten first).
pub(crate) const MAX_TOKENS: usize = 256;
/// Addresses whose wrong codes are remembered (the oldest is forgotten first).
pub(crate) const MAX_ADDRESSES: usize = 4096;

/// Random bytes accepted by the rejection sampling: the largest multiple of 31 that fits in a
/// byte, so that every symbol is equally likely.
const ACCEPT_BELOW: u8 = (256 / ALPHABET.len() * ALPHABET.len()) as u8;

/// A new random code.
pub(crate) fn random_code() -> Result<String, getrandom::Error> {
    let mut code = String::with_capacity(CODE_LEN);
    let mut bytes = [0u8; 16];
    while code.len() < CODE_LEN {
        getrandom::fill(&mut bytes)?;
        code_symbols(&bytes, &mut code);
    }
    Ok(code)
}

/// Appends to `code` the symbols that `bytes` give, up to [`CODE_LEN`] (rejection sampling:
/// bytes from [`ACCEPT_BELOW`] up are skipped).
fn code_symbols(bytes: &[u8], code: &mut String) {
    for &byte in bytes {
        if code.len() >= CODE_LEN {
            return;
        }
        if byte < ACCEPT_BELOW {
            code.push(char::from(ALPHABET[usize::from(byte) % ALPHABET.len()]));
        }
    }
}

/// A code as typed, made comparable: upper case, without spaces nor dashes.
pub(crate) fn normalize_code(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_uppercase)
        .collect()
}

/// A new session token: 32 random bytes in hexadecimal.
pub(crate) fn random_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Equality that takes the same time whatever the position of the first difference.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Answer to a code typed from an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The right code: the address's record is cleared; give it a token.
    Right,
    /// A wrong code; `remaining` wrong codes left before the address must wait.
    Wrong { remaining: u32 },
    /// Too many wrong codes: wait `retry_after` seconds (the code was not even compared).
    TooMany { retry_after: u64 },
}

/// The code, the tokens given, and the recent wrong codes of each address.
#[derive(Debug)]
pub(crate) struct Access {
    code: String,
    tokens: VecDeque<String>,
    wrong: HashMap<IpAddr, VecDeque<Instant>>,
}

impl Access {
    pub fn new(code: String) -> Self {
        Self {
            code,
            tokens: VecDeque::new(),
            wrong: HashMap::new(),
        }
    }

    /// Checks a code typed from `address` at `now`.
    pub fn try_code(&mut self, address: IpAddr, typed: &str, now: Instant) -> Verdict {
        if let Some(times) = self.wrong.get_mut(&address) {
            forget_old(times, now);
            if times.len() >= MAX_WRONG {
                let oldest = times.front().copied().unwrap_or(now);
                let wait = WINDOW.saturating_sub(now.saturating_duration_since(oldest));
                let retry_after = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
                return Verdict::TooMany {
                    retry_after: retry_after.max(1),
                };
            }
        }
        if constant_time_eq(normalize_code(typed).as_bytes(), self.code.as_bytes()) {
            self.wrong.remove(&address);
            return Verdict::Right;
        }
        if !self.wrong.contains_key(&address) && self.wrong.len() >= MAX_ADDRESSES {
            self.make_room(now);
        }
        let times = self.wrong.entry(address).or_default();
        times.push_back(now);
        let remaining = MAX_WRONG.saturating_sub(times.len());
        Verdict::Wrong {
            remaining: u32::try_from(remaining).unwrap_or(0),
        }
    }

    /// Forgets the addresses whose wrong codes are all too old; if that is not enough, the one
    /// whose last wrong code is the oldest.
    fn make_room(&mut self, now: Instant) {
        self.wrong.retain(|_, times| {
            forget_old(times, now);
            !times.is_empty()
        });
        if self.wrong.len() >= MAX_ADDRESSES {
            let oldest = self
                .wrong
                .iter()
                .min_by_key(|(_, times)| times.back().copied())
                .map(|(address, _)| *address);
            if let Some(address) = oldest {
                self.wrong.remove(&address);
            }
        }
    }

    /// Keeps a token given to a viewer (at most [`MAX_TOKENS`]).
    pub fn add_token(&mut self, token: String) {
        while self.tokens.len() >= MAX_TOKENS {
            self.tokens.pop_front();
        }
        self.tokens.push_back(token);
    }

    /// True if `candidate` is one of the tokens given (compared in constant time).
    pub fn token_valid(&self, candidate: &str) -> bool {
        self.tokens.iter().fold(false, |found, token| {
            found | constant_time_eq(token.as_bytes(), candidate.as_bytes())
        })
    }

    /// Addresses remembered (tests).
    #[cfg(test)]
    fn addresses(&self) -> usize {
        self.wrong.len()
    }
}

/// Drops the wrong codes older than [`WINDOW`].
fn forget_old(times: &mut VecDeque<Instant>, now: Instant) {
    while times
        .front()
        .is_some_and(|t| now.saturating_duration_since(*t) >= WINDOW)
    {
        times.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, last))
    }

    #[test]
    fn codes_use_the_alphabet() {
        for _ in 0..200 {
            let code = random_code().unwrap();
            assert_eq!(code.len(), CODE_LEN);
            assert!(code.bytes().all(|b| ALPHABET.contains(&b)), "{code}");
        }
        for ambiguous in ['0', 'O', '1', 'I', 'L'] {
            assert!(!ALPHABET.contains(&(ambiguous as u8)));
        }
        let mut sorted = ALPHABET.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 31);
    }

    #[test]
    fn rejection_sampling_is_uniform() {
        // Every byte value once: each symbol comes out exactly 8 times (248 = 31 × 8 bytes
        // accepted), the 8 bytes from 248 up are skipped.
        assert_eq!(ACCEPT_BELOW, 248);
        let mut counts = [0usize; 31];
        for chunk in (0..=255u8).collect::<Vec<_>>().chunks(6) {
            let mut code = String::new();
            code_symbols(chunk, &mut code);
            for b in code.bytes() {
                let index = ALPHABET.iter().position(|&a| a == b).unwrap();
                counts[index] += 1;
            }
        }
        assert!(counts.iter().all(|&c| c == 8), "{counts:?}");
        let mut code = String::new();
        code_symbols(&[248, 255, 0, 30, 31, 247, 1, 2, 3], &mut code);
        assert_eq!(code, "A9A9BC");
    }

    #[test]
    fn random_codes_spread_over_the_alphabet() {
        let mut counts = [0usize; 31];
        for _ in 0..2000 {
            for b in random_code().unwrap().bytes() {
                counts[ALPHABET.iter().position(|&a| a == b).unwrap()] += 1;
            }
        }
        // 12 000 symbols: 387 expected each; a fair generator stays far inside these bounds.
        assert!(
            counts.iter().all(|&c| (250..550).contains(&c)),
            "{counts:?}"
        );
    }

    #[test]
    fn typed_codes_are_normalised() {
        assert_eq!(normalize_code("k7m q2x"), "K7MQ2X");
        assert_eq!(normalize_code(" K7M-Q2X "), "K7MQ2X");
        assert_eq!(normalize_code("k\t7m\u{a0}q2x"), "K7MQ2X");
    }

    #[test]
    fn tokens() {
        let token = random_token().unwrap();
        assert_eq!(token.len(), 64);
        assert!(
            token
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert_ne!(token, random_token().unwrap());

        let mut access = Access::new("ABCDEF".into());
        assert!(!access.token_valid(&token));
        access.add_token(token.clone());
        assert!(access.token_valid(&token));
        assert!(!access.token_valid(&token[..63]));
        assert!(!access.token_valid(""));
        for _ in 0..MAX_TOKENS {
            access.add_token(random_token().unwrap());
        }
        assert!(!access.token_valid(&token), "the oldest token is forgotten");
        assert_eq!(access.tokens.len(), MAX_TOKENS);
    }

    #[test]
    fn right_and_wrong_codes() {
        let mut access = Access::new("K7MQ2X".into());
        let now = Instant::now();
        assert_eq!(
            access.try_code(ip(1), "AAAAAA", now),
            Verdict::Wrong { remaining: 4 }
        );
        assert_eq!(access.try_code(ip(1), "k7m-q2x", now), Verdict::Right);
        // A right code clears the record.
        assert_eq!(access.addresses(), 0);
        assert_eq!(
            access.try_code(ip(1), "", now),
            Verdict::Wrong { remaining: 4 }
        );
    }

    #[test]
    fn attempts_limit_and_window() {
        let mut access = Access::new("K7MQ2X".into());
        let start = Instant::now();
        for i in 0..5u32 {
            let at = start + Duration::from_secs(u64::from(i) * 10);
            assert_eq!(
                access.try_code(ip(7), "WRONG1", at),
                Verdict::Wrong { remaining: 4 - i }
            );
        }
        // The sixth attempt within the window is refused, even with the right code.
        let at = start + Duration::from_secs(60);
        assert_eq!(
            access.try_code(ip(7), "K7MQ2X", at),
            Verdict::TooMany { retry_after: 240 }
        );
        // Another address is not affected.
        assert_eq!(access.try_code(ip(8), "K7MQ2X", at), Verdict::Right);
        // Half a second before the oldest wrong code is 5 minutes old: still refused (1 s).
        let at = start + WINDOW - Duration::from_millis(500);
        assert_eq!(
            access.try_code(ip(7), "K7MQ2X", at),
            Verdict::TooMany { retry_after: 1 }
        );
        // When it is, one more attempt is allowed.
        let at = start + WINDOW;
        assert_eq!(
            access.try_code(ip(7), "WRONG2", at),
            Verdict::Wrong { remaining: 0 }
        );
        assert_eq!(
            access.try_code(ip(7), "K7MQ2X", at),
            Verdict::TooMany { retry_after: 10 }
        );
        // Long after, every wrong code has expired.
        let at = start + 3 * WINDOW;
        assert_eq!(access.try_code(ip(7), "K7MQ2X", at), Verdict::Right);
    }

    #[test]
    fn addresses_remembered_are_bounded() {
        let mut access = Access::new("K7MQ2X".into());
        let start = Instant::now();
        for i in 0..MAX_ADDRESSES as u32 {
            let address = IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + i));
            let at = start + Duration::from_millis(u64::from(i));
            access.try_code(address, "WRONG1", at);
        }
        assert_eq!(access.addresses(), MAX_ADDRESSES);
        let late = start + Duration::from_secs(10);
        access.try_code(ip(1), "WRONG1", late);
        assert_eq!(access.addresses(), MAX_ADDRESSES);
        // The oldest (10.0.0.0) was forgotten; the newest are still there.
        assert!(
            !access
                .wrong
                .contains_key(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)))
        );
        assert!(access.wrong.contains_key(&ip(1)));
        // After the window, the expired records make room at once.
        let later = start + WINDOW + Duration::from_secs(5);
        access.try_code(ip(2), "WRONG1", later);
        assert_eq!(access.addresses(), 2);
    }
}
