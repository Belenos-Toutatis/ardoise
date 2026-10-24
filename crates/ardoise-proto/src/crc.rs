//! CRC-32 of a downloaded page (protocol.md §5.1).
//!
//! The tablet ends a download with `c8 05 ed <crc32 LE>`, computed over every byte received on
//! the offline characteristic for that file (file header and final packet included). The
//! algorithm is the standard IEEE CRC-32, as in zlib.

/// IEEE CRC-32 (zlib, PNG, Ethernet) of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn empty_input() {
        assert_eq!(crc32(&[]), 0);
    }
}
