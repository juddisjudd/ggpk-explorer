//! `.rs` room sets: the `.arm` rooms a level generator draws from, with allowed rotations.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RsFile {
    pub version: u32,
    pub rooms: Vec<Room>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Room {
    pub weight: Option<u32>,
    pub arm_file: String,
    pub rotations: Vec<String>,
}

fn room(c: &mut Cursor) -> Option<Room> {
    let weight = c.uint();
    let arm_file = c.skip_space().file("arm")?;
    let rotations = c.many(|c| c.sp()?.word().map(String::from));
    Some(Room { weight, arm_file, rotations })
}

pub fn parse(text: &str) -> Result<RsFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let version = lines.version()?;
    let rooms = lines.many(room);
    lines.end()?;
    Ok(RsFile { version, rooms })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_weighted_and_rotated_rooms() {
        let text = "  version 2\r\n\r\n\"Metadata/Rooms/peak_01.arm\" R270 FI\r\n//----\r\n1 \"Metadata/Rooms/Fill_01.arm\"\r\n2\"Metadata/Rooms/Fill_02.arm\"\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.rooms.len(), 3);
        assert_eq!(f.rooms[0].rotations, vec!["R270", "FI"]);
        assert_eq!(f.rooms[1].weight, Some(1));
        assert_eq!(f.rooms[2].weight, Some(2));
    }
}
