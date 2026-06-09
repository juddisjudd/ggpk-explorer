use serde::{Serialize, Serializer};
use serde::ser::SerializeStruct;

// Orbit radii differ between graph types. The passive skill tree (graph_type 0)
// and the atlas tree (graph_type 1) place outer orbits at slightly different
// radii; using the wrong table drifts nodes by a few pixels on the outer rings.
// Values verified against poe2-skilltree-export (passive) and poe2-atlas
// constants (atlas).
pub const PASSIVE_ORBIT_RADII: [i32; 10] = [0, 82, 164, 334, 488, 657, 839, 250, 1076, 1320];
pub const ATLAS_ORBIT_RADII: [i32; 10] = [0, 82, 162, 335, 493, 662, 846, 251, 1080, 1332];

#[derive(Debug, Clone)]
pub struct PsgFile {
    pub graph_type: u8,
    pub roots: Vec<u32>,
    pub groups: Vec<PsgGroup>,
    pub passives_per_orbit: Vec<u8>,
}

impl PsgFile {
    /// Orbit radii for this graph, selected by `graph_type` (1 = atlas).
    pub fn orbit_radii(&self) -> [f32; 10] {
        let src = if self.graph_type == 1 { ATLAS_ORBIT_RADII } else { PASSIVE_ORBIT_RADII };
        std::array::from_fn(|i| src[i] as f32)
    }
}

impl Serialize for PsgFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("PsgFile", 4)?;
        state.serialize_field("roots", &self.roots)?;
        state.serialize_field("groups", &self.groups)?;

        let orbit_radii: &[i32] = if self.graph_type == 1 {
            &ATLAS_ORBIT_RADII[..]
        } else {
            &PASSIVE_ORBIT_RADII[..]
        };

        state.serialize_field("orbitRadii", orbit_radii)?;
        state.serialize_field("orbitSizes", &self.passives_per_orbit)?;
        state.end()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PsgGroup {
    pub x: f32,
    pub y: f32,
    #[serde(rename = "isProxy")]
    pub is_proxy: bool,
    pub nodes: Vec<PsgNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PsgConnection {
    pub node_id: u32,
    pub orbit: i32, 
}

#[derive(Debug, Clone, Serialize)]
pub struct PsgNode {
    pub skill_id: u32,
    pub radius: u32,
    pub position: u32,
    pub connections: Vec<PsgConnection>,
}

pub fn parse_psg(data: &[u8]) -> Result<PsgFile, String> {
    let mut offset = 0;
    
    // Helper to read u8
    let read_u8 = |offset: &mut usize| -> Result<u8, String> {
        let val = *data.get(*offset).ok_or_else(|| "Unexpected EOF parsing u8".to_string())?;
        *offset += 1;
        Ok(val)
    };
    
    // Helper to read u32 LE
    let read_u32 = |offset: &mut usize| -> Result<u32, String> {
        let slice = data.get(*offset..*offset+4).ok_or_else(|| "Unexpected EOF parsing u32".to_string())?;
        let bytes: [u8; 4] = slice.try_into().map_err(|_| "Failed to convert slice to array".to_string())?;
        *offset += 4;
        Ok(u32::from_le_bytes(bytes))
    };
    
    // Helper to read i32 LE (for curvature)
    let read_i32 = |offset: &mut usize| -> Result<i32, String> {
        let slice = data.get(*offset..*offset+4).ok_or_else(|| "Unexpected EOF parsing i32".to_string())?;
        let bytes: [u8; 4] = slice.try_into().map_err(|_| "Failed to convert slice to array".to_string())?;
        *offset += 4;
        Ok(i32::from_le_bytes(bytes))
    };

    // Helper to read f32 LE
    let read_f32 = |offset: &mut usize| -> Result<f32, String> {
        let slice = data.get(*offset..*offset+4).ok_or_else(|| "Unexpected EOF parsing f32".to_string())?;
        let bytes: [u8; 4] = slice.try_into().map_err(|_| "Failed to convert slice to array".to_string())?;
        *offset += 4;
        Ok(f32::from_le_bytes(bytes))
    };

    // Header Parsing
    let _version = read_u8(&mut offset)?;
    let graph_type = read_u8(&mut offset)?;
    let passives_per_orbit_len = read_u8(&mut offset)?;
    let mut passives_per_orbit = Vec::new();
    for _ in 0..passives_per_orbit_len {
        passives_per_orbit.push(read_u8(&mut offset)?);
    }
    
    // Root Length (u32)
    let root_length = read_u32(&mut offset)?;
    if root_length > 1000 {
        return Err(format!("Unrealistic root length: {}", root_length));
    }
    
    let mut roots = Vec::new();
    for _ in 0..root_length {
        let connection = read_u32(&mut offset)?;
        let _curvature = read_u32(&mut offset)?;
        
        roots.push(connection);
    }
    
    // Group Length (u32)
    let group_length = read_u32(&mut offset)?;
    
    let mut groups = Vec::new();
    for _ in 0..group_length as usize {
        // Group Header: x(f32), y(f32), flag(u32), unknown1(I), unknown2(I), passive_length
        let x = read_f32(&mut offset)?;
        let y = read_f32(&mut offset)?;
        let _flag = read_u32(&mut offset)?;
        let _unknown1 = read_u32(&mut offset)?;
        let unknown2 = read_u8(&mut offset)?;
        let passive_length = read_u32(&mut offset)?;
        
        let mut nodes = Vec::new();
        for _ in 0..passive_length {
            let rowid = read_u32(&mut offset)?;
            let radius = read_u32(&mut offset)?;
            let position = read_u32(&mut offset)?;
            let connections_length = read_u32(&mut offset)?;
            
            let mut connections = Vec::new();
            for _ in 0..connections_length {
                let conn_id = read_u32(&mut offset)?;
                let orbit = read_i32(&mut offset)?;
                connections.push(PsgConnection { node_id: conn_id, orbit });
            }
            
            nodes.push(PsgNode {
                skill_id: rowid,
                radius,
                position,
                connections,
            });
        }
        
        groups.push(PsgGroup {
            x,
            y,
            is_proxy: unknown2 == 1,
            nodes,
        });
    }

    
    Ok(PsgFile {
        graph_type,
        roots,
        groups,
        passives_per_orbit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_psg_parse() {
        let mut buffer = Vec::new();
        buffer.push(2);
        buffer.push(1);
        buffer.push(10);
        buffer.extend_from_slice(&[1, 12, 24, 24, 72, 72, 72, 24, 72, 144]);
        
        buffer.extend_from_slice(&1u32.to_le_bytes());
        buffer.extend_from_slice(&100u32.to_le_bytes());
        buffer.extend_from_slice(&0u32.to_le_bytes());
        
        buffer.extend_from_slice(&1u32.to_le_bytes());
        buffer.extend_from_slice(&500.0f32.to_le_bytes());
        buffer.extend_from_slice(&600.0f32.to_le_bytes());
        buffer.extend_from_slice(&0u32.to_le_bytes());
        buffer.extend_from_slice(&0u32.to_le_bytes());
        buffer.push(0);
        buffer.extend_from_slice(&1u32.to_le_bytes());
        
        buffer.extend_from_slice(&200u32.to_le_bytes());
        buffer.extend_from_slice(&10u32.to_le_bytes());
        buffer.extend_from_slice(&5u32.to_le_bytes());
        buffer.extend_from_slice(&1u32.to_le_bytes());
        
        buffer.extend_from_slice(&300u32.to_le_bytes());
        buffer.extend_from_slice(&0i32.to_le_bytes());
        
        let result = parse_psg(&buffer).expect("Failed to parse PSG");
        assert_eq!(result.roots.len(), 1);
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0].x, 500.0);
        assert_eq!(result.groups[0].y, 600.0);
        assert_eq!(result.groups[0].nodes[0].skill_id, 200);
        assert_eq!(result.groups[0].nodes[0].position, 5);
    }

    #[test]
    fn test_psg_serialization() {
        let psg = PsgFile {
            graph_type: 0,
            roots: vec![100],
            groups: vec![],
            passives_per_orbit: vec![1, 12, 24, 24, 72, 72, 72, 24, 72, 144],
        };
        let serialized = serde_json::to_string(&psg).expect("Failed to serialize");
        let val: serde_json::Value = serde_json::from_str(&serialized).expect("Failed to parse JSON");
        assert_eq!(val.get("roots").unwrap().as_array().unwrap()[0].as_u64().unwrap(), 100);
        assert_eq!(val.get("orbitRadii").unwrap(), &serde_json::json!([0, 82, 164, 334, 488, 657, 839, 250, 1076, 1320]));
    }

    #[test]
    fn test_dump_psgs() {
        let dumps = [
            (
                "examples/metadata/passiveskillgraph.psg",
                "examples/metadata/passiveskillgraph.json",
            ),
            (
                "examples/metadata/atlasskillgraphs/atlasskillgraph.psg",
                "examples/metadata/atlasskillgraphs/atlasskillgraph.json",
            ),
        ];
        for (psg_path, out_path) in dumps {
            if let Ok(bytes) = std::fs::read(psg_path) {
                if let Ok(psg) = parse_psg(&bytes) {
                    if let Ok(json_val) = serde_json::to_value(&psg) {
                        std::fs::write(out_path, serde_json::to_string_pretty(&json_val).unwrap())
                            .unwrap_or_else(|e| panic!("Failed to write {out_path}: {e}"));
                        println!("Dumped {out_path}");
                    }
                }
            }
        }
    }
}
