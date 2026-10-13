//! Table layouts read out of the game client, which hard-codes every table's row size and pointer columns.

use crate::dat::reader::get_column_size;
use crate::dat::schema::{Column, Table, TableReference};
use crate::dat::table_store;
use crate::data_export::source::GameFiles;
use crate::settings::Game;
use iced_x86::{Decoder, DecoderOptions, Instruction, InstructionInfoFactory, Mnemonic, OpAccess, OpKind, Register};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

const STORED: &str = "client_layout.json";
const ROW_MARKER: u64 = 0xBBBB_BBBB_BBBB_BBBB;
const ROW_SUFFIX: [u8; 10] = [b'_', 0, b'R', 0, b'o', 0, b'w', 0, 0, 0];
const VOLATILE: [Register; 7] = [Register::RAX, Register::RCX, Register::RDX, Register::R8, Register::R9, Register::R10, Register::R11];
// The small PathOfExile_x64.exe beside the client is only a launcher.
const MIN_CLIENT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FUNCTION_BYTES: u32 = 1 << 20;
const MAX_GETTER_BYTES: u32 = 0x400;
const MAX_HELPER_BYTES: u32 = 0x200;
const MAX_PATH_UNITS: usize = 260;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    String,
    Key,
    /// A key into the table's own rows.
    Row,
    List,
}

impl Kind {
    fn width(self) -> usize {
        match self {
            Kind::String | Kind::Row => 8,
            Kind::Key | Kind::List => 16,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pointer {
    pub offset: usize,
    pub kind: Kind,
    /// Bytes per list element, known when the client walks the elements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<usize>,
    /// The table a key, or a list's keys, point into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The client indexes rows of the target's size here, so the target is more than a trace through the code.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub proven: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableLayout {
    pub table: String,
    pub row_size: usize,
    /// False when no row loop was found, so an empty `pointers` proves nothing.
    pub walked: bool,
    pub pointers: Vec<Pointer>,
}

impl Pointer {
    pub fn width(&self) -> usize {
        self.kind.width()
    }

    /// Whether a schema column could be this pointer column: the same kind, and no known difference in what it points at.
    pub fn fits(&self, col: &Column) -> bool {
        let keys = matches!(col.r#type.as_str(), "foreignrow" | "foreign_row");
        let same_target = match (&self.target, &col.references) {
            (Some(theirs), Some(ours)) => theirs.eq_ignore_ascii_case(&ours.table),
            _ => true,
        };
        schema_kind(col) == Some(self.kind) && (self.element != Some(16) || keys || col.r#type == "array") && same_target
    }

    /// A schema column for a pointer no layout names.
    pub fn column(&self) -> Column {
        let (kind, array) = match (self.kind, self.element) {
            (Kind::String, _) => ("string", false),
            (Kind::Key, _) => ("foreignrow", false),
            (Kind::Row, _) => ("row", false),
            (Kind::List, Some(16)) => ("foreignrow", true),
            (Kind::List, _) => ("array", true),
        };
        Column {
            name: None,
            description: Some("read as the game client reads it".to_string()),
            array,
            r#type: kind.to_string(),
            unique: false,
            localized: false,
            references: self.target.clone().map(|table| TableReference { table, column: None }),
            interval: false,
            file: None,
            files: None,
        }
    }
}

impl TableLayout {
    /// Whether a schema column at `offset` cannot be what the client reads there.
    pub fn contradicts(&self, col: &Column, offset: usize) -> bool {
        let end = offset + get_column_size(col, true);
        let mut under = self.pointers.iter().filter(|p| p.offset < end && p.offset + p.width() > offset);
        match (schema_kind(col), under.next()) {
            (Some(kind), Some(pointer)) => pointer.offset != offset || pointer.kind != kind || under.next().is_some(),
            // A key the client never follows leaves no trace; a string or a list always does.
            (Some(kind), None) => matches!(kind, Kind::String | Kind::List),
            (None, pointer) => pointer.is_some(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientLayouts {
    pub tables: Vec<TableLayout>,
}

impl ClientLayouts {
    pub fn find(&self, name: &str) -> Option<&TableLayout> {
        self.tables.iter().find(|t| t.table.eq_ignore_ascii_case(name))
    }

    pub fn read(exe: &[u8]) -> Result<Self, String> {
        let image = Image::parse(exe)?;
        let names = image.row_names();
        let starts: HashSet<u32> = names.iter().flatten().map(|(rva, _)| *rva).collect();
        let references = image.references(&starts);
        let mut found: Vec<(String, usize, Option<u32>)> = Vec::new();
        for candidates in &names {
            let loader = candidates.iter().find_map(|(rva, name)| {
                let callers = references.get(rva)?;
                let (size, fixup) = callers.iter().filter_map(|at| image.function(*at)).find_map(|(begin, end)| loader(&image.decode(begin, end)))?;
                Some((name.clone(), size, fixup))
            });
            if let Some(loader) = loader {
                if !found.iter().any(|(name, _, _)| name.eq_ignore_ascii_case(&loader.0)) {
                    found.push(loader);
                }
            }
        }
        if found.is_empty() {
            return Err("no table loaders found in this program".to_string());
        }
        let sizes = found.iter().map(|(name, size, _)| (name.to_ascii_lowercase(), *size)).collect();
        let mut fixups = Fixups { image: &image, getters: HashMap::new(), helpers: HashMap::new(), sizes };
        let mut tables: Vec<TableLayout> = found
            .into_iter()
            .map(|(table, row_size, fixup)| {
                let body = fixup.and_then(|f| image.function(f).filter(|(begin, _)| *begin == f));
                let pointers = body.and_then(|(begin, end)| fixups.pointers(&image.decode(begin, end), row_size));
                TableLayout { table, row_size, walked: pointers.is_some(), pointers: pointers.unwrap_or_default() }
            })
            .collect();
        tables.sort_by_key(|t| t.table.to_ascii_lowercase());
        Ok(ClientLayouts { tables })
    }
}

fn stored_path(game: Game, version: &str) -> PathBuf {
    table_store::store_dir(game).join(version).join(STORED)
}

/// The largest `PathOfExile*.exe` in an install folder.
pub fn client_binary(root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            name.starts_with("pathofexile") && name.ends_with(".exe")
        })
        .filter_map(|e| Some((e.metadata().ok()?.len(), e.path())))
        .filter(|(len, _)| *len >= MIN_CLIENT_BYTES)
        .max_by_key(|(len, _)| *len)
        .map(|(_, path)| path)
}

/// The layouts kept beside a patch's tables.
pub fn kept(game: Game, version: &str) -> Option<ClientLayouts> {
    serde_json::from_str(&std::fs::read_to_string(stored_path(game, version)).ok()?).ok()
}

/// A patch's layouts: the kept copy, else read from the client in the install or on the CDN and kept.
pub fn for_patch(files: &GameFiles, game: Game, version: &str) -> Result<ClientLayouts, String> {
    let path = stored_path(game, version);
    if let Some(kept) = kept(game, version) {
        return Ok(kept);
    }
    let binary = match files.install_root().as_deref().and_then(client_binary) {
        Some(exe) => std::fs::read(&exe).map_err(|e| format!("{}: {}", exe.display(), e))?,
        None => {
            let cdn = files.cdn.as_ref().ok_or("no game client beside this install")?;
            cdn.fetch_client().map_err(|e| e.to_string())?
        }
    };
    let layouts = ClientLayouts::read(&binary)?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, serde_json::to_string(&layouts).unwrap_or_default());
    Ok(layouts)
}

pub fn schema_kind(col: &Column) -> Option<Kind> {
    if col.array {
        return Some(Kind::List);
    }
    match col.r#type.as_str() {
        "string" | "ref|string" => Some(Kind::String),
        "foreignrow" | "foreign_row" => Some(Kind::Key),
        "row" => Some(Kind::Row),
        _ => None,
    }
}

fn describe_column(col: &Column) -> String {
    let name = col.name.as_deref().filter(|n| !n.is_empty()).unwrap_or("an unnamed column");
    match col.array {
        true => format!("{} ([{}])", name, col.r#type),
        false => format!("{} ({})", name, col.r#type),
    }
}

pub(crate) fn describe_pointer(pointer: &Pointer) -> String {
    let what = match (pointer.kind, pointer.element) {
        (Kind::String, _) => "a string",
        (Kind::Key, _) => "a key",
        (Kind::Row, _) => "a key into its own table",
        (Kind::List, Some(16)) => "a list of keys",
        (Kind::List, _) => "a list",
    };
    match &pointer.target {
        Some(target) => format!("{} to {}", what, target),
        None => what.to_string(),
    }
}

impl ClientLayouts {
    /// Where the schema's reading of a table contradicts the client's, within the bytes the schema covers.
    pub fn disagreements(&self, table: &Table, layout: &TableLayout) -> Vec<String> {
        if !layout.walked {
            return Vec::new();
        }
        let mut at = 0;
        let spans: Vec<(usize, usize, &Column)> = table
            .columns
            .iter()
            .map(|col| {
                let start = at;
                at += get_column_size(col, true);
                (start, at - start, col)
            })
            .collect();
        let schema_len = at;
        let rows_of = |name: &str| self.find(name).map(|t| t.row_size);
        let mut out = Vec::new();
        for pointer in layout.pointers.iter().filter(|p| p.offset < schema_len) {
            let Some((start, _, col)) = spans.iter().find(|(start, len, _)| (*start..start + len).contains(&pointer.offset)) else { continue };
            let client = describe_pointer(pointer);
            let keys = matches!(col.r#type.as_str(), "foreignrow" | "foreign_row");
            if *start != pointer.offset {
                out.push(format!("byte {}: the client reads {}, where the schema has {} from byte {}", pointer.offset, client, describe_column(col), start));
            } else if schema_kind(col) != Some(pointer.kind) || (pointer.element == Some(16) && !keys && col.r#type != "array") {
                out.push(format!("byte {}: the client reads {}, the schema has {}", pointer.offset, client, describe_column(col)));
            } else if let (true, Some(theirs), Some(ours)) = (pointer.proven, pointer.target.as_deref(), col.references.as_ref().map(|r| r.table.as_str())) {
                // Only a row size the schema's target cannot have settles it: two tables may share one.
                if let (Some(indexed), Some(claimed)) = (rows_of(theirs), rows_of(ours)) {
                    if indexed != claimed {
                        let column = describe_column(col);
                        out.push(format!("byte {}: the client reads {} ({}-byte rows), the schema points {} at {} ({}-byte rows)", pointer.offset, client, indexed, column, ours, claimed));
                    }
                }
            }
        }
        // A key the client never follows is left as raw bytes, so only strings and lists must show up.
        for (start, len, col) in &spans {
            let fixed = matches!(schema_kind(col), Some(Kind::String | Kind::List));
            let touched = layout.pointers.iter().any(|p| p.offset < start + len && p.offset + p.kind.width() > *start);
            if fixed && start + len <= layout.row_size && !touched {
                out.push(format!("byte {}: the schema has {}, the client fixes up nothing there", start, describe_column(col)));
            }
        }
        out
    }
}

fn le16(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn le32(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

struct Section {
    rva: u32,
    len: u32,
    file: u32,
    code: bool,
}

struct Image<'a> {
    data: &'a [u8],
    sections: Vec<Section>,
    /// `.pdata`: each function's start, end and unwind record.
    functions: Vec<(u32, u32, u32)>,
}

impl<'a> Image<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, String> {
        let bad = || "not a 64-bit Windows program".to_string();
        let pe = le32(data, 0x3C).ok_or_else(bad)? as usize;
        let optional = pe + 24;
        if data.get(pe..pe + 4) != Some(b"PE\0\0".as_slice()) || le16(data, pe + 4) != Some(0x8664) || le16(data, optional) != Some(0x20B) {
            return Err(bad());
        }
        let count = le16(data, pe + 6).ok_or_else(bad)? as usize;
        let first = optional + le16(data, pe + 20).ok_or_else(bad)? as usize;
        let sections = (0..count)
            .map(|i| {
                let at = first + i * 40;
                Some(Section { rva: le32(data, at + 12)?, len: le32(data, at + 16)?, file: le32(data, at + 20)?, code: le32(data, at + 36)? & 0x2000_0000 != 0 })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(bad)?;
        let mut image = Image { data, sections, functions: Vec::new() };
        let exceptions = optional + 112 + 3 * 8;
        let (rva, len) = (le32(data, exceptions).ok_or_else(bad)?, le32(data, exceptions + 4).ok_or_else(bad)?);
        let start = image.offset(rva).ok_or_else(bad)?;
        image.functions = (0..len as usize / 12)
            .filter_map(|i| {
                let at = start + i * 12;
                Some((le32(data, at)?, le32(data, at + 4)?, le32(data, at + 8)?))
            })
            .collect();
        Ok(image)
    }

    fn offset(&self, rva: u32) -> Option<usize> {
        let s = self.sections.iter().find(|s| rva >= s.rva && rva - s.rva < s.len)?;
        Some(s.file as usize + (rva - s.rva) as usize).filter(|at| *at < self.data.len())
    }

    fn bytes(&self, section: &Section) -> &'a [u8] {
        let start = (section.file as usize).min(self.data.len());
        &self.data[start..(start + section.len as usize).min(self.data.len())]
    }

    /// The function holding `rva`, following chained unwind records back to the function they continue.
    fn function(&self, rva: u32) -> Option<(u32, u32)> {
        let mut i = self.functions.partition_point(|f| f.0 <= rva).checked_sub(1)?;
        if rva >= self.functions[i].1 {
            return None;
        }
        for _ in 0..8 {
            let Some(unwind) = self.offset(self.functions[i].2) else { break };
            let (Some(flags), Some(codes)) = (self.data.get(unwind), self.data.get(unwind + 2)) else { break };
            if (flags >> 3) & 4 == 0 {
                break;
            }
            let Some(parent) = le32(self.data, unwind + 4 + ((*codes as usize + 1) & !1) * 2) else { break };
            match self.functions.binary_search_by_key(&parent, |f| f.0) {
                Ok(found) => i = found,
                Err(_) => break,
            }
        }
        Some((self.functions[i].0, self.functions[i].1))
    }

    /// Instructions addressed by RVA, so every branch and RIP-relative target is an RVA too.
    fn decode(&self, begin: u32, end: u32) -> Vec<Instruction> {
        let Some(start) = self.offset(begin) else { return Vec::new() };
        let len = end.saturating_sub(begin).min(MAX_FUNCTION_BYTES) as usize;
        let Some(code) = self.data.get(start..start + len) else { return Vec::new() };
        Decoder::with_ip(64, code, begin as u64, DecoderOptions::NONE).into_iter().collect()
    }

    fn wide_string(&self, rva: u32) -> Option<String> {
        let start = self.offset(rva)?;
        let units: Vec<u16> = (0..MAX_PATH_UNITS).map_while(|i| le16(self.data, start + i * 2)).take_while(|u| *u != 0).collect();
        units.iter().all(|u| (0x20..0x7F).contains(u)).then(|| units.iter().map(|u| *u as u8 as char).collect())
    }

    /// The table a `Data/…/<Table>.dat` path string names.
    fn table_named(&self, rva: u32) -> Option<String> {
        let path = self.wide_string(rva)?;
        let name = path.strip_prefix("Data/")?.strip_suffix(".dat")?;
        Some(name.rsplit('/').next().unwrap_or(name).to_string())
    }

    /// Every `<Table>_Row` string, as its possible starts, longest first: only the true start is referenced.
    fn row_names(&self) -> Vec<Vec<(u32, String)>> {
        let ident = |at: usize, d: &[u8]| (d[at].is_ascii_alphanumeric() || d[at] == b'_') && d[at + 1] == 0;
        let mut out = Vec::new();
        for section in self.sections.iter().filter(|s| !s.code) {
            let data = self.bytes(section);
            let mut at = 0;
            while at + ROW_SUFFIX.len() <= data.len() {
                if data[at] != b'_' || data[at..at + ROW_SUFFIX.len()] != ROW_SUFFIX {
                    at += 1;
                    continue;
                }
                let mut start = at;
                while start >= 2 && ident(start - 2, data) {
                    start -= 2;
                }
                let candidates: Vec<(u32, String)> = (start..at)
                    .step_by(2)
                    .map(|s| (section.rva + s as u32, data[s..at].iter().step_by(2).map(|b| *b as char).collect()))
                    .collect();
                if !candidates.is_empty() {
                    out.push(candidates);
                }
                at += ROW_SUFFIX.len();
            }
        }
        out
    }

    /// Where code names each target through a RIP-relative displacement.
    fn references(&self, targets: &HashSet<u32>) -> HashMap<u32, Vec<u32>> {
        let mut out: HashMap<u32, Vec<u32>> = HashMap::new();
        let (Some(low), Some(high)) = (targets.iter().min().copied(), targets.iter().max().copied()) else { return out };
        for section in self.sections.iter().filter(|s| s.code) {
            let code = self.bytes(section);
            for (i, window) in code.windows(4).enumerate() {
                let rel = i32::from_le_bytes([window[0], window[1], window[2], window[3]]) as i64;
                let target = section.rva as i64 + i as i64 + 4 + rel;
                if target >= low as i64 && target <= high as i64 && targets.contains(&(target as u32)) {
                    out.entry(target as u32).or_default().push(section.rva + i as u32);
                }
            }
        }
        out
    }
}

fn register(ins: &Instruction, n: u32) -> Option<Register> {
    (n < ins.op_count() && ins.op_kind(n) == OpKind::Register).then(|| ins.op_register(n).full_register())
}

fn immediate(ins: &Instruction, n: u32) -> Option<i64> {
    let is_immediate = n < ins.op_count()
        && matches!(
            ins.op_kind(n),
            OpKind::Immediate8
                | OpKind::Immediate16
                | OpKind::Immediate32
                | OpKind::Immediate64
                | OpKind::Immediate8to16
                | OpKind::Immediate8to32
                | OpKind::Immediate8to64
                | OpKind::Immediate32to64
        );
    is_immediate.then(|| ins.immediate(n) as i64)
}

fn has_memory(ins: &Instruction) -> bool {
    (0..ins.op_count()).any(|n| ins.op_kind(n) == OpKind::Memory)
}

fn is_stack(reg: Register) -> bool {
    matches!(reg, Register::RSP | Register::RBP)
}

fn stack_slot(ins: &Instruction) -> Option<(Register, i64)> {
    (has_memory(ins) && is_stack(ins.memory_base()) && ins.memory_index() == Register::None).then(|| (ins.memory_base(), ins.memory_displacement64() as i64))
}

fn access(kind: OpAccess) -> (bool, bool) {
    let reads = matches!(kind, OpAccess::Read | OpAccess::CondRead | OpAccess::ReadWrite | OpAccess::ReadCondWrite);
    let writes = matches!(kind, OpAccess::Write | OpAccess::CondWrite | OpAccess::ReadWrite | OpAccess::ReadCondWrite);
    (reads, writes)
}

/// A register's value as `rows * a + b`, where `rows` is the count read from the file.
type Sum = (i64, i64);

struct Sums {
    regs: HashMap<Register, Sum>,
    slots: HashMap<(Register, i64), Sum>,
    info: InstructionInfoFactory,
}

impl Sums {
    fn new() -> Self {
        Sums { regs: HashMap::new(), slots: HashMap::new(), info: InstructionInfoFactory::new() }
    }

    fn set(&mut self, reg: Register, value: Option<Sum>) {
        match value {
            Some(v) => self.regs.insert(reg, v),
            None => self.regs.remove(&reg),
        };
    }

    fn step(&mut self, ins: &Instruction) {
        let known = |regs: &HashMap<Register, Sum>, n: u32| register(ins, n).and_then(|r| regs.get(&r).copied());
        match (ins.mnemonic(), register(ins, 0)) {
            (Mnemonic::Mov | Mnemonic::Movzx | Mnemonic::Movsxd | Mnemonic::Movsx, Some(dst)) => {
                let value = match ins.op_kind(1) {
                    OpKind::Register => known(&self.regs, 1),
                    OpKind::Memory => match stack_slot(ins) {
                        Some(slot) => self.slots.get(&slot).copied(),
                        None => {
                            let base = ins.memory_base();
                            let count = ins.mnemonic() == Mnemonic::Mov && ins.memory_size().size() == 4 && ins.memory_index() == Register::None;
                            (count && !is_stack(base) && !matches!(base, Register::None | Register::RIP)).then_some((1, 0))
                        }
                    },
                    _ => immediate(ins, 1).map(|v| (0, v)),
                };
                self.set(dst, value);
            }
            (Mnemonic::Mov, None) if ins.op_kind(0) == OpKind::Memory => {
                if let Some(slot) = stack_slot(ins) {
                    match known(&self.regs, 1) {
                        Some(v) => self.slots.insert(slot, v),
                        None => self.slots.remove(&slot),
                    };
                }
            }
            (Mnemonic::Shl, Some(dst)) => {
                let value = self.regs.get(&dst).copied().zip(immediate(ins, 1)).map(|(v, n)| (v.0.wrapping_shl(n as u32), v.1.wrapping_shl(n as u32)));
                self.set(dst, value);
            }
            (Mnemonic::Imul, Some(dst)) if ins.op_count() == 3 => {
                let value = known(&self.regs, 1).zip(immediate(ins, 2)).map(|(v, n)| (v.0.wrapping_mul(n), v.1.wrapping_mul(n)));
                self.set(dst, value);
            }
            (Mnemonic::Lea, Some(dst)) => {
                let mut value = (!ins.is_ip_rel_memory_operand()).then_some((0i64, ins.memory_displacement64() as i64));
                for (reg, scale) in [(ins.memory_base(), 1), (ins.memory_index(), ins.memory_index_scale() as i64)] {
                    if reg != Register::None {
                        let part = self.regs.get(&reg.full_register()).copied();
                        value = value.zip(part).map(|(v, p)| (v.0.wrapping_add(p.0.wrapping_mul(scale)), v.1.wrapping_add(p.1.wrapping_mul(scale))));
                    }
                }
                self.set(dst, value);
            }
            (Mnemonic::Add | Mnemonic::Sub, Some(dst)) => {
                let sign = if ins.mnemonic() == Mnemonic::Add { 1 } else { -1 };
                let by = immediate(ins, 1).map(|n| (0, n)).or_else(|| known(&self.regs, 1));
                let value = self.regs.get(&dst).copied().zip(by).map(|(v, by)| (v.0.wrapping_add(sign * by.0), v.1.wrapping_add(by.1.wrapping_mul(sign))));
                self.set(dst, value);
            }
            (Mnemonic::Call, _) => {
                for reg in VOLATILE {
                    self.regs.remove(&reg);
                }
            }
            _ => {
                for used in self.info.info(ins).used_registers() {
                    if access(used.access()).1 {
                        self.regs.remove(&used.register().full_register());
                    }
                }
            }
        }
    }
}

/// A table loader's row size and the routine it hands the rows to: the size is what the row count is multiplied by to find the `0xBB` marker.
fn loader(insns: &[Instruction]) -> Option<(usize, Option<u32>)> {
    for (i, ins) in insns.iter().enumerate() {
        if ins.mnemonic() != Mnemonic::Mov || ins.op_count() != 2 || ins.op_kind(1) != OpKind::Immediate64 || ins.immediate(1) != ROW_MARKER {
            continue;
        }
        let marker = register(ins, 0);
        let is_check = |c: &Instruction| c.mnemonic() == Mnemonic::Cmp && c.op_kind(0) == OpKind::Memory && marker.is_some() && register(c, 1) == marker;
        let Some(check) = (i + 1..insns.len().min(i + 6)).find(|j| is_check(&insns[*j])) else { continue };
        let mut sums = Sums::new();
        for ins in &insns[i.saturating_sub(60)..check] {
            sums.step(ins);
        }
        let at = &insns[check];
        let sizes: Vec<i64> = [(at.memory_base(), 1), (at.memory_index(), at.memory_index_scale() as i64)]
            .into_iter()
            .filter_map(|(reg, scale)| sums.regs.get(&reg.full_register()).filter(|v| v.0 != 0).map(|v| v.0.wrapping_mul(scale)))
            .collect();
        let [size] = sizes[..] else { continue };
        if size <= 0 {
            continue;
        }
        let mut fixup = None;
        for ins in &insns[check..] {
            if ins.mnemonic() == Mnemonic::Call && ins.op_kind(0) == OpKind::NearBranch64 && sums.regs.get(&Register::R8) == Some(&(size, 0)) {
                fixup = Some(ins.near_branch_target() as u32);
                break;
            }
            sums.step(ins);
        }
        return Some((size as usize, fixup));
    }
    None
}

/// The register that walks the rows and the instructions of one pass over a row.
fn row_loop(insns: &[Instruction], row_size: usize) -> Option<(Register, usize, usize)> {
    let size = row_size as i64;
    // MSVC writes a 128-byte step as `sub reg, -128`, which fits one byte where `add reg, 128` does not.
    let steps = |ins: &Instruction| -> Option<Register> {
        let (reg, by) = (register(ins, 0)?, immediate(ins, 1)?);
        let forward = (ins.mnemonic() == Mnemonic::Add && by == size) || (ins.mnemonic() == Mnemonic::Sub && by == -size);
        (forward && !is_stack(reg)).then_some(reg)
    };
    let uses = |reg: Register| {
        insns
            .iter()
            .filter(|i| has_memory(i) && i.memory_base() == reg && i.memory_index() == Register::None && (0..size).contains(&(i.memory_displacement64() as i64)))
            .count()
    };
    let mut steppers: Vec<Register> = insns.iter().filter_map(steps).collect();
    steppers.sort_by_key(|r| *r as usize);
    steppers.dedup();
    let row = steppers.into_iter().max_by_key(|r| uses(*r))?;
    let end = insns.iter().rposition(|i| steps(i) == Some(row))?;
    let loads = |i: &Instruction| i.mnemonic() == Mnemonic::Mov && i.op_count() == 2 && i.op_kind(0) == OpKind::Register && i.op_register(0) == row;
    let begin = insns[..end].iter().rposition(loads).map_or(0, |i| i + 1);
    Some((row, begin, end))
}

#[derive(Clone)]
enum Holds {
    /// The row pointer plus this many bytes.
    Row(i64),
    /// An element of the list whose count sits at this row offset.
    Item(i64),
    Handle(String),
    HandlePointer(String),
    Slot((Register, i64)),
}

#[derive(Default, Clone, Copy)]
struct Cell {
    read8: bool,
    write8: bool,
    read4: bool,
    element: Option<usize>,
}

struct Fixups<'i, 'a> {
    image: &'i Image<'a>,
    getters: HashMap<u32, Option<String>>,
    helpers: HashMap<u32, Option<usize>>,
    /// Row size by lower-cased table name.
    sizes: HashMap<String, usize>,
}

impl Fixups<'_, '_> {
    /// The table a small accessor returns a handle to, when it names exactly one.
    fn getter(&mut self, rva: u32) -> Option<String> {
        if let Some(known) = self.getters.get(&rva) {
            return known.clone();
        }
        let image = self.image;
        let named = image.function(rva).filter(|(begin, end)| *begin == rva && end - begin <= MAX_GETTER_BYTES).and_then(|(begin, end)| {
            let mut names: Vec<String> = image
                .decode(begin, end)
                .iter()
                .filter(|i| i.mnemonic() == Mnemonic::Lea && i.is_ip_rel_memory_operand())
                .filter_map(|i| image.table_named(i.ip_rel_memory_address() as u32))
                .collect();
            names.sort();
            names.dedup();
            (names.len() == 1).then(|| names.remove(0))
        });
        self.getters.insert(rva, named.clone());
        named
    }

    /// The row size of the table a key-resolving helper indexes: it multiplies the key by it.
    fn helper_row_size(&mut self, rva: u32) -> Option<usize> {
        if let Some(known) = self.helpers.get(&rva) {
            return *known;
        }
        let image = self.image;
        let size = image.function(rva).filter(|(begin, end)| *begin == rva && end - begin <= MAX_HELPER_BYTES).and_then(|(begin, end)| {
            let mut sums = Sums::new();
            sums.regs.insert(Register::RDX, (1, 0));
            for ins in image.decode(begin, end) {
                let scaled = |reg: Register| sums.regs.get(&reg.full_register()).filter(|v| v.0 > 0 && v.1 == 0).map(|v| v.0);
                let found = match ins.mnemonic() {
                    Mnemonic::Add if has_memory(&ins) && ins.memory_size().size() == 8 => register(&ins, 0).and_then(scaled),
                    Mnemonic::Lea if ins.memory_index() != Register::None && ins.memory_displacement64() == 0 && !sums.regs.contains_key(&ins.memory_base()) => {
                        scaled(ins.memory_index()).map(|a| a * ins.memory_index_scale() as i64)
                    }
                    _ => None,
                };
                if let Some(size) = found {
                    return Some(size as usize);
                }
                sums.step(&ins);
            }
            None
        });
        self.helpers.insert(rva, size);
        size
    }

    /// The pointer columns a fixup routine rewrites in each row, or `None` when it has no row loop.
    fn pointers(&mut self, insns: &[Instruction], row_size: usize) -> Option<Vec<Pointer>> {
        let (row, begin, end) = row_loop(insns, row_size)?;
        let size = row_size as i64;
        let mut info = InstructionInfoFactory::new();
        let mut cells: BTreeMap<i64, Cell> = BTreeMap::new();
        let mut regs: HashMap<Register, Holds> = HashMap::new();
        let mut slots: HashMap<(Register, i64), String> = HashMap::new();
        let mut targets: HashMap<i64, (String, Option<u32>)> = HashMap::new();
        let mut pending: Option<String> = None;

        for (i, ins) in insns.iter().enumerate() {
            let in_loop = (begin..end).contains(&i);
            let within = |regs: &HashMap<Register, Holds>, base: Register, disp: i64| -> Option<i64> {
                let from_row = match regs.get(&base) {
                    _ if base == row => 0,
                    Some(Holds::Row(k)) => *k,
                    _ => return None,
                };
                (in_loop && (0..size).contains(&(from_row + disp))).then_some(from_row + disp)
            };
            let handle_of = |regs: &HashMap<Register, Holds>, n: u32| match register(ins, n).and_then(|r| regs.get(&r)) {
                Some(Holds::Handle(table)) if ins.mnemonic() == Mnemonic::Mov => Some(table.clone()),
                _ => None,
            };
            let stored = handle_of(&regs, 1);

            for used in info.info(ins).used_memory() {
                let (reads, writes) = access(used.access());
                if !(reads || writes) || used.index() != Register::None {
                    continue;
                }
                let (base, disp, width) = (used.base().full_register(), used.displacement() as i64, used.memory_size().size() as i64);
                if writes && is_stack(base) {
                    for part in (0..width.max(8)).step_by(8) {
                        slots.remove(&(base, disp + part));
                    }
                }
                if let Some(at) = within(&regs, base, disp) {
                    match width {
                        // Two neighbouring strings are rebased with one 16-byte SSE add.
                        8 | 16 | 32 => {
                            for part in (0..width).step_by(8) {
                                let cell = cells.entry(at + part).or_default();
                                cell.read8 |= reads;
                                cell.write8 |= writes;
                            }
                        }
                        4 => cells.entry(at).or_default().read4 |= reads,
                        _ => {}
                    }
                    if let (true, 8, Some(table)) = (writes, width, &stored) {
                        targets.insert(at - 8, (table.clone(), None));
                    }
                } else if let (true, true, 8, 8, Some(Holds::Item(list)), Some(table)) = (in_loop, writes, width, disp, regs.get(&base), &stored) {
                    targets.insert(*list, (table.clone(), None));
                }
            }

            let dst = register(ins, 0);
            let plain = has_memory(ins) && ins.memory_index() == Register::None;
            let (base, disp) = (ins.memory_base(), ins.memory_displacement64() as i64);
            let mut set: Option<Holds> = None;
            match ins.mnemonic() {
                Mnemonic::Lea if ins.is_ip_rel_memory_operand() => {
                    if let Some(table) = self.image.table_named(ins.ip_rel_memory_address() as u32) {
                        pending = Some(table);
                    }
                }
                Mnemonic::Lea if plain => {
                    set = match within(&regs, base, disp) {
                        Some(at) => Some(Holds::Row(at)),
                        None => is_stack(base).then_some(Holds::Slot((base, disp))),
                    };
                }
                Mnemonic::Mov if dst.is_some() && ins.op_kind(1) == OpKind::Register => {
                    let from = ins.op_register(1).full_register();
                    set = if in_loop && from == row { Some(Holds::Row(0)) } else { regs.get(&from).cloned() };
                }
                Mnemonic::Mov if dst.is_some() && plain => {
                    set = match regs.get(&base) {
                        Some(Holds::HandlePointer(table)) if disp == 0 => Some(Holds::Handle(table.clone())),
                        _ => slots.get(&(base, disp)).map(|table| Holds::Handle(table.clone())),
                    };
                }
                Mnemonic::Mov if plain && is_stack(base) => {
                    if let Some(table) = &stored {
                        slots.insert((base, disp), table.clone());
                    }
                }
                Mnemonic::Add if dst.is_some() && plain => {
                    if let Some(at) = within(&regs, base, disp) {
                        set = Some(Holds::Item(at - 8));
                        let shift = i.checked_sub(1).map(|p| &insns[p]).filter(|p| p.mnemonic() == Mnemonic::Shl && register(p, 0) == dst).and_then(|p| immediate(p, 1));
                        if let Some(shift) = shift.filter(|s| (0..8).contains(s)) {
                            cells.entry(at).or_default().element = Some(1 << shift);
                        }
                    }
                }
                Mnemonic::Call => {
                    let (first, second, third) = (regs.get(&Register::RCX).cloned(), regs.get(&Register::RDX).cloned(), regs.get(&Register::R8).cloned());
                    let callee = (ins.op_kind(0) == OpKind::NearBranch64).then(|| ins.near_branch_target() as u32);
                    if let (true, Some(Holds::Row(at) | Holds::Item(at)), Some(Holds::Slot(slot))) = (in_loop, &first, &third) {
                        if let Some(table) = slots.get(slot) {
                            targets.insert(*at, (table.clone(), callee));
                        }
                    }
                    for reg in VOLATILE {
                        regs.remove(&reg);
                    }
                    let named = callee.and_then(|rva| self.getter(rva));
                    match (named, first, second) {
                        (Some(table), first, _) => {
                            if let Some(Holds::Slot(slot)) = first {
                                slots.insert(slot, table.clone());
                            }
                            regs.insert(Register::RAX, Holds::HandlePointer(table));
                        }
                        // A lookup by path takes the path and the place for the handle, both on the stack.
                        (None, Some(Holds::Slot(slot)), Some(Holds::Slot(_))) => {
                            if let Some(table) = pending.take() {
                                slots.insert(slot, table);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                _ => {}
            }
            for used in info.info(ins).used_registers() {
                if access(used.access()).1 {
                    regs.remove(&used.register().full_register());
                }
            }
            if let (Some(dst), Some(holds)) = (dst, set) {
                regs.insert(dst, holds);
            }
        }

        // A helper that indexes rows of another size was handed some other table: the trace went wrong.
        let targets: HashMap<i64, (String, bool)> = targets
            .into_iter()
            .filter_map(|(at, (table, helper))| {
                let baked = helper.and_then(|rva| self.helper_row_size(rva));
                let proven = baked.is_some() && baked == self.sizes.get(&table.to_ascii_lowercase()).copied();
                (baked.is_none() || proven).then_some((at, (table, proven)))
            })
            .collect();
        let mut pointers = Vec::new();
        let mut taken: HashSet<i64> = HashSet::new();
        for (&at, cell) in &cells {
            if taken.contains(&at) {
                continue;
            }
            let next = cells.get(&(at + 8)).copied().unwrap_or_default();
            let kind = if (cell.read8 || cell.read4) && cell.write8 && next.write8 && !next.read8 && !next.read4 {
                Kind::Key
            } else if cell.read8 && !cell.write8 && next.read8 && next.write8 {
                Kind::List
            } else if cell.read8 && cell.write8 {
                Kind::String
            } else if cell.read4 && cell.write8 {
                Kind::Row
            } else {
                continue;
            };
            if kind.width() == 16 {
                taken.insert(at + 8);
            }
            let element = if kind == Kind::List { next.element } else { None };
            let (target, proven) = match targets.get(&at).filter(|_| matches!(kind, Kind::Key | Kind::List)) {
                Some((table, proven)) => (Some(table.clone()), *proven),
                None => (None, false),
            };
            pointers.push(Pointer { offset: at as usize, kind, element, target, proven });
        }
        Some(pointers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::schema::TableReference;

    fn col(name: &str, kind: &str, array: bool, target: Option<&str>) -> Column {
        Column {
            name: Some(name.to_string()),
            description: None,
            array,
            r#type: kind.to_string(),
            unique: false,
            localized: false,
            references: target.map(|t| TableReference { table: t.to_string(), column: None }),
            interval: false,
            file: None,
            files: None,
        }
    }

    fn pointer(offset: usize, kind: Kind) -> Pointer {
        Pointer { offset, kind, element: None, target: None, proven: false }
    }

    fn key(offset: usize, target: &str) -> Pointer {
        Pointer { target: Some(target.to_string()), proven: true, ..pointer(offset, Kind::Key) }
    }

    fn table(columns: Vec<Column>) -> Table {
        Table { name: "T".to_string(), columns, tags: None, valid_for: None, custom: false }
    }

    fn layout(table: &str, row_size: usize, pointers: Vec<Pointer>) -> TableLayout {
        TableLayout { table: table.to_string(), row_size, walked: true, pointers }
    }

    fn findings(schema: &Table, pointers: Vec<Pointer>, row_size: usize) -> Vec<String> {
        let all = ClientLayouts { tables: vec![layout("T", row_size, pointers), layout("Stats", 106, Vec::new()), layout("Mods", 693, Vec::new()), layout("Tags", 106, Vec::new())] };
        all.disagreements(schema, &all.tables[0])
    }

    #[test]
    fn a_schema_the_client_agrees_with_has_no_findings() {
        let schema = table(vec![col("Id", "string", false, None), col("Level", "i32", false, None), col("Stat", "foreignrow", false, Some("Stats")), col("Tags", "foreignrow", true, Some("Tags"))]);
        let tags = Pointer { element: Some(16), ..pointer(28, Kind::List) };
        assert_eq!(findings(&schema, vec![pointer(0, Kind::String), key(12, "stats"), tags], 44), Vec::<String>::new());
    }

    #[test]
    fn a_key_the_client_never_follows_is_not_a_finding() {
        let schema = table(vec![col("Id", "string", false, None), col("Area", "foreignrow", false, Some("WorldAreas"))]);
        assert!(findings(&schema, vec![pointer(0, Kind::String)], 24).is_empty());
    }

    #[test]
    fn misread_columns_are_named_by_byte() {
        let schema = table(vec![
            col("Id", "string", false, None),
            col("Recipe", "foreignrow", false, Some("Items")),
            col("Flag", "bool", false, None),
            col("Scripts", "string", true, None),
            col("Weight", "i32", false, None),
            col("Text", "string", false, None),
            col("Stat", "foreignrow", false, Some("Stats")),
        ]);
        let found = findings(&schema, vec![pointer(0, Kind::String), pointer(8, Kind::List), pointer(26, Kind::List), key(53, "Mods")], 69);
        assert_eq!(found.len(), 4, "{:#?}", found);
        assert!(found[0].starts_with("byte 8: the client reads a list, the schema has Recipe (foreignrow)"));
        assert!(found[1].starts_with("byte 26: the client reads a list, where the schema has Scripts ([string]) from byte 25"));
        assert_eq!(found[2], "byte 53: the client reads a key to Mods (693-byte rows), the schema points Stat (foreignrow) at Stats (106-byte rows)");
        assert!(found[3].starts_with("byte 45: the schema has Text (string), the client fixes up nothing there"));
    }

    #[test]
    fn a_target_is_disputed_only_on_proof() {
        let schema = table(vec![col("Stat", "foreignrow", false, Some("Stats"))]);
        let traced = Pointer { proven: false, ..key(0, "Mods") };
        assert!(findings(&schema, vec![traced], 16).is_empty());
        assert!(findings(&schema, vec![key(0, "Tags")], 16).is_empty(), "Tags and Stats share a row size");
        assert_eq!(findings(&schema, vec![key(0, "Mods")], 16).len(), 1);
    }

    #[test]
    fn a_list_of_keys_fits_a_list_the_schema_cannot_type() {
        let schema = table(vec![col("Things", "array", true, None), col("Counts", "i32", true, None)]);
        let things = Pointer { element: Some(16), ..pointer(0, Kind::List) };
        let counts = Pointer { element: Some(16), ..pointer(16, Kind::List) };
        let found = findings(&schema, vec![things, counts], 32);
        assert_eq!(found, vec!["byte 16: the client reads a list of keys, the schema has Counts ([i32])".to_string()]);
    }

    #[test]
    fn nothing_is_claimed_when_no_row_loop_was_found() {
        let schema = table(vec![col("Id", "string", false, None)]);
        let all = ClientLayouts { tables: vec![TableLayout { table: "T".to_string(), row_size: 8, walked: false, pointers: Vec::new() }] };
        assert!(all.disagreements(&schema, &all.tables[0]).is_empty());
    }
}

#[cfg(test)]
mod real_data_tests {
    use super::*;
    use crate::dat::reader::DatReader;
    use crate::dat::relational::FileSource;
    use crate::dat::schema::Schema;
    use crate::dat::table_store::StoredTables;
    use crate::settings::AppSettings;

    /// `cargo test --release client_layouts_match_kept_tables -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn client_layouts_match_kept_tables() {
        let settings = AppSettings::load();
        let path = settings.schema_local_path.clone().map(PathBuf::from).unwrap_or_else(|| AppSettings::get_app_data_dir().join("schema.min.json"));
        let schema: Schema = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut games = 0;
        for game in [Game::Poe2, Game::Poe1] {
            let exe = settings.ggpk_path_for(game).and_then(|p| Path::new(p).parent().and_then(client_binary));
            let (Some(exe), Some(version)) = (exe, table_store::versions(game).into_iter().next()) else { continue };
            let stored = StoredTables::open(game, &version).unwrap();
            let started = std::time::Instant::now();
            let layouts = ClientLayouts::read(&std::fs::read(&exe).unwrap()).unwrap();
            println!("{}: {} table(s) from {} in {:.1?}, against kept patch {}", game.label(), layouts.tables.len(), exe.display(), started.elapsed(), version);

            let (mut sized, mut same_size, mut compared, mut agreed) = (0, 0, 0, 0);
            let (mut targets, mut same_target, mut pointers) = (0, 0, 0);
            for layout in &layouts.tables {
                pointers += layout.pointers.len();
                let file = format!("{}{}.datc64", table_store::table_dir(game), layout.table.to_ascii_lowercase());
                let reader = stored.fetch(&file).and_then(|bytes| DatReader::new(bytes, &file).ok());
                if let Some(len) = reader.as_ref().filter(|r| r.row_count > 0).and_then(|r| r.row_length) {
                    sized += 1;
                    same_size += usize::from(len == layout.row_size);
                    if len != layout.row_size {
                        println!("  row size differs {}: client {} file {}", layout.table, layout.row_size, len);
                    }
                }
                let Some(def) = schema.find_table(&layout.table, game.is_poe2()) else { continue };
                let schema_len: usize = def.columns.iter().map(|c| get_column_size(c, true)).sum();
                if schema_len != layout.row_size {
                    continue;
                }
                compared += 1;
                let found = layouts.disagreements(def, layout);
                agreed += usize::from(found.is_empty());
                for line in &found {
                    println!("  {}: {}", layout.table, line);
                }
                let mut at = 0;
                for col in &def.columns {
                    let client = layout.pointers.iter().find(|p| p.offset == at).and_then(|p| p.target.as_ref());
                    if let (Some(client), Some(wanted)) = (client, &col.references) {
                        targets += 1;
                        same_target += usize::from(client.eq_ignore_ascii_case(&wanted.table));
                    }
                    at += get_column_size(col, true);
                }
            }
            println!("  row size equals the kept file: {} of {}", same_size, sized);
            println!("  no disagreement with the schema: {} of {} tables of equal row size", agreed, compared);
            println!("  {} pointer column(s); key target equals the schema's: {} of {}", pointers, same_target, targets);
            assert!(layouts.tables.len() > 700, "too few loaders found");
            assert!(same_size * 100 >= sized * 98, "row sizes disagree with the kept files");
            assert!(agreed * 100 >= compared * 90, "pointer columns disagree with the schema");
            games += 1;
        }
        assert!(games > 0, "no configured install with a client and kept tables");
    }
}

#[cfg(test)]
mod machine_code_tests {
    use super::*;
    use iced_x86::code_asm::*;
    use iced_x86::BlockEncoderOptions;

    const BASE: u64 = 0x1000;

    fn decode(code: &[u8]) -> Vec<Instruction> {
        Decoder::with_ip(64, code, BASE, DecoderOptions::NONE).into_iter().collect()
    }

    #[test]
    fn a_loader_gives_its_row_size_and_fixup() {
        let mut a = CodeAssembler::new(64).unwrap();
        let mut bad = a.create_label();
        a.mov(edx, dword_ptr(r9)).unwrap();
        a.lea(r8, qword_ptr(r9 + 4)).unwrap();
        a.imul_3(r13, rdx, 48).unwrap();
        a.mov(qword_ptr(rbp + 0x60), r13).unwrap();
        a.mov(rax, ROW_MARKER).unwrap();
        a.cmp(qword_ptr(r8 + r13), rax).unwrap();
        a.jne(bad).unwrap();
        a.mov(r9, r14).unwrap();
        a.mov(r8, qword_ptr(rbp + 0x60)).unwrap();
        a.mov(rcx, rsi).unwrap();
        a.call(0x9000u64).unwrap();
        a.set_label(&mut bad).unwrap();
        a.ret().unwrap();
        assert_eq!(loader(&decode(&a.assemble(BASE).unwrap())), Some((48, Some(0x9000))));
    }

    #[test]
    fn a_row_size_built_from_shifts_is_followed() {
        let mut a = CodeAssembler::new(64).unwrap();
        a.mov(edx, dword_ptr(r9)).unwrap();
        a.lea(rax, qword_ptr(rdx + rdx * 2)).unwrap();
        a.shl(rax, 3).unwrap();
        a.mov(r13, rax).unwrap();
        a.mov(rcx, ROW_MARKER).unwrap();
        a.cmp(qword_ptr(r9 + r13 + 4), rcx).unwrap();
        a.ret().unwrap();
        assert_eq!(loader(&decode(&a.assemble(BASE).unwrap())), Some((24, None)));
    }

    #[test]
    fn a_row_step_of_128_is_a_subtraction() {
        let mut a = CodeAssembler::new(64).unwrap();
        a.mov(rbx, qword_ptr(r12)).unwrap();
        a.mov(rax, qword_ptr(rbx + 8)).unwrap();
        a.sub(rbx, -128).unwrap();
        a.ret().unwrap();
        assert_eq!(row_loop(&decode(&a.assemble(BASE).unwrap()), 128), Some((Register::RBX, 1, 2)));
    }

    /// A 48-byte row: a string, a key and a list of keys into Stats, and a key into the table itself.
    #[test]
    fn a_fixup_gives_each_pointer_column_and_its_target() {
        let mut a = CodeAssembler::new(64).unwrap();
        let (mut getter, mut helper, mut fixup, mut path, mut unwind) = (a.create_label(), a.create_label(), a.create_label(), a.create_label(), a.create_label());
        let (mut top, mut done, mut resolve, mut after_key) = (a.create_label(), a.create_label(), a.create_label(), a.create_label());
        let (mut empty, mut store, mut each, mut list_done, mut own, mut own_store) = (a.create_label(), a.create_label(), a.create_label(), a.create_label(), a.create_label(), a.create_label());

        a.set_label(&mut getter).unwrap();
        a.lea(rax, ptr(path)).unwrap();
        a.ret().unwrap();

        a.set_label(&mut helper).unwrap();
        a.mov(rsi, rdx).unwrap();
        a.mov(rax, qword_ptr(r8)).unwrap();
        a.mov(rcx, qword_ptr(rax + 0x28)).unwrap();
        a.imul_3(rax, rsi, 106).unwrap();
        a.add(rax, qword_ptr(rcx)).unwrap();
        a.ret().unwrap();

        a.set_label(&mut fixup).unwrap();
        a.mov(r12, rcx).unwrap();
        a.lea(rcx, qword_ptr(rsp + 0x48)).unwrap();
        a.call(getter).unwrap();
        a.mov(rdx, qword_ptr(rax)).unwrap();
        a.mov(qword_ptr(rsp + 0x30), rdx).unwrap();
        a.mov(rbx, qword_ptr(r12)).unwrap();
        a.mov(r14, 0xFEFE_FEFE_FEFE_FEFEu64).unwrap();
        a.set_label(&mut top).unwrap();
        a.cmp(rbx, qword_ptr(r12 + 8)).unwrap();
        a.je(done).unwrap();

        a.mov(rax, qword_ptr(r12 + 0x18)).unwrap();
        a.add(rax, qword_ptr(rbx)).unwrap();
        a.mov(qword_ptr(rbx), rax).unwrap();

        a.lea(rcx, qword_ptr(rbx + 8)).unwrap();
        a.mov(rax, qword_ptr(rcx)).unwrap();
        a.cmp(rax, r14).unwrap();
        a.jne(resolve).unwrap();
        a.mov(qword_ptr(rcx), r13).unwrap();
        a.mov(qword_ptr(rcx + 8), r13).unwrap();
        a.jmp(after_key).unwrap();
        a.set_label(&mut resolve).unwrap();
        a.mov(edx, eax).unwrap();
        a.lea(r8, qword_ptr(rsp + 0x30)).unwrap();
        a.call(helper).unwrap();
        a.set_label(&mut after_key).unwrap();

        a.cmp(qword_ptr(rbx + 24), 0).unwrap();
        a.jbe(empty).unwrap();
        a.mov(rax, qword_ptr(rbx + 32)).unwrap();
        a.add(rax, qword_ptr(r12 + 0x18)).unwrap();
        a.jmp(store).unwrap();
        a.set_label(&mut empty).unwrap();
        a.xor(eax, eax).unwrap();
        a.set_label(&mut store).unwrap();
        a.mov(qword_ptr(rbx + 32), rax).unwrap();
        a.xor(esi, esi).unwrap();
        a.set_label(&mut each).unwrap();
        a.cmp(rsi, qword_ptr(rbx + 24)).unwrap();
        a.jae(list_done).unwrap();
        a.mov(rcx, rsi).unwrap();
        a.shl(rcx, 4).unwrap();
        a.add(rcx, qword_ptr(rbx + 32)).unwrap();
        a.mov(eax, dword_ptr(rcx)).unwrap();
        a.mov(rdx, rax).unwrap();
        a.lea(r8, qword_ptr(rsp + 0x30)).unwrap();
        a.call(helper).unwrap();
        a.inc(rsi).unwrap();
        a.jmp(each).unwrap();
        a.set_label(&mut list_done).unwrap();

        a.mov(eax, dword_ptr(rbx + 40)).unwrap();
        a.cmp(eax, 0xFEFE_FEFEu32).unwrap();
        a.jne(own).unwrap();
        a.xor(eax, eax).unwrap();
        a.jmp(own_store).unwrap();
        a.set_label(&mut own).unwrap();
        a.imul_3(rax, rax, 48).unwrap();
        a.add(rax, qword_ptr(r12)).unwrap();
        a.set_label(&mut own_store).unwrap();
        a.mov(qword_ptr(rbx + 40), rax).unwrap();
        a.add(rbx, 48).unwrap();
        a.jmp(top).unwrap();
        a.set_label(&mut done).unwrap();
        a.ret().unwrap();

        a.set_label(&mut path).unwrap();
        a.dw(&"Data/Stats.dat\0".encode_utf16().collect::<Vec<u16>>()).unwrap();
        a.set_label(&mut unwind).unwrap();
        a.db(&[0; 8]).unwrap();

        let built = a.assemble_options(BASE, BlockEncoderOptions::RETURN_NEW_INSTRUCTION_OFFSETS).unwrap();
        let at = |label: &CodeLabel| built.label_ip(label).unwrap() as u32;
        let code = &built.inner.code_buffer;
        let image = Image {
            data: code,
            sections: vec![Section { rva: BASE as u32, len: code.len() as u32, file: 0, code: true }],
            functions: vec![(at(&getter), at(&helper), at(&unwind)), (at(&helper), at(&fixup), at(&unwind)), (at(&fixup), at(&path), at(&unwind))],
        };
        let mut fixups = Fixups { image: &image, getters: HashMap::new(), helpers: HashMap::new(), sizes: [("stats".to_string(), 106)].into() };
        let pointers = fixups.pointers(&image.decode(at(&fixup), at(&path)), 48).unwrap();

        let stats = Some("Stats".to_string());
        assert_eq!(
            pointers,
            vec![
                Pointer { offset: 0, kind: Kind::String, element: None, target: None, proven: false },
                Pointer { offset: 8, kind: Kind::Key, element: None, target: stats.clone(), proven: true },
                Pointer { offset: 24, kind: Kind::List, element: Some(16), target: stats, proven: true },
                Pointer { offset: 40, kind: Kind::Row, element: None, target: None, proven: false },
            ]
        );
        fixups.sizes.insert("stats".to_string(), 64);
        let unproven = fixups.pointers(&image.decode(at(&fixup), at(&path)), 48).unwrap();
        assert!(unproven.iter().all(|p| p.target.is_none()), "a helper that indexes rows of another size disproves the target");
    }
}
