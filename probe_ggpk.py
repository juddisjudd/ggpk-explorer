"""Walk loose GGPK records (PDIR/FILE) in Content.ggpk and report the tree structure.

The bundle index only covers files inside bundles; this shows what lives as loose
FILE records in the GGPK itself (e.g. FMOD .bank audio).
"""
import struct, sys
from collections import Counter, defaultdict

GGPK_PATH = r"S:\Grinding Gear Games\Path of Exile 2\Content.ggpk"

f = open(GGPK_PATH, "rb")

def read_at(off, n):
    f.seek(off)
    return f.read(n)

# GGPK header
hdr = read_at(0, 32)
length, tag, version, root_off, free_off = struct.unpack("<I4sIQQ", hdr[:28])
assert tag == b"GGPK", tag
print(f"GGPK version={version} root_offset={root_off}")

def read_name_utf32(data, pos, name_len):
    n = max(name_len - 1, 0)
    if version == 4:
        chars = struct.unpack_from(f"<{n}I", data, pos)
        pos += 4 * n + 4  # skip null terminator
        return "".join(chr(c) if c <= 0x10FFFF else "?" for c in chars), pos
    # version <= 3: UTF-16LE
    raw = data[pos:pos + 2 * n]
    pos += 2 * n + 2  # skip null terminator
    return raw.decode("utf-16-le", errors="replace"), pos

def parse_dir(off):
    head = read_at(off, 8)
    length, tag = struct.unpack("<I4s", head)
    if tag != b"PDIR":
        return None
    data = read_at(off, length)
    name_len, total = struct.unpack_from("<II", data, 8)
    pos = 16 + 32  # skip hash
    name, pos = read_name_utf32(data, pos, name_len)
    entries = []
    for _ in range(total):
        h, eoff = struct.unpack_from("<IQ", data, pos)
        pos += 12
        entries.append(eoff)
    return name, entries

def parse_file_name(off):
    head = read_at(off, 8)
    length, tag = struct.unpack("<I4s", head)
    if tag != b"FILE":
        return None, None
    # name_len + hash; name can be long, read generously but capped
    data = read_at(off, min(length, 4 + 4 + 4 + 32 + 4 * 600))
    name_len = struct.unpack_from("<I", data, 8)[0]
    name, pos = read_name_utf32(data, 12 + 32, name_len)
    data_len = length - pos
    return name, data_len

ext_stats = defaultdict(lambda: [0, 0])   # top_dir -> {ext: (count, bytes)} flattened
tree_counts = defaultdict(Counter)
tree_bytes = defaultdict(Counter)
bank_files = []
file_total = 0

def walk(off, path, top):
    global file_total
    head = read_at(off, 8)
    length, tag = struct.unpack("<I4s", head)
    if tag == b"PDIR":
        res = parse_dir(off)
        if not res:
            return
        name, entries = res
        full = f"{path}/{name}" if path else name
        new_top = top if top else (name or "<root>")
        # Don't recurse into Bundles2 file contents-by-name printing, but still count
        for e in entries:
            walk(e, full, new_top)
    elif tag == b"FILE":
        name, dlen = parse_file_name(off)
        if name is None:
            return
        file_total += 1
        ext = name.rsplit(".", 1)[-1].lower() if "." in name else "<noext>"
        tree_counts[top or "<root>"][ext] += 1
        tree_bytes[top or "<root>"][ext] += dlen
        full = f"{path}/{name}" if path else name
        if ext == "bank" or "fmod" in full.lower():
            bank_files.append((full, dlen))

root = parse_dir(root_off)
print(f"Root dir name={root[0]!r} entries={len(root[1])}")
# First: print top-level children
for e in root[1]:
    head = read_at(e, 8)
    length, tag = struct.unpack("<I4s", head)
    if tag == b"PDIR":
        n, ents = parse_dir(e)
        print(f"  DIR  {n!r} ({len(ents)} entries)")
    elif tag == b"FILE":
        n, dlen = parse_file_name(e)
        print(f"  FILE {n!r} ({dlen} bytes)")
    else:
        print(f"  {tag} record")

print("\nWalking full loose tree (this may take a minute)...")
walk(root_off, "", "")
print(f"\nTotal loose FILE records: {file_total}")
for top, exts in sorted(tree_counts.items()):
    total_files = sum(exts.values())
    total_b = sum(tree_bytes[top].values())
    print(f"\n[{top}] {total_files} files, {total_b/1e9:.2f} GB")
    for ext, cnt in exts.most_common(15):
        print(f"   .{ext:<12} x{cnt:<8} {tree_bytes[top][ext]/1e9:.3f} GB")

print(f"\n.bank / FMOD files found: {len(bank_files)}")
for p, sz in bank_files[:40]:
    print(f"   {p}  ({sz/1e6:.1f} MB)")
if len(bank_files) > 40:
    print(f"   ... and {len(bank_files)-40} more")
