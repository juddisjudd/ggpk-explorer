use anyhow::{anyhow, Context, Result};
use regex::Regex;

/// Parse UTF-16 with BOM (Byte Order Mark) detection
/// Returns the string with automatic endianness detection
pub fn utf16_bom_to_string(bytes: &[u8]) -> Result<String> {
    if bytes.len() < 2 {
        return Err(anyhow!("Not enough bytes for BOM"));
    }

    let (first, rest) = bytes.split_at(2);

    match first {
        [0xff, 0xfe] => {
            // UTF-16 LE
            let bytes_u16: Vec<u16> = rest
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            String::from_utf16(&bytes_u16).context("Failed to decode UTF-16 LE")
        }
        [0xfe, 0xff] => {
            // UTF-16 BE
            let bytes_u16: Vec<u16> = rest
                .chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect();
            String::from_utf16(&bytes_u16).context("Failed to decode UTF-16 BE")
        }
        _ => Err(anyhow!(
            "Invalid UTF-16 BOM: {:02x} {:02x}",
            first[0],
            first[1]
        )),
    }
}

/// Remove trailing commas before closing brackets (for JSON-like formats)
pub fn remove_trailing_commas(content: &str) -> String {
    if let Ok(re) = Regex::new(r",\s*([\]}\)])") {
        re.replace_all(content, "$1").to_string()
    } else {
        content.to_string()
    }
}

/// Parse a UTF-16 LE string from bytes without BOM
pub fn utf16le_to_string(bytes: &[u8]) -> Result<String> {
    if bytes.len() % 2 != 0 {
        return Err(anyhow!("UTF-16 bytes must be even length"));
    }

    let bytes_u16: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();

    String::from_utf16(&bytes_u16).context("Failed to decode UTF-16 LE")
}

/// Little-endian u32 parsing
pub fn read_le_u32(bytes: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*bytes)
}

/// Little-endian f32 parsing
pub fn read_le_f32(bytes: &[u8; 4]) -> f32 {
    f32::from_le_bytes(*bytes)
}

/// Little-endian f16 parsing
pub fn read_le_f16(bytes: &[u8; 2]) -> f32 {
    // Convert f16 to f32 (approximation)
    let bits = u16::from_le_bytes(*bytes);
    f16_to_f32(bits)
}

/// Convert f16 (half-precision float) to f32
/// This is a simple approximation; a proper implementation would use the half crate
fn f16_to_f32(f16_bits: u16) -> f32 {
    let sign = (f16_bits >> 15) & 1;
    let exponent = (f16_bits >> 10) & 0x1f;
    let mantissa = f16_bits & 0x3ff;

    let sign_f = if sign == 1 { -1.0 } else { 1.0 };

    if exponent == 0 {
        if mantissa == 0 {
            sign_f * 0.0
        } else {
            sign_f * (mantissa as f32 / 1024.0) * (2.0_f32).powi(-14)
        }
    } else if exponent == 31 {
        if mantissa == 0 {
            sign_f * f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        let exp = exponent as i32 - 15;
        sign_f * (1.0 + (mantissa as f32 / 1024.0)) * (2.0_f32).powi(exp)
    }
}

/// Read a null-terminated UTF-16 LE string
pub fn read_utf16le_cstring(bytes: &[u8]) -> Result<(String, usize)> {
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == 0 && bytes[i + 1] == 0 {
            let string = utf16le_to_string(&bytes[0..i])?;
            return Ok((string, i + 2));
        }
        i += 2;
    }
    Err(anyhow!("Null-terminated UTF-16 string not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf16_bom_le() {
        let data = b"\xff\xfeH\x00e\x00l\x00l\x00o\x00"; // "Hello" in UTF-16 LE with BOM
        let result = utf16_bom_to_string(data).unwrap();
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_remove_trailing_commas() {
        let input = r#"{"key": "value",}"#;
        let output = remove_trailing_commas(input);
        assert_eq!(output, r#"{"key": "value"}"#);
    }
}
