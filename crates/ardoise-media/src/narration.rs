//! The narration stored with a note: its time base and its `audio.opus` member (an Ogg Opus
//! stream, `docs/file-format.md` §4 and `docs/design/media.md`, "Narration storage").
//!
//! # `audio.opus`
//!
//! An Ogg Opus stream (RFC 7845 over RFC 3533 pages, `ogg.rs`), so that any player (VLC, a
//! browser) plays the member once extracted:
//!
//! - page 0: the `OpusHead` packet alone, beginning of stream; page 1: `OpusTags`; then the
//!   audio, about one second (50 packets of 20 ms) per page, the last page marked end of
//!   stream;
//! - an audio page's granule position is the pre-skip plus the samples decoded up to the end of
//!   its last packet; the last page's is the pre-skip plus the clip's length, so a decoder trims
//!   the encoder's padding and gets the clip back sample for sample (RFC 7845 §4.4);
//! - `OpusTags` vendor `Ardoise` and the narration's time base as user comments:
//!   `ARDOISE_BASE=replay|note`, `ARDOISE_SPEED=<float>`, `ARDOISE_MAX_PAUSE_MS=<integer>|none`,
//!   `ARDOISE_START_MS=<signed integer>`. Each is optional on read; a missing or unreadable one
//!   takes its default (replay, 1, none, 0), a speed outside 0.25 to 16 counts as 1, and a
//!   narration in note time always has speed 1 and no trimming.
//!
//! # Reading hostile bytes
//!
//! `audio.opus` comes from note files, which can be damaged. [`decode_narration`] checks every
//! page's CRC, never trusts a length or a count, and stops with [`MediaError::InvalidFile`] on
//! anything malformed, truncated (the end-of-stream page must be there) or not mono/stereo
//! Opus. What it allocates is bounded twice:
//!
//! - by the input: the packets' bytes, and the decoded sound, which Opus itself limits (a
//!   packet of at least one byte plus its lacing byte decodes to at most 120 ms);
//!   `MAX_FRAMES_PER_INPUT_BYTE` enforces that bound explicitly;
//! - by the stream's own length, known before any audio is decoded (the last page's granule
//!   position): a stream longer than 4 hours is refused at once, and decoding stops as soon as
//!   it yields more than that length and one packet (a lying granule position).
//!
//! [`narration_header`] reads the time base and the length the same way, without decoding the
//! audio (for the editor's narration line).

use std::time::Duration;

use crate::ogg::{self, FLAG_BOS, FLAG_EOS, NO_GRANULE, PacketReader, PageWriter};
use crate::opus::{
    OpusEncoder, OpusHead, OpusPacket, OpusPacketDecoder, default_opus_bitrate, samples_to_duration,
};
use crate::replay::{MAX_SPEED, MIN_SPEED, ReplayOptions};
use crate::{AudioClip, MediaError, NARRATION_SAMPLE_RATE};

/// The time base a narration was recorded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrationBase {
    /// During a replay: replay time with the options of that replay.
    Replay,
    /// In live mode: note time (speed 1, no trimming).
    Note,
}

/// A narration and where it sits on the replay.
#[derive(Debug, Clone, PartialEq)]
pub struct Narration {
    /// 48 kHz.
    pub clip: AudioClip,
    pub base: NarrationBase,
    /// The time base: the replay options it was recorded with (for [`NarrationBase::Note`],
    /// speed 1 and no trimming).
    pub options: ReplayOptions,
    /// Replay instant, in that time base, of the first sample, in milliseconds; negative when
    /// the recording began before the note's time origin.
    pub start_ms: i64,
}

/// Samples per channel in a millisecond.
const FRAMES_PER_MS: i128 = NARRATION_SAMPLE_RATE as i128 / 1000;

/// Longest track [`Narration::merge`] builds (4 hours): two narrations further apart keep only
/// the later one, rather than allocating gigabytes of silence between them.
const MAX_MERGED_FRAMES: i128 = 4 * 3600 * NARRATION_SAMPLE_RATE as i128;

/// Longest `audio.opus` read (4 hours, like [`MAX_MERGED_FRAMES`]), in samples per channel.
const MAX_STREAM_FRAMES: u64 = 4 * 3600 * NARRATION_SAMPLE_RATE as u64;

impl Narration {
    /// Length of the clip.
    pub fn duration(&self) -> Duration {
        self.clip.duration()
    }

    /// Replay instant just after the last sample, in milliseconds (rounded up).
    pub fn end_ms(&self) -> i64 {
        let ms = self.duration().as_nanos().div_ceil(1_000_000);
        self.start_ms
            .saturating_add(i64::try_from(ms).unwrap_or(i64::MAX))
    }

    /// Whether [`merge`](Self::merge) keeps both: one track holds them, at their instants.
    pub fn fits_with(&self, later: &Narration) -> bool {
        let first_start = i128::from(self.start_ms) * FRAMES_PER_MS;
        let later_start = i128::from(later.start_ms) * FRAMES_PER_MS;
        let end = (first_start + frames(&self.clip) as i128)
            .max(later_start + frames(&later.clip) as i128);
        end - first_start.min(later_start) <= MAX_MERGED_FRAMES
    }

    /// One narration holding `self` and `later` (same base and options) at their instants,
    /// silence between them; where they overlap, `later` wins.
    ///
    /// The result has the larger channel count of the two (a mono clip is copied to both sides
    /// of a stereo one), starts at the earlier start and keeps `self`'s base and options. An
    /// empty clip adds nothing; two clips more than 4 hours apart give `later` alone.
    ///
    /// When `later` starts after `self` ends, with as many channels (pieces of one recording),
    /// `self`'s track grows in place instead of being copied into a new one.
    pub fn merge(mut self, later: Narration) -> Narration {
        let (first_frames, later_frames) = (frames(&self.clip), frames(&later.clip));
        if later_frames == 0 {
            return self;
        }
        if first_frames == 0 {
            return later;
        }
        let first_start = i128::from(self.start_ms) * FRAMES_PER_MS;
        let later_start = i128::from(later.start_ms) * FRAMES_PER_MS;
        let start = first_start.min(later_start);
        let end = (first_start + first_frames as i128).max(later_start + later_frames as i128);
        if end - start > MAX_MERGED_FRAMES {
            return later;
        }
        if later_start >= first_start + first_frames as i128
            && self.clip.channels == later.clip.channels
            && self.clip.channels > 0
        {
            let channels = usize::from(self.clip.channels);
            // Bounded by MAX_MERGED_FRAMES above; a partial last frame is left out, as below.
            let gap = (later_start - first_start) as usize;
            let samples = &mut self.clip.samples;
            samples.truncate(first_frames * channels);
            samples.resize(gap * channels, 0);
            samples.extend_from_slice(
                later
                    .clip
                    .samples
                    .get(..later_frames * channels)
                    .unwrap_or_default(),
            );
            return self;
        }
        let channels = self.clip.channels.max(later.clip.channels).max(1);
        let out_channels = usize::from(channels);
        // Bounded by MAX_MERGED_FRAMES above.
        let length = (end - start) as usize;
        let mut samples = vec![0i16; length * out_channels];
        place(
            &mut samples,
            out_channels,
            (first_start - start) as usize,
            &self.clip,
        );
        place(
            &mut samples,
            out_channels,
            (later_start - start) as usize,
            &later.clip,
        );
        Narration {
            clip: AudioClip { channels, samples },
            base: self.base,
            options: self.options,
            start_ms: self.start_ms.min(later.start_ms),
        }
    }
}

/// Samples per channel of `clip`.
fn frames(clip: &AudioClip) -> usize {
    clip.samples.len() / usize::from(clip.channels.max(1))
}

/// Copies `clip` into `track` (interleaved, `out_channels` channels) from frame `offset`; a
/// channel the clip does not have repeats its last one.
fn place(track: &mut [i16], out_channels: usize, offset: usize, clip: &AudioClip) {
    let channels = usize::from(clip.channels.max(1));
    let Some(target) = track.get_mut(offset * out_channels..) else {
        return;
    };
    for (to, from) in target
        .chunks_exact_mut(out_channels)
        .zip(clip.samples.chunks_exact(channels))
    {
        for (o, sample) in to.iter_mut().enumerate() {
            *sample = from[o.min(channels - 1)];
        }
    }
}

// --- audio.opus ---

/// Serial number of the Opus stream (any value will do: the file holds one stream).
const SERIAL: u32 = 0x4172_646F;
const VENDOR: &str = "Ardoise";
const TAG_BASE: &str = "ARDOISE_BASE";
const TAG_SPEED: &str = "ARDOISE_SPEED";
const TAG_MAX_PAUSE: &str = "ARDOISE_MAX_PAUSE_MS";
const TAG_START: &str = "ARDOISE_START_MS";
const OPUS_TAGS: &[u8; 8] = b"OpusTags";
/// About one second of 20 ms packets per page.
const PACKETS_PER_PAGE: usize = 50;
/// Most lacing values on a page.
const MAX_SEGMENTS: usize = 255;
/// Samples per channel decoded from each byte of input, at most. Opus allows 120 ms (5760
/// samples) in a packet of 2 bytes plus its lacing value: 1920 per byte; the margin keeps any
/// valid stream readable while the decoded size stays proportional to the input.
const MAX_FRAMES_PER_INPUT_BYTE: u64 = 2880;
/// Samples per channel in an Opus packet, at most (120 ms, RFC 6716 §3.2.5): how far decoding
/// may go past the length the granule positions give (the end trimming drops the encoder's
/// padding, less than one packet in the streams of every encoder known here).
const MAX_PACKET_FRAMES: u64 = 5760;
/// Offset of the output gain and of the channel mapping family in `OpusHead`.
const HEAD_GAIN: usize = 16;
const HEAD_MAPPING: usize = 18;

/// The `audio.opus` member holding `narration`.
pub fn encode_narration(narration: &Narration) -> Result<Vec<u8>, MediaError> {
    encode_stream(&narration.clip, &tags_packet(narration))
}

/// The Ogg Opus stream of `clip` with `tags` as its `OpusTags` packet.
fn encode_stream(clip: &AudioClip, tags: &[u8]) -> Result<Vec<u8>, MediaError> {
    let mut encoder = OpusEncoder::new(clip.channels, default_opus_bitrate(clip.channels))?;
    let pre_skip = u64::from(encoder.pre_skip());
    let mut packets = encoder.push(&clip.samples)?;
    packets.extend(encoder.finish()?);
    let length = frames(clip) as u64;

    let mut writer = PageWriter::new(SERIAL);
    writer.page(&[&encoder_head(clip.channels, pre_skip)], 0, FLAG_BOS)?;
    writer.page(&[tags], 0, 0)?;
    let pages = group_packets(&packets);
    if pages.is_empty() {
        // `finish` always encodes at least the encoder's delay: never expected.
        return Err(MediaError::Encoding {
            codec: "Opus",
            message: "no audio packet to end the stream with".into(),
        });
    }
    for (index, page) in pages.iter().enumerate() {
        let last = index + 1 == pages.len();
        let (granule, flags) = if last {
            // End trimming: the encoder's padding after the clip is dropped by decoders.
            (pre_skip + length, FLAG_EOS)
        } else {
            let end = page.last().map_or(0, |p| p.start + u64::from(p.samples));
            (end, 0)
        };
        let data: Vec<&[u8]> = page.iter().map(|p| p.data.as_slice()).collect();
        writer.page(&data, granule, flags)?;
    }
    Ok(writer.into_bytes())
}

/// The `OpusHead` of the stream (the encoder's own, rebuilt here once it is consumed).
fn encoder_head(channels: u16, pre_skip: u64) -> Vec<u8> {
    OpusHead {
        channels,
        pre_skip: u16::try_from(pre_skip).unwrap_or(u16::MAX),
        input_sample_rate: NARRATION_SAMPLE_RATE,
    }
    .to_bytes()
}

/// Packets grouped into pages: at most [`PACKETS_PER_PAGE`] and 255 lacing values each.
fn group_packets(packets: &[OpusPacket]) -> Vec<Vec<&OpusPacket>> {
    let mut pages = Vec::new();
    let mut page: Vec<&OpusPacket> = Vec::new();
    let mut segments = 0;
    for packet in packets {
        let needed = PageWriter::segments(packet.data.len());
        if !page.is_empty() && (page.len() == PACKETS_PER_PAGE || segments + needed > MAX_SEGMENTS)
        {
            pages.push(std::mem::take(&mut page));
            segments = 0;
        }
        page.push(packet);
        segments += needed;
    }
    if !page.is_empty() {
        pages.push(page);
    }
    pages
}

/// The `OpusTags` packet (RFC 7845 §5.2) of `narration`.
fn tags_packet(narration: &Narration) -> Vec<u8> {
    let (base, options) = match narration.base {
        NarrationBase::Replay => ("replay", narration.options),
        NarrationBase::Note => ("note", ReplayOptions::default()),
    };
    let max_pause = options.max_pause.map_or_else(
        || "none".to_string(),
        |pause| {
            let ms = (pause.as_nanos() + 500_000) / 1_000_000;
            u64::try_from(ms).unwrap_or(u64::MAX).to_string()
        },
    );
    opus_tags(&[
        &format!("{TAG_BASE}={base}"),
        // `{}` writes the shortest text that reads back as the same number.
        &format!("{TAG_SPEED}={}", options.speed),
        &format!("{TAG_MAX_PAUSE}={max_pause}"),
        &format!("{TAG_START}={}", narration.start_ms),
    ])
}

/// An `OpusTags` packet: vendor [`VENDOR`] and `comments`.
fn opus_tags(comments: &[&str]) -> Vec<u8> {
    let mut packet = OPUS_TAGS.to_vec();
    let string = |packet: &mut Vec<u8>, text: &str| {
        packet.extend_from_slice(&(text.len() as u32).to_le_bytes());
        packet.extend_from_slice(text.as_bytes());
    };
    string(&mut packet, VENDOR);
    packet.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for comment in comments {
        string(&mut packet, comment);
    }
    packet
}

/// Reads an `audio.opus` member. [`MediaError::InvalidFile`] when it is not Ogg Opus, or when
/// it is longer than 4 hours.
pub fn decode_narration(bytes: &[u8]) -> Result<Narration, MediaError> {
    let invalid = |what: &str| MediaError::InvalidFile(format!("narration: {what}"));
    let parsed = parse(bytes)?;
    let packets = &parsed.stream.packets;

    // Decode every audio packet, never past what the stream says it holds.
    let channels = usize::from(parsed.head.channels);
    let limit = (bytes.len() as u64)
        .saturating_mul(MAX_FRAMES_PER_INPUT_BYTE)
        .min(parsed.end.saturating_add(MAX_PACKET_FRAMES));
    let mut decoder = OpusPacketDecoder::new(parsed.head.channels)?;
    let mut samples: Vec<i16> = Vec::new();
    for index in 2..packets.len() {
        let decoded = decoder.decode_samples(packets.packet(index).unwrap_or_default())?;
        samples.extend_from_slice(decoded);
        if (samples.len() / channels) as u64 > limit {
            return Err(invalid("more sound than the file can hold"));
        }
    }
    let total = (samples.len() / channels) as u64;
    let end = parsed.end.min(total);
    let start = u64::from(parsed.head.pre_skip).min(end);
    // Both at most `total`, which fits in memory.
    samples.truncate(end as usize * channels);
    samples.drain(..start as usize * channels);
    if parsed.gain != 0 {
        apply_gain(&mut samples, parsed.gain);
    }

    let tags = parsed.tags;
    Ok(Narration {
        clip: AudioClip {
            channels: parsed.head.channels,
            samples,
        },
        base: tags.base,
        options: tags.options,
        start_ms: tags.start_ms,
    })
}

/// What [`narration_header`] reads of an `audio.opus`: its time base and its length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NarrationHeader {
    /// 1 or 2.
    pub channels: u16,
    pub base: NarrationBase,
    pub options: ReplayOptions,
    pub start_ms: i64,
    /// Length given by the granule positions: what [`decode_narration`] returns for an intact
    /// stream.
    pub duration: Duration,
}

/// The headers of an `audio.opus` member and its length, read without decoding the audio: the
/// checks of [`decode_narration`] but those of the audio packets themselves.
pub fn narration_header(bytes: &[u8]) -> Result<NarrationHeader, MediaError> {
    let parsed = parse(bytes)?;
    let frames = parsed.end.saturating_sub(u64::from(parsed.head.pre_skip));
    Ok(NarrationHeader {
        channels: parsed.head.channels,
        base: parsed.tags.base,
        options: parsed.tags.options,
        start_ms: parsed.tags.start_ms,
        duration: samples_to_duration(frames),
    })
}

/// An `audio.opus` member read up to its audio packets.
struct Parsed {
    stream: OpusStream,
    head: OpusHead,
    /// Output gain of `OpusHead` (Q7.8 dB).
    gain: i16,
    tags: Tags,
    /// Samples per channel from the start of the stream (pre-skip included) to the end
    /// trimming: the last granule position less the origin.
    end: u64,
}

/// Checks the layout of an `audio.opus` member (RFC 7845 §3) and reads its headers and the
/// length its granule positions give; refuses a stream longer than 4 hours.
fn parse(bytes: &[u8]) -> Result<Parsed, MediaError> {
    let invalid = |what: &str| MediaError::InvalidFile(format!("narration: {what}"));
    let stream = read_stream(bytes)?;
    let packets = &stream.packets;

    // Identification header, alone on the first page (RFC 7845 §3).
    let head_bytes = packets.packet(0).unwrap_or_default();
    let head = OpusHead::parse(head_bytes)?;
    if head_bytes.get(HEAD_MAPPING) != Some(&0) || head.channels > 2 {
        return Err(invalid(
            "only mono and stereo Opus (channel mapping family 0) is read",
        ));
    }
    let gain = match head_bytes.get(HEAD_GAIN..HEAD_GAIN + 2) {
        Some(&[low, high]) => i16::from_le_bytes([low, high]),
        _ => 0,
    };
    if stream.pages.first().map(|p| p.packets) != Some(1) {
        return Err(invalid("OpusHead is not alone on the first page"));
    }
    // Comment header, ending its page(s): audio starts on a fresh page.
    let tags = packets
        .packet(1)
        .filter(|p| p.starts_with(OPUS_TAGS))
        .ok_or_else(|| invalid("missing OpusTags"))?;
    let audio_pages = stream.pages.get(1..).unwrap_or_default();
    if audio_pages.first().map(|p| p.packets) != Some(2) {
        return Err(invalid("audio does not start on a page of its own"));
    }
    let audio_pages = audio_pages.get(1..).unwrap_or_default();
    let (Some(first_page), Some(last_page)) = (audio_pages.first(), audio_pages.last()) else {
        return Err(invalid("no audio"));
    };
    if first_page.granule == NO_GRANULE || last_page.granule == NO_GRANULE {
        return Err(invalid("audio page without a granule position"));
    }

    // Where the first sample sits on the granule timeline (RFC 7845 §4.5): normally 0. The
    // durations of the first page's packets come from their table of contents, undecoded.
    let mut first_page_frames = 0u64;
    for index in 2..first_page.packets {
        let packet = packets.packet(index).unwrap_or_default();
        first_page_frames += packet_frames(packet).ok_or_else(|| invalid("bad Opus packet"))?;
    }
    let origin = match first_page.granule.checked_sub(first_page_frames) {
        Some(origin) => origin,
        // Fewer samples than in its packets: only the end trimming of a one-page stream.
        None if first_page.eos => 0,
        None => return Err(invalid("first audio granule position too small")),
    };
    let end = last_page.granule.saturating_sub(origin);
    if end.saturating_sub(u64::from(head.pre_skip)) > MAX_STREAM_FRAMES {
        return Err(invalid("longer than 4 hours"));
    }
    let tags = parse_tags(tags);
    Ok(Parsed {
        stream,
        head,
        gain,
        tags,
        end,
    })
}

/// Samples per channel at 48 kHz in an Opus packet, from its table of contents (RFC 6716 §3.1:
/// the frame size of its configuration times its frame count); `None` for an empty packet, a
/// code 3 packet without its frame count, or one holding no sound or more than 120 ms.
fn packet_frames(packet: &[u8]) -> Option<u64> {
    let toc = *packet.first()?;
    let config = usize::from(toc >> 3);
    let frame = match config {
        // SILK only (narrow-, medium- and wideband): 10, 20, 40 and 60 ms.
        0..=11 => [480, 960, 1920, 2880][config % 4],
        // Hybrid (super-wide- and fullband): 10 and 20 ms.
        12..=15 => [480, 960][config % 2],
        // CELT only: 2.5, 5, 10 and 20 ms.
        _ => [120, 240, 480, 960][config % 4],
    };
    let count = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        // Code 3: the frame count is in the low 6 bits of the second byte.
        _ => u64::from(*packet.get(1)? & 0x3F),
    };
    let frames = frame * count;
    (1..=MAX_PACKET_FRAMES).contains(&frames).then_some(frames)
}

/// A page of the Opus stream on which packets end.
#[derive(Debug, Clone, Copy)]
struct PageMark {
    /// Packets completed from the start of the stream up to the end of this page.
    packets: usize,
    granule: u64,
    eos: bool,
}

/// The packets of the Opus stream of an Ogg file.
struct OpusStream {
    packets: PacketReader,
    pages: Vec<PageMark>,
}

/// Walks the pages: the first logical stream whose first packet is `OpusHead`, up to its
/// end-of-stream page; other streams are skipped.
fn read_stream(bytes: &[u8]) -> Result<OpusStream, MediaError> {
    let invalid = |what: &str| MediaError::InvalidFile(format!("narration: {what}"));
    let mut serial = None;
    let mut packets = PacketReader::default();
    let mut pages = Vec::new();
    for page in ogg::pages(bytes) {
        let page = page?;
        match serial {
            None if page.has(FLAG_BOS) && page.body.starts_with(b"OpusHead") => {
                serial = Some(page.serial);
            }
            Some(serial) if serial == page.serial => {}
            _ => continue,
        }
        let completed = packets.push(&page)?;
        let eos = page.has(FLAG_EOS);
        if completed > 0 {
            pages.push(PageMark {
                packets: packets.len(),
                granule: page.granule,
                eos,
            });
        }
        if eos {
            if packets.has_partial() {
                return Err(invalid("unfinished packet at the end of the stream"));
            }
            return Ok(OpusStream { packets, pages });
        }
    }
    Err(match serial {
        None => invalid("not an Ogg Opus stream"),
        Some(_) => invalid("truncated (no end-of-stream page)"),
    })
}

/// Applies the `OpusHead` output gain (Q7.8 dB, RFC 7845 §5.1).
fn apply_gain(samples: &mut [i16], gain: i16) {
    let factor = 10f64.powf(f64::from(gain) / (20.0 * 256.0));
    for sample in samples {
        *sample = (f64::from(*sample) * factor)
            .round()
            .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
    }
}

/// The time base read from `OpusTags`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tags {
    base: NarrationBase,
    options: ReplayOptions,
    start_ms: i64,
}

/// Reads the Ardoise comments of an `OpusTags` packet; whatever is missing or unreadable
/// takes its default. Lengths are checked against the packet before anything is read.
fn parse_tags(packet: &[u8]) -> Tags {
    let mut base = None;
    let mut speed = None;
    let mut max_pause = None;
    let mut start_ms = None;
    for comment in comments(packet) {
        let Some((key, value)) = comment.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if key.eq_ignore_ascii_case(TAG_BASE) && base.is_none() {
            base = if value.eq_ignore_ascii_case("note") {
                Some(NarrationBase::Note)
            } else {
                Some(NarrationBase::Replay)
            };
        } else if key.eq_ignore_ascii_case(TAG_SPEED) && speed.is_none() {
            speed = Some(
                value
                    .parse::<f64>()
                    .ok()
                    .filter(|s| (MIN_SPEED..=MAX_SPEED).contains(s))
                    .unwrap_or(1.0),
            );
        } else if key.eq_ignore_ascii_case(TAG_MAX_PAUSE) && max_pause.is_none() {
            max_pause = Some(value.parse::<u64>().ok().map(Duration::from_millis));
        } else if key.eq_ignore_ascii_case(TAG_START) && start_ms.is_none() {
            start_ms = value.parse::<i64>().ok();
        }
    }
    let base = base.unwrap_or(NarrationBase::Replay);
    let options = match base {
        NarrationBase::Replay => ReplayOptions {
            speed: speed.unwrap_or(1.0),
            max_pause: max_pause.flatten(),
        },
        NarrationBase::Note => ReplayOptions::default(),
    };
    Tags {
        base,
        options,
        start_ms: start_ms.unwrap_or(0),
    }
}

/// The user comments of an `OpusTags` packet that are UTF-8, in order, up to the first one
/// that does not fit in the packet.
fn comments(packet: &[u8]) -> Vec<&str> {
    fn take<'a>(rest: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
        let (taken, after) = rest.split_at_checked(len)?;
        *rest = after;
        Some(taken)
    }
    fn length(rest: &mut &[u8]) -> Option<usize> {
        let bytes = take(rest, 4)?;
        usize::try_from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).ok()
    }
    let mut found = Vec::new();
    let mut rest = packet.get(OPUS_TAGS.len()..).unwrap_or_default();
    let Some(vendor) = length(&mut rest) else {
        return found;
    };
    if take(&mut rest, vendor).is_none() {
        return found;
    }
    let Some(count) = length(&mut rest) else {
        return found;
    };
    // Each comment takes at least its 4-byte length: the count is never trusted further.
    for _ in 0..count {
        let Some(comment) = length(&mut rest).and_then(|len| take(&mut rest, len)) else {
            break;
        };
        if let Ok(text) = std::str::from_utf8(comment) {
            found.push(text);
        }
    }
    found
}

// --- WAV ---

/// Length of the header of [`wav_bytes`].
const WAV_HEADER_LEN: usize = 44;

/// `clip` as a 16-bit PCM WAV file (the editor plays it during the replay).
pub fn wav_bytes(clip: &AudioClip) -> Vec<u8> {
    let channels = clip.channels.max(1);
    let block_align = u32::from(channels) * 2;
    // A RIFF size is 32 bits: a clip too long for it is cut to whole frames that fit.
    let max_data = (u32::MAX - 36) / block_align * block_align;
    let whole = clip.samples.len() / usize::from(channels) * usize::from(channels);
    let samples = &clip.samples[..whole.min(max_data as usize / 2)];
    let data_len = (samples.len() * 2) as u32;

    let mut wav = Vec::with_capacity(WAV_HEADER_LEN + samples.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&NARRATION_SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(NARRATION_SAMPLE_RATE * block_align).to_le_bytes());
    wav.extend_from_slice(&(block_align as u16).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests;
