use serde::{Serialize, Serializer};
use serde::ser::SerializeStruct;

#[derive(Debug, Clone)]
pub struct PsgFile {
    pub roots: Vec<u32>,
    pub groups: Vec<PsgGroup>,
    pub passives_per_orbit: Vec<u8>,
}

impl Serialize for PsgFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("PsgFile", 4)?;
        state.serialize_field("roots", &self.roots)?;
        state.serialize_field("groups", &self.groups)?;
        
        let orbit_radii: &[i32] = &[0, 82, 164, 334, 488, 657, 839, 250, 1076, 1320];
        
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

pub fn get_group_offset(index: usize) -> (f32, f32) {
    match index {
        88 => (-179.20, 202.24),
        92 => (-28.88, -17.30),
        113 => (112.52, -15.35),
        116 => (-3.82, 0.00),
        172 => (-189.71, 219.97),
        188 => (106.57, -154.69),
        198 => (-16.63, 5151.17),
        203 => (-6.16, 258.64),
        224 => (-70.79, -282.32),
        242 => (-110.06, -44.02),
        263 => (807.98, 235.12),
        267 => (13870.36, -3435.60),
        285 => (615.54, -319.45),
        292 => (-464.11, -732.71),
        311 => (366.62, -462.77),
        331 => (66.65, 1104.82),
        338 => (-641.89, 499.32),
        341 => (262.79, 0.42),
        345 => (28.54, -85.63),
        346 => (-189.59, 725.75),
        353 => (31.71, -177.60),
        356 => (32.53, -1105.18),
        373 => (-2.55, 7.69),
        379 => (27.59, -55.17),
        382 => (15.92, -177.11),
        405 => (-768.39, 1184.88),
        406 => (8.35, -41.73),
        418 => (-13.21, 88.10),
        426 => (0.00, 2.05),
        435 => (-99.28, -512.00),
        439 => (-82.71, -479.00),
        490 => (-34.55, 59.84),
        551 => (-344.45, -1748.45),
        570 => (-50.31, -56.59),
        587 => (-5156.53, -1680.28),
        620 => (-19.63, 19.63),
        677 => (-175.26, 25.04),
        684 => (3.09, -75.44),
        698 => (1002.03, -306.13),
        699 => (-8.20, -4.90),
        702 => (-345.46, -87.00),
        705 => (-219.62, -6.03),
        758 => (-4.23, 33.83),
        773 => (-70.77, 59.16),
        776 => (2.78, 30.00),
        781 => (-6.69, 0.00),
        786 => (278.90, 30.88),
        788 => (253.95, 1.30),
        790 => (92.07, 95.61),
        797 => (16.77, 21.57),
        798 => (20.25, 13.01),
        801 => (276.19, 149.27),
        809 => (194.83, 160.93),
        812 => (-16.95, 156.70),
        818 => (-12.71, 177.87),
        845 => (-4.39, 752.10),
        849 => (-0.51, 1.57),
        879 => (-7.88, -13.13),
        906 => (2892.76, -3313.22),
        1012 => (-14.43, 31.71),
        1037 => (-462.86, -72.85),
        1050 => (60.55, 204.69),
        1100 => (-761.30, -395.00),
        1105 => (1176.87, 356.04),
        1167 => (-66.33, 100.91),
        1174 => (-1480.00, -253.16),
        1181 => (-0.00, -1.75),
        1194 => (42.29, 45.80),
        1236 => (-24.92, 6.37),
        1250 => (631.57, -988.90),
        1254 => (827.32, -530.43),
        1279 => (4411.18, -3052.40),
        1283 => (-62.44, -455.36),
        1301 => (-68.78, 1549.43),
        1306 => (-125.88, -103.96),
        1313 => (133.20, -0.10),
        1316 => (1040.90, 361.64),
        1326 => (-78.57, 208.81),
        1328 => (-64.59, -29.38),
        1339 => (150.25, 1639.38),
        1342 => (-11.81, -118.09),
        1359 => (104.31, 488.99),
        1363 => (1064.81, -470.68),
        1394 => (-39.01, 149.71),
        1434 => (-50.00, -419.99),
        1466 => (-99.96, 180.00),
        _ => (0.0, 0.0),
    }
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
    let _graph_type = read_u8(&mut offset)?;
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
    for i in 0..group_length as usize {
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
        
        let offset = get_group_offset(i);
        groups.push(PsgGroup {
            x: x + offset.0,
            y: y + offset.1,
            is_proxy: unknown2 == 1,
            nodes,
        });
    }

    
    Ok(PsgFile {
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
        assert_eq!(result.groups[0].nodes[0].skill_id, 200);
    }

    #[test]
    fn test_psg_serialization() {
        let psg = PsgFile {
            roots: vec![100],
            groups: vec![],
            passives_per_orbit: vec![1, 12, 24, 24, 72, 72, 72, 24, 72, 144],
        };
        let serialized = serde_json::to_string(&psg).expect("Failed to serialize");
        let val: serde_json::Value = serde_json::from_str(&serialized).expect("Failed to parse JSON");
        assert_eq!(val.get("roots").unwrap().as_array().unwrap()[0].as_u64().unwrap(), 100);
        assert_eq!(val.get("orbitRadii").unwrap(), &serde_json::json!([0, 82, 164, 334, 488, 657, 839, 250, 1076, 1320]));
    }
}
