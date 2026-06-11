//! FMOD Studio `.bank` parsing and playback.
//!
//! A `.bank` is a RIFF/FEV container whose `SND ` chunk embeds an FSB5
//! sample bank. PoE2 banks use the Vorbis codec, where FMOD strips the
//! Vorbis ident/comment/setup headers and instead stores a CRC32 of the
//! setup header. We rebuild the headers (setup headers come from a lookup
//! table assembled by the python-fsb5 project, MIT, (c) 2016 Simon
//! Pinfold — see assets/fsb5_vorbis_headers.bin) and decode the raw
//! packets to PCM with lewton. Decoded streams are returned as WAV bytes,
//! which avoids the slow and lossy Vorbis re-encode that existing
//! extraction tools perform.

use std::collections::HashMap;
use std::sync::OnceLock;

/// crc32 -> full Vorbis setup-header packet, parsed lazily from the
/// embedded binary table: [count u32] then per entry [crc u32][len u32][bytes].
fn setup_header_table() -> &'static HashMap<u32, &'static [u8]> {
    static TABLE: OnceLock<HashMap<u32, &'static [u8]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        static DATA: &[u8] = include_bytes!("../../assets/fsb5_vorbis_headers.bin");
        let mut map = HashMap::new();
        let count = u32::from_le_bytes(DATA[0..4].try_into().unwrap()) as usize;
        let mut pos = 4usize;
        for _ in 0..count {
            let crc = u32::from_le_bytes(DATA[pos..pos + 4].try_into().unwrap());
            let len = u32::from_le_bytes(DATA[pos + 4..pos + 8].try_into().unwrap()) as usize;
            pos += 8;
            map.insert(crc, &DATA[pos..pos + len]);
            pos += len;
        }
        map
    })
}

#[derive(Clone)]
pub struct BankStreamInfo {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u8,
    pub sample_count: u64,
    pub size: u32,
}

impl BankStreamInfo {
    pub fn duration_secs(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.sample_count as f32 / self.sample_rate as f32
    }
}

#[derive(Clone)]
pub struct FmodBankInfo {
    pub format: String,
    /// File extension decoded streams use ("wav").
    pub extension: &'static str,
    pub streams: Vec<BankStreamInfo>,
}

struct Fsb5Sample {
    name: String,
    sample_rate: u32,
    channels: u8,
    sample_count: u64,
    /// Byte range of this sample's data within the FSB5 data section.
    data_start: usize,
    data_end: usize,
    vorbis_crc32: Option<u32>,
}

struct Fsb5 {
    mode: u32,
    samples: Vec<Fsb5Sample>,
}

const MODE_PCM8: u32 = 1;
const MODE_PCM16: u32 = 2;
const MODE_PCM32: u32 = 4;
const MODE_PCMFLOAT: u32 = 5;
const MODE_VORBIS: u32 = 15;

fn mode_name(mode: u32) -> &'static str {
    match mode {
        0 => "None",
        MODE_PCM8 => "PCM8",
        MODE_PCM16 => "PCM16",
        3 => "PCM24",
        MODE_PCM32 => "PCM32",
        MODE_PCMFLOAT => "PCM Float",
        6 => "GCADPCM",
        7 => "IMAADPCM",
        8 => "VAG",
        9 => "HEVAG",
        10 => "XMA",
        11 => "MPEG",
        12 => "CELT",
        13 => "ATRAC9",
        14 => "xWMA",
        MODE_VORBIS => "Vorbis",
        16 => "FADPCM",
        _ => "Unknown",
    }
}

/// Finds the FSB5 payload: the file is either a bare FSB5, or a RIFF/FEV
/// FMOD Studio bank whose `SND ` chunk embeds one (the FSB5 magic can sit
/// after alignment padding inside the chunk).
fn find_fsb5(data: &[u8]) -> Option<&[u8]> {
    if data.starts_with(b"FSB5") {
        return Some(data);
    }
    if data.len() < 12 || &data[0..4] != b"RIFF" {
        return None;
    }
    let mut pos = 12usize;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().ok()?) as usize;
        let chunk_start = pos + 8;
        let chunk_end = chunk_start.checked_add(size)?.min(data.len());
        if id == b"SND " {
            let chunk = &data[chunk_start..chunk_end];
            if let Some(off) = chunk.windows(4).position(|w| w == b"FSB5") {
                return Some(&chunk[off..]);
            }
        }
        pos = chunk_start + size + (size & 1);
    }
    None
}

fn read_u32(data: &[u8], pos: usize) -> Result<u32, String> {
    data.get(pos..pos + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| "FSB5 truncated".to_string())
}

fn read_u64(data: &[u8], pos: usize) -> Result<u64, String> {
    data.get(pos..pos + 8)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| "FSB5 truncated".to_string())
}

fn frequency_from_id(id: u64) -> Option<u32> {
    match id {
        1 => Some(8000),
        2 => Some(11000),
        3 => Some(11025),
        4 => Some(16000),
        5 => Some(22050),
        6 => Some(24000),
        7 => Some(32000),
        8 => Some(44100),
        9 => Some(48000),
        10 => Some(96000),
        _ => None,
    }
}

fn parse_fsb5(fsb: &[u8]) -> Result<Fsb5, String> {
    if !fsb.starts_with(b"FSB5") {
        return Err("Not an FSB5 payload".to_string());
    }
    let version = read_u32(fsb, 4)?;
    let num_samples = read_u32(fsb, 8)? as usize;
    let sample_headers_size = read_u32(fsb, 12)? as usize;
    let name_table_size = read_u32(fsb, 16)? as usize;
    let data_size = read_u32(fsb, 20)? as usize;
    let mode = read_u32(fsb, 24)?;
    let header_size = if version == 0 { 64 } else { 60 };

    let name_table_offset = header_size + sample_headers_size;
    let data_base = name_table_offset + name_table_size;

    // Sample headers: packed u64 + optional metadata chunks
    let mut pos = header_size;
    let mut raw_samples = Vec::with_capacity(num_samples);
    for _ in 0..num_samples {
        let raw = read_u64(fsb, pos)?;
        pos += 8;
        let mut has_chunks = raw & 1 != 0;
        let freq_id = (raw >> 1) & 0x0f;
        let mut channels = (((raw >> 5) & 1) + 1) as u8;
        let data_offset = (((raw >> 6) & 0x0fff_ffff) * 16) as usize;
        let sample_count = (raw >> 34) & 0x3fff_ffff;

        let mut sample_rate = frequency_from_id(freq_id);
        let mut vorbis_crc32 = None;

        while has_chunks {
            let chunk = read_u32(fsb, pos)?;
            pos += 4;
            has_chunks = chunk & 1 != 0;
            let chunk_size = ((chunk >> 1) & 0x00ff_ffff) as usize;
            let chunk_type = (chunk >> 25) & 0x7f;
            match chunk_type {
                1 => {
                    // CHANNELS override
                    if let Some(&c) = fsb.get(pos) {
                        channels = c;
                    }
                }
                2 => {
                    // FREQUENCY override
                    sample_rate = Some(read_u32(fsb, pos)?);
                }
                11 => {
                    // VORBISDATA: crc32 of the setup header (+ seek table we ignore)
                    vorbis_crc32 = Some(read_u32(fsb, pos)?);
                }
                _ => {}
            }
            pos += chunk_size;
        }

        raw_samples.push((
            sample_rate.ok_or_else(|| format!("Unknown frequency id {}", freq_id))?,
            channels,
            data_offset,
            sample_count,
            vorbis_crc32,
        ));
    }

    // Name table
    let mut names = vec![String::new(); num_samples];
    if name_table_size > 0 {
        for (i, name) in names.iter_mut().enumerate() {
            let off = read_u32(fsb, name_table_offset + i * 4)? as usize;
            let start = name_table_offset + off;
            let end = fsb[start..]
                .iter()
                .position(|&b| b == 0)
                .map(|p| start + p)
                .unwrap_or(start);
            *name = String::from_utf8_lossy(&fsb[start..end]).to_string();
        }
    }

    // Each sample's data runs until the next sample's offset (or data end)
    let mut samples = Vec::with_capacity(num_samples);
    for (i, &(sample_rate, channels, data_offset, sample_count, vorbis_crc32)) in
        raw_samples.iter().enumerate()
    {
        let next_offset = raw_samples
            .get(i + 1)
            .map(|s| s.2)
            .unwrap_or(data_size);
        let name = if names[i].is_empty() {
            format!("stream_{:03}", i)
        } else {
            names[i].clone()
        };
        samples.push(Fsb5Sample {
            name,
            sample_rate,
            channels,
            sample_count,
            data_start: data_base + data_offset,
            data_end: (data_base + next_offset).min(fsb.len()),
            vorbis_crc32,
        });
    }

    Ok(Fsb5 { mode, samples })
}

/// Parses bank/FSB5 headers and the stream name table without decoding any
/// audio. Cheap enough to run on selection.
pub fn parse_bank_info(data: &[u8]) -> Result<FmodBankInfo, String> {
    let Some(fsb) = find_fsb5(data) else {
        if data.starts_with(b"RIFF") {
            // Valid FMOD bank with no embedded audio: event/mixer metadata or
            // a .strings.bank GUID table.
            return Ok(FmodBankInfo {
                format: "No audio (metadata only)".to_string(),
                extension: "wav",
                streams: Vec::new(),
            });
        }
        return Err("No FSB5 payload found in bank".to_string());
    };
    let bank = parse_fsb5(fsb)?;
    Ok(FmodBankInfo {
        format: mode_name(bank.mode).to_string(),
        extension: "wav",
        streams: bank
            .samples
            .iter()
            .map(|s| BankStreamInfo {
                name: s.name.clone(),
                sample_rate: s.sample_rate,
                channels: s.channels,
                sample_count: s.sample_count,
                size: (s.data_end.saturating_sub(s.data_start)) as u32,
            })
            .collect(),
    })
}

/// Decodes a single stream (by index) to WAV bytes.
pub fn decode_stream(data: &[u8], index: usize) -> Result<Vec<u8>, String> {
    let fsb = find_fsb5(data).ok_or("No FSB5 payload found in bank")?;
    let bank = parse_fsb5(fsb)?;
    let sample = bank
        .samples
        .get(index)
        .ok_or_else(|| format!("Stream index {} out of range", index))?;
    let stream_data = fsb
        .get(sample.data_start..sample.data_end)
        .ok_or("Sample data out of bounds")?;

    let pcm: Vec<i16> = match bank.mode {
        MODE_VORBIS => decode_vorbis(sample, stream_data)?,
        MODE_PCM16 => stream_data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect(),
        MODE_PCM8 => stream_data
            .iter()
            .map(|&b| (b as i16 - 128) << 8)
            .collect(),
        MODE_PCMFLOAT => stream_data
            .chunks_exact(4)
            .map(|b| {
                let f = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                (f.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
            })
            .collect(),
        other => {
            return Err(format!(
                "Codec {} is not supported for playback",
                mode_name(other)
            ))
        }
    };

    pcm_to_wav(&pcm, sample.channels, sample.sample_rate)
}

/// Decodes FMOD's headerless Vorbis: rebuild ident/comment headers, fetch
/// the setup header from the CRC table, then decode the length-prefixed
/// packet stream with lewton.
fn decode_vorbis(sample: &Fsb5Sample, stream_data: &[u8]) -> Result<Vec<i16>, String> {
    let crc = sample
        .vorbis_crc32
        .ok_or("Vorbis stream missing setup-header CRC chunk")?;
    let setup_packet = *setup_header_table()
        .get(&crc)
        .ok_or_else(|| format!("Unknown Vorbis setup header (crc32 {:#010x})", crc))?;

    let ident_packet = build_ident_packet(sample.channels, sample.sample_rate);
    let comment_packet = build_comment_packet();

    let ident = lewton::header::read_header_ident(&ident_packet)
        .map_err(|e| format!("Ident header: {:?}", e))?;
    let comment = lewton::header::read_header_comment(&comment_packet)
        .map_err(|e| format!("Comment header: {:?}", e))?;
    let _ = comment;
    let setup = lewton::header::read_header_setup(
        setup_packet,
        sample.channels,
        (ident.blocksize_0, ident.blocksize_1),
    )
    .map_err(|e| format!("Setup header: {:?}", e))?;

    let expected = (sample.sample_count as usize).saturating_mul(sample.channels as usize);
    let mut pcm: Vec<i16> = Vec::with_capacity(expected);
    let mut pwr = lewton::audio::PreviousWindowRight::new();

    let mut pos = 0usize;
    while pos + 2 <= stream_data.len() {
        let size = u16::from_le_bytes([stream_data[pos], stream_data[pos + 1]]) as usize;
        pos += 2;
        if size == 0 || pos + size > stream_data.len() {
            break;
        }
        let packet = &stream_data[pos..pos + size];
        pos += size;

        match lewton::audio::read_audio_packet_generic::<lewton::samples::InterleavedSamples<i16>>(
            &ident, &setup, packet, &mut pwr,
        ) {
            Ok(samples) => pcm.extend_from_slice(&samples.samples),
            Err(e) => return Err(format!("Packet decode: {:?}", e)),
        }
        if pcm.len() >= expected && expected > 0 {
            break;
        }
    }

    if expected > 0 && pcm.len() > expected {
        pcm.truncate(expected);
    }
    Ok(pcm)
}

fn build_ident_packet(channels: u8, sample_rate: u32) -> Vec<u8> {
    let mut p = Vec::with_capacity(30);
    p.push(1);
    p.extend_from_slice(b"vorbis");
    p.extend_from_slice(&0u32.to_le_bytes()); // vorbis_version
    p.push(channels);
    p.extend_from_slice(&sample_rate.to_le_bytes());
    p.extend_from_slice(&0i32.to_le_bytes()); // bitrate_maximum
    p.extend_from_slice(&0i32.to_le_bytes()); // bitrate_nominal
    p.extend_from_slice(&0i32.to_le_bytes()); // bitrate_minimum
    // FMOD always encodes with blocksizes 256/2048 (exponents 8 and 11)
    p.push((11 << 4) | 8);
    p.push(1); // framing
    p
}

fn build_comment_packet() -> Vec<u8> {
    let vendor = b"ggpk-explorer";
    let mut p = Vec::with_capacity(7 + 4 + vendor.len() + 5);
    p.push(3);
    p.extend_from_slice(b"vorbis");
    p.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    p.extend_from_slice(vendor);
    p.extend_from_slice(&0u32.to_le_bytes()); // user comment count
    p.push(1); // framing
    p
}

fn pcm_to_wav(pcm: &[i16], channels: u8, sample_rate: u32) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels: channels as u16,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer =
            hound::WavWriter::new(&mut cursor, spec).map_err(|e| e.to_string())?;
        let mut i16_writer = writer.get_i16_writer(pcm.len() as u32);
        for &s in pcm {
            i16_writer.write_sample(s);
        }
        i16_writer.flush().map_err(|e| e.to_string())?;
        writer.finalize().map_err(|e| e.to_string())?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Scans every loose .bank in the configured GGPK: parses stream listings
    // for all and decodes the first stream of each, to catch unknown setup
    // header CRCs or codecs. Run with:
    //   cargo test --release decode_all_ggpk_banks -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_decode_all_ggpk_banks() {
        let settings = crate::settings::AppSettings::load();
        let Some(ggpk_path) = settings.ggpk_path else {
            println!("no ggpk_path configured, skipping");
            return;
        };
        let reader = crate::ggpk::reader::GgpkReader::open(&ggpk_path).unwrap();
        let banks: Vec<(String, u64)> = reader
            .collect_loose_files()
            .into_iter()
            .filter(|(p, _)| p.ends_with(".bank"))
            .collect();
        println!("found {} banks", banks.len());

        let mut parsed = 0;
        let mut parse_failed = 0;
        let mut decoded = 0;
        let mut decode_failed = 0;
        let t = std::time::Instant::now();
        for (path, _) in &banks {
            let rec = reader.read_file_by_path(path).unwrap().unwrap();
            let data = reader.get_data_slice(rec.data_offset, rec.data_length).unwrap();
            match parse_bank_info(data) {
                Ok(info) => {
                    parsed += 1;
                    if !info.streams.is_empty() {
                        match decode_stream(data, 0) {
                            Ok(_) => decoded += 1,
                            Err(e) => {
                                decode_failed += 1;
                                println!("DECODE FAIL {} [{}]: {}", path, info.format, e);
                            }
                        }
                    }
                }
                Err(e) => {
                    parse_failed += 1;
                    println!("PARSE FAIL {}: {}", path, e);
                }
            }
        }
        println!(
            "parsed {}/{} banks ({} failed), decoded first stream of {} ({} failed) in {:?}",
            parsed,
            banks.len(),
            parse_failed,
            decoded,
            decode_failed,
            t.elapsed()
        );
        assert_eq!(parse_failed, 0);
        assert_eq!(decode_failed, 0);
    }

    // Spike test against banks extracted from the local Content.ggpk.
    // Run with: cargo test fmod_bank -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_parse_real_banks() {
        let temp = std::env::temp_dir();
        for name in [
            "UI_PassiveTree.bank",
            "Four_Earthquake.bank",
            "Music_Act2_MarakethTrials.bank",
        ] {
            let path = temp.join(name);
            if !path.exists() {
                println!("skipping {} (not extracted)", name);
                continue;
            }
            let data = std::fs::read(&path).unwrap();
            let t = std::time::Instant::now();
            let info = parse_bank_info(&data).unwrap_or_else(|e| panic!("{}: {}", name, e));
            println!(
                "{}: format={} streams={} (info in {:?})",
                name,
                info.format,
                info.streams.len(),
                t.elapsed()
            );
            let mut ok = 0;
            let mut failed = 0;
            let t = std::time::Instant::now();
            for (i, s) in info.streams.iter().enumerate() {
                match decode_stream(&data, i) {
                    Ok(bytes) => {
                        ok += 1;
                        let decoder = rodio::Decoder::new(std::io::Cursor::new(bytes));
                        assert!(decoder.is_ok(), "rodio failed to decode {}", s.name);
                    }
                    Err(e) => {
                        failed += 1;
                        println!("  FAIL {}: {}", s.name, e);
                    }
                }
            }
            println!(
                "  {}/{} streams decoded in {:?} ({} failed)",
                ok,
                info.streams.len(),
                t.elapsed(),
                failed
            );
            assert!(ok > 0, "{}: no streams decoded", name);
        }
    }
}
