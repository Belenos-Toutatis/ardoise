//! Frames exchanged on the Nordic UART characteristics (protocol.md §2.1).
//!
//! Both directions use the same layout: one opcode byte, one length byte `L`, then `L`
//! argument bytes. A frame is never split across notifications, and one notification carries
//! one frame.

use std::fmt;

/// Largest number of argument bytes a frame can carry (the length is a single byte).
pub const MAX_ARGS: usize = 255;

/// One command or reply: `opcode, length, arguments`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Frame {
    /// First byte, which selects the message.
    pub opcode: u8,
    /// Argument bytes (the length byte is derived from them).
    pub args: Vec<u8>,
}

/// Failure while encoding or decoding a frame.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// Fewer than the two bytes of opcode and length.
    #[error("frame of {len} bytes is too short (opcode and length need 2)")]
    TooShort {
        /// Number of bytes received.
        len: usize,
    },
    /// The length byte does not match the number of argument bytes present.
    #[error("frame length byte says {declared} argument bytes but {actual} are present")]
    LengthMismatch {
        /// Value of the length byte.
        declared: usize,
        /// Number of bytes after the length byte.
        actual: usize,
    },
    /// More arguments than a length byte can describe.
    #[error("{len} argument bytes do not fit in a frame (at most {MAX_ARGS})")]
    TooLong {
        /// Number of argument bytes.
        len: usize,
    },
}

impl Frame {
    /// Builds a frame from its opcode and arguments.
    pub fn new(opcode: u8, args: impl Into<Vec<u8>>) -> Self {
        Self {
            opcode,
            args: args.into(),
        }
    }

    /// Serializes the frame as `opcode, length, arguments`.
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        let len = u8::try_from(self.args.len()).map_err(|_| FrameError::TooLong {
            len: self.args.len(),
        })?;
        let mut bytes = Vec::with_capacity(2 + self.args.len());
        bytes.push(self.opcode);
        bytes.push(len);
        bytes.extend_from_slice(&self.args);
        Ok(bytes)
    }

    /// Parses one whole notification. The length byte must match exactly.
    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        let (frame, rest) = Self::decode_prefix(bytes)?;
        if rest.is_empty() {
            Ok(frame)
        } else {
            Err(FrameError::LengthMismatch {
                declared: frame.args.len(),
                actual: frame.args.len() + rest.len(),
            })
        }
    }

    /// Parses the frame at the start of `bytes` and returns it with the bytes that follow it.
    pub fn decode_prefix(bytes: &[u8]) -> Result<(Self, &[u8]), FrameError> {
        let [opcode, len, rest @ ..] = bytes else {
            return Err(FrameError::TooShort { len: bytes.len() });
        };
        let declared = usize::from(*len);
        let (args, tail) = rest
            .split_at_checked(declared)
            .ok_or(FrameError::LengthMismatch {
                declared,
                actual: rest.len(),
            })?;
        Ok((Self::new(*opcode, args), tail))
    }
}

/// Hex dump of the encoded frame, e.g. `b3 01 00` (`b3 ?? ...` if it is too long to encode).
impl fmt::Display for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.encode() {
            Ok(bytes) => f.write_str(&crate::hex::encode(&bytes)),
            Err(_) => write!(
                f,
                "{:02x} ?? {}",
                self.opcode,
                crate::hex::encode(&self.args)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_opcode_length_and_args() {
        let frame = Frame::new(0xb3, [0x00]);
        assert_eq!(frame.encode().unwrap(), [0xb3, 0x01, 0x00]);
        assert_eq!(Frame::new(0xe4, []).encode().unwrap(), [0xe4, 0x00]);
    }

    #[test]
    fn decodes_and_round_trips() {
        let bytes = [0xeb, 0x06, 0x03, 0x00, 0x60, 0x54, 0x00, 0x00];
        let frame = Frame::decode(&bytes).unwrap();
        assert_eq!(frame.opcode, 0xeb);
        assert_eq!(frame.args, [0x03, 0x00, 0x60, 0x54, 0x00, 0x00]);
        assert_eq!(frame.encode().unwrap(), bytes);
        assert_eq!(frame.to_string(), "eb 06 03 00 60 54 00 00");
    }

    #[test]
    fn rejects_short_and_mismatched_frames() {
        assert_eq!(Frame::decode(&[]), Err(FrameError::TooShort { len: 0 }));
        assert_eq!(Frame::decode(&[0xb3]), Err(FrameError::TooShort { len: 1 }));
        assert_eq!(
            Frame::decode(&[0xb3, 0x01]),
            Err(FrameError::LengthMismatch {
                declared: 1,
                actual: 0
            })
        );
        assert_eq!(
            Frame::decode(&[0xb3, 0x01, 0x00, 0x00]),
            Err(FrameError::LengthMismatch {
                declared: 1,
                actual: 2
            })
        );
    }

    #[test]
    fn prefix_decoding_returns_the_rest() {
        let (frame, rest) = Frame::decode_prefix(&[0xb3, 0x01, 0x00, 0xc9, 0x00]).unwrap();
        assert_eq!(frame, Frame::new(0xb3, [0x00]));
        assert_eq!(rest, [0xc9, 0x00]);
    }

    #[test]
    fn too_many_args_cannot_be_encoded() {
        let frame = Frame::new(0xbb, vec![0x41; 256]);
        assert_eq!(frame.encode(), Err(FrameError::TooLong { len: 256 }));
        assert!(Frame::new(0xbb, vec![0x41; 255]).encode().is_ok());
    }
}
