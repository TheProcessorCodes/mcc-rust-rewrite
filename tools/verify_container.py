"""Verify the MCC Halo 3 (U13) .map container facts in docs/format/container.md
against real bytes.  Read-only; never writes into the game folder.

Run:  python -I tools/verify_container.py [maps_folder]

Prints the header, section translation, tag index, tag classes (with their
string ids resolved), selected tags, the string-id namespace mapping and the
shared/campaign header differences.  Everything printed here is what the
Rust unit tests in blam_cache should expect.
"""
import datetime
import os
import struct
import sys

MAPS = sys.argv[1] if len(sys.argv) > 1 else (
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Halo The Master Chief Collection\halo3\maps")

INDEX_BITS, NAMESPACE_BITS, LENGTH_BITS = 19, 8, 5   # MccHalo3Strings.xml id="U13"
MASK = (1 << INDEX_BITS) - 1


def u32(b, o): return struct.unpack_from("<I", b, o)[0]
def i32(b, o): return struct.unpack_from("<i", b, o)[0]
def u64(b, o): return struct.unpack_from("<Q", b, o)[0]
def i16(b, o): return struct.unpack_from("<h", b, o)[0]
def u16(b, o): return struct.unpack_from("<H", b, o)[0]


def cstr(b, o, maxlen=None):
    """Null-terminated latin-1 string at b[o:], optionally capped at maxlen bytes."""
    if maxlen is None:
        end = b.index(b"\0", o)
    else:
        rel = b[o:o + maxlen].find(b"\0")
        end = o + (rel if rel >= 0 else maxlen)
    return b[o:end].decode("latin-1")


def fourcc(v):
    """Class id stored as little-endian int; the readable code is the bytes reversed."""
    return struct.pack("<I", v & 0xFFFFFFFF)[::-1].decode("latin-1")


class Header:
    def __init__(self, raw):
        assert len(raw) == 0x4000
        self.raw = raw
        self.magic = raw[0:4]
        self.version = u32(raw, 0x04)
        self.file_size = u32(raw, 0x08)
        self.unk_0c = u32(raw, 0x0C)
        self.tag_data_address = u32(raw, 0x10)
        self.virtual_size = u32(raw, 0x14)
        self.type_fields = struct.unpack_from("<4h", raw, 0x18)
        self.tag_count = u32(raw, 0x20)
        self.tag_name_table = u32(raw, 0x24)
        self.tag_name_table_size = u32(raw, 0x28)
        self.tag_name_index = u32(raw, 0x2C)
        self.string_count = u32(raw, 0x30)
        self.string_table = u32(raw, 0x34)
        self.string_table_size = u32(raw, 0x38)
        self.string_index = u32(raw, 0x3C)
        self.ns_count = u32(raw, 0x40)
        self.ns_table = u32(raw, 0x44)
        self.filetime = u64(raw, 0x50)
        self.build = cstr(raw, 0xA0, 32)
        self.map_name = cstr(raw, 0xC0, 32)
        self.scenario = cstr(raw, 0xE0, 256)
        self.virtual_base = u64(raw, 0x2E0)
        self.tag_index_ptr = u64(raw, 0x2E8)
        self.partitions = [(u64(raw, 0x300 + 16 * i), u64(raw, 0x308 + 16 * i)) for i in range(6)]
        self.section_offsets = [u32(raw, 0x4CC + 4 * i) for i in range(4)]
        self.sections = [(u32(raw, 0x4DC + 8 * i), u32(raw, 0x4E0 + 8 * i)) for i in range(4)]
        self.footer = raw[0x3FFC:0x4000]

    # --- address translation (Reclaimer SectionAddressTranslator / TagAddressTranslator) ---
    def section_file_base(self, i):
        return (self.sections[i][0] + self.section_offsets[i]) & 0xFFFFFFFF

    def header_ptr(self, p):          # 32-bit pointer in the header -> file offset (section 0)
        return (p + self.section_offsets[0]) & 0xFFFFFFFF

    def tag_addr(self, v):             # 64-bit virtual tag address -> file offset (section 2)
        return v - self.virtual_base + self.section_file_base(2)

    def tag_ptr32(self, p):            # shifted 32-bit pointer inside tag data -> file offset
        return self.tag_addr((p & 0xFFFFFFFF) << 2)


def filetime_str(ft):
    if ft == 0:
        return "0"
    try:
        t = datetime.datetime(1601, 1, 1) + datetime.timedelta(microseconds=ft // 10)
        return t.strftime("%Y-%m-%d %H:%M:%S UTC")
    except OverflowError:
        return "not a valid FILETIME"


def dump_header(name, h, size):
    print(f"\n## {name}: header (file size {size:#x} = {size})")
    rows = [
        ("0x000", "magic", h.magic.decode("latin-1")),
        ("0x004", "version", h.version),
        ("0x008", "file size", h.file_size),
        ("0x00C", "unknown", h.unk_0c),
        ("0x010", "tag data address (= sections[2].addr)", h.tag_data_address),
        ("0x014", "virtual size (= sections[2].size)", h.virtual_size),
        ("0x018", "4 x i16 (type?, ...)", h.type_fields),
        ("0x020", "tag count", h.tag_count),
        ("0x024", "tag-name table ptr", h.tag_name_table),
        ("0x028", "tag-name table size", h.tag_name_table_size),
        ("0x02C", "tag-name index ptr", h.tag_name_index),
        ("0x030", "string count", h.string_count),
        ("0x034", "string table ptr", h.string_table),
        ("0x038", "string table size", h.string_table_size),
        ("0x03C", "string index ptr", h.string_index),
        ("0x040", "string namespace count", h.ns_count),
        ("0x044", "string namespace table ptr", h.ns_table),
        ("0x050", "FILETIME?", f"{h.filetime:#x} = {filetime_str(h.filetime)}"),
        ("0x0A0", "build string", h.build),
        ("0x0C0", "map name", h.map_name),
        ("0x0E0", "scenario path", h.scenario),
        ("0x2E0", "virtual base", h.virtual_base),
        ("0x2E8", "tag index pointer", h.tag_index_ptr),
        ("0x3FFC", "footer", h.footer.decode("latin-1")),
    ]
    for off, what, val in rows:
        if isinstance(val, int):
            print(f"  {off:7} {what:42} {val:#x} = {val}")
        else:
            print(f"  {off:7} {what:42} {val}")
    print("  partitions (0x300):")
    total = 0
    for i, (a, s) in enumerate(h.partitions):
        total += s
        print(f"    [{i}] addr {a:#x} size {s:#x} = {s}")
    print(f"    sum of sizes {total:#x}  (virtual size {h.virtual_size:#x}, equal: {total == h.virtual_size})")
    print("  section offsets (0x4CC):", [f"{x:#010x}" for x in h.section_offsets])
    print("  sections (0x4DC) and file ranges:")
    names = ["debug/strings", "resources", "tags", "locales"]
    for i, (a, s) in enumerate(h.sections):
        base = h.section_file_base(i)
        print(f"    [{i}] {names[i]:14} virt {a:#010x} size {s:#010x}  -> file {base:#010x}..{base + s:#010x}")
    nz = [i for i in range(0, 0x4000, 16) if any(h.raw[i:i + 16])]
    ranges, s0, prev = [], None, None
    for i in nz:
        if s0 is None:
            s0 = i
        elif i != prev + 16:
            ranges.append((s0, prev + 16))
            s0 = i
        prev = i
    if s0 is not None:
        ranges.append((s0, prev + 16))
    print("  non-zero 16-byte rows:", [(f"{a:#x}", f"{b:#x}") for a, b in ranges])


def load_strings(f, h):
    """String table + header-based namespace translator (Reclaimer StringIdTranslator)."""
    f.seek(h.header_ptr(h.string_index))
    idx = struct.unpack(f"<{h.string_count}i", f.read(4 * h.string_count))
    f.seek(h.header_ptr(h.string_table))
    blob = f.read(h.string_table_size)
    items = [None] * h.string_count
    for i, o in enumerate(idx):
        if o >= 0:
            items[i] = cstr(blob, o)
    f.seek(h.header_ptr(h.ns_table))
    ns_raw = struct.unpack(f"<{h.ns_count}I", f.read(4 * h.ns_count))
    namespaces = {}
    start = ns_raw[0] & MASK
    for i in range(1, h.ns_count):
        namespaces[i] = (0, start)                 # (min, start)
        start += ns_raw[i] & MASK
    namespaces[0] = (ns_raw[0] & MASK, start)
    return items, idx, blob, ns_raw, namespaces


def string_index(sid, namespaces):
    index = sid & MASK
    ns = (sid >> INDEX_BITS) & ((1 << NAMESPACE_BITS) - 1)
    while ns not in namespaces and ns > 0:
        ns -= 1
    mn, st = namespaces[ns]
    return index if index < mn else index - mn + st


def show_code(c):
    return c if c.strip("\xff") else "none"


def main():
    paths = {n: os.path.join(MAPS, n + ".map") for n in ("riverworld", "shared", "campaign")}
    hdrs = {}
    for n, p in paths.items():
        with open(p, "rb") as f:
            hdrs[n] = Header(f.read(0x4000))
            size = os.path.getsize(p)
        dump_header(n, hdrs[n], size)

    h = hdrs["riverworld"]
    f = open(paths["riverworld"], "rb")

    # ---------------- tag index ----------------
    ti = h.tag_addr(h.tag_index_ptr)
    f.seek(ti)
    tib = f.read(0x48)
    print(f"\n## riverworld: tag index at virtual {h.tag_index_ptr:#x} -> file {ti:#x}")
    class_count, class_ptr = i32(tib, 0x00), u64(tib, 0x08)
    tag_count, tag_ptr = i32(tib, 0x10), u64(tib, 0x18)
    glob_count, glob_ptr = i32(tib, 0x20), u64(tib, 0x28)
    glob2_count, glob2_ptr = i32(tib, 0x30), u64(tib, 0x38)
    checksum, sig = u32(tib, 0x40), tib[0x44:0x48]
    print(f"  0x00 class count          {class_count:#x} = {class_count}")
    print(f"  0x08 class table ptr      {class_ptr:#x} -> file {h.tag_addr(class_ptr):#x}")
    print(f"  0x10 tag count            {tag_count:#x} = {tag_count}  (header tag count {h.tag_count})")
    print(f"  0x18 tag table ptr        {tag_ptr:#x} -> file {h.tag_addr(tag_ptr):#x}")
    print(f"  0x20 global tag count     {glob_count}")
    print(f"  0x28 global tag table ptr {glob_ptr:#x} -> file {h.tag_addr(glob_ptr):#x}")
    print(f"  0x30 count 2              {glob2_count}")
    print(f"  0x38 ptr 2                {glob2_ptr:#x}")
    print(f"  0x40 checksum?            {checksum:#x}")
    print(f"  0x44 signature            {sig!r} = {fourcc(u32(tib, 0x44))!r}")
    print(f"  tag table ptr - global tag table ptr = {tag_ptr - glob_ptr:#x} ({glob_count} x 8)")
    print(f"  tag index end {ti + 0x48:#x}; class table starts at file {h.tag_addr(class_ptr):#x}")

    # ---------------- strings ----------------
    items, idx, blob, ns_raw, namespaces = load_strings(f, h)
    print("\n## riverworld: string table")
    print(f"  string index at file {h.header_ptr(h.string_index):#x}, table at {h.header_ptr(h.string_table):#x}, size {h.string_table_size:#x}")
    print(f"  entries with offset -1: {sum(1 for o in idx if o < 0)}; first offsets {idx[:6]}")
    print(f"  strings[0..8] = {items[:8]}")
    print(f"  namespace table at file {h.header_ptr(h.ns_table):#x}: raw = {[f'{x:#x}' for x in ns_raw]}")
    print(f"  counts (& 0x7FFFF)      = {[x & MASK for x in ns_raw]}")
    print(f"  upper bits (>> 19)      = {[x >> INDEX_BITS for x in ns_raw]}")
    print("  namespace -> (min, start):")
    for k in sorted(namespaces):
        mn, st = namespaces[k]
        print(f"    ns {k:2}: min {mn:#x} = {mn:5}  start {st:#x} = {st}")
    total = sum(x & MASK for x in ns_raw)
    print(f"  sum of counts = {total}; string count = {h.string_count}; ns0 strings after the others = {h.string_count - total}")
    # first string of each namespace region, to make the layout visible
    for k in sorted(namespaces):
        mn, st = namespaces[k]
        print(f"    ns {k} region starts at table index {st}: {items[st:st+3]}")

    # ---------------- classes ----------------
    f.seek(h.tag_addr(class_ptr))
    cb = f.read(16 * class_count)
    classes = []
    print(f"\n## riverworld: {class_count} tag classes (markdown)")
    print("| # | code | parent | grandparent | name string id | ns | idx | len bits | table index | name |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    bad = 0
    for i in range(class_count):
        cid = u32(cb, 16 * i)
        parent = fourcc(u32(cb, 16 * i + 4))
        gparent = fourcc(u32(cb, 16 * i + 8))
        sid = u32(cb, 16 * i + 12)
        si = string_index(sid, namespaces)
        name = items[si] if 0 <= si < len(items) else None
        ns = (sid >> INDEX_BITS) & 0xFF
        ln = sid >> (INDEX_BITS + NAMESPACE_BITS)
        if name is None or ln != min(len(name), (1 << LENGTH_BITS) - 1):
            bad += 1
        classes.append((fourcc(cid), parent, gparent, sid, name))
        print(f"| {i} | `{fourcc(cid)}` | `{show_code(parent)}` | `{show_code(gparent)}` | {sid:#010x} | {ns} | {sid & MASK:#x} | {ln} | {si} | `{name}` |")
    print(f"  classes whose length bits != min(len(name),31): {bad}")
    print(f"  all names resolved: {all(c[4] for c in classes)}")
    print(f"  class name string ids by namespace: { {ns: sum(1 for c in classes if ((c[3] >> INDEX_BITS) & 0xFF) == ns) for ns in range(h.ns_count)} }")
    mn0 = namespaces[0][0]
    print(f"  class name ids with index < ns0.min ({mn0}): {sum(1 for c in classes if (c[3] & MASK) < mn0)}; >= min: {sum(1 for c in classes if (c[3] & MASK) >= mn0)}")

    # ---------------- tag items ----------------
    f.seek(h.tag_addr(tag_ptr))
    tb = f.read(8 * tag_count)
    f.seek(h.header_ptr(h.tag_name_index))
    nidx = struct.unpack(f"<{tag_count}i", f.read(4 * tag_count))
    f.seek(h.header_ptr(h.tag_name_table))
    nblob = f.read(h.tag_name_table_size)
    tags = []
    for i in range(tag_count):
        ci, salt, mp = i16(tb, 8 * i), u16(tb, 8 * i + 2), u32(tb, 8 * i + 4)
        name = cstr(nblob, nidx[i]) if nidx[i] >= 0 else None
        tags.append((ci, salt, mp, name))
    empties = [i for i, t in enumerate(tags) if t[0] < 0]
    print(f"\n## riverworld: tag items at file {h.tag_addr(tag_ptr):#x}; tag names index at file {h.header_ptr(h.tag_name_index):#x}, table at {h.header_ptr(h.tag_name_table):#x}")
    print(f"  empty tags (class index -1): {empties} -> raw {[tb[8*i:8*i+8].hex() for i in empties]}, name index {[nidx[i] for i in empties]}")
    print(f"  tags with name index -1: {sum(1 for x in nidx if x < 0)}")
    print(f"  tag name index offsets strictly increasing: {all(nidx[i] < nidx[i+1] for i in range(tag_count-1) if nidx[i] >= 0 and nidx[i+1] >= 0)}; first {nidx[:5]}")
    print(f"  tags with meta pointer 0: {[(i, classes[t[0]][0]) for i, t in enumerate(tags) if t[0] >= 0 and t[2] == 0][:10]}")
    salts_ok = all(t[1] == ((0xE175 + i) & 0xFFFF) for i, t in enumerate(tags) if t[0] >= 0)
    print(f"  salt == 0xE175 + index for every non-empty tag: {salts_ok}")
    if not salts_ok:
        offs = {}
        for i, t in enumerate(tags):
            if t[0] >= 0:
                offs.setdefault((t[1] - i) & 0xFFFF, []).append(i)
        print("  salt - index values (first index, last index, count):", {f"{k:#x}": (v[0], v[-1], len(v)) for k, v in offs.items()})
    mps = [t[2] for t in tags if t[0] >= 0 and t[2]]
    print(f"  meta pointers (stored, shifted) min {min(mps):#x} max {max(mps):#x}; expanded min {min(mps) << 2:#x} max {max(mps) << 2:#x}; virtual base {h.virtual_base:#x}, tag section end {h.virtual_base + h.virtual_size:#x}")
    print(f"  meta pointers monotonically increasing with index (non-empty, non-zero): {all(a <= b for a, b in zip(mps, mps[1:]))}")

    f.seek(h.tag_addr(glob_ptr))
    gb = f.read(8 * glob_count)
    print("  global tags table (class, datum id, index, salt, matches item):")
    for i in range(glob_count):
        cls, datum = u32(gb, 8 * i), u32(gb, 8 * i + 4)
        ix, salt = datum & 0xFFFF, datum >> 16
        t = tags[ix]
        ok = classes[t[0]][0] == fourcc(cls) and t[1] == salt
        print(f"    {fourcc(cls)!r}  datum {datum:#010x}  index {ix}  salt {salt:#x}  item class {classes[t[0]][0]!r} salt {t[1]:#x} name {t[3]!r}  ok={ok}")

    def tag_row(i):
        ci, salt, mp, name = tags[i]
        code = classes[ci][0] if ci >= 0 else "(empty)"
        exp = (mp << 2) & 0xFFFFFFFFF
        fo = h.tag_ptr32(mp) if mp else 0
        return f"| {i} | `{code}` | {salt:#06x} | {mp:#010x} | {exp:#x} | {fo:#x} | `{name}` |"

    print("\n## riverworld: tags 0..9 (markdown)")
    print("| index | class | salt | meta ptr (stored) | <<2 (virtual) | file offset | name |")
    print("|---|---|---|---|---|---|---|")
    for i in range(10):
        print(tag_row(i))
    print("\n## riverworld: sbsp / scnr / matg / play / zone / ugh! / draw tags (markdown)")
    print("| index | class | salt | meta ptr (stored) | <<2 (virtual) | file offset | name |")
    print("|---|---|---|---|---|---|---|")
    for code in ("sbsp", "scnr", "matg", "play", "zone", "ugh!", "draw"):
        for i, t in enumerate(tags):
            if t[0] >= 0 and classes[t[0]][0] == code:
                print(tag_row(i))
    scnr = [i for i, t in enumerate(tags) if t[0] >= 0 and classes[t[0]][0] == "scnr"]
    match = [i for i in scnr if tags[i][3] and tags[i][3].lower() == h.scenario.lower()]
    print(f"  scnr tags: {scnr}; the one whose name == header scenario path: {match}")
    hist = {}
    for t in tags:
        if t[0] >= 0:
            hist[classes[t[0]][0]] = hist.get(classes[t[0]][0], 0) + 1
    print("  class histogram (top 12):", sorted(hist.items(), key=lambda kv: -kv[1])[:12])
    print(f"  non-empty tag count: {sum(hist.values())}; classes used: {len(hist)} of {class_count}")
    print(f"  last tag: {tag_row(tag_count - 1)}")

    # ---------------- partitions: which tags live where ----------------
    print("\n## riverworld: tags per partition")
    per = {}
    for i, t in enumerate(tags):
        if t[0] < 0 or not t[2]:
            continue
        v = t[2] << 2
        pi = next((k for k, (a, s) in enumerate(h.partitions) if s and a <= v < a + s), None)
        per.setdefault(pi, []).append(i)
    for k in sorted(per, key=lambda x: (x is None, x)):
        ix = per[k]
        cls = {}
        for i in ix:
            c = classes[tags[i][0]][0]
            cls[c] = cls.get(c, 0) + 1
        local = sum(1 for i in ix if (tags[i][3] or "").lower().startswith("levels\\multi\\riverworld"))
        top = sorted(cls.items(), key=lambda kv: -kv[1])[:8]
        print(f"  partition {k}: {len(ix)} tags, index range {ix[0]}..{ix[-1]}, named levels\\multi\\riverworld*: {local}, top classes {top}")
    for nm, v in (("tag index", h.tag_index_ptr), ("class table", class_ptr), ("tag table", tag_ptr), ("global tags table", glob_ptr)):
        pi = next((k for k, (a, s) in enumerate(h.partitions) if s and a <= v < a + s), None)
        print(f"  {nm} {v:#x} lies in partition {pi}")

    # ---------------- end-to-end: TagBlock + TagReference + pointer expansion ----------------
    # Reclaimer Halo3/Config/ScenarioTag.config.cs: for MccHalo3U13 StructureBsps block is at scnr+24,
    # StructureBspBlock.BspReference at +0 (TagReference, 16 bytes).
    print("\n## riverworld: end-to-end check via scenario.structure_bsps")
    si = match[0]
    scnr_file = h.tag_ptr32(tags[si][2])
    f.seek(scnr_file + 24)
    cnt, ptr, pad = struct.unpack("<iIi", f.read(12))
    blk_file = h.tag_ptr32(ptr)
    print(f"  scnr meta at file {scnr_file:#x}; structure_bsps TagBlock at +24: count {cnt}, ptr {ptr:#x} (<<2 = {ptr << 2:#x}) -> file {blk_file:#x}, pad {pad}")
    f.seek(blk_file)
    ref = f.read(16)
    cls, skip8, datum = u32(ref, 0), ref[4:12], u32(ref, 12)
    print(f"  first StructureBspBlock.BspReference: class {fourcc(cls)!r}, middle 8 bytes {skip8.hex()}, datum {datum:#010x} -> index {datum & 0xFFFF}, salt {datum >> 16:#x}")
    t = tags[datum & 0xFFFF]
    print(f"  tag[{datum & 0xFFFF}] is class {classes[t[0]][0]!r} salt {t[1]:#x} name {t[3]!r}; reference consistent: {classes[t[0]][0] == fourcc(cls) and t[1] == datum >> 16}")

    # ---------------- locale index (noted only) ----------------
    # Reclaimer MccHalo3/CacheFileU6.cs: Halo3 >= U12 -> LocaleIndex(cache, 24, 80, 12):
    # 12 LanguageDefinitions of 16 bytes at matg+24+i*80; offsets are section-3 pointers.
    print("\n## riverworld: locale index (first language) from matg")
    mi = [i for i, t in enumerate(tags) if t[0] >= 0 and classes[t[0]][0] == "matg"][0]
    matg_file = h.tag_ptr32(tags[mi][2])
    sec3 = h.section_file_base(3)
    for lang in range(3):
        f.seek(matg_file + 24 + lang * 80)
        sc, ss, io, so = struct.unpack("<4i", f.read(16))
        io_f = (io + h.section_offsets[3]) & 0xFFFFFFFF
        so_f = (so + h.section_offsets[3]) & 0xFFFFFFFF
        f.seek(io_f)
        e_sid, e_off = struct.unpack("<Ii", f.read(8))
        f.seek(so_f + e_off)
        first = cstr(f.read(64), 0, 64)
        print(f"  lang {lang}: count {sc}, strings size {ss:#x}, indices ptr {io:#x} -> file {io_f:#x}, strings ptr {so:#x} -> file {so_f:#x}; entry0 sid {e_sid:#010x} off {e_off} -> {first!r}")
    print(f"  locale section file range {sec3:#x}..{sec3 + h.sections[3][1]:#x}")

    # ---------------- survey of other maps ----------------
    print("\n## other maps: type fields, empty tags, tag index checksum")
    for n in ("deadlock", "cyberdyne", "005_intro", "010_jungle", "120_halo"):
        p = os.path.join(MAPS, n + ".map")
        if not os.path.exists(p):
            continue
        with open(p, "rb") as g:
            hh = Header(g.read(0x4000))
            ti2 = hh.tag_addr(hh.tag_index_ptr)
            g.seek(ti2)
            b = g.read(0x48)
            tc2, tp2 = i32(b, 0x10), u64(b, 0x18)
            g.seek(hh.tag_addr(tp2))
            tb2 = g.read(8 * tc2)
            empt = [i for i in range(tc2) if i16(tb2, 8 * i) < 0]
            zero_meta = sum(1 for i in range(tc2) if i16(tb2, 8 * i) >= 0 and u32(tb2, 8 * i + 4) == 0)
            print(f"  {n:11} type {hh.type_fields} tags {tc2} classes {i32(b,0)} globals {i32(b,0x20)} empty-tags {empt[:5]}{'...' if len(empt) > 5 else ''} ({len(empt)}) zero-meta {zero_meta} idx+0x40 {u32(b,0x40):#x} footer {fourcc(u32(b,0x44))!r} ns-count {hh.ns_count} ns-raw {[x & MASK for x in struct.unpack('<%dI' % hh.ns_count, (g.seek(hh.header_ptr(hh.ns_table)), g.read(4*hh.ns_count))[1])]}")

    # ---------------- shared / campaign ----------------
    for n in ("shared", "campaign"):
        hh = hdrs[n]
        with open(paths[n], "rb") as g:
            print(f"\n## {n}: tag index")
            if hh.tag_index_ptr:
                ti = hh.tag_addr(hh.tag_index_ptr)
                g.seek(ti)
                b = g.read(0x48)
                print(f"  tag index at virtual {hh.tag_index_ptr:#x} -> file {ti:#x}: class count {i32(b,0)}, tag count {i32(b,0x10)}, global count {i32(b,0x20)}, footer {fourcc(u32(b,0x44))!r}")
                cc, tc = i32(b, 0), i32(b, 0x10)
                codes = []
                if cc > 0:
                    g.seek(hh.tag_addr(u64(b, 8)))
                    cb = g.read(16 * cc)
                    codes = [fourcc(u32(cb, 16 * i)) for i in range(cc)]
                    print(f"  classes: {codes}")
                if tc > 0:
                    g.seek(hh.tag_addr(u64(b, 0x18)))
                    tb = g.read(8 * tc)
                    g.seek(hh.header_ptr(hh.tag_name_index))
                    ni = struct.unpack(f"<{tc}i", g.read(4 * tc))
                    g.seek(hh.header_ptr(hh.tag_name_table))
                    nb = g.read(hh.tag_name_table_size)
                    for i in range(min(tc, 12)):
                        ci = i16(tb, 8 * i)
                        nm = cstr(nb, ni[i]) if ni[i] >= 0 else None
                        print(f"    tag {i}: class {codes[ci] if ci >= 0 else '(empty)'} salt {u16(tb,8*i+2):#x} meta {u32(tb,8*i+4):#x} name {nm!r}")
            if hh.string_count:
                its, ix, bl, nsr, nss = load_strings(g, hh)
            else:
                its, nsr = [], []
            print(f"  string count {hh.string_count}, namespace raw {[f'{x:#x}' for x in nsr]}, first strings {its[:5]}")


if __name__ == "__main__":
    main()
