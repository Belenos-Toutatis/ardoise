//! Ogg pages (RFC 3533), the container of the narration's `audio.opus` member (RFC 7845,
//! `narration.rs`).
//!
//! A page is a 27-byte header (`OggS`, version 0, flags, granule position, stream serial
//! number, page sequence number, CRC-32, segment count), a segment table of lacing values and
//! the body. Packets are cut into 255-byte segments; a lacing value below 255 ends a packet, so
//! a packet whose length is a multiple of 255 ends with a zero, and a page whose last lacing
//! value is 255 leaves its last packet to be continued on the next page.
//!
//! The writer only needs whole packets per page (Opus packets are at most a few kilobytes).
//! The reader takes any well-formed stream: packets continued across pages (other muxers
//! split large `OpusTags`), several logical streams (pages of other serial numbers are
//! skipped by the caller). It never trusts a length: every slice is checked against what is
//! left of the input, and it never allocates more than the input holds.

use crate::MediaError;

/// Header flag: the page starts with the rest of a packet begun on the previous page.
pub(crate) const FLAG_CONTINUED: u8 = 0x01;
/// Header flag: first page of a logical stream.
pub(crate) const FLAG_BOS: u8 = 0x02;
/// Header flag: last page of a logical stream.
pub(crate) const FLAG_EOS: u8 = 0x04;
/// Granule position of a page on which no packet ends (-1 as a signed 64-bit integer).
pub(crate) const NO_GRANULE: u64 = u64::MAX;

const CAPTURE: &[u8; 4] = b"OggS";
const HEADER_LEN: usize = 27;
/// Offset of the CRC field in the header.
const CRC_OFFSET: usize = 22;
const MAX_SEGMENTS: usize = 255;

/// CRC-32 of the Ogg pages: polynomial 0x04C11DB7, not reflected, initial value 0, no final
/// XOR (RFC 3533 §6).
const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = (i as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Continues the Ogg CRC of `crc` over `bytes`.
fn crc_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc = (crc << 8) ^ CRC_TABLE[usize::from((crc >> 24) as u8 ^ byte)];
    }
    crc
}

/// The CRC of a whole page, its CRC field counted as zeros.
fn page_crc(page: &[u8]) -> u32 {
    let (before, rest) = page.split_at(CRC_OFFSET.min(page.len()));
    let after = rest.get(4..).unwrap_or_default();
    let crc = crc_update(0, before);
    let crc = crc_update(crc, &[0; 4]);
    crc_update(crc, after)
}

/// Writes the pages of one logical stream.
#[derive(Debug)]
pub(crate) struct PageWriter {
    serial: u32,
    sequence: u32,
    out: Vec<u8>,
}

impl PageWriter {
    pub(crate) fn new(serial: u32) -> Self {
        Self {
            serial,
            sequence: 0,
            out: Vec::new(),
        }
    }

    /// Lacing values a packet of `len` bytes takes.
    pub(crate) fn segments(len: usize) -> usize {
        len / 255 + 1
    }

    /// Appends a page holding `packets`, each whole (at most 255 lacing values in all), with
    /// header `flags` and `granule` (the position at the end of the last packet).
    pub(crate) fn page(
        &mut self,
        packets: &[&[u8]],
        granule: u64,
        flags: u8,
    ) -> Result<(), MediaError> {
        let segments: usize = packets.iter().map(|p| Self::segments(p.len())).sum();
        if segments > MAX_SEGMENTS {
            return Err(MediaError::Encoding {
                codec: "Ogg",
                message: format!("{segments} segments do not fit on a page"),
            });
        }
        let start = self.out.len();
        self.out.extend_from_slice(CAPTURE);
        self.out.push(0); // stream structure version
        self.out.push(flags);
        self.out.extend_from_slice(&granule.to_le_bytes());
        self.out.extend_from_slice(&self.serial.to_le_bytes());
        self.out.extend_from_slice(&self.sequence.to_le_bytes());
        self.out.extend_from_slice(&[0; 4]); // CRC, set below
        self.out.push(segments as u8);
        for packet in packets {
            let full = packet.len() / 255;
            self.out.extend(std::iter::repeat_n(255u8, full));
            self.out.push((packet.len() % 255) as u8);
        }
        for packet in packets {
            self.out.extend_from_slice(packet);
        }
        let crc = page_crc(&self.out[start..]);
        self.out[start + CRC_OFFSET..start + CRC_OFFSET + 4].copy_from_slice(&crc.to_le_bytes());
        self.sequence = self.sequence.wrapping_add(1);
        Ok(())
    }

    /// The stream written.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.out
    }
}

/// One page as read: its header fields, lacing values and body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Page<'a> {
    pub(crate) flags: u8,
    pub(crate) granule: u64,
    pub(crate) serial: u32,
    pub(crate) sequence: u32,
    pub(crate) lacing: &'a [u8],
    pub(crate) body: &'a [u8],
}

impl Page<'_> {
    pub(crate) fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

/// Reads the page at the start of `bytes`; returns it and its length. Fails on anything but a
/// complete version-0 page with the right CRC.
pub(crate) fn read_page(bytes: &[u8]) -> Result<(Page<'_>, usize), MediaError> {
    let invalid = |what: &str| MediaError::InvalidFile(format!("Ogg: {what}"));
    let header = bytes
        .get(..HEADER_LEN)
        .ok_or_else(|| invalid("truncated page header"))?;
    if &header[..4] != CAPTURE {
        return Err(invalid("missing page capture pattern"));
    }
    if header[4] != 0 {
        return Err(invalid("unknown stream structure version"));
    }
    let le32 = |at: usize| {
        u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    let mut granule = [0u8; 8];
    granule.copy_from_slice(&header[6..14]);
    let segments = usize::from(header[26]);
    let lacing = bytes
        .get(HEADER_LEN..HEADER_LEN + segments)
        .ok_or_else(|| invalid("truncated segment table"))?;
    let body_len: usize = lacing.iter().map(|&l| usize::from(l)).sum();
    let len = HEADER_LEN + segments + body_len;
    let page = bytes.get(..len).ok_or_else(|| invalid("truncated page"))?;
    if page_crc(page) != le32(CRC_OFFSET) {
        return Err(invalid("page CRC mismatch"));
    }
    Ok((
        Page {
            flags: header[5],
            granule: u64::from_le_bytes(granule),
            serial: le32(14),
            sequence: le32(18),
            lacing,
            body: &page[HEADER_LEN + segments..],
        },
        len,
    ))
}

/// The pages of `bytes`, back to back from the start (no resynchronisation: anything between
/// pages is an error).
pub(crate) fn pages(bytes: &[u8]) -> impl Iterator<Item = Result<Page<'_>, MediaError>> {
    let mut rest = bytes;
    let mut failed = false;
    std::iter::from_fn(move || {
        if rest.is_empty() || failed {
            return None;
        }
        match read_page(rest) {
            Ok((page, len)) => {
                rest = rest.get(len..).unwrap_or_default();
                Some(Ok(page))
            }
            Err(error) => {
                failed = true;
                Some(Err(error))
            }
        }
    })
}

/// Rebuilds the packets of one logical stream from its pages, in order.
///
/// Packets are appended to one buffer (`data`), so a stream of many small packets costs
/// little more than its bytes.
#[derive(Debug, Default)]
pub(crate) struct PacketReader {
    /// Every byte of the completed packets, back to back, then those of a packet still to be
    /// continued on the next page.
    data: Vec<u8>,
    /// End offset in `data` of each completed packet.
    ends: Vec<usize>,
    /// True while the bytes after the last completed packet wait for their continuation.
    partial: bool,
    next_sequence: Option<u32>,
}

impl PacketReader {
    /// Adds the packets of `page` (of this stream). Returns how many packets end on it.
    pub(crate) fn push(&mut self, page: &Page<'_>) -> Result<usize, MediaError> {
        let invalid = |what: &str| MediaError::InvalidFile(format!("Ogg: {what}"));
        if self.next_sequence.is_some_and(|next| next != page.sequence) {
            return Err(invalid("missing page (sequence number gap)"));
        }
        self.next_sequence = Some(page.sequence.wrapping_add(1));
        match (self.partial, page.has(FLAG_CONTINUED)) {
            (true, false) => return Err(invalid("packet not continued on the next page")),
            (false, true) => return Err(invalid("continued page without a packet to continue")),
            _ => {}
        }
        // `read_page` checked that the body holds exactly the sum of the lacing values.
        let mut start = 0;
        let mut run = 0;
        let mut completed = 0;
        for &value in page.lacing {
            run += usize::from(value);
            if value < 255 {
                let packet = page.body.get(start..start + run).unwrap_or_default();
                self.data.extend_from_slice(packet);
                self.ends.push(self.data.len());
                self.partial = false;
                start += run;
                run = 0;
                completed += 1;
            }
        }
        if run > 0 {
            // The last lacing value is 255: the packet goes on on the next page.
            let packet = page.body.get(start..start + run).unwrap_or_default();
            self.data.extend_from_slice(packet);
            self.partial = true;
        }
        Ok(completed)
    }

    /// True while a packet waits for its continuation.
    pub(crate) fn has_partial(&self) -> bool {
        self.partial
    }

    /// Number of completed packets.
    pub(crate) fn len(&self) -> usize {
        self.ends.len()
    }

    /// Completed packet `index`.
    pub(crate) fn packet(&self, index: usize) -> Option<&[u8]> {
        let end = *self.ends.get(index)?;
        let start = match index.checked_sub(1) {
            Some(previous) => *self.ends.get(previous)?,
            None => 0,
        };
        self.data.get(start..end)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Largest packet that fits on one page: 254 lacing values of 255 and a last one of 254.
    const MAX_PACKET_ON_PAGE: usize = (MAX_SEGMENTS - 1) * 255 + 254;

    /// A page from raw parts (any lacing, any flags), with a correct CRC.
    pub(crate) fn raw_page(
        flags: u8,
        granule: u64,
        serial: u32,
        sequence: u32,
        lacing: &[u8],
        body: &[u8],
    ) -> Vec<u8> {
        let mut page = Vec::new();
        page.extend_from_slice(CAPTURE);
        page.push(0);
        page.push(flags);
        page.extend_from_slice(&granule.to_le_bytes());
        page.extend_from_slice(&serial.to_le_bytes());
        page.extend_from_slice(&sequence.to_le_bytes());
        page.extend_from_slice(&[0; 4]);
        page.push(lacing.len() as u8);
        page.extend_from_slice(lacing);
        page.extend_from_slice(body);
        fix_crc(&mut page);
        page
    }

    /// Recomputes the CRC of the page at the start of `page` (its lacing taken as is).
    pub(crate) fn fix_crc(page: &mut [u8]) {
        let crc = page_crc(page);
        page[CRC_OFFSET..CRC_OFFSET + 4].copy_from_slice(&crc.to_le_bytes());
    }

    /// Recomputes the CRC of every page of a stream, skipping what does not parse as a page
    /// header; returns false if the stream cannot be walked page by page.
    pub(crate) fn fix_all_crcs(bytes: &mut [u8]) -> bool {
        let mut at = 0;
        while at < bytes.len() {
            let Some(header) = bytes.get(at..at + HEADER_LEN) else {
                return false;
            };
            let segments = usize::from(header[26]);
            let Some(lacing) = bytes.get(at + HEADER_LEN..at + HEADER_LEN + segments) else {
                return false;
            };
            let len = HEADER_LEN + segments + lacing.iter().map(|&l| usize::from(l)).sum::<usize>();
            let Some(page) = bytes.get_mut(at..at + len) else {
                return false;
            };
            fix_crc(page);
            at += len;
        }
        true
    }

    #[test]
    fn crc_matches_the_reference_values() {
        // CRC-32/MPEG-2 without its initial value and final XOR (CRC-32/OGG): the check
        // value of "123456789" is 0x89A1897F.
        assert_eq!(crc_update(0, b"123456789"), 0x89A1_897F);
        assert_eq!(crc_update(0, b""), 0);
    }

    #[test]
    fn pages_round_trip() {
        let mut writer = PageWriter::new(0x1234_5678);
        let big = vec![7u8; 600];
        let exact = vec![9u8; 510];
        writer.page(&[b"head"], 0, FLAG_BOS).unwrap();
        writer
            .page(&[&big, b"", &exact, b"x"], 4242, FLAG_EOS)
            .unwrap();
        let bytes = writer.into_bytes();

        let pages: Vec<Page<'_>> = pages(&bytes).collect::<Result<_, _>>().unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(
            (
                pages[0].flags,
                pages[0].granule,
                pages[0].serial,
                pages[0].sequence
            ),
            (FLAG_BOS, 0, 0x1234_5678, 0)
        );
        assert_eq!(
            (pages[1].flags, pages[1].granule, pages[1].sequence),
            (FLAG_EOS, 4242, 1)
        );
        // 600 = 255 + 255 + 90; 0; 510 = 255 + 255 + 0; 1.
        assert_eq!(pages[1].lacing, &[255, 255, 90, 0, 255, 255, 0, 1]);

        let mut reader = PacketReader::default();
        assert_eq!(reader.push(&pages[0]).unwrap(), 1);
        assert_eq!(reader.push(&pages[1]).unwrap(), 4);
        assert!(!reader.has_partial());
        assert_eq!(reader.len(), 5);
        assert_eq!(reader.packet(0), Some(&b"head"[..]));
        assert_eq!(reader.packet(1), Some(&big[..]));
        assert_eq!(reader.packet(2), Some(&b""[..]));
        assert_eq!(reader.packet(3), Some(&exact[..]));
        assert_eq!(reader.packet(4), Some(&b"x"[..]));
        assert_eq!(reader.packet(5), None);
    }

    #[test]
    fn too_many_segments_are_refused() {
        let mut writer = PageWriter::new(1);
        let packet = vec![0u8; MAX_PACKET_ON_PAGE];
        writer.page(&[&packet], 0, 0).unwrap();
        let packet = vec![0u8; MAX_PACKET_ON_PAGE + 1];
        assert!(matches!(
            writer.page(&[&packet], 0, 0),
            Err(MediaError::Encoding { codec: "Ogg", .. })
        ));
    }

    #[test]
    fn packets_continue_across_pages() {
        let packet: Vec<u8> = (0..700u32).map(|i| i as u8).collect();
        let first = raw_page(FLAG_BOS, NO_GRANULE, 5, 0, &[255, 255], &packet[..510]);
        let second = raw_page(
            FLAG_CONTINUED,
            10,
            5,
            1,
            &[190, 3],
            &[&packet[510..], b"abc"].concat(),
        );
        let bytes = [first, second].concat();
        let mut reader = PacketReader::default();
        let mut completed = Vec::new();
        for page in pages(&bytes) {
            completed.push(reader.push(&page.unwrap()).unwrap());
        }
        assert_eq!(completed, [0, 2]);
        assert_eq!(reader.packet(0), Some(&packet[..]));
        assert_eq!(reader.packet(1), Some(&b"abc"[..]));
    }

    #[test]
    fn damaged_pages_are_refused() {
        let mut writer = PageWriter::new(9);
        writer.page(&[b"hello"], 0, FLAG_BOS).unwrap();
        writer.page(&[b"world"], 5, FLAG_EOS).unwrap();
        let bytes = writer.into_bytes();
        assert!(pages(&bytes).all(|p| p.is_ok()));

        // Any flipped bit is caught by the CRC (or the header checks).
        for at in 0..bytes.len() {
            let mut damaged = bytes.clone();
            damaged[at] ^= 0x10;
            assert!(
                pages(&damaged).any(|p| p.is_err()),
                "bit flip at {at} not detected"
            );
        }
        // Every truncation fails on its last page.
        for len in 1..bytes.len() {
            if len == bytes.len() / 2 {
                continue; // the boundary between the two (equal-sized) pages
            }
            assert!(pages(&bytes[..len]).any(|p| p.is_err()), "prefix {len}");
        }
        // Version 1 is refused even with a right CRC.
        let mut version = bytes.clone();
        version[4] = 1;
        fix_crc(&mut version);
        assert!(read_page(&version).is_err());
    }

    #[test]
    fn continuation_mismatches_and_gaps_are_refused() {
        let open = raw_page(0, NO_GRANULE, 1, 0, &[255], &[0; 255]);
        let not_continued = raw_page(0, 3, 1, 1, &[1], &[0]);
        let mut reader = PacketReader::default();
        reader.push(&read_page(&open).unwrap().0).unwrap();
        assert!(reader.has_partial());
        assert!(reader.push(&read_page(&not_continued).unwrap().0).is_err());

        let continued = raw_page(FLAG_CONTINUED, 3, 1, 0, &[1], &[0]);
        let mut reader = PacketReader::default();
        assert!(reader.push(&read_page(&continued).unwrap().0).is_err());

        let first = raw_page(0, 0, 1, 0, &[1], &[0]);
        let skipped = raw_page(0, 0, 1, 2, &[1], &[0]);
        let mut reader = PacketReader::default();
        reader.push(&read_page(&first).unwrap().0).unwrap();
        assert!(reader.push(&read_page(&skipped).unwrap().0).is_err());
    }
}
