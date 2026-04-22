use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PsgFile {
    pub roots: Vec<u32>,
    pub groups: Vec<PsgGroup>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PsgGroup {
    pub x: f32,
    pub y: f32,
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
        if *offset + 1 > data.len() { return Err("Unexpected EOF parsing u8".to_string()); }
        let val = data[*offset];
        *offset += 1;
        Ok(val)
    };
    
    // Helper to read u32 LE
    let read_u32 = |offset: &mut usize| -> Result<u32, String> {
        if *offset + 4 > data.len() { return Err("Unexpected EOF parsing u32".to_string()); }
        let bytes = [data[*offset], data[*offset+1], data[*offset+2], data[*offset+3]];
        *offset += 4;
        Ok(u32::from_le_bytes(bytes))
    };
    
    // Helper to read i32 LE (for curvature)
    let read_i32 = |offset: &mut usize| -> Result<i32, String> {
        if *offset + 4 > data.len() { return Err("Unexpected EOF parsing i32".to_string()); }
        let bytes = [data[*offset], data[*offset+1], data[*offset+2], data[*offset+3]];
        *offset += 4;
        Ok(i32::from_le_bytes(bytes))
    };

    // Helper to read f32 LE
    let read_f32 = |offset: &mut usize| -> Result<f32, String> {
        if *offset + 4 > data.len() { return Err("Unexpected EOF parsing f32".to_string()); }
        let bytes = [data[*offset], data[*offset+1], data[*offset+2], data[*offset+3]];
        *offset += 4;
        Ok(f32::from_le_bytes(bytes))
    };

    // Header Parsing based on psg2.py
    // version? u8
    // Header Parsing based on Gist research: Root starts at 13.
    // So we skip 13 bytes.
    let header_size = 13;
    if data.len() < header_size {
        return Err("File too small for header".to_string());
    }
    offset += header_size;
    
    // Root Length (u32)
    let root_length = read_u32(&mut offset)?;
    if root_length > 1000 {
        return Err(format!("Unrealistic root length: {}", root_length));
    }
    
    let mut roots = Vec::new();
    for _ in 0..root_length {
        let connection = read_u32(&mut offset)?;
        let _curvature = read_u32(&mut offset)?; // Read 4 bytes for curvature? python says II (two unsigned ints)
        // Wait, python unpack `<II` is 4 bytes + 4 bytes.
        
        roots.push(connection);
    }
    
    // Group Length (u32)
    let group_length = read_u32(&mut offset)?;
    
    let mut groups = Vec::new();
    for _ in 0..group_length {
        // Group Header: x(f32), y(f32), flag(u32, python says I?), unknown1(I), unknown2(I), passive_length(b? no, python says I at end)
        
        let x = read_f32(&mut offset)?;
        let y = read_f32(&mut offset)?;
        let _flag = read_u32(&mut offset)?;
        let _unknown1 = read_u32(&mut offset)?;
        let _unknown2 = read_u8(&mut offset)?; // This is the 'b'
        let passive_length = read_u32(&mut offset)?;
        
        // Skip padding? No padding mentioned.
        
        let mut nodes = Vec::new();
        for _ in 0..passive_length {
            // Node Header: rowid(I), radius(I), position(I), connections_length(I)
            // Python: `<IIII` = 16 bytes
            let rowid = read_u32(&mut offset)?;
            let radius = read_u32(&mut offset)?;
            let position = read_u32(&mut offset)?;
            let connections_length = read_u32(&mut offset)?;
            
            let mut connections = Vec::new();
            for _ in 0..connections_length {
                // Connection: connection(I), curvature(i)
                // Python: `<Ii` = 4 + 4 = 8 bytes.
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
            nodes,
        });
    }
    
    Ok(PsgFile {
        roots,
        groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_psg_parse() {
        // Construct a mock PSG buffer
        let mut buffer = Vec::new();
        
        // Header
        buffer.push(0); // Version
        // Unknown data (11 bytes)
        for _ in 0..11 { buffer.push(0); }
        
        // Root Length: 1
        buffer.extend_from_slice(&1u32.to_le_bytes());
        // Root 1: id=100, curvature=0
        buffer.extend_from_slice(&100u32.to_le_bytes());
        buffer.extend_from_slice(&0u32.to_le_bytes());
        
        // Group Length: 1
        buffer.extend_from_slice(&1u32.to_le_bytes());
        
        // Group 1 Header
        buffer.extend_from_slice(&500.0f32.to_le_bytes()); // x
        buffer.extend_from_slice(&600.0f32.to_le_bytes()); // y
        buffer.extend_from_slice(&0u32.to_le_bytes()); // flag
        buffer.extend_from_slice(&0u32.to_le_bytes()); // unk1
        buffer.push(0); // unk2 (byte)
        buffer.extend_from_slice(&1u32.to_le_bytes()); // passive_length
        
        // Group 1 -> Node 1
        buffer.extend_from_slice(&200u32.to_le_bytes()); // rowid
        buffer.extend_from_slice(&10u32.to_le_bytes()); // radius
        buffer.extend_from_slice(&5u32.to_le_bytes()); // position
        buffer.extend_from_slice(&1u32.to_le_bytes()); // connections_length
        
        // Node 1 -> Connection 1
        buffer.extend_from_slice(&300u32.to_le_bytes()); // conn_id
        buffer.extend_from_slice(&0i32.to_le_bytes()); // curvature
        
        let result = parse_psg(&buffer).expect("Failed to parse PSG");
        assert_eq!(result.roots.len(), 1);
        assert_eq!(result.roots[0], 100);
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0].nodes.len(), 1);
        assert_eq!(result.groups[0].nodes[0].skill_id, 200);
        assert_eq!(result.groups[0].nodes[0].connections[0].node_id, 300);
        assert_eq!(result.groups[0].nodes[0].connections[0].orbit, 0);
    }
}
