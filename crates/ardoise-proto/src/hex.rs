//! Hex dumps in the `e6 06 ab cd` style of protocol.md and of the diagnostic log.

use std::fmt::Write as _;

/// Failure while reading a hex dump.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HexError {
    /// A character that is neither a hex digit nor whitespace.
    #[error("invalid hex character {ch:?} at position {position}")]
    InvalidChar {
        /// Character position in the input (in characters).
        position: usize,
        /// Offending character.
        ch: char,
    },
    /// The digits do not pair up into whole bytes.
    #[error("odd number of hex digits")]
    OddDigits,
}

/// Lowercase hex, one space between bytes: `[0xe6, 0x06]` gives `e6 06`.
pub fn encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 3);
    for (i, byte) in bytes.iter().enumerate() {
        if i > 0 {
            text.push(' ');
        }
        // Writing to a String cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Reads hex digits, ignoring whitespace: `e6 06 ab`, `E606AB` and `e606 ab` are the same.
pub fn decode(text: &str) -> Result<Vec<u8>, HexError> {
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let mut high: Option<u8> = None;
    for (position, ch) in text.chars().enumerate() {
        if ch.is_whitespace() {
            continue;
        }
        let digit = ch
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
            .ok_or(HexError::InvalidChar { position, ch })?;
        match high.take() {
            Some(h) => bytes.push((h << 4) | digit),
            None => high = Some(digit),
        }
    }
    if high.is_some() {
        return Err(HexError::OddDigits);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_with_spaces() {
        assert_eq!(encode(&[0xe6, 0x06, 0xab]), "e6 06 ab");
        assert_eq!(encode(&[0x00]), "00");
        assert_eq!(encode(&[]), "");
    }

    #[test]
    fn decodes_any_spacing_and_case() {
        assert_eq!(decode("e6 06 ab").unwrap(), [0xe6, 0x06, 0xab]);
        assert_eq!(decode("E606AB").unwrap(), [0xe6, 0x06, 0xab]);
        assert_eq!(decode("  e6\n06\tab ").unwrap(), [0xe6, 0x06, 0xab]);
        assert_eq!(decode("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(
            decode("e6 0g"),
            Err(HexError::InvalidChar {
                position: 4,
                ch: 'g'
            })
        );
        assert_eq!(decode("e6 0"), Err(HexError::OddDigits));
    }

    #[test]
    fn round_trips() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&bytes)).unwrap(), bytes);
    }
}
