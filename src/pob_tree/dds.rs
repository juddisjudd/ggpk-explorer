//! The `.dds.zst` sheets PoB-PoE2 draws its tree from: every texture of one
//! size and format stacked into a DDS texture array, each layer's mip chain
//! copied from the game file as it is, compressed with zstd at level 3.
//!
//! The layout is what SimpleGraphic's `Texture:StackTextures`/`Save` writes
//! through gli: a DX10 header even for one layer, the linear size field
//! holding the size of every layer together, and layers one after another.

/// One game texture, cut down to what stacking needs.
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub mips: u32,
    /// Every mip level of the texture, largest first.
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc6h,
    Bc7,
    Rgba,
}

impl Format {
    /// `Texture:Info().formatStr`, which names the sheet file.
    pub fn name(self) -> &'static str {
        match self {
            Format::Bc1 => "BC1",
            Format::Bc2 => "BC2",
            Format::Bc3 => "BC3",
            Format::Bc4 => "BC4",
            Format::Bc5 => "BC5",
            Format::Bc6h => "BC6H",
            Format::Bc7 => "BC7",
            Format::Rgba => "RGBA",
        }
    }

    fn block_bytes(self) -> Option<usize> {
        match self {
            Format::Bc1 | Format::Bc4 => Some(8),
            Format::Rgba => None,
            _ => Some(16),
        }
    }

    fn dxgi(self) -> u32 {
        match self {
            Format::Bc1 => 71,
            Format::Bc2 => 74,
            Format::Bc3 => 77,
            Format::Bc4 => 80,
            Format::Bc5 => 83,
            Format::Bc6h => 95,
            Format::Bc7 => 98,
            Format::Rgba => 28,
        }
    }

    fn bits_per_pixel(self) -> u32 {
        match self {
            Format::Bc1 | Format::Bc4 => 4,
            Format::Rgba => 32,
            _ => 8,
        }
    }

    fn level_size(self, width: u32, height: u32) -> usize {
        let (w, h) = (width.max(1) as usize, height.max(1) as usize);
        match self.block_bytes() {
            Some(block) => w.div_ceil(4) * h.div_ceil(4) * block,
            None => w * h * 4,
        }
    }
}

const DDSD_MIPMAPCOUNT: u32 = 0x20000;
const DDPF_FOURCC: u32 = 0x4;

/// Reads a game `.dds`. The game keeps some behind a few bytes of prefix, so
/// the header is found by its magic.
pub fn read(bytes: &[u8]) -> Result<Texture, String> {
    let at = bytes
        .windows(4)
        .take(64)
        .position(|w| w == b"DDS ")
        .ok_or("no DDS header")?;
    let b = &bytes[at..];
    let u32_at = |o: usize| -> Result<u32, String> {
        b.get(o..o + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
            .ok_or_else(|| "truncated DDS header".to_string())
    };
    let flags = u32_at(8)?;
    let height = u32_at(12)?;
    let width = u32_at(16)?;
    let mips = if flags & DDSD_MIPMAPCOUNT != 0 { u32_at(28)?.max(1) } else { 1 };
    let pf_flags = u32_at(80)?;
    let four_cc = b.get(84..88).ok_or("truncated DDS header")?;
    let (format, offset, bgra) = if pf_flags & DDPF_FOURCC != 0 {
        match four_cc {
            b"DXT1" => (Format::Bc1, 128, false),
            b"DXT2" | b"DXT3" => (Format::Bc2, 128, false),
            b"DXT4" | b"DXT5" => (Format::Bc3, 128, false),
            b"ATI1" | b"BC4U" => (Format::Bc4, 128, false),
            b"ATI2" | b"BC5U" => (Format::Bc5, 128, false),
            b"DX10" => {
                let format = match u32_at(128)? {
                    70..=72 => Format::Bc1,
                    73..=75 => Format::Bc2,
                    76..=78 => Format::Bc3,
                    79..=81 => Format::Bc4,
                    82..=84 => Format::Bc5,
                    94..=96 => Format::Bc6h,
                    97..=99 => Format::Bc7,
                    27..=32 => Format::Rgba,
                    87 | 90 | 91 => return swizzled(b, width, height, mips, 148),
                    other => return Err(format!("unsupported DXGI format {}", other)),
                };
                (format, 148, false)
            }
            other => return Err(format!("unsupported FourCC {:?}", String::from_utf8_lossy(other))),
        }
    } else if u32_at(88)? == 32 {
        (Format::Rgba, 128, u32_at(92)? == 0x00ff_0000)
    } else {
        return Err("unsupported uncompressed DDS layout".into());
    };
    if bgra {
        return swizzled(b, width, height, mips, offset);
    }
    let size = chain_size(format, width, height, mips);
    let data = b.get(offset..offset + size).ok_or("DDS data shorter than its mip chain")?.to_vec();
    Ok(Texture { width, height, format, mips, data })
}

/// A BGRA texture, reordered to the RGBA the sheets hold.
fn swizzled(b: &[u8], width: u32, height: u32, mips: u32, offset: usize) -> Result<Texture, String> {
    let size = chain_size(Format::Rgba, width, height, mips);
    let mut data = b.get(offset..offset + size).ok_or("DDS data shorter than its mip chain")?.to_vec();
    for px in data.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    Ok(Texture { width, height, format: Format::Rgba, mips, data })
}

fn chain_size(format: Format, width: u32, height: u32, mips: u32) -> usize {
    (0..mips).map(|level| format.level_size(width >> level, height >> level)).sum()
}

/// `StackTextures` then `Save`: the layers as one array texture, zstd
/// compressed the way SimpleGraphic does it (one shot, level 3).
pub fn stack(layers: &[&Texture]) -> Result<Vec<u8>, String> {
    let first = layers.first().ok_or("no textures to stack")?;
    if let Some(odd) = layers.iter().find(|t| t.mips != first.mips) {
        return Err(format!("layers disagree on mip count ({} and {})", first.mips, odd.mips));
    }
    let format = first.format;
    let data_size: usize = layers.iter().map(|t| t.data.len()).sum();
    let mut out = Vec::with_capacity(148 + data_size);
    let mut put = |v: u32| out.extend_from_slice(&v.to_le_bytes());
    put(u32::from_le_bytes(*b"DDS "));
    put(124);
    put(if format.block_bytes().is_some() { 0xA1007 } else { 0x2100F });
    put(first.height);
    put(first.width);
    put(match format.block_bytes() {
        Some(_) => data_size as u32,
        None => first.width * 4,
    });
    put(0);
    put(first.mips);
    for _ in 0..11 {
        put(0);
    }
    put(32);
    put(DDPF_FOURCC);
    put(u32::from_le_bytes(*b"DX10"));
    put(format.bits_per_pixel());
    let masks = match format {
        Format::Rgba => [0xff, 0xff00, 0xff_0000, 0xff00_0000],
        _ => [0; 4],
    };
    for mask in masks {
        put(mask);
    }
    put(0x401000);
    for _ in 0..4 {
        put(0);
    }
    put(format.dxgi());
    put(3);
    put(0);
    put(layers.len() as u32);
    put(0);
    for layer in layers {
        out.extend_from_slice(&layer.data);
    }
    zstd::bulk::compress(&out, 3).map_err(|e| format!("zstd: {}", e))
}
