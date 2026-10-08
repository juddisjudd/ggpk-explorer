//! `.dlp` doodad layer presets: placement settings and the `.fmt` models scattered with them.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DlpFile {
    pub version: Option<u32>,
    pub headers: Headers,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Headers {
    V2(HeadersV2),
    V3(Vec<Header>),
}

#[derive(Debug, Clone, Serialize)]
pub struct HeadersV2 {
    pub scale_min: f64,
    pub scale_max: f64,
    pub allow_waving: bool,
    pub allow_on_blocking: bool,
    pub max_rotation: Option<u32>,
    pub uint1: Option<u32>,
    pub uint2: Option<u32>,
    pub float1: Option<f64>,
    pub audio_type: Option<u32>,
    pub float2: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum Header {
    RandomScale { min: f64, max: f64 },
    AllowWaving,
    AllowOnBlocking,
    MaxRotation(u32),
    MinEdgeScale(f64),
    AudioType(u32),
    DelayMultiplier(f64),
    SizeMultiplier(f64),
    TimeMultiplier(f64),
    Seed(u32),
    Other { key: String, rest: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub fmt_file: String,
    pub float: f64,
    pub points: Vec<(f64, f64)>,
}

fn headers_v2(c: &mut Cursor) -> Option<HeadersV2> {
    let scale_min = c.float()?;
    let scale_max = c.sp()?.float()?;
    let allow_waving = c.sp()?.bool()?;
    let allow_on_blocking = c.sp()?.bool()?;
    let max_rotation = c.opt(|c| c.sp()?.uint());
    let uint1 = c.opt(|c| c.sp()?.uint());
    let uint2 = c.opt(|c| c.sp()?.uint());
    let float1 = c.opt(|c| c.sp()?.float());
    let audio_type = c.opt(|c| c.sp()?.uint());
    let float2 = c.opt(|c| c.sp()?.float());
    Some(HeadersV2 { scale_min, scale_max, allow_waving, allow_on_blocking, max_rotation, uint1, uint2, float1, audio_type, float2 })
}

fn header(c: &mut Cursor) -> Option<Header> {
    let key = c.lit("-")?.sp()?.word()?;
    c.skip_space();
    let header = match key {
        "RandomScale" => Header::RandomScale { min: c.float()?, max: c.sp()?.float()? },
        "AllowWaving" => Header::AllowWaving,
        "AllowOnBlocking" => Header::AllowOnBlocking,
        "MaxRotation" => Header::MaxRotation(c.uint()?),
        "MinEdgeScale" => Header::MinEdgeScale(c.float()?),
        "AudioType" => Header::AudioType(c.uint()?),
        "DelayMultiplier" => Header::DelayMultiplier(c.float()?),
        "TimeMultiplier" => Header::TimeMultiplier(c.float()?),
        "SizeMultiplier" => Header::SizeMultiplier(c.float()?),
        "Seed" => Header::Seed(c.uint()?),
        key => Header::Other { key: key.to_string(), rest: c.rest().to_string() },
    };
    Some(header)
}

fn point(c: &mut Cursor) -> Option<(f64, f64)> {
    if c.eat("(") {
        let x = c.float()?;
        let y = c.lit(",")?.float()?;
        c.lit(")")?;
        Some((x, y))
    } else {
        let x = c.lit("[")?.float()?;
        let y = c.sp()?.float()?;
        c.lit("]")?;
        Some((x, y))
    }
}

fn entry(c: &mut Cursor) -> Option<Entry> {
    let fmt_file = c.file("fmt")?;
    let float = c.sp()?.float()?;
    let points = c.many(|c| point(c.sp()?));
    Some(Entry { fmt_file, float, points })
}

pub fn parse(text: &str) -> Result<DlpFile, String> {
    let mut lines = Lines::new(text, None);
    let version = lines.try_line(super::version);
    let headers = match version.unwrap_or(0) {
        ..3 => Headers::V2(lines.line("placement settings", headers_v2)?),
        _ => Headers::V3(lines.many(header)),
    };
    let entries = lines.many(entry);
    lines.end()?;
    Ok(DlpFile { version, headers, entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keyed_headers_and_points() {
        let text = "version 3\r\n- RandomScale 0.7 1\r\n- AllowWaving\r\n- MaxRotation 360\r\n- MaxRotationZ 15\r\n\"Art\\Models\\Terrain\\Foliage\\Jungle\\SandGrassTall04.fmt\" 4  (0.953,0.322)  [0.774 0.124]\r\n\"Art/Models/a.fmt\" 1\r\n";
        let f = parse(text).unwrap();
        let Headers::V3(headers) = &f.headers else { panic!("expected keyed headers") };
        assert_eq!(headers.len(), 4);
        assert!(matches!(&headers[3], Header::Other { key, rest } if key == "MaxRotationZ" && rest == "15"));
        assert_eq!(f.entries[0].points, vec![(0.953, 0.322), (0.774, 0.124)]);
        assert!(f.entries[1].points.is_empty());
    }

    #[test]
    fn unversioned_files_use_the_positional_header() {
        let f = parse("1.15 1.8 1 1 360 1 1 1 17 1\n\"Art/a.fmt\" 12.5\n").unwrap();
        let Headers::V2(h) = &f.headers else { panic!("expected positional headers") };
        assert_eq!(h.max_rotation, Some(360));
        assert_eq!(h.audio_type, Some(17));
        assert_eq!(h.float2, Some(1.0));
    }
}
