//! Schema-less protobuf reading (varints, zigzag, packed fields), written by hand.
//!
//! Every function works on borrowed bytes and fails with [`DecodeError`] on malformed input:
//! no panic, and every allocation is bounded by the length of the input (a field takes at
//! least one byte, a packed value at least one byte).

use crate::DecodeError;

/// Longest varint (10 bytes encode 64 bits).
const MAX_VARINT_LEN: usize = 10;

/// One field value, by wire type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Value<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}

/// One field of a message: number and value, in wire order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Field<'a> {
    pub number: u32,
    pub value: Value<'a>,
}

/// Reads a varint at `*pos` and advances it.
pub(crate) fn read_varint(bytes: &[u8], pos: &mut usize) -> Result<u64, DecodeError> {
    let mut result = 0u64;
    for i in 0..MAX_VARINT_LEN {
        let byte = *bytes.get(*pos).ok_or(DecodeError::Truncated)?;
        *pos += 1;
        let payload = u64::from(byte & 0x7f);
        if i == MAX_VARINT_LEN - 1 && payload > 1 {
            return Err(DecodeError::VarintTooLong);
        }
        result |= payload << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(DecodeError::VarintTooLong)
}

/// Zigzag decoding (`sint32`/`sint64`).
pub(crate) fn zigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

fn take<'a>(bytes: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8], DecodeError> {
    let end = pos.checked_add(len).ok_or(DecodeError::Truncated)?;
    let slice = bytes.get(*pos..end).ok_or(DecodeError::Truncated)?;
    *pos = end;
    Ok(slice)
}

/// Parses every field of one message, in order.
pub(crate) fn parse_message(bytes: &[u8]) -> Result<Vec<Field<'_>>, DecodeError> {
    let mut fields = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let key = read_varint(bytes, &mut pos)?;
        let number = u32::try_from(key >> 3).map_err(|_| DecodeError::FieldNumber)?;
        if number == 0 || number > 0x1fff_ffff {
            return Err(DecodeError::FieldNumber);
        }
        let value = match key & 7 {
            0 => Value::Varint(read_varint(bytes, &mut pos)?),
            1 => {
                let raw = take(bytes, &mut pos, 8)?;
                let mut le = [0u8; 8];
                le.copy_from_slice(raw);
                Value::Fixed64(u64::from_le_bytes(le))
            }
            2 => {
                let len = read_varint(bytes, &mut pos)?;
                let len = usize::try_from(len).map_err(|_| DecodeError::Truncated)?;
                Value::Bytes(take(bytes, &mut pos, len)?)
            }
            5 => {
                let raw = take(bytes, &mut pos, 4)?;
                let mut le = [0u8; 4];
                le.copy_from_slice(raw);
                Value::Fixed32(u32::from_le_bytes(le))
            }
            wire => return Err(DecodeError::WireType(wire as u8)),
        };
        fields.push(Field { number, value });
    }
    Ok(fields)
}

/// First length-delimited field `number`.
pub(crate) fn bytes_field<'a>(fields: &[Field<'a>], number: u32) -> Option<&'a [u8]> {
    fields.iter().find_map(|f| match f.value {
        Value::Bytes(b) if f.number == number => Some(b),
        _ => None,
    })
}

/// First varint field `number`.
pub(crate) fn varint_field(fields: &[Field<'_>], number: u32) -> Option<u64> {
    fields.iter().find_map(|f| match f.value {
        Value::Varint(v) if f.number == number => Some(v),
        _ => None,
    })
}

/// Every value of a repeated `sint32`/`sint64` field, accepting both the packed encoding
/// (one length-delimited field) and the unpacked one (one varint per value), concatenated in
/// wire order as protobuf requires.
pub(crate) fn repeated_sint(fields: &[Field<'_>], number: u32) -> Result<Vec<i64>, DecodeError> {
    let mut values = Vec::new();
    for field in fields.iter().filter(|f| f.number == number) {
        match field.value {
            Value::Varint(v) => values.push(zigzag(v)),
            Value::Bytes(packed) => {
                values.reserve(packed.len().min(1 << 20));
                let mut pos = 0;
                while pos < packed.len() {
                    values.push(zigzag(read_varint(packed, &mut pos)?));
                }
            }
            _ => return Err(DecodeError::WireTypeMismatch { field: number }),
        }
    }
    Ok(values)
}

/// Reads a leading varint length and returns exactly that many bytes after it (the
/// "length-delimited message" framing of stroke payloads). Trailing bytes are ignored.
pub(crate) fn length_delimited(bytes: &[u8]) -> Result<&[u8], DecodeError> {
    let mut pos = 0;
    let len = read_varint(bytes, &mut pos)?;
    let len = usize::try_from(len).map_err(|_| DecodeError::Truncated)?;
    take(bytes, &mut pos, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints() {
        let mut pos = 0;
        assert_eq!(read_varint(&[0x96, 0x01], &mut pos), Ok(150));
        assert_eq!(pos, 2);
        let mut pos = 0;
        assert_eq!(read_varint(&[0x80], &mut pos), Err(DecodeError::Truncated));
        let max = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
        let mut pos = 0;
        assert_eq!(read_varint(&max, &mut pos), Ok(u64::MAX));
        let too_big = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02];
        let mut pos = 0;
        assert_eq!(
            read_varint(&too_big, &mut pos),
            Err(DecodeError::VarintTooLong)
        );
        let endless = [0x80u8; 20];
        let mut pos = 0;
        assert_eq!(
            read_varint(&endless, &mut pos),
            Err(DecodeError::VarintTooLong)
        );
    }

    #[test]
    fn zigzag_values() {
        assert_eq!(zigzag(0), 0);
        assert_eq!(zigzag(1), -1);
        assert_eq!(zigzag(2), 1);
        assert_eq!(zigzag(80313), -40157);
        assert_eq!(zigzag(u64::MAX), i64::MIN);
        // Research §5 colour bytes `fe e7 bf 9a 0a`: zigzag-decoded, they give 0x51A7F9FF
        // (the blue of §5.2; the research text's "#BB3E9AFF" for these bytes is a slip).
        let mut pos = 0;
        let raw = read_varint(&[0xfe, 0xe7, 0xbf, 0x9a, 0x0a], &mut pos).unwrap();
        assert_eq!(zigzag(raw) as u32, 0x51A7_F9FF);
    }

    #[test]
    fn messages() {
        // 1: varint 2, 2: "ab", 3: fixed32 1.0, 4: fixed64 7
        let bytes = [
            0x08, 0x02, 0x12, 0x02, b'a', b'b', 0x1d, 0x00, 0x00, 0x80, 0x3f, 0x21, 7, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let fields = parse_message(&bytes).unwrap();
        assert_eq!(fields.len(), 4);
        assert_eq!(varint_field(&fields, 1), Some(2));
        assert_eq!(bytes_field(&fields, 2), Some(&b"ab"[..]));
        assert_eq!(fields[2].value, Value::Fixed32(1.0f32.to_bits()));
        assert_eq!(fields[3].value, Value::Fixed64(7));
        assert_eq!(bytes_field(&fields, 9), None);
    }

    #[test]
    fn malformed_messages_fail_cleanly() {
        assert_eq!(parse_message(&[0x12, 0x05, 1]), Err(DecodeError::Truncated));
        assert_eq!(parse_message(&[0x00, 0x01]), Err(DecodeError::FieldNumber));
        assert_eq!(parse_message(&[0x0b]), Err(DecodeError::WireType(3)));
        assert_eq!(parse_message(&[0x1d, 0x00]), Err(DecodeError::Truncated));
        // A length close to u64::MAX must not overflow or allocate.
        let huge = [
            0x12, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01,
        ];
        assert_eq!(parse_message(&huge), Err(DecodeError::Truncated));
        assert_eq!(parse_message(&[]), Ok(vec![]));
    }

    #[test]
    fn packed_and_unpacked_repeated_fields() {
        // field 4 packed [1, -1, 2], then field 4 unpacked -2
        let bytes = [0x22, 0x03, 0x02, 0x01, 0x04, 0x20, 0x03];
        let fields = parse_message(&bytes).unwrap();
        assert_eq!(repeated_sint(&fields, 4), Ok(vec![1, -1, 2, -2]));
        assert_eq!(repeated_sint(&fields, 5), Ok(vec![]));
        let bad = parse_message(&[0x22, 0x01, 0x80]).unwrap();
        assert_eq!(repeated_sint(&bad, 4), Err(DecodeError::Truncated));
        let wrong = parse_message(&[0x25, 0, 0, 0, 0]).unwrap();
        assert_eq!(
            repeated_sint(&wrong, 4),
            Err(DecodeError::WireTypeMismatch { field: 4 })
        );
    }

    #[test]
    fn length_delimited_framing() {
        assert_eq!(
            length_delimited(&[0x02, 0x08, 0x01, 0xff]),
            Ok(&[0x08, 0x01][..])
        );
        assert_eq!(length_delimited(&[0x05, 0x08]), Err(DecodeError::Truncated));
        assert_eq!(length_delimited(&[]), Err(DecodeError::Truncated));
    }

    #[test]
    fn random_bytes_never_panic() {
        // Deterministic pseudo-random garbage through every entry point.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        for len in 0..400 {
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state >> 24) as u8
                })
                .collect();
            if let Ok(fields) = parse_message(&bytes) {
                let _ = repeated_sint(&fields, 4);
                let _ = bytes_field(&fields, 1);
            }
            let _ = length_delimited(&bytes);
        }
    }
}
