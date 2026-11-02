//! Who may read the event stream (`docs/design/lan-share.md`, "Access: code, token, attempts"):
//! the 6-character code, the session tokens it gives, the limit of wrong codes per address, and
//! how often an address may open a stream.
//!
//! Time is passed in (`now`) so that the tests can move the clock.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

/// The 31 unambiguous symbols of a code (no `0 O 1 I L`).
pub(crate) const ALPHABET: &[u8; 31] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
/// Characters in a code.
pub(crate) const CODE_LEN: usize = 6;
/// Wrong codes allowed per address within [`WINDOW`].
pub(crate) const MAX_WRONG: usize = 5;
/// How long a wrong code counts.
pub(crate) const WINDOW: Duration = Duration::from_secs(5 * 60);
/// Session tokens kept per address: a new join from an address that has as many forgets that
/// address's oldest token, so that one address can never push out the others' tokens.
pub(crate) const MAX_TOKENS_PER_ADDRESS: usize = 8;
/// Session tokens kept in all (a last resort: 32 addresses at their own limit). When full, the
/// oldest token of the address that holds the most is forgotten: an address with a single
/// token (the projector) keeps it while others hold several.
pub(crate) const MAX_TOKENS: usize = 256;
/// Addresses whose wrong codes are remembered. When it is full of records still running, an
/// address without one gets 429 until the first record expires: nothing is forgotten early,
/// which would let many addresses reset each other's count. Nearby addresses have a table of
/// their own (see [`Access::try_code`]).
pub(crate) const MAX_ADDRESSES: usize = 4096;
/// Addresses that joined, remembered (the first ones) so that they always have room to try a
/// code again.
pub(crate) const MAX_JOINED: usize = 64;
/// Addresses whose stream openings are counted ([`StreamRate`]); each is forgotten once its
/// allowance is whole again.
pub(crate) const MAX_RATE_ADDRESSES: usize = 4096;

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
    /// Tokens given, oldest first, with the address that joined.
    tokens: VecDeque<(IpAddr, String)>,
    /// Recent wrong codes of the other addresses: at most [`MAX_ADDRESSES`].
    wrong: HashMap<IpAddr, VecDeque<Instant>>,
    /// Recent wrong codes of the nearby addresses (see [`Access::is_near`]): never refused for
    /// want of room, and bounded by construction (254 per address of this PC, plus
    /// [`MAX_JOINED`]).
    near: HashMap<IpAddr, VecDeque<Instant>>,
    /// This PC's addresses when the share started: their /24 networks are "nearby".
    local: Vec<Ipv4Addr>,
    /// The first addresses that joined.
    joined: Vec<IpAddr>,
}

impl Access {
    /// `local`: this PC's IPv4 addresses (their /24 networks keep room in the attempts table).
    pub fn new(code: String, local: Vec<Ipv4Addr>) -> Self {
        Self {
            code,
            tokens: VecDeque::new(),
            wrong: HashMap::new(),
            near: HashMap::new(),
            local,
            joined: Vec::new(),
        }
    }

    /// Whether `address` counts its wrong codes in the reserved table: it is on the same /24
    /// network as one of this PC's addresses (at most 254 such addresses each, however many
    /// addresses a PC elsewhere takes), or it already joined (it had the code). The projector,
    /// usually in the classroom's network, can then always try the code, even when PCs with
    /// thousands of addresses fill the general table.
    fn is_near(&self, address: IpAddr) -> bool {
        let same_network = match address {
            IpAddr::V4(v4) => self
                .local
                .iter()
                .any(|local| u32::from(*local) >> 8 == u32::from(v4) >> 8),
            IpAddr::V6(_) => false,
        };
        same_network || self.joined.contains(&address)
    }

    /// The table of wrong codes of `address`'s kind.
    fn table(&mut self, near: bool) -> &mut HashMap<IpAddr, VecDeque<Instant>> {
        if near {
            &mut self.near
        } else {
            &mut self.wrong
        }
    }

    /// Checks a code typed from `address` at `now`.
    pub fn try_code(&mut self, address: IpAddr, typed: &str, now: Instant) -> Verdict {
        let near = self.is_near(address);
        if let Some(refused) = self.refusal(address, near, now) {
            return refused;
        }
        if constant_time_eq(normalize_code(typed).as_bytes(), self.code.as_bytes()) {
            self.wrong.remove(&address);
            self.near.remove(&address);
            if self.joined.len() < MAX_JOINED && !self.joined.contains(&address) {
                self.joined.push(address);
            }
            return Verdict::Right;
        }
        let times = self.table(near).entry(address).or_default();
        times.push_back(now);
        let remaining = MAX_WRONG.saturating_sub(times.len());
        Verdict::Wrong {
            remaining: u32::try_from(remaining).unwrap_or(0),
        }
    }

    /// `TooMany` if `address` may not try a code at `now`: too many wrong codes lately, or a
    /// new address of the general kind while its table is full.
    fn refusal(&mut self, address: IpAddr, near: bool, now: Instant) -> Option<Verdict> {
        let table = self.table(near);
        if let Some(times) = table.get_mut(&address) {
            forget_old(times, now);
            if times.len() >= MAX_WRONG {
                let oldest = times.front().copied().unwrap_or(now);
                return Some(Verdict::TooMany {
                    retry_after: seconds_until_expiry(oldest, now),
                });
            }
        } else if !near && table.len() >= MAX_ADDRESSES {
            // A new address and no room to count its attempts: refused before the code is
            // compared, or the limit could be dodged from new addresses.
            table.retain(|_, times| {
                forget_old(times, now);
                !times.is_empty()
            });
            if table.len() >= MAX_ADDRESSES {
                let first_to_expire = table.values().filter_map(|t| t.back().copied()).min();
                return Some(Verdict::TooMany {
                    retry_after: seconds_until_expiry(first_to_expire.unwrap_or(now), now),
                });
            }
        }
        None
    }

    /// Keeps a token given to a viewer at `address`: at most [`MAX_TOKENS_PER_ADDRESS`] per
    /// address (that address's oldest is forgotten), [`MAX_TOKENS`] in all (the oldest token of
    /// the address that holds the most is forgotten).
    pub fn add_token(&mut self, address: IpAddr, token: String) {
        let own = self.tokens.iter().filter(|(a, _)| *a == address).count();
        if own >= MAX_TOKENS_PER_ADDRESS
            && let Some(index) = self.tokens.iter().position(|(a, _)| *a == address)
        {
            self.tokens.remove(index);
        }
        while self.tokens.len() >= MAX_TOKENS {
            let mut held: HashMap<IpAddr, usize> = HashMap::new();
            for (a, _) in &self.tokens {
                *held.entry(*a).or_default() += 1;
            }
            let most = held.values().copied().max().unwrap_or(0);
            match self
                .tokens
                .iter()
                .position(|(a, _)| held.get(a) == Some(&most))
            {
                Some(index) => {
                    self.tokens.remove(index);
                }
                None => {
                    self.tokens.pop_front();
                }
            }
        }
        self.tokens.push_back((address, token));
    }

    /// True if `candidate` is one of the tokens given (compared in constant time).
    pub fn token_valid(&self, candidate: &str) -> bool {
        self.tokens.iter().fold(false, |found, (_, token)| {
            found | constant_time_eq(token.as_bytes(), candidate.as_bytes())
        })
    }

    /// True if a token given to `address` is still valid: its connections may use the slots
    /// reserved for screens that joined.
    pub fn holds_token(&self, address: IpAddr) -> bool {
        self.tokens.iter().any(|(a, _)| *a == address)
    }

    /// Addresses remembered (tests).
    #[cfg(test)]
    fn addresses(&self) -> usize {
        self.wrong.len() + self.near.len()
    }
}

/// How often each address may open an event stream: [`Limits::stream_opens`] in a row, then one
/// more every [`Limits::stream_open_every`] (a token bucket, kept as the time at which the
/// address's allowance is whole again: the "generic cell rate algorithm").
///
/// [`Limits::stream_opens`]: crate::Limits::stream_opens
/// [`Limits::stream_open_every`]: crate::Limits::stream_open_every
#[derive(Debug)]
pub(crate) struct StreamRate {
    every: Duration,
    /// How far ahead of now an address's "whole again" time may be: `every × (burst − 1)`.
    tolerance: Duration,
    /// When the allowance of each address is whole again; only addresses for which that is
    /// still to come are kept.
    whole_at: HashMap<IpAddr, Instant>,
}

impl StreamRate {
    pub fn new(burst: u32, every: Duration) -> Self {
        Self {
            every,
            tolerance: every.saturating_mul(burst.max(1) - 1),
            whole_at: HashMap::new(),
        }
    }

    /// `Ok` if `address` may open a stream at `now`, else how long to wait.
    pub fn check(&self, address: IpAddr, now: Instant) -> Result<(), Duration> {
        let ahead = self
            .whole_at
            .get(&address)
            .map_or(Duration::ZERO, |whole| whole.saturating_duration_since(now));
        if ahead > self.tolerance {
            Err(ahead - self.tolerance)
        } else {
            Ok(())
        }
    }

    /// Counts a stream opened by `address` at `now` (after [`StreamRate::check`] allowed it).
    pub fn spend(&mut self, address: IpAddr, now: Instant) {
        if !self.whole_at.contains_key(&address) && self.whole_at.len() >= MAX_RATE_ADDRESSES {
            self.whole_at.retain(|_, whole| *whole > now);
            if self.whole_at.len() >= MAX_RATE_ADDRESSES {
                // Thousands of addresses holding the code, all within a few seconds: not
                // counted rather than refused (the connection limits still hold).
                return;
            }
        }
        let start = self
            .whole_at
            .get(&address)
            .copied()
            .map_or(now, |whole| whole.max(now));
        let whole = start.checked_add(self.every).unwrap_or(start);
        self.whole_at.insert(address, whole);
    }

    /// Addresses counted (tests).
    #[cfg(test)]
    fn addresses(&self) -> usize {
        self.whole_at.len()
    }
}

/// Whole seconds (at least 1) in `wait`, for a `Retry-After` header.
pub(crate) fn whole_seconds(wait: Duration) -> u64 {
    (wait.as_secs() + u64::from(wait.subsec_nanos() > 0)).max(1)
}

/// Whole seconds (at least 1) until a wrong code made at `at` stops counting.
fn seconds_until_expiry(at: Instant, now: Instant) -> u64 {
    whole_seconds(WINDOW.saturating_sub(now.saturating_duration_since(at)))
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

        let mut access = Access::new("ABCDEF".into(), Vec::new());
        assert!(!access.token_valid(&token));
        access.add_token(ip(1), token.clone());
        assert!(access.token_valid(&token));
        assert!(!access.token_valid(&token[..63]));
        assert!(!access.token_valid(""));
    }

    #[test]
    fn one_address_cannot_push_out_the_others_tokens() {
        let mut access = Access::new("ABCDEF".into(), Vec::new());
        let first = random_token().unwrap();
        access.add_token(ip(1), first.clone());
        let mut own = Vec::new();
        for _ in 0..1000 {
            let token = random_token().unwrap();
            access.add_token(ip(2), token.clone());
            own.push(token);
        }
        assert!(
            access.token_valid(&first),
            "another address's token is kept"
        );
        // The address keeps its last 8 tokens only.
        let (old, last) = own.split_at(own.len() - MAX_TOKENS_PER_ADDRESS);
        assert!(last.iter().all(|t| access.token_valid(t)));
        assert!(old.iter().all(|t| !access.token_valid(t)));
        assert_eq!(access.tokens.len(), 1 + MAX_TOKENS_PER_ADDRESS);

        // The global limit, as a last resort: one address with a single token (the projector),
        // then 40 addresses at their own limit. Tokens are taken from the addresses that hold
        // the most, oldest first: the single token stays, every address keeps some.
        let mut access = Access::new("ABCDEF".into(), Vec::new());
        let projector = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let single = random_token().unwrap();
        access.add_token(projector, single.clone());
        for a in 0..40u8 {
            for _ in 0..MAX_TOKENS_PER_ADDRESS {
                access.add_token(ip(a), random_token().unwrap());
            }
        }
        assert_eq!(access.tokens.len(), MAX_TOKENS);
        assert!(access.token_valid(&single));
        assert!(access.holds_token(projector));
        for a in 0..40u8 {
            let held = access.tokens.iter().filter(|(t, _)| *t == ip(a)).count();
            assert!((6..=MAX_TOKENS_PER_ADDRESS).contains(&held), "{a}: {held}");
        }
        assert!(!access.holds_token(ip(200)));
    }

    #[test]
    fn nearby_addresses_always_have_room_to_try() {
        let local = Ipv4Addr::new(192, 168, 1, 23);
        let mut access = Access::new("K7MQ2X".into(), vec![local]);
        let start = Instant::now();
        // PCs elsewhere fill the general table.
        let far = |i: u32| IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + i));
        for i in 0..MAX_ADDRESSES as u32 {
            access.try_code(far(i), "WRONG1", start);
        }
        let later = start + Duration::from_secs(10);
        let stranger = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 9));
        assert_eq!(
            access.try_code(stranger, "K7MQ2X", later),
            Verdict::TooMany { retry_after: 290 }
        );
        // The same /24 as this PC: counted, and the right code works.
        assert_eq!(
            access.try_code(ip(40), "WRONG1", later),
            Verdict::Wrong { remaining: 4 }
        );
        assert_eq!(access.try_code(ip(41), "k7m q2x", later), Verdict::Right);
        // Still limited to 5 wrong codes in 5 minutes.
        for remaining in (0..4).rev() {
            assert_eq!(
                access.try_code(ip(40), "WRONG1", later),
                Verdict::Wrong { remaining }
            );
        }
        assert_eq!(
            access.try_code(ip(40), "K7MQ2X", later),
            Verdict::TooMany { retry_after: 300 }
        );
        // Another /24 is not near.
        let neighbour = IpAddr::V4(Ipv4Addr::new(192, 168, 2, 5));
        assert!(matches!(
            access.try_code(neighbour, "K7MQ2X", later),
            Verdict::TooMany { .. }
        ));
        assert_eq!(access.wrong.len(), MAX_ADDRESSES);
    }

    #[test]
    fn addresses_that_joined_always_have_room_to_try() {
        let mut access = Access::new("K7MQ2X".into(), Vec::new());
        let start = Instant::now();
        let projector = IpAddr::V4(Ipv4Addr::new(172, 20, 5, 7));
        assert_eq!(access.try_code(projector, "K7MQ2X", start), Verdict::Right);
        let far = |i: u32| IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + i));
        for i in 0..MAX_ADDRESSES as u32 {
            access.try_code(far(i), "WRONG1", start);
        }
        // The projector, which joined before, may try again (after a new share of its token,
        // a lost cookie…); its wrong codes still count.
        assert_eq!(
            access.try_code(projector, "WRONG1", start),
            Verdict::Wrong { remaining: 4 }
        );
        assert_eq!(access.try_code(projector, "K7MQ2X", start), Verdict::Right);
        // Only the first addresses that joined are remembered.
        let mut access = Access::new("K7MQ2X".into(), Vec::new());
        for i in 0..(MAX_JOINED as u32 + 10) {
            assert_eq!(access.try_code(far(i), "K7MQ2X", start), Verdict::Right);
        }
        assert_eq!(access.joined.len(), MAX_JOINED);
        assert!(access.is_near(far(0)));
        assert!(!access.is_near(far(MAX_JOINED as u32)));
    }

    #[test]
    fn stream_openings_per_address() {
        let every = Duration::from_secs(2);
        let mut rate = StreamRate::new(3, every);
        let start = Instant::now();
        let a = ip(1);
        for _ in 0..3 {
            assert_eq!(rate.check(a, start), Ok(()));
            rate.spend(a, start);
        }
        // The fourth in a row waits for the next one: 2 s.
        assert_eq!(rate.check(a, start), Err(every));
        assert_eq!(whole_seconds(every), 2);
        // Another address is not affected.
        assert_eq!(rate.check(ip(2), start), Ok(()));
        // 1.5 s later, half a second more.
        let later = start + Duration::from_millis(1500);
        assert_eq!(rate.check(a, later), Err(Duration::from_millis(500)));
        assert_eq!(whole_seconds(Duration::from_millis(500)), 1);
        // Then one every 2 s.
        let later = start + every;
        assert_eq!(rate.check(a, later), Ok(()));
        rate.spend(a, later);
        assert!(rate.check(a, later).is_err());
        assert_eq!(rate.check(a, later + every), Ok(()));
        // After a long pause, a whole burst again.
        let much_later = start + Duration::from_secs(60);
        for _ in 0..3 {
            assert_eq!(rate.check(a, much_later), Ok(()));
            rate.spend(a, much_later);
        }
        assert!(rate.check(a, much_later).is_err());
        // A burst of one: one every `every`.
        let mut rate = StreamRate::new(0, every);
        assert_eq!(rate.check(a, start), Ok(()));
        rate.spend(a, start);
        assert_eq!(rate.check(a, start), Err(every));
    }

    #[test]
    fn the_stream_rate_table_is_bounded() {
        let mut rate = StreamRate::new(2, Duration::from_secs(2));
        let start = Instant::now();
        let address = |i: u32| IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + i));
        for i in 0..MAX_RATE_ADDRESSES as u32 {
            rate.spend(address(i), start);
        }
        assert_eq!(rate.addresses(), MAX_RATE_ADDRESSES);
        // Full of running allowances: a new address is not counted (never refused).
        let extra = address(MAX_RATE_ADDRESSES as u32);
        rate.spend(extra, start);
        assert_eq!(rate.addresses(), MAX_RATE_ADDRESSES);
        assert_eq!(rate.check(extra, start), Ok(()));
        // Once the allowances are whole again, they are forgotten for newcomers.
        let later = start + Duration::from_secs(3);
        rate.spend(extra, later);
        assert_eq!(rate.addresses(), 1);
    }

    #[test]
    fn right_and_wrong_codes() {
        let mut access = Access::new("K7MQ2X".into(), Vec::new());
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
        let mut access = Access::new("K7MQ2X".into(), Vec::new());
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
    fn a_full_table_refuses_new_addresses_without_forgetting() {
        let mut access = Access::new("K7MQ2X".into(), Vec::new());
        let start = Instant::now();
        let address = |i: u32| IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + i));
        for i in 0..MAX_ADDRESSES as u32 {
            let at = start + Duration::from_millis(u64::from(i));
            access.try_code(address(i), "WRONG1", at);
        }
        assert_eq!(access.addresses(), MAX_ADDRESSES);

        // A new address is refused, even with the right code, until the first record expires
        // (the one made at `start`): 300 s - 10 s = 290 s.
        let late = start + Duration::from_secs(10);
        assert_eq!(
            access.try_code(ip(1), "K7MQ2X", late),
            Verdict::TooMany { retry_after: 290 }
        );
        assert_eq!(
            access.try_code(ip(1), "WRONG1", late),
            Verdict::TooMany { retry_after: 290 }
        );
        assert_eq!(access.addresses(), MAX_ADDRESSES);
        assert!(!access.wrong.contains_key(&ip(1)));
        // Nothing was forgotten: the first address still has its wrong code.
        assert_eq!(access.wrong[&address(0)].len(), 1);
        // An address already counted goes on as before.
        assert_eq!(
            access.try_code(address(7), "WRONG1", late),
            Verdict::Wrong { remaining: 3 }
        );
        assert_eq!(access.try_code(address(8), "K7MQ2X", late), Verdict::Right);
        // A right code freed a place: the next new address is counted.
        assert_eq!(
            access.try_code(ip(1), "WRONG1", late),
            Verdict::Wrong { remaining: 4 }
        );
        assert_eq!(
            access.try_code(ip(2), "K7MQ2X", late),
            Verdict::TooMany { retry_after: 290 }
        );

        // Once the first records expire, they make room at once.
        let later = start + WINDOW + Duration::from_secs(1);
        assert_eq!(access.try_code(ip(2), "K7MQ2X", later), Verdict::Right);
        assert_eq!(
            access.try_code(ip(3), "WRONG1", later),
            Verdict::Wrong { remaining: 4 }
        );
        assert!(access.addresses() < MAX_ADDRESSES);
    }
}
