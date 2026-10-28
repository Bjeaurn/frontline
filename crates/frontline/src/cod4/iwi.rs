//! IWI v6, Call of Duty 4's image container: a 28-byte header followed by the mip chain from
//! the largest level down. Only the largest level is decoded, to 8-bit RGBA.
//!
//! Header (little endian): `"IWi"`, version (6), format, flags, width, height and depth as
//! `u16`s, then four `u32` file sizes (the whole file, and with the 1-3 largest mips dropped).
//! Pure functions over bytes: nothing here touches the disk or the renderer.

use std::fmt;

pub const HEADER_LEN: usize = 28;
const MAGIC: &[u8; 3] = b"IWi";
const VERSION: u8 = 6;

/// Pixel formats of IWI v6 this decoder understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IwiFormat {
    /// 32-bit BGRA.
    Argb32,
    /// 24-bit BGR.
    Rgb24,
    /// 16-bit grey + alpha.
    Ga16,
    /// 8-bit alpha only.
    A8,
    Dxt1,
    Dxt3,
    Dxt5,
}

impl IwiFormat {
    fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0x01 => Self::Argb32,
            0x02 => Self::Rgb24,
            0x03 => Self::Ga16,
            0x04 => Self::A8,
            0x0b => Self::Dxt1,
            0x0c => Self::Dxt3,
            0x0d => Self::Dxt5,
            _ => return None,
        })
    }

    /// Bytes of the largest mip level of a `width` x `height` image.
    fn level_len(self, width: usize, height: usize) -> usize {
        let blocks = width.div_ceil(4) * height.div_ceil(4);
        match self {
            Self::Argb32 => width * height * 4,
            Self::Rgb24 => width * height * 3,
            Self::Ga16 => width * height * 2,
            Self::A8 => width * height,
            Self::Dxt1 => blocks * 8,
            Self::Dxt3 | Self::Dxt5 => blocks * 16,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IwiHeader {
    pub format: IwiFormat,
    pub flags: u8,
    pub width: u32,
    pub height: u32,
    /// File sizes with 0..=3 of the largest mips dropped (index 0 is the whole file).
    pub file_sizes: [u32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IwiError {
    TooShort,
    BadMagic,
    UnsupportedVersion(u8),
    UnsupportedFormat(u8),
    EmptyImage,
    /// The pixel data ends before the largest mip does.
    Truncated,
}

impl fmt::Display for IwiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "shorter than an IWI header"),
            Self::BadMagic => write!(f, "not an IWI file"),
            Self::UnsupportedVersion(version) => write!(f, "IWI version {version} (need 6)"),
            Self::UnsupportedFormat(format) => write!(f, "IWI format 0x{format:02x} unsupported"),
            Self::EmptyImage => write!(f, "zero-sized image"),
            Self::Truncated => write!(f, "pixel data cut short"),
        }
    }
}

pub fn parse_header(bytes: &[u8]) -> Result<IwiHeader, IwiError> {
    if bytes.len() < HEADER_LEN {
        return Err(IwiError::TooShort);
    }
    if &bytes[..3] != MAGIC {
        return Err(IwiError::BadMagic);
    }
    if bytes[3] != VERSION {
        return Err(IwiError::UnsupportedVersion(bytes[3]));
    }
    let format = IwiFormat::from_byte(bytes[4]).ok_or(IwiError::UnsupportedFormat(bytes[4]))?;
    let u16_at = |at: usize| u32::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]));
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    let (width, height) = (u16_at(6), u16_at(8));
    if width == 0 || height == 0 {
        return Err(IwiError::EmptyImage);
    }
    Ok(IwiHeader {
        format,
        flags: bytes[5],
        width,
        height,
        file_sizes: [u32_at(12), u32_at(16), u32_at(20), u32_at(24)],
    })
}

/// A decoded image: `width * height` RGBA8 pixels, row-major from the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Decodes the largest mip level.
pub fn decode(bytes: &[u8]) -> Result<Rgba8, IwiError> {
    let header = parse_header(bytes)?;
    let (width, height) = (header.width as usize, header.height as usize);
    let body = &bytes[HEADER_LEN..];
    let body = body
        .get(..header.format.level_len(width, height))
        .ok_or(IwiError::Truncated)?;
    let mut data = vec![0u8; width * height * 4];
    match header.format {
        IwiFormat::Argb32 => {
            let (texels, _) = body.as_chunks::<4>();
            for (out, src) in data.as_chunks_mut::<4>().0.iter_mut().zip(texels) {
                *out = [src[2], src[1], src[0], src[3]];
            }
        }
        IwiFormat::Rgb24 => {
            let (texels, _) = body.as_chunks::<3>();
            for (out, src) in data.as_chunks_mut::<4>().0.iter_mut().zip(texels) {
                *out = [src[2], src[1], src[0], 255];
            }
        }
        IwiFormat::Ga16 => {
            let (texels, _) = body.as_chunks::<2>();
            for (out, src) in data.as_chunks_mut::<4>().0.iter_mut().zip(texels) {
                *out = [src[0], src[0], src[0], src[1]];
            }
        }
        IwiFormat::A8 => {
            for (out, &alpha) in data.as_chunks_mut::<4>().0.iter_mut().zip(body) {
                *out = [255, 255, 255, alpha];
            }
        }
        IwiFormat::Dxt1 | IwiFormat::Dxt3 | IwiFormat::Dxt5 => {
            decode_blocks(header.format, body, width, height, &mut data);
        }
    }
    Ok(Rgba8 {
        width: header.width,
        height: header.height,
        data,
    })
}

fn decode_blocks(format: IwiFormat, body: &[u8], width: usize, height: usize, out: &mut [u8]) {
    let block_size = if format == IwiFormat::Dxt1 { 8 } else { 16 };
    let blocks_per_row = width.div_ceil(4);
    for (index, block) in body.chunks_exact(block_size).enumerate() {
        let (block_x, block_y) = ((index % blocks_per_row) * 4, (index / blocks_per_row) * 4);
        let (alpha_part, colour_part) = block.split_at(block_size - 8);
        let colours = colour_palette(colour_part, format == IwiFormat::Dxt1);
        let indices = u32::from_le_bytes([
            colour_part[4],
            colour_part[5],
            colour_part[6],
            colour_part[7],
        ]);
        let alphas = match format {
            IwiFormat::Dxt3 => dxt3_alphas(alpha_part),
            IwiFormat::Dxt5 => dxt5_alphas(alpha_part),
            _ => [255; 16],
        };
        for texel in 0..16 {
            let (x, y) = (block_x + texel % 4, block_y + texel / 4);
            if x >= width || y >= height {
                continue;
            }
            let [r, g, b, a] = colours[(indices >> (2 * texel)) as usize & 3];
            let alpha = if format == IwiFormat::Dxt1 {
                a
            } else {
                alphas[texel]
            };
            let at = (y * width + x) * 4;
            out[at..at + 4].copy_from_slice(&[r, g, b, alpha]);
        }
    }
}

fn rgb565(value: u16) -> [u8; 3] {
    let expand = |bits: u16, max: u16| (u32::from(bits) * 255 / u32::from(max)) as u8;
    [
        expand(value >> 11, 31),
        expand((value >> 5) & 63, 63),
        expand(value & 31, 31),
    ]
}

/// The four colours of a DXT colour block. In DXT1 `c0 <= c1` selects the 3-colour mode whose
/// fourth entry is transparent black; DXT3/5 always use the 4-colour mode.
fn colour_palette(block: &[u8], dxt1: bool) -> [[u8; 4]; 4] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |a: u8, b: u8, wa: u32, wb: u32, div: u32| {
        ((u32::from(a) * wa + u32::from(b) * wb) / div) as u8
    };
    let blend = |wa: u32, wb: u32, div: u32| {
        [
            mix(p0[0], p1[0], wa, wb, div),
            mix(p0[1], p1[1], wa, wb, div),
            mix(p0[2], p1[2], wa, wb, div),
            255,
        ]
    };
    let opaque = |p: [u8; 3]| [p[0], p[1], p[2], 255];
    if c0 > c1 || !dxt1 {
        [opaque(p0), opaque(p1), blend(2, 1, 3), blend(1, 2, 3)]
    } else {
        [opaque(p0), opaque(p1), blend(1, 1, 2), [0, 0, 0, 0]]
    }
}

fn dxt3_alphas(block: &[u8]) -> [u8; 16] {
    std::array::from_fn(|i| {
        let nibble = (block[i / 2] >> (4 * (i % 2))) & 15;
        nibble * 17
    })
}

fn dxt5_alphas(block: &[u8]) -> [u8; 16] {
    let (a0, a1) = (u32::from(block[0]), u32::from(block[1]));
    let palette: [u8; 8] = if a0 > a1 {
        std::array::from_fn(|i| match i {
            0 => a0 as u8,
            1 => a1 as u8,
            _ => (((8 - i as u32) * a0 + (i as u32 - 1) * a1) / 7) as u8,
        })
    } else {
        std::array::from_fn(|i| match i {
            0 => a0 as u8,
            1 => a1 as u8,
            6 => 0,
            7 => 255,
            _ => (((6 - i as u32) * a0 + (i as u32 - 1) * a1) / 5) as u8,
        })
    };
    let mut bits = 0u64;
    for (i, byte) in block[2..8].iter().enumerate() {
        bits |= u64::from(*byte) << (8 * i);
    }
    std::array::from_fn(|i| palette[((bits >> (3 * i)) & 7) as usize])
}

impl Rgba8 {
    /// Box-filters by 2 in each direction until neither side exceeds `max_side`.
    pub fn downscaled_to(mut self, max_side: u32) -> Rgba8 {
        while self.width.max(self.height) > max_side {
            self = self.halved();
        }
        self
    }

    fn halved(&self) -> Rgba8 {
        let (width, height) = (self.width as usize, self.height as usize);
        let (next_width, next_height) = ((width / 2).max(1), (height / 2).max(1));
        let mut data = Vec::with_capacity(next_width * next_height * 4);
        for y in 0..next_height {
            let rows = [2 * y, (2 * y + 1).min(height - 1)];
            for x in 0..next_width {
                let columns = [2 * x, (2 * x + 1).min(width - 1)];
                for channel in 0..4 {
                    let sum: u32 = rows
                        .iter()
                        .flat_map(|&row| columns.iter().map(move |&column| (row, column)))
                        .map(|(row, column)| {
                            u32::from(self.data[(row * width + column) * 4 + channel])
                        })
                        .sum();
                    data.push(((sum + 2) / 4) as u8);
                }
            }
        }
        Rgba8 {
            width: next_width as u32,
            height: next_height as u32,
            data,
        }
    }

    /// Reads this image as a CoD4 normal map and rewrites it as a regular RGB tangent-space
    /// one. CoD4 stores X in alpha and Y in green (the "DXT5nm" layout) and leaves Z out. Y
    /// grows downwards, like DirectX normal maps (checked against the colour maps' shading),
    /// so the material flips it back.
    pub fn into_normal_map(mut self) -> Rgba8 {
        for texel in self.data.as_chunks_mut::<4>().0 {
            let x = f32::from(texel[3]) / 127.5 - 1.0;
            let y = f32::from(texel[1]) / 127.5 - 1.0;
            let z = (1.0 - x * x - y * y).max(0.0).sqrt();
            let encode = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
            *texel = [encode(x), encode(y), encode(z), 255];
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic header for a `width` x `height` image with `format` and `pixels` bytes of
    /// pixel data; no real image data anywhere.
    fn file(format: u8, width: u16, height: u16, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = b"IWi\x06".to_vec();
        bytes.extend([format, 0]);
        bytes.extend(width.to_le_bytes());
        bytes.extend(height.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        let size = (HEADER_LEN + pixels.len()) as u32;
        for _ in 0..4 {
            bytes.extend(size.to_le_bytes());
        }
        bytes.extend(pixels);
        bytes
    }

    #[test]
    fn parses_the_header() {
        let header = parse_header(&file(0x0d, 256, 128, &[0; 16])).unwrap();
        assert_eq!(header.format, IwiFormat::Dxt5);
        assert_eq!((header.width, header.height), (256, 128));
        assert_eq!(header.file_sizes[0], 44);
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        let good = file(0x0b, 4, 4, &[0; 8]);
        assert_eq!(parse_header(&good[..10]), Err(IwiError::TooShort));
        let mut bad_magic = good.clone();
        bad_magic[0] = b'X';
        assert_eq!(parse_header(&bad_magic), Err(IwiError::BadMagic));
        let mut bad_version = good.clone();
        bad_version[3] = 8;
        assert_eq!(
            parse_header(&bad_version),
            Err(IwiError::UnsupportedVersion(8))
        );
        assert_eq!(
            parse_header(&file(0x20, 4, 4, &[0; 8])),
            Err(IwiError::UnsupportedFormat(0x20))
        );
        assert_eq!(
            parse_header(&file(0x0b, 0, 4, &[])),
            Err(IwiError::EmptyImage)
        );
        assert_eq!(decode(&good[..good.len() - 1]), Err(IwiError::Truncated));
    }

    #[test]
    fn decodes_a_solid_dxt1_block() {
        // c0 = pure red (565), c1 = black, every index 0.
        let block = [0x00, 0xf8, 0x00, 0x00, 0, 0, 0, 0];
        let image = decode(&file(0x0b, 4, 4, &block)).unwrap();
        assert_eq!((image.width, image.height), (4, 4));
        assert!(image.data.chunks(4).all(|px| px == [255, 0, 0, 255]));
    }

    #[test]
    fn dxt1_three_colour_mode_has_transparent_black() {
        // c0 = black <= c1 = pure red: index 3 is transparent black, index 2 their midpoint.
        let block = [0x00, 0x00, 0x00, 0xf8, 0b1110_0100, 0, 0, 0];
        let image = decode(&file(0x0b, 4, 4, &block)).unwrap();
        let px = |i: usize| &image.data[i * 4..i * 4 + 4];
        assert_eq!(px(0), [0, 0, 0, 255]);
        assert_eq!(px(1), [255, 0, 0, 255]);
        assert_eq!(px(2), [127, 0, 0, 255]);
        assert_eq!(px(3), [0, 0, 0, 0]);
    }

    #[test]
    fn dxt5_alpha_and_crops_non_multiple_of_four() {
        // alpha endpoints 255 and 0, all indices 0 (alpha 255); colour: solid green.
        let mut block = vec![255, 0, 0, 0, 0, 0, 0, 0];
        block.extend([0xe0, 0x07, 0xe0, 0x07, 0, 0, 0, 0]);
        let image = decode(&file(0x0d, 2, 3, &block)).unwrap();
        assert_eq!(image.data.len(), 2 * 3 * 4);
        assert!(image.data.chunks(4).all(|px| px == [0, 255, 0, 255]));
    }

    #[test]
    fn decodes_uncompressed_formats() {
        let argb = decode(&file(0x01, 1, 1, &[10, 20, 30, 40])).unwrap();
        assert_eq!(argb.data, [30, 20, 10, 40]);
        let rgb = decode(&file(0x02, 1, 1, &[10, 20, 30])).unwrap();
        assert_eq!(rgb.data, [30, 20, 10, 255]);
        let grey = decode(&file(0x03, 1, 1, &[7, 9])).unwrap();
        assert_eq!(grey.data, [7, 7, 7, 9]);
        let alpha = decode(&file(0x04, 1, 1, &[5])).unwrap();
        assert_eq!(alpha.data, [255, 255, 255, 5]);
    }

    #[test]
    fn downscales_to_the_limit() {
        let image = Rgba8 {
            width: 4,
            height: 2,
            data: (0..4 * 2 * 4).map(|i| (i % 2) as u8 * 100).collect(),
        };
        let small = image.clone().downscaled_to(2);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(small.data.len(), 2 * 4);
        assert_eq!(image.clone().downscaled_to(4), image);
    }

    #[test]
    fn flat_normal_map_points_up() {
        let flat = Rgba8 {
            width: 1,
            height: 1,
            data: vec![128, 128, 128, 128],
        };
        let normal = flat.into_normal_map();
        assert!(normal.data[0].abs_diff(128) <= 1 && normal.data[1].abs_diff(128) <= 1);
        assert_eq!(normal.data[2..], [255, 255]);
    }
}
