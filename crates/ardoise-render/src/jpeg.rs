//! A baseline JPEG encoder, written from the standard (ITU-T T.81 | ISO/IEC 10918-1):
//! 8-bit samples, sequential DCT, the example quantisation and Huffman tables of Annex K,
//! a JFIF header (sRGB-like YCbCr, with the resolution). The JPEG encoders published on
//! crates.io port libjpeg's integer DCT and carry its IJG licence; this one computes the DCT
//! from its definition (T.81 A.3.3), which is fast enough for exports.
//!
//! Chroma is subsampled 2 × 2 (4:2:0) below quality 90 and kept at full resolution (4:4:4)
//! from 90 on, where coloured ink edges stay sharp.

use crate::{Image, RenderError};

/// Quality from which chroma is not subsampled.
const FULL_CHROMA_QUALITY: u8 = 90;

/// Natural (row-major) index of each coefficient in zig-zag order (T.81 figure A.6).
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// Luminance quantisation table, natural order (T.81 table K.1).
const LUMA_QUANT: [u8; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, //
    12, 12, 14, 19, 26, 58, 60, 55, //
    14, 13, 16, 24, 40, 57, 69, 56, //
    14, 17, 22, 29, 51, 87, 80, 62, //
    18, 22, 37, 56, 68, 109, 103, 77, //
    24, 35, 55, 64, 81, 104, 113, 92, //
    49, 64, 78, 87, 103, 121, 120, 101, //
    72, 92, 95, 98, 112, 100, 103, 99,
];

/// Chrominance quantisation table, natural order (T.81 table K.2).
const CHROMA_QUANT: [u8; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, //
    18, 21, 26, 66, 99, 99, 99, 99, //
    24, 26, 56, 99, 99, 99, 99, 99, //
    47, 66, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99,
];

/// A Huffman table as stored in a DHT segment: the number of codes of each length 1..=16,
/// then the symbols in code order (T.81 B.2.4.2).
struct HuffmanSpec {
    counts: [u8; 16],
    symbols: &'static [u8],
}

/// T.81 table K.3.
const DC_LUMA: HuffmanSpec = HuffmanSpec {
    counts: [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
    symbols: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

/// T.81 table K.4.
const DC_CHROMA: HuffmanSpec = HuffmanSpec {
    counts: [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
    symbols: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

/// T.81 table K.5.
const AC_LUMA: HuffmanSpec = HuffmanSpec {
    counts: [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d],
    symbols: &[
        0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61,
        0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52,
        0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25,
        0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45,
        0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64,
        0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83,
        0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99,
        0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6,
        0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3,
        0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8,
        0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
    ],
};

/// T.81 table K.6.
const AC_CHROMA: HuffmanSpec = HuffmanSpec {
    counts: [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77],
    symbols: &[
        0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61,
        0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33,
        0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18,
        0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44,
        0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63,
        0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a,
        0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97,
        0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4,
        0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca,
        0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7,
        0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
    ],
};

/// Code and length of each symbol (T.81 annex C).
struct HuffmanCodes {
    codes: [(u16, u8); 256],
}

impl HuffmanCodes {
    fn new(spec: &HuffmanSpec) -> Self {
        let mut codes = [(0u16, 0u8); 256];
        let mut code = 0u32;
        let mut symbols = spec.symbols.iter();
        for (index, &count) in spec.counts.iter().enumerate() {
            let length = index as u8 + 1;
            for _ in 0..count {
                if let Some(&symbol) = symbols.next() {
                    codes[usize::from(symbol)] = (code as u16, length);
                }
                code += 1;
            }
            code <<= 1;
        }
        Self { codes }
    }

    fn put(&self, out: &mut BitWriter, symbol: u8) {
        let (code, length) = self.codes[usize::from(symbol)];
        out.put(u32::from(code), u32::from(length));
    }
}

/// Entropy-coded data, with a 0x00 stuffed after every 0xFF byte (T.81 F.1.2.3).
struct BitWriter {
    out: Vec<u8>,
    bits: u32,
    count: u32,
}

impl BitWriter {
    /// Appends the low `length` (at most 16) bits of `value`, most significant first.
    fn put(&mut self, value: u32, length: u32) {
        if length == 0 {
            return;
        }
        self.bits = (self.bits << length) | (value & ((1 << length) - 1));
        self.count += length;
        while self.count >= 8 {
            self.count -= 8;
            let byte = (self.bits >> self.count) as u8;
            self.out.push(byte);
            if byte == 0xff {
                self.out.push(0);
            }
        }
        self.bits &= (1 << self.count) - 1;
    }

    /// Pads the last byte with 1 bits.
    fn finish(mut self) -> Vec<u8> {
        let pad = (8 - self.count % 8) % 8;
        self.put((1 << pad) - 1, pad);
        self.out
    }
}

/// The quantisation table for `quality` (1..=100): the Annex K table as printed at 50, scaled
/// by 50 / quality below and by (100 − quality) / 50 above, each entry kept in 1..=255.
fn quant_table(base: &[u8; 64], quality: u8) -> [u8; 64] {
    let q = u32::from(quality.clamp(1, 100));
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    base.map(|b| ((u32::from(b) * scale + 50) / 100).clamp(1, 255) as u8)
}

/// Basis of the 8-point DCT-II: `m[u][x] = C(u)/2 · cos((2x + 1)uπ/16)`, so that the 2-D
/// transform of T.81 A.3.3 is `M · f · Mᵀ`.
fn dct_matrix() -> [[f32; 8]; 8] {
    let mut m = [[0f32; 8]; 8];
    for (u, row) in m.iter_mut().enumerate() {
        let c = if u == 0 {
            std::f64::consts::FRAC_1_SQRT_2
        } else {
            1.0
        };
        for (x, value) in row.iter_mut().enumerate() {
            let angle = (2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0;
            *value = (c / 2.0 * angle.cos()) as f32;
        }
    }
    m
}

/// One component's coding state.
struct Component<'a> {
    quant: [f32; 64],
    dc: &'a HuffmanCodes,
    ac: &'a HuffmanCodes,
    previous_dc: i32,
}

/// Number of bits of the magnitude of `v` (its category, T.81 F.1.2.1).
fn category(v: i32) -> u32 {
    32 - v.unsigned_abs().leading_zeros()
}

/// The `size` low bits that follow a category: `v` itself, or `v − 1` for negative values.
fn magnitude_bits(v: i32, size: u32) -> u32 {
    if v < 0 {
        (v - 1) as u32 & ((1 << size) - 1)
    } else {
        v as u32
    }
}

struct Encoder {
    dct: [[f32; 8]; 8],
    out: BitWriter,
}

impl Encoder {
    /// Transforms, quantises and codes one block of level-shifted samples (row-major).
    fn block(&mut self, samples: &[f32; 64], component: &mut Component) {
        let m = &self.dct;
        // Rows: rows[y][u] = Σx m[u][x]·f[y][x]; then columns: F[v][u] = Σy m[v][y]·rows[y][u].
        let mut rows = [[0f32; 8]; 8];
        for (row, line) in rows.iter_mut().zip(samples.as_chunks::<8>().0) {
            for (out, basis) in row.iter_mut().zip(m) {
                *out = basis.iter().zip(line).map(|(a, b)| a * b).sum();
            }
        }
        let mut coefficients = [0i32; 64];
        for (i, coefficient) in coefficients.iter_mut().enumerate() {
            let (v, u) = (i / 8, i % 8);
            let f: f32 = m[v].iter().zip(&rows).map(|(a, row)| a * row[u]).sum();
            *coefficient = (f / component.quant[i]).round() as i32;
        }
        // DC: difference with the previous block of the component (at most 11 bits).
        let dc = coefficients[0].clamp(-2047, 2047);
        let diff = (dc - component.previous_dc).clamp(-2047, 2047);
        component.previous_dc = dc;
        let size = category(diff);
        component.dc.put(&mut self.out, size as u8);
        self.out.put(magnitude_bits(diff, size), size);
        // AC: runs of zeros and values (at most 10 bits) in zig-zag order.
        let mut run = 0u32;
        for &index in &ZIGZAG[1..] {
            let v = coefficients[index].clamp(-1023, 1023);
            if v == 0 {
                run += 1;
                continue;
            }
            while run > 15 {
                component.ac.put(&mut self.out, 0xf0);
                run -= 16;
            }
            let size = category(v);
            component.ac.put(&mut self.out, ((run << 4) | size) as u8);
            self.out.put(magnitude_bits(v, size), size);
            run = 0;
        }
        if run > 0 {
            component.ac.put(&mut self.out, 0x00);
        }
    }
}

/// JFIF YCbCr of `rgb` (T.871), not yet level-shifted.
fn ycbcr(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb;
    [
        0.299 * r + 0.587 * g + 0.114 * b,
        -0.168_736 * r - 0.331_264 * g + 0.5 * b + 128.0,
        0.5 * r - 0.418_688 * g - 0.081_312 * b + 128.0,
    ]
}

fn segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) -> Result<(), RenderError> {
    let length = u16::try_from(payload.len() + 2)
        .map_err(|_| RenderError::Encode("JPEG segment too long".into()))?;
    out.extend_from_slice(&[0xff, marker]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(())
}

fn huffman_segment(class_and_id: u8, spec: &HuffmanSpec) -> Vec<u8> {
    let mut payload = vec![class_and_id];
    payload.extend_from_slice(&spec.counts);
    payload.extend_from_slice(spec.symbols);
    payload
}

/// Encodes `rgb(x, y)` (each channel 0..=255) as a baseline JPEG of `width × height` pixels.
/// `dpi` goes into the JFIF header (`None`: square pixels, no resolution).
pub(crate) fn encode(
    width: u32,
    height: u32,
    quality: u8,
    dpi: Option<u32>,
    rgb: impl Fn(u32, u32) -> [u8; 3],
) -> Result<Vec<u8>, RenderError> {
    let size_error = RenderError::Size { width, height };
    let (Ok(w16), Ok(h16)) = (u16::try_from(width), u16::try_from(height)) else {
        return Err(size_error);
    };
    if width == 0 || height == 0 {
        return Err(size_error);
    }
    let quality = quality.clamp(1, 100);
    let subsample = quality < FULL_CHROMA_QUALITY;
    let luma_quant = quant_table(&LUMA_QUANT, quality);
    let chroma_quant = quant_table(&CHROMA_QUANT, quality);

    let mut out = Vec::new();
    out.extend_from_slice(&[0xff, 0xd8]);
    // JFIF 1.01; units 1 = dots per inch, 0 = aspect ratio only.
    let (units, density) = match dpi.and_then(|d| u16::try_from(d).ok()).filter(|&d| d > 0) {
        Some(d) => (1u8, d),
        None => (0u8, 1u16),
    };
    let mut app0 = b"JFIF\0\x01\x01".to_vec();
    app0.push(units);
    app0.extend_from_slice(&density.to_be_bytes());
    app0.extend_from_slice(&density.to_be_bytes());
    app0.extend_from_slice(&[0, 0]);
    segment(&mut out, 0xe0, &app0)?;
    // Quantisation tables 0 (luma) and 1 (chroma), 8-bit, in zig-zag order.
    let mut dqt = Vec::with_capacity(130);
    for (id, table) in [(0u8, &luma_quant), (1, &chroma_quant)] {
        dqt.push(id);
        dqt.extend(ZIGZAG.iter().map(|&i| table[i]));
    }
    segment(&mut out, 0xdb, &dqt)?;
    // Baseline frame: 8-bit precision, three components (Y, Cb, Cr).
    let luma_sampling = if subsample { 0x22 } else { 0x11 };
    let mut sof = vec![8];
    sof.extend_from_slice(&h16.to_be_bytes());
    sof.extend_from_slice(&w16.to_be_bytes());
    sof.extend_from_slice(&[3, 1, luma_sampling, 0, 2, 0x11, 1, 3, 0x11, 1]);
    segment(&mut out, 0xc0, &sof)?;
    let mut dht = Vec::new();
    for (class_and_id, spec) in [
        (0x00, &DC_LUMA),
        (0x10, &AC_LUMA),
        (0x01, &DC_CHROMA),
        (0x11, &AC_CHROMA),
    ] {
        dht.extend(huffman_segment(class_and_id, spec));
    }
    segment(&mut out, 0xc4, &dht)?;
    // One interleaved scan of all coefficients.
    segment(&mut out, 0xda, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0])?;

    let codes = [
        HuffmanCodes::new(&DC_LUMA),
        HuffmanCodes::new(&AC_LUMA),
        HuffmanCodes::new(&DC_CHROMA),
        HuffmanCodes::new(&AC_CHROMA),
    ];
    let component = |quant: &[u8; 64], dc, ac| Component {
        quant: quant.map(f32::from),
        dc,
        ac,
        previous_dc: 0,
    };
    let mut y = component(&luma_quant, &codes[0], &codes[1]);
    let mut cb = component(&chroma_quant, &codes[2], &codes[3]);
    let mut cr = component(&chroma_quant, &codes[2], &codes[3]);
    let mut encoder = Encoder {
        dct: dct_matrix(),
        out: BitWriter {
            out: Vec::new(),
            bits: 0,
            count: 0,
        },
    };

    // Pixels past the right and bottom edges repeat the last column and row.
    let (max_x, max_y) = (width - 1, height - 1);
    let mcu = if subsample { 16 } else { 8 };
    let mut ycc = vec![[0f32; 3]; mcu * mcu];
    for my in 0..height.div_ceil(mcu as u32) {
        for mx in 0..width.div_ceil(mcu as u32) {
            for (i, sample) in ycc.iter_mut().enumerate() {
                let x = (mx * mcu as u32 + (i % mcu) as u32).min(max_x);
                let y = (my * mcu as u32 + (i / mcu) as u32).min(max_y);
                *sample = ycbcr(rgb(x, y).map(f32::from));
            }
            let mut block = [0f32; 64];
            // Luma: one block per 8 × 8 pixels, left to right then top to bottom.
            for by in 0..mcu / 8 {
                for bx in 0..mcu / 8 {
                    for (i, value) in block.iter_mut().enumerate() {
                        let (px, py) = (bx * 8 + i % 8, by * 8 + i / 8);
                        *value = ycc[py * mcu + px][0] - 128.0;
                    }
                    encoder.block(&block, &mut y);
                }
            }
            // Chroma: one block per MCU, each sample the mean of 2 × 2 pixels if subsampled.
            for (channel, component) in [(1, &mut cb), (2, &mut cr)] {
                for (i, value) in block.iter_mut().enumerate() {
                    let (bx, by) = (i % 8, i / 8);
                    *value = if subsample {
                        let at = |dx: usize, dy: usize| ycc[(by * 2 + dy) * mcu + bx * 2 + dx];
                        (at(0, 0)[channel]
                            + at(1, 0)[channel]
                            + at(0, 1)[channel]
                            + at(1, 1)[channel])
                            / 4.0
                            - 128.0
                    } else {
                        ycc[by * mcu + bx][channel] - 128.0
                    };
                }
                encoder.block(&block, component);
            }
        }
    }
    out.extend(encoder.out.finish());
    out.extend_from_slice(&[0xff, 0xd9]);
    Ok(out)
}

/// `image` as a baseline JPEG, alpha flattened over white.
pub(crate) fn encode_image(
    image: &Image,
    quality: u8,
    dpi: Option<u32>,
) -> Result<Vec<u8>, RenderError> {
    crate::check_pixels(image)?;
    let width = image.width as usize;
    encode(image.width, image.height, quality, dpi, |x, y| {
        let i = (y as usize * width + x as usize) * 4;
        match image.pixels.get(i..i + 4) {
            Some(&[r, g, b, a]) => crate::over_white([r, g, b, a]),
            _ => [255, 255, 255],
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huffman_tables_hold_every_symbol_once() {
        for spec in [&DC_LUMA, &DC_CHROMA] {
            let total: usize = spec.counts.iter().map(|&c| usize::from(c)).sum();
            assert_eq!(total, spec.symbols.len());
            assert_eq!(spec.symbols, (0..12).collect::<Vec<u8>>().as_slice());
        }
        // Every AC symbol of a baseline scan: end of block, sixteen zeros, run/size pairs.
        let mut all: Vec<u8> = vec![0x00, 0xf0];
        for run in 0..16u8 {
            for size in 1..=10u8 {
                all.push((run << 4) | size);
            }
        }
        all.sort_unstable();
        for spec in [&AC_LUMA, &AC_CHROMA] {
            let total: usize = spec.counts.iter().map(|&c| usize::from(c)).sum();
            assert_eq!(total, spec.symbols.len());
            let mut symbols = spec.symbols.to_vec();
            symbols.sort_unstable();
            assert_eq!(symbols, all);
        }
    }

    #[test]
    fn huffman_codes_are_prefix_free() {
        for spec in [&DC_LUMA, &AC_LUMA, &DC_CHROMA, &AC_CHROMA] {
            let codes = HuffmanCodes::new(spec);
            let used: Vec<(u16, u8)> = spec
                .symbols
                .iter()
                .map(|&s| codes.codes[usize::from(s)])
                .collect();
            for (i, &(a, la)) in used.iter().enumerate() {
                assert!((1..=16).contains(&la));
                // No code is all ones (T.81 C: reserved for padding).
                assert_ne!(u32::from(a), (1u32 << la) - 1);
                for &(b, lb) in &used[i + 1..] {
                    let (short, long, ls, ll) = if la <= lb {
                        (a, b, la, lb)
                    } else {
                        (b, a, lb, la)
                    };
                    assert_ne!(u32::from(long) >> (ll - ls), u32::from(short), "prefix");
                }
            }
        }
    }

    #[test]
    fn categories_and_magnitudes() {
        assert_eq!(category(0), 0);
        assert_eq!(category(1), 1);
        assert_eq!(category(-1), 1);
        assert_eq!(category(3), 2);
        assert_eq!(category(-1023), 10);
        assert_eq!(category(2047), 11);
        assert_eq!(magnitude_bits(-1, 1), 0);
        assert_eq!(magnitude_bits(-3, 2), 0);
        assert_eq!(magnitude_bits(-2, 2), 1);
        assert_eq!(magnitude_bits(2, 2), 2);
        assert_eq!(magnitude_bits(3, 2), 3);
    }

    #[test]
    fn quality_scales_the_tables() {
        assert_eq!(quant_table(&LUMA_QUANT, 50), LUMA_QUANT);
        assert!(quant_table(&LUMA_QUANT, 100).iter().all(|&q| q == 1));
        assert!(quant_table(&LUMA_QUANT, 1).iter().all(|&q| q == 255));
        assert_eq!(quant_table(&LUMA_QUANT, 75)[0], 8);
    }

    #[test]
    fn bits_are_stuffed_and_padded() {
        let mut out = BitWriter {
            out: Vec::new(),
            bits: 0,
            count: 0,
        };
        out.put(0xff, 8);
        out.put(0b101, 3);
        assert_eq!(out.finish(), [0xff, 0x00, 0b1011_1111]);
    }

    #[test]
    fn the_dct_is_orthonormal_and_a_flat_block_has_only_a_dc() {
        let m = dct_matrix();
        for u in 0..8 {
            for v in 0..8 {
                let dot: f32 = (0..8).map(|x| m[u][x] * m[v][x]).sum();
                let expected = if u == v { 1.0 } else { 0.0 };
                assert!((dot - expected).abs() < 1e-5, "{u} {v} {dot}");
            }
        }
        // A flat block of 100 (level-shifted) has F(0,0) = 8 × 100 and nothing else.
        let mut encoder = Encoder {
            dct: m,
            out: BitWriter {
                out: Vec::new(),
                bits: 0,
                count: 0,
            },
        };
        let (dc, ac) = (HuffmanCodes::new(&DC_LUMA), HuffmanCodes::new(&AC_LUMA));
        let mut component = Component {
            quant: [1.0; 64],
            dc: &dc,
            ac: &ac,
            previous_dc: 0,
        };
        encoder.block(&[100.0; 64], &mut component);
        assert_eq!(component.previous_dc, 800);
        // Category 10 (code 11111110 in table K.3), 800 on 10 bits (1100100000), end of block
        // (1010 in table K.5), then two padding ones.
        assert_eq!(
            encoder.out.finish(),
            [0b1111_1110, 0b1100_1000, 0b0010_1011]
        );
    }

    #[test]
    fn markers_are_in_order() {
        let jpeg = encode(10, 3, 80, Some(150), |x, _| [x as u8 * 20, 0, 255]).unwrap();
        assert_eq!(&jpeg[..2], [0xff, 0xd8]);
        assert_eq!(&jpeg[jpeg.len() - 2..], [0xff, 0xd9]);
        assert_eq!(&jpeg[2..4], [0xff, 0xe0]);
        assert_eq!(&jpeg[6..11], b"JFIF\0");
        // Dots per inch, 150 × 150.
        assert_eq!(jpeg[13], 1);
        assert_eq!(&jpeg[14..18], [0, 150, 0, 150]);
        assert!(matches!(
            encode(0, 3, 80, None, |_, _| [0; 3]),
            Err(RenderError::Size { .. })
        ));
        assert!(matches!(
            encode(70_000, 3, 80, None, |_, _| [0; 3]),
            Err(RenderError::Size { .. })
        ));
    }
}
