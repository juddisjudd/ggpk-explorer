//! `data/mat_table.dat`: the client's "mat table", not a DAT table despite its ending. It saves the renderer recompiling shaders: each entry, most likely a material, leads to its graph combination, and a combination to the parameters inlined into its shader.

pub struct MatTable {
    pub version: u32,
    /// Entry to graph combination, both as hashes the renderer computes, sorted by entry; many entries share a combination.
    pub keys: Vec<(u64, u64)>,
    /// Graph combination to its inlined parameters, as offset and count into a blob of 16-bit numbers, sorted by combination.
    pub records: Vec<(u64, u32, u16)>,
    pub blob_len: usize,
}

impl MatTable {
    /// How many parameters are inlined over all combinations, the figure the client logs on load.
    pub fn inlined(&self) -> usize {
        self.records.iter().map(|(_, _, count)| *count as usize).sum()
    }
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

/// Reads the file's two tables; fails on anything that does not have this shape, so another `.dat` is not taken for one.
pub fn parse(data: &[u8]) -> Result<MatTable, String> {
    let short = || "the file ends before its tables do".to_string();
    let version = u32_at(data, 0).ok_or_else(short)?;
    let count = u32_at(data, 4).ok_or_else(short)? as usize;
    let records_at = count.checked_mul(16).and_then(|bytes| bytes.checked_add(8)).filter(|end| *end + 4 <= data.len()).ok_or_else(short)?;
    let keys: Vec<(u64, u64)> = (0..count).filter_map(|i| Some((u64_at(data, 8 + i * 16)?, u64_at(data, 16 + i * 16)?))).collect();
    if !keys.windows(2).all(|pair| pair[0].0 < pair[1].0) {
        return Err("the keys are not in order".to_string());
    }
    let record_count = u32_at(data, records_at).ok_or_else(short)? as usize;
    let blob_at = record_count.checked_mul(14).and_then(|bytes| bytes.checked_add(records_at + 4)).filter(|end| *end <= data.len()).ok_or_else(short)?;
    let records: Vec<(u64, u32, u16)> = (0..record_count)
        .filter_map(|i| {
            let at = records_at + 4 + i * 14;
            Some((u64_at(data, at)?, u32_at(data, at + 8)?, u16::from_le_bytes(data.get(at + 12..at + 14)?.try_into().ok()?)))
        })
        .collect();
    let blob_len = data.len() - blob_at;
    if records.iter().any(|(_, offset, count)| (*offset as usize + *count as usize) * 2 > blob_len) {
        return Err("a record points past the end of the file".to_string());
    }
    Ok(MatTable { version, keys, records, blob_len })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(keys: &[(u64, u64)], records: &[(u64, u32, u16)], blob: &[u8]) -> Vec<u8> {
        let mut out = 2u32.to_le_bytes().to_vec();
        out.extend((keys.len() as u32).to_le_bytes());
        keys.iter().for_each(|(key, id)| out.extend(key.to_le_bytes().into_iter().chain(id.to_le_bytes())));
        out.extend((records.len() as u32).to_le_bytes());
        for (id, offset, len) in records {
            out.extend(id.to_le_bytes());
            out.extend(offset.to_le_bytes());
            out.extend(len.to_le_bytes());
        }
        out.extend(blob);
        out
    }

    #[test]
    fn both_tables_are_read_and_a_wrong_shape_is_refused() {
        let data = file(&[(5, 100), (9, 100), (12, 200)], &[(100, 0, 3), (200, 3, 2)], &[0; 10]);
        let table = parse(&data).unwrap();
        assert_eq!((table.version, table.keys.len(), table.records.len(), table.blob_len), (2, 3, 2, 10));
        assert_eq!((table.records[1], table.inlined()), ((200, 3, 2), 5));
        assert!(parse(&file(&[(9, 1), (5, 1)], &[], &[])).is_err(), "keys out of order");
        assert!(parse(&file(&[(5, 1)], &[(1, 2, 2)], &[0; 6])).is_err(), "a slice past the blob");
        assert!(parse(&data[..20]).is_err());
        assert!(parse(&[4, 0, 0, 0, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB]).is_err(), "a real DAT table is not one of these");
    }

    /// `cargo test --release mat_table_real_data -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn mat_table_real_data() {
        let (reader, index) = crate::parsers::real_source();
        let info = crate::bundles::extract::find_file_info_by_path(&index, "data/mat_table.dat").expect("no data/mat_table.dat in this install");
        let data = crate::bundles::extract::extract_bundle_file_sync(info, &index, Some(&reader), None).unwrap();
        let table = parse(&data).unwrap();
        let combinations: std::collections::HashSet<u64> = table.keys.iter().map(|(_, combination)| *combination).collect();
        println!("version {}: {} entries lead to {} graph combinations; {} combinations have inlined parameters, {} in all", table.version, table.keys.len(), combinations.len(), table.records.len(), table.inlined());
        assert!(table.records.iter().all(|(combination, _, count)| *count > 0 && combinations.contains(combination)), "every record is a combination some entry uses");
        // The blob holds the parameters end to end, then four bytes this reader does not use.
        assert_eq!(table.inlined() * 2 + 4, table.blob_len);
        assert!(table.records.windows(2).all(|pair| pair[0].1 + pair[0].2 as u32 == pair[1].1), "each list starts where the last one ends");
        let as_paths = table.keys.iter().filter(|(entry, _)| index.files.contains_key(entry)).count();
        assert_eq!(as_paths, 0, "an entry's hash is not the path hash of a file: the renderer computes it at run time");
    }
}
