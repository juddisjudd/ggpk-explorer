use std::fmt;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub msg: String,
    pub offset: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.msg, self.offset)
    }
}

impl std::error::Error for ParseError {}

pub type PResult<T> = Result<T, ParseError>;

/// Little-endian byte cursor with bounds-checked reads.
pub struct Cur<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cur<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    pub fn err(&self, msg: impl Into<String>) -> ParseError {
        ParseError { msg: msg.into(), offset: self.pos }
    }

    pub fn take(&mut self, n: usize) -> PResult<&'a [u8]> {
        if n > self.remaining() {
            return Err(self.err(format!("need {} bytes, {} left", n, self.remaining())));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> PResult<()> {
        self.take(n).map(|_| ())
    }

    pub fn expect(&mut self, lit: &[u8], what: &str) -> PResult<()> {
        let got = self.take(lit.len())?;
        if got != lit {
            return Err(ParseError { msg: format!("expected {} ({:?}), found {:?}", what, lit, got), offset: self.pos - lit.len() });
        }
        Ok(())
    }

    pub fn u8(&mut self) -> PResult<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn i8(&mut self) -> PResult<i8> {
        Ok(self.take(1)?[0] as i8)
    }

    pub fn u16(&mut self) -> PResult<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> PResult<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64(&mut self) -> PResult<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn f32(&mut self) -> PResult<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub fn f16(&mut self) -> PResult<f32> {
        Ok(half_to_f32(self.u16()?))
    }

    pub fn arr<const N: usize>(&mut self) -> PResult<[u8; N]> {
        let b = self.take(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(b);
        Ok(out)
    }

    pub fn bytes(&mut self, n: usize) -> PResult<Vec<u8>> {
        self.take(n).map(|b| b.to_vec())
    }

    pub fn f32s<const N: usize>(&mut self) -> PResult<[f32; N]> {
        let mut out = [0f32; N];
        for v in out.iter_mut() {
            *v = self.f32()?;
        }
        Ok(out)
    }

    pub fn f16s<const N: usize>(&mut self) -> PResult<[f32; N]> {
        let mut out = [0f32; N];
        for v in out.iter_mut() {
            *v = self.f16()?;
        }
        Ok(out)
    }

    pub fn i8s<const N: usize>(&mut self) -> PResult<[i8; N]> {
        let mut out = [0i8; N];
        for v in out.iter_mut() {
            *v = self.i8()?;
        }
        Ok(out)
    }

    pub fn u32s<const N: usize>(&mut self) -> PResult<[u32; N]> {
        let mut out = [0u32; N];
        for v in out.iter_mut() {
            *v = self.u32()?;
        }
        Ok(out)
    }

    pub fn utf8(&mut self, n: usize) -> PResult<String> {
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    /// Reads `n_bytes` of UTF-16LE text.
    pub fn utf16le(&mut self, n_bytes: usize) -> PResult<String> {
        let b = self.take(n_bytes)?;
        let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Ok(String::from_utf16_lossy(&units))
    }

    /// Reads `n_bytes` of raw bytes and, when the count is nonsensical, fails early
    /// instead of allocating; use for count-prefixed vectors.
    pub fn check_count(&self, count: usize, elem_size: usize) -> PResult<()> {
        let need = count.checked_mul(elem_size).ok_or_else(|| self.err("count overflow"))?;
        if need > self.remaining() {
            return Err(self.err(format!("{} elements × {} bytes exceeds remaining {} bytes", count, elem_size, self.remaining())));
        }
        Ok(())
    }
}

pub fn half_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) & 1) as u32;
    let exp = ((h >> 10) & 0x1f) as u32;
    let frac = (h & 0x3ff) as u32;
    let bits = if exp == 0 {
        if frac == 0 {
            sign << 31
        } else {
            // subnormal: normalise
            let mut e = 127 - 15 + 1;
            let mut f = frac;
            while f & 0x400 == 0 {
                f <<= 1;
                e -= 1;
            }
            (sign << 31) | ((e as u32) << 23) | ((f & 0x3ff) << 13)
        }
    } else if exp == 0x1f {
        (sign << 31) | 0x7f80_0000 | (frac << 13)
    } else {
        (sign << 31) | ((exp + 127 - 15) << 23) | (frac << 13)
    };
    f32::from_bits(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_conversion() {
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0xc000), -2.0);
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert!((half_to_f32(0x3555) - 0.333252).abs() < 1e-5);
        assert!(half_to_f32(0x7c00).is_infinite());
    }
}
