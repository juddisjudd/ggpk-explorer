//! `.tmo` tile material overrides: pairs of `.mat` files, the second replacing the first.

use super::Lines;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TmoFile {
    pub version: u32,
    pub overrides: Vec<Override>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Override {
    pub from: String,
    pub to: String,
}

pub fn parse(text: &str) -> Result<TmoFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let version = lines.version()?;
    let overrides = lines.many(|c| {
        let from = c.file("mat")?;
        // A shipped hideout file doubles the replacement's opening quote.
        let to = c.skip_space().file("mat").or_else(|| c.opt(|c| c.lit("\"")?.file("mat")))?;
        // Some lines trail a comment or other leftovers after the pair.
        c.rest();
        Some(Override { from, to })
    });
    lines.end()?;
    Ok(TmoFile { version, overrides })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_override_pairs() {
        let text = "version 1// FOLIAGE\r\n\r\n// Null\r\n\"Art/Textures/RockyLedgec.mat\" \"Art/Textures/masks/transparentobjectsc.mat\"\r\n\"Art/a.mat\"\t\t\t\"Art\\b.mat\" // why\r\n\"Art/c.mat\"\"Art/d.mat\"\r\n\"Art/e.mat\" \"\"Art/f.mat\"\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.version, 1);
        assert_eq!(f.overrides.len(), 4);
        assert_eq!(f.overrides[1].to, "Art\\b.mat");
        assert_eq!(f.overrides[2].from, "Art/c.mat");
        assert_eq!(f.overrides[3].to, "Art/f.mat");
    }
}
