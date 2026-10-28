"""Probe MCC Halo 3 (U13) bitmap (bitm) tags and their pixel pages. Read-only.

usage: python -I probe_bitmaps.py <maps dir> [map name] [--all]

Companion to docs/format/bitmap.md. Parses every bitm tag, resolves the resource
entries / segments / pages, measures how many bytes each segment owns in its page and
compares that with the computed mip-chain sizes, inspects the resource definition data
(the fixup slice) of bitmap resources, and finally inflates a few pages to dump and
decode the first blocks of mip 0 (BC1 / BC3 / BC5). Every number printed is an
EXPECTED VALUE for the Rust reader.
"""
import collections
import os
import struct
import sys
import zlib

MAPS = sys.argv[1]
NAME = sys.argv[2] if len(sys.argv) > 2 and not sys.argv[2].startswith("--") else "riverworld.map"
ALL = "--all" in sys.argv
path = os.path.join(MAPS, NAME)
f = open(path, "rb")


def rd(off, n):
    f.seek(off)
    return f.read(n)


def i32(off): return struct.unpack("<i", rd(off, 4))[0]
def u32(off): return struct.unpack("<I", rd(off, 4))[0]
def i16(off): return struct.unpack("<h", rd(off, 2))[0]
def i64(off): return struct.unpack("<q", rd(off, 8))[0]


# ---- header (U6+ layout) ----
virt_base = i64(0x2E0)
idx_ptr = i64(0x2E8)
sec_off = struct.unpack("<4I", rd(0x4CC, 16))
sec = [struct.unpack("<II", rd(0x4DC + 8 * i, 8)) for i in range(4)]


def hdr_addr(p):
    return (p + sec_off[0]) & 0xFFFFFFFF


def tag_addr(v):
    return v - virt_base + ((sec[2][0] + sec_off[2]) & 0xFFFFFFFF)


def ptr32(p):
    if p == 0:
        return -1
    if p >= 0x80000000:
        p -= 0x100000000
    return tag_addr(p << 2)


# ---- tag index ----
ti = tag_addr(idx_ptr)
class_count = i32(ti)
class_tbl = tag_addr(i64(ti + 8))
tag_count = i32(ti + 16)
tag_tbl = tag_addr(i64(ti + 24))
classes = [rd(class_tbl + 16 * i, 4)[::-1].decode("latin1") for i in range(class_count)]
items = []
for i in range(tag_count):
    ci, unk, mp = struct.unpack("<hhI", rd(tag_tbl + 8 * i, 8))
    items.append((ci, mp))
name_idx = hdr_addr(u32(0x2C))
name_tbl = hdr_addr(u32(0x24))


def cstr(off):
    f.seek(off)
    s = b""
    while True:
        c = f.read(1)
        if c in (b"", b"\0"):
            break
        s += c
    return s.decode("latin1")


names = []
for i in range(tag_count):
    o = i32(name_idx + 4 * i)
    names.append(None if o < 0 else cstr(name_tbl + o))


def find(cls, name=None):
    return [i for i, (ci, mp) in enumerate(items)
            if ci >= 0 and classes[ci] == cls and (name is None or names[i] == name)]


def block(off):
    cnt, p = struct.unpack("<iI", rd(off, 8))
    return cnt, (ptr32(p) if cnt else -1)


# ---- zone / play ----
zone_meta = ptr32(items[find("zone")[0]][1])
play_i = find("play")[0]
play_meta = zone_meta + 28 if items[play_i][1] == 0 else ptr32(items[play_i][1])
sc_cnt, sc_ptr = block(play_meta + 12)
pg_cnt, pg_ptr = block(play_meta + 24)
seg_cnt, seg_ptr = block(play_meta + 60)
shared = []
for i in range(sc_cnt):
    b = rd(sc_ptr + 264 * i, 264)
    shared.append(b[:256].split(b"\0")[0].decode("latin1"))
re_cnt, re_ptr = block(zone_meta + 100)
fix_size = i32(zone_meta + 328)
fix_ptr = ptr32(u32(zone_meta + 340))
rt_cnt, rt_ptr = block(zone_meta + 4)


def page(i):
    b = rd(pg_ptr + 88 * i, 88)
    salt, flags, codec, cache_idx, unk6, data_off, csize, usize, crc = struct.unpack_from("<HBbhhiiiI", b, 0)
    asset_cnt = struct.unpack_from("<h", b, 84)[0]
    return dict(idx=i, flags=flags, codec=codec, cache_idx=cache_idx, data_off=data_off, csize=csize,
                usize=usize, asset_cnt=asset_cnt)


def segment(i):
    pp, sp, po, so, ps, ss = struct.unpack("<hhiihh", rd(seg_ptr + 16 * i, 16))
    return dict(idx=i, primary_page=pp, secondary_page=sp, primary_off=po, secondary_off=so)


def entry(i):
    b = rd(re_ptr + 64 * i, 64)
    cls = b[0:4][::-1].decode("latin1")
    tag_id = struct.unpack_from("<i", b, 12)[0]
    salt = struct.unpack_from("<H", b, 16)[0]
    fix_off, fix_len, sec_fix_off = struct.unpack_from("<iii", b, 20)
    loc_type, seg_idx = struct.unpack_from("<hh", b, 32)
    def_addr = struct.unpack_from("<I", b, 36)[0]
    rf_cnt, rf_ptr = struct.unpack_from("<iI", b, 40)
    df_cnt, df_ptr = struct.unpack_from("<iI", b, 52)
    rf = []
    if rf_cnt:
        base = ptr32(rf_ptr)
        for k in range(rf_cnt):
            rf.append(struct.unpack("<iI", rd(base + 8 * k, 8)))
    df = []
    if df_cnt:
        base = ptr32(df_ptr)
        for k in range(df_cnt):
            df.append(struct.unpack("<Ii", rd(base + 8 * k, 8)))
    return dict(owner_cls=cls, owner_tag=tag_id, idx=i, salt=salt, type_idx=b[18], flags=b[19],
                fix_off=fix_off, fix_len=fix_len, sec_fix_off=sec_fix_off, loc_type=loc_type,
                seg_idx=seg_idx, def_addr=def_addr, rf=rf, df=df)


def page_file(p):
    return NAME if p["cache_idx"] < 0 else os.path.basename(shared[p["cache_idx"]].replace("\\", "/"))


# segment extents: for every page, the sorted list of segment offsets using it
page_offsets = collections.defaultdict(list)
for si in range(seg_cnt):
    s = segment(si)
    if s["primary_page"] >= 0:
        page_offsets[s["primary_page"]].append(s["primary_off"])
    if s["secondary_page"] >= 0:
        page_offsets[s["secondary_page"]].append(s["secondary_off"])
for k in page_offsets:
    page_offsets[k] = sorted(set(page_offsets[k]))


def extent(page_idx, off):
    """bytes from this segment offset to the next segment offset in the same page (or page end)."""
    lst = page_offsets[page_idx]
    usize = page(page_idx)["usize"]
    for o in lst:
        if o > off:
            return o - off
    return usize - off


page_cache = {}


def load_page(pi):
    if pi in page_cache:
        return page_cache[pi]
    p = page(pi)
    fn = page_file(p)
    g = open(os.path.join(MAPS, fn), "rb")
    g.seek(0x4D0)
    base = struct.unpack("<I", g.read(4))[0]
    g.seek(base + p["data_off"])
    comp = g.read(p["csize"])
    g.close()
    if p["csize"] == p["usize"]:
        data = comp
    else:
        data = zlib.decompressobj(-15).decompress(comp)
    assert len(data) == p["usize"], (pi, len(data), p["usize"])
    page_cache[pi] = data
    return data


# ---- bitmap formats: (name, bits per pixel, block width, bytes per block) ----
FORMATS = {
    0: ("A8", 8, 1, 1), 1: ("Y8", 8, 1, 1), 2: ("AY8", 8, 1, 1), 3: ("A8Y8", 16, 1, 2),
    6: ("R5G6B5", 16, 1, 2), 8: ("A1R5G5B5", 16, 1, 2), 9: ("A4R4G4B4", 16, 1, 2),
    10: ("X8R8G8B8", 32, 1, 4), 11: ("A8R8G8B8", 32, 1, 4),
    14: ("DXT1", 4, 4, 8), 15: ("DXT3", 8, 4, 16), 16: ("DXT5", 8, 4, 16),
    17: ("P8_bump", 8, 1, 1), 18: ("P8", 8, 1, 1), 19: ("ARGBFP32", 128, 1, 16),
    20: ("RGBFP32", 96, 1, 12), 21: ("RGBFP16", 64, 1, 8), 22: ("U8V8", 16, 1, 2),
    31: ("DXT5a", 4, 4, 8), 33: ("DXN", 8, 4, 16), 34: ("CTX1", 4, 4, 8),
    35: ("DXT3a_alpha", 4, 4, 8), 36: ("DXT3a_mono", 4, 4, 8), 37: ("DXT5a_alpha", 4, 4, 8),
    38: ("DXT5a_mono", 4, 4, 8), 39: ("DXN_mono_alpha", 8, 4, 16),
}
TYPES = {0: "2D", 1: "3D", 2: "cube", 3: "array"}
CURVES = {0: "unknown", 1: "xRGB", 2: "gamma2", 3: "linear", 4: "offset_log", 5: "sRGB"}


def fmt_name(v):
    return FORMATS[v][0] if v in FORMATS else f"fmt{v}"


def mip_bytes(fmtv, w, h):
    name, bpp, bw, bpb = FORMATS[fmtv]
    if bw > 1:
        return ((w + 3) // 4) * ((h + 3) // 4) * bpb
    return w * h * bpp // 8


def chain(fmtv, w, h, mips, faces, first=0):
    """sum of face count x mip sizes for mip levels first..mips (inclusive)."""
    total = 0
    for i in range(first, mips + 1):
        total += faces * mip_bytes(fmtv, max(1, w >> i), max(1, h >> i))
    return total


def mip_offset(fmtv, w, h, level, faces=1):
    return chain(fmtv, w, h, level - 1, faces) if level > 0 else 0


# ---- parse all bitm tags ----
def parse_bitm(tidx):
    meta = ptr32(items[tidx][1])
    seq_cnt, seq_ptr = block(meta + 84)
    bm_cnt, bm_ptr = block(meta + 96)
    rs_cnt, rs_ptr = block(meta + 140)
    il_cnt, il_ptr = block(meta + 152)
    bitmaps = []
    for i in range(bm_cnt):
        b = rd(bm_ptr + 56 * i, 56)
        d = dict(cls=b[0:4][::-1].decode("latin1"))
        d["w"], d["h"] = struct.unpack_from("<hh", b, 4)
        d["depth"], d["flags"] = b[8], b[9]
        d["type"], d["fmt"], d["more"] = struct.unpack_from("<hhh", b, 10)
        d["regx"], d["regy"] = struct.unpack_from("<hh", b, 16)
        d["mips"], d["curve"], d["il_idx"], d["idx2"] = b[20], b[21], b[22], b[23]
        d["pix_off"], d["pix_size"] = struct.unpack_from("<ii", b, 24)
        d["tail"] = b[32:56]
        d["raw"] = b
        bitmaps.append(d)
    res = [struct.unpack("<Ii", rd(rs_ptr + 8 * i, 8)) for i in range(rs_cnt)]
    il = [struct.unpack("<Ii", rd(il_ptr + 8 * i, 8)) for i in range(il_cnt)]
    seqs = []
    for i in range(seq_cnt):
        b = rd(seq_ptr + 64 * i, 64)
        sname = b[:32].split(b"\0")[0].decode("latin1")
        first, cnt = struct.unpack_from("<hh", b, 32)
        sp_cnt = struct.unpack_from("<i", b, 52)[0]
        seqs.append((sname, first, cnt, sp_cnt))
    return dict(tidx=tidx, name=names[tidx], meta=meta, head=rd(meta, 164), seqs=seqs, bitmaps=bitmaps,
                res=res, il=il)


bitm_tags = find("bitm")
print(f"{NAME}: {len(bitm_tags)} bitm tags; zone entries {re_cnt}; play pages {pg_cnt}; segments {seg_cnt}")
parsed = [parse_bitm(t) for t in bitm_tags]

# header-level stats
hist = collections.Counter
h_fmt, h_type, h_flags, h_more, h_curve, h_mips, h_depth, h_idx2 = (hist() for _ in range(8))
h_pixoff, h_pixsize, h_tail, h_resunk, h_cls = hist(), hist(), hist(), hist(), hist()
h_counts = hist()
h_il = hist()
h_seq = hist()
for t in parsed:
    h_counts[(len(t["bitmaps"]), len(t["res"]), len(t["il"]))] += 1
    h_seq[len(t["seqs"])] += 1
    for b in t["bitmaps"]:
        h_fmt[(b["fmt"], fmt_name(b["fmt"]))] += 1
        h_type[(b["type"], TYPES.get(b["type"]))] += 1
        h_flags[b["flags"]] += 1
        h_more[b["more"]] += 1
        h_curve[(b["curve"], CURVES.get(b["curve"]))] += 1
        h_mips[b["mips"]] += 1
        h_depth[(TYPES.get(b["type"]), b["depth"])] += 1
        h_idx2[(len(t["il"]) > 0, b["il_idx"], b["idx2"])] += 1
        h_pixoff[b["pix_off"]] += 1
        h_pixsize[b["pix_size"]] += 1
        h_tail[b["tail"].hex()] += 1
        h_cls[b["cls"]] += 1
    for rid, unk in t["res"] + t["il"]:
        h_resunk[unk] += 1
print("\n== bitm tag-level histograms ==")
print(" (bitmap count, resource count, interleaved resource count):", dict(sorted(h_counts.items())))
print(" sequence counts:", dict(sorted(h_seq.items())))
print(" bitmap class field:", dict(h_cls))
print(" format:", dict(sorted(h_fmt.items())))
print(" type:", dict(sorted(h_type.items())))
print(" flags byte (+9):", dict(sorted(h_flags.items())))
print(" more flags i16 (+14):", dict(sorted(h_more.items())))
print(" curve (+21):", dict(sorted(h_curve.items())))
print(" mipmap count (+20):", dict(sorted(h_mips.items())))
print(" (type, depth):", dict(sorted(h_depth.items(), key=str)))
print(" (tag has interleaved block, il_idx +22, idx2 +23):", dict(sorted(h_idx2.items())))
print(" pixels offset i32 (+24):", dict(sorted(h_pixoff.items())))
print(" pixels size i32 (+28):", dict(sorted(h_pixsize.items())))
print(" tail bytes 32..56 distinct:", len(h_tail), " most common:", h_tail.most_common(3))
print(" resource block second dword:", dict(sorted(h_resunk.items())))

# tag header bytes (the 164 bytes before the blocks) for a few tags
print("\n== bitm header (164 bytes) of three tags ==")
for t in parsed:
    if t["name"] and t["name"].endswith(("cust_base_floor_dif", "cust_base_floor_bump", "riverworld_riverworld_cubemaps")):
        print(f" {t['name']}:")
        hb = t["head"]
        for o in range(0, 164, 32):
            print(f"   +{o:3d} (0x{o:02x}) {hb[o:o + 32].hex(' ', 4)}")

# ---- resolve resources: segment extents vs chain sizes ----
print("\n== resource resolution ==")
cat = hist()
examples = collections.defaultdict(list)
deflen = hist()
rf_hist = hist()
nib_hist = hist()
loc_hist = hist()
ent_type = hist()
unresolved = 0
per_bitmap = []  # (tag, bitmap idx, resource info)
for t in parsed:
    for bi, b in enumerate(t["bitmaps"]):
        if t["il"]:
            rid, unk = t["il"][b["il_idx"]]
            src = "interleaved"
        else:
            if bi >= len(t["res"]):
                unresolved += 1
                continue
            rid, unk = t["res"][bi]
            src = "plain"
        e = entry(rid & 0xFFFF)
        ok = ((e["salt"] << 16) | e["idx"]) == rid and e["type_idx"] != 0xFF
        ent_type[(src, e["type_idx"], e["owner_cls"], ok)] += 1
        if not ok or e["seg_idx"] < 0:
            unresolved += 1
            continue
        s = segment(e["seg_idx"])
        loc_hist[(e["loc_type"], s["secondary_page"] >= 0)] += 1
        deflen[(src, e["fix_len"])] += 1
        rf_hist[(src, len(e["rf"]), len(e["df"]))] += 1
        for wa, val in e["rf"]:
            nib_hist[(src, val >> 28, wa)] += 1
        faces = 6 if b["type"] == 2 else (b["depth"] if b["type"] in (1, 3) else 1)
        if b["fmt"] not in FORMATS:
            cat[("unknown format", b["fmt"])] += 1
            continue
        full = chain(b["fmt"], b["w"], b["h"], b["mips"], faces)
        mip0 = chain(b["fmt"], b["w"], b["h"], 0, faces)
        rest = full - mip0
        pext = extent(s["primary_page"], s["primary_off"]) if s["primary_page"] >= 0 else None
        sext = extent(s["secondary_page"], s["secondary_off"]) if s["secondary_page"] >= 0 else None
        pfile = page_file(page(s["primary_page"])) if s["primary_page"] >= 0 else None
        sfile = page_file(page(s["secondary_page"])) if s["secondary_page"] >= 0 else None
        info = dict(tag=t, bi=bi, b=b, rid=rid, e=e, s=s, src=src, faces=faces, full=full, mip0=mip0,
                    rest=rest, pext=pext, sext=sext, pfile=pfile, sfile=sfile)
        per_bitmap.append(info)
        if src == "interleaved":
            continue  # handled below
        if sext is None:
            k = ("primary only", "pext==full" if pext == full else ("pext==mip0" if pext == mip0 else f"pext-full={pext - full}"))
        else:
            kp = "pext==rest" if pext == rest else ("pext==full" if pext == full else ("pext==mip0" if pext == mip0 else f"pext-rest={pext - rest}"))
            ks = "sext==mip0" if sext == mip0 else ("sext==full" if sext == full else f"sext-mip0={sext - mip0}")
            k = ("primary+secondary", kp, ks)
        cat[k] += 1
        if len(examples[k]) < 3:
            examples[k].append(f"{t['name']}[{bi}] {b['w']}x{b['h']} {fmt_name(b['fmt'])} {TYPES.get(b['type'])} mips={b['mips']} faces={faces} full={full:#x} mip0={mip0:#x} pext={pext} sext={sext}")
print(" unresolved (dead/stale/no segment):", unresolved)
print(" (source, entry type, owner class, salt ok):", dict(sorted(ent_type.items(), key=str)))
print(" (entry loc_type, has secondary page):", dict(sorted(loc_hist.items())))
print(" definition (fixup slice) length by source:", dict(sorted(deflen.items())))
print(" (source, resource fixup count, definition fixup count):", dict(sorted(rf_hist.items())))
print(" (source, fixup value nibble, write-at offset):", dict(sorted(nib_hist.items())))
print(" segment extent categories (non-interleaved):")
for k, v in sorted(cat.items(), key=str):
    print(f"   {v:5d}  {k}")
    for ex in examples[k]:
        print(f"           e.g. {ex}")

# where do the pages live?
where = hist()
for info in per_bitmap:
    where[(info["src"], info["pfile"], info["sfile"])] += 1
print(" (source, primary file, secondary file):", dict(sorted(where.items(), key=str)))

# ---- definition data of bitmap resources ----
print("\n== resource definition data (zone fixup slice) for bitmap resources ==")


def show_def(info):
    e = info["e"]
    b = info["b"]
    d = rd(fix_ptr + e["fix_off"], e["fix_len"])
    print(f" {info['tag']['name']}[{info['bi']}] {b['w']}x{b['h']} depth={b['depth']} {fmt_name(b['fmt'])} {TYPES.get(b['type'])} mips={b['mips']} curve={b['curve']} flags={b['flags']:#x} il_idx={b['il_idx']} idx2={b['idx2']}")
    print(f"   rid={info['rid']:#010x} entry {e['idx']} type={e['type_idx']} flags={e['flags']:#x} loc={e['loc_type']} seg={e['seg_idx']} def_addr={e['def_addr']:#x} fix_off={e['fix_off']:#x} len={e['fix_len']}")
    print(f"   segment: primary page {info['s']['primary_page']} ({info['pfile']}) off {info['s']['primary_off']:#x} extent {info['pext']}; secondary page {info['s']['secondary_page']} ({info['sfile']}) off {info['s']['secondary_off']:#x} extent {info['sext']}")
    print(f"   sizes: mip0(all faces)={info['mip0']:#x} rest={info['rest']:#x} full={info['full']:#x}")
    for o in range(0, len(d), 16):
        print(f"   def+{o:3d} (0x{o:02x}) {d[o:o + 16].hex(' ', 4)}")
    print("   resource fixups (write-at <- value):", [(f"{wa:#x}", f"{v:#010x}") for wa, v in e["rf"]])
    print("   definition fixups (addr, struct type):", [(f"{a:#010x}", ty) for a, ty in e["df"]])
    return d


shown = set()
for info in per_bitmap:
    key = (info["src"], info["sext"] is not None, info["b"]["type"], info["b"]["fmt"] in (14, 16, 33))
    if key in shown:
        continue
    shown.add(key)
    show_def(info)

# interleaved pairs
print("\n== interleaved resources ==")
il_tags = [t for t in parsed if t["il"]]
print(f" tags with an interleaved block: {len(il_tags)}")
pair_cat = hist()
for t in il_tags[:400]:
    groups = collections.defaultdict(list)
    for bi, b in enumerate(t["bitmaps"]):
        groups[b["il_idx"]].append((bi, b))
    for gi, members in groups.items():
        rid, unk = t["il"][gi]
        e = entry(rid & 0xFFFF)
        if e["seg_idx"] < 0:
            continue
        s = segment(e["seg_idx"])
        pext = extent(s["primary_page"], s["primary_off"]) if s["primary_page"] >= 0 else None
        sext = extent(s["secondary_page"], s["secondary_off"]) if s["secondary_page"] >= 0 else None
        sizes = []
        for bi, b in members:
            faces = 6 if b["type"] == 2 else (b["depth"] if b["type"] in (1, 3) else 1)
            sizes.append((chain(b["fmt"], b["w"], b["h"], 0, faces), chain(b["fmt"], b["w"], b["h"], b["mips"], faces)))
        sum_mip0 = sum(m for m, fl in sizes)
        sum_full = sum(fl for m, fl in sizes)
        sum_rest = sum_full - sum_mip0
        kp = "pext==sum_rest" if pext == sum_rest else ("pext==sum_full" if pext == sum_full else f"pext-sum_rest={pext - sum_rest}" if pext is not None else None)
        ks = None if sext is None else ("sext==sum_mip0" if sext == sum_mip0 else ("sext==sum_full" if sext == sum_full else f"sext-sum_mip0={sext - sum_mip0}"))
        key = (len(members), tuple(b["idx2"] for bi, b in members), tuple(TYPES.get(b["type"]) for bi, b in members), kp, ks)
        pair_cat[key] += 1
print(" (members, idx2 values, types, primary extent vs sums, secondary extent vs sums):")
for k, v in sorted(pair_cat.items(), key=str):
    print(f"   {v:5d}  {k}")
if il_tags:
    t = il_tags[0]
    print(f" first interleaved tag: {t['name']} bitmaps={len(t['bitmaps'])} il={len(t['il'])} res={len(t['res'])}")
    for bi, b in enumerate(t["bitmaps"]):
        print(f"   [{bi}] {b['w']}x{b['h']} {fmt_name(b['fmt'])} {TYPES.get(b['type'])} mips={b['mips']} il_idx={b['il_idx']} idx2={b['idx2']} flags={b['flags']:#x} more={b['more']:#x} curve={b['curve']}")
    for info in per_bitmap:
        if info["tag"] is t:
            show_def(info)
            break

# ---- BC decoders (first block) ----


def rgb565(v):
    r = (v >> 11) & 31
    g = (v >> 5) & 63
    b = v & 31
    return ((r * 255 + 15) // 31, (g * 255 + 31) // 63, (b * 255 + 15) // 31)


def bc1_block(blk):
    c0, c1, idx = struct.unpack("<HHI", blk[:8])
    p0, p1 = rgb565(c0), rgb565(c1)
    if c0 > c1:
        p2 = tuple((2 * a + b) // 3 for a, b in zip(p0, p1))
        p3 = tuple((a + 2 * b) // 3 for a, b in zip(p0, p1))
    else:
        p2 = tuple((a + b) // 2 for a, b in zip(p0, p1))
        p3 = (0, 0, 0)
    pal = [p0, p1, p2, p3]
    pix = [pal[(idx >> (2 * i)) & 3] for i in range(16)]
    return c0, c1, pal, pix


def bc4_block(blk):
    a0, a1 = blk[0], blk[1]
    bits = int.from_bytes(blk[2:8], "little")
    if a0 > a1:
        pal = [a0, a1] + [((7 - i) * a0 + i * a1) // 7 for i in range(1, 7)]
    else:
        pal = [a0, a1] + [((5 - i) * a0 + i * a1) // 5 for i in range(1, 5)] + [0, 255]
    return a0, a1, [pal[(bits >> (3 * i)) & 7] for i in range(16)]


def bc1_decode(data, w, h):
    """full BC1 decode -> list of (r,g,b) row-major (colour only)."""
    bw, bh = (w + 3) // 4, (h + 3) // 4
    out = [(0, 0, 0)] * (w * h)
    for by in range(bh):
        for bx in range(bw):
            o = (by * bw + bx) * 8
            _, _, _, pix = bc1_block(data[o:o + 8])
            for i in range(16):
                x, y = bx * 4 + (i & 3), by * 4 + (i >> 2)
                if x < w and y < h:
                    out[y * w + x] = pix[i]
    return out


def bc3_colour_decode(data, w, h):
    bw, bh = (w + 3) // 4, (h + 3) // 4
    out = [(0, 0, 0)] * (w * h)
    for by in range(bh):
        for bx in range(bw):
            o = (by * bw + bx) * 16 + 8
            _, _, _, pix = bc1_block(data[o:o + 8])
            for i in range(16):
                x, y = bx * 4 + (i & 3), by * 4 + (i >> 2)
                if x < w and y < h:
                    out[y * w + x] = pix[i]
    return out


def mae_downsample(big, w, h, small):
    """mean abs diff between 2x2 box-downsampled big (w x h) and small (w/2 x h/2)."""
    w2, h2 = w // 2, h // 2
    tot = 0
    n = 0
    for y in range(h2):
        for x in range(w2):
            s = small[y * w2 + x]
            acc = [0, 0, 0]
            for dy in (0, 1):
                for dx in (0, 1):
                    p = big[(2 * y + dy) * w + 2 * x + dx]
                    acc[0] += p[0]
                    acc[1] += p[1]
                    acc[2] += p[2]
            tot += sum(abs(acc[c] // 4 - s[c]) for c in range(3))
            n += 3
    return tot / n


def resource_bytes(info, which):
    s = info["s"]
    pi = s[f"{which}_page"]
    off = s[f"{which}_off"]
    data = load_page(pi)
    ext = extent(pi, off)
    return data[off:off + ext]


print("\n== mip 0 dumps and decodes ==")
WANT = [
    ("levels\\multi\\riverworld\\bitmaps\\forerunner\\cust_base_floor_dif", 0),
    ("levels\\multi\\riverworld\\bitmaps\\forerunner\\cust_base_floor_bump", 0),
    ("objects\\levels\\multi\\riverworld\\bitmaps\\man_cannon_diff", 0),
    ("levels\\multi\\riverworld\\sky\\bitmaps\\mountains_snow", 0),
    ("levels\\multi\\riverworld\\riverworld_riverworld_cubemaps", 0),
    ("levels\\multi\\riverworld\\bitmaps\\cliff_rock_base_brown", 0),
    ("levels\\multi\\riverworld\\bitmaps\\riverworld_metal", 0),
]
# also pick: one DXT5 2D, one DXT1 2D with secondary page, one array/3D if any, one A8R8G8B8 if any
picked = {}
for info in per_bitmap:
    b = info["b"]
    key = None
    if b["type"] == 3 and "array" not in picked:
        key = "array"
    elif b["type"] == 1 and "3d" not in picked:
        key = "3d"
    elif b["fmt"] == 16 and b["type"] == 0 and info["sext"] and "dxt5-2d" not in picked:
        key = "dxt5-2d"
    elif b["fmt"] in (10, 11) and "argb" not in picked:
        key = "argb"
    elif b["fmt"] == 0 and "a8" not in picked:
        key = "a8"
    elif b["fmt"] == 31 and "dxt5a" not in picked:
        key = "dxt5a"
    elif b["fmt"] == 1 and "y8" not in picked:
        key = "y8"
    elif b["fmt"] == 14 and b["type"] == 0 and info["sext"] is None and b["mips"] > 1 and "dxt1-primary-only" not in picked:
        key = "dxt1-primary-only"
    if key:
        picked[key] = info
        WANT.append((info["tag"]["name"], info["bi"]))
print(" extra picks:", {k: f"{v['tag']['name']}[{v['bi']}]" for k, v in picked.items()})

done = set()
for want_name, want_bi in WANT:
    if (want_name, want_bi) in done:
        continue
    done.add((want_name, want_bi))
    info = next((x for x in per_bitmap if x["tag"]["name"] == want_name and x["bi"] == want_bi), None)
    if info is None:
        print(f" !! {want_name}[{want_bi}] not resolved")
        continue
    b = info["b"]
    s = info["s"]
    print(f"\n-- {want_name}[{want_bi}]: {b['w']}x{b['h']}x{b['depth']} {TYPES.get(b['type'])} {fmt_name(b['fmt'])} mips={b['mips']} curve={CURVES.get(b['curve'])} flags={b['flags']:#x} rid={info['rid']:#010x} entry {info['e']['idx']} seg {s['idx']} loc={info['e']['loc_type']}")
    print(f"   primary: page {s['primary_page']} in {info['pfile']} (cache_idx {page(s['primary_page'])['cache_idx']}) off {s['primary_off']:#x} extent {info['pext']} ; secondary: page {s['secondary_page']} in {info['sfile']} off {s['secondary_off']:#x} extent {info['sext']}")
    print(f"   expected: mip0={info['mip0']:#x} ({info['mip0']}) rest={info['rest']:#x} full={info['full']:#x}")
    hi = resource_bytes(info, "secondary" if s["secondary_page"] >= 0 else "primary")
    print(f"   first 16 bytes of mip 0 ({'secondary' if s['secondary_page'] >= 0 else 'primary'} page): {hi[:16].hex(' ', 4)}")
    print(f"   last 16 bytes of that segment: {hi[-16:].hex(' ', 4)} ; zero bytes in segment: {hi.count(0)}/{len(hi)}")
    fmtv = b["fmt"]
    if fmtv == 14:
        c0, c1, pal, pix = bc1_block(hi[:8])
        print(f"   BC1 block 0: c0={c0:#06x} {pal[0]} c1={c1:#06x} {pal[1]} mode={'4-colour' if c0 > c1 else '3-colour+black'} palette={pal}")
        print(f"   pixel indices row0..3: {[(struct.unpack_from('<I', hi, 4)[0] >> (2 * i)) & 3 for i in range(16)]}")
    elif fmtv in (15, 16):
        a0, a1, apix = bc4_block(hi[:8])
        c0, c1, pal, pix = bc1_block(hi[8:16])
        print(f"   BC3 block 0: alpha a0={a0} a1={a1} alphas={apix}")
        print(f"   colour c0={c0:#06x} {pal[0]} c1={c1:#06x} {pal[1]} palette={pal}")
    elif fmtv == 33:
        r0, r1, rpix = bc4_block(hi[:8])
        g0, g1, gpix = bc4_block(hi[8:16])
        print(f"   BC5 block 0: red(X) endpoints {r0},{r1} values {rpix}")
        print(f"               green(Y) endpoints {g0},{g1} values {gpix}")
        sr = [(v - 256 if v > 127 else v) for v in rpix]
        print(f"   as unsigned: mean X={sum(rpix) / 16:.1f} mean Y={sum(gpix) / 16:.1f} (flat normal would be ~128)")
        print(f"   as signed  : mean X={sum(sr) / 16:.1f} (flat normal would be ~0)")
    elif fmtv == 31:
        a0, a1, apix = bc4_block(hi[:8])
        print(f"   BC4 block 0: endpoints {a0},{a1} values {apix}")
    # mip layout check for DXT1 / DXT5 2D textures: does mip 1 follow mip 0 directly?
    if b["type"] in (0, 2) and fmtv in (14, 16) and b["mips"] >= 1 and b["w"] >= 8 and b["h"] >= 8:
        faces = info["faces"]
        w, h = b["w"], b["h"]
        dec = bc1_decode if fmtv == 14 else bc3_colour_decode
        m0 = mip_bytes(fmtv, w, h)
        m1 = mip_bytes(fmtv, w // 2, h // 2)
        src_hi = hi
        big = dec(src_hi[:m0], w, h)
        cands = {}
        if s["secondary_page"] >= 0:
            lo = resource_bytes(info, "primary")
            # secondary holds mip 0 (all faces); primary holds mips 1..n: mip 1 of face 0 is at primary offset 0 (mip-major) or 0 (face-major) as well
            cands["primary+0 (mip1 of face0 if primary = mips 1..n)"] = lo[:m1]
            if faces > 1:
                cands[f"secondary+{m0:#x} (face1 mip0 if mip-major)"] = src_hi[m0:m0 + m0]
                cands[f"primary+{m1 * faces:#x} (mip2 face0 if mip-major in primary)"] = lo[m1 * faces:m1 * faces + mip_bytes(fmtv, w // 4, h // 4)]
                cands[f"primary+{m1:#x} (face1 mip1 if mip-major in primary)"] = lo[m1:2 * m1]
        else:
            cands[f"primary+{m0 * faces:#x} (mip1 right after all mip0 faces)"] = src_hi[m0 * faces:m0 * faces + m1]
            cands[f"primary+{m0:#x} (mip1 right after face0 mip0)"] = src_hi[m0:m0 + m1]
            if m0 % 4:
                cands[f"primary+{(m0 + 3) & ~3:#x} (4-aligned)"] = src_hi[(m0 + 3) & ~3:((m0 + 3) & ~3) + m1]
        for label, blob in cands.items():
            if len(blob) < m1 and "mip2" not in label and "face1" not in label:
                print(f"   candidate {label}: too short ({len(blob)} < {m1})")
                continue
            if "mip2" in label or "face1" in label:
                # just report whether it is non-empty and distinct
                print(f"   candidate {label}: first 16 bytes {blob[:16].hex(' ', 4)} zeros={blob.count(0)}/{len(blob)}")
                continue
            small = dec(blob, w // 2, h // 2)
            mae = mae_downsample(big, w, h, small)
            # control: compare against a shifted misalignment (half a block row later)
            print(f"   candidate {label}: MAE(downsampled mip0, candidate mip1) = {mae:.2f}  (good match < ~8, random data ~40+)")
        # control: random alignment inside mip 0 itself
        ctrl = dec(src_hi[m0 // 3:m0 // 3 + m1], w // 2, h // 2)
        print(f"   control (bytes from inside mip 0): MAE = {mae_downsample(big, w, h, ctrl):.2f}")

print("\n== five-bitmap summary table for the doc ==")
for want_name, want_bi in WANT[:7]:
    info = next((x for x in per_bitmap if x["tag"]["name"] == want_name and x["bi"] == want_bi), None)
    if info is None:
        continue
    b, s, e = info["b"], info["s"], info["e"]
    print(f" | `{want_name}` | tag #{info['tag']['tidx']} ({info['tag']['tidx']:#x}) | {b['w']}x{b['h']} | {TYPES.get(b['type'])} | {fmt_name(b['fmt'])} ({b['fmt']}) | {b['mips']} | {CURVES.get(b['curve'])} | `{info['rid']:#010x}` -> entry {e['idx']} | seg {s['idx']}: primary page {s['primary_page']} {info['pfile']} +{s['primary_off']:#x} ({info['pext']} B); secondary page {s['secondary_page']} {info['sfile']} +{s['secondary_off']:#x} ({info['sext']} B) |")


# ======================================================================================
# Pass 2: verify the 56-byte texture definition against every resolved bitmap
# ======================================================================================
print("\n== definition (56-byte texture description + 12-byte d3d struct) checks over all resolved bitmaps ==")
rtype_names = {}
for i in range(rt_cnt):
    rtype_names[i] = rd(rt_ptr + 28 * i, 28)[24:28].hex()
print(" resource type index used by bitmap entries (per map the table order differs; resolve by name):",
      dict(hist(e["type_idx"] for e in (info["e"] for info in per_bitmap))))
DXGI = {71: "BC1_UNORM", 72: "BC1_UNORM_SRGB", 74: "BC2_UNORM", 75: "BC2_UNORM_SRGB", 77: "BC3_UNORM",
        78: "BC3_UNORM_SRGB", 80: "BC4_UNORM", 81: "BC4_SNORM", 83: "BC5_UNORM", 84: "BC5_SNORM",
        87: "B8G8R8A8_UNORM", 91: "B8G8R8A8_UNORM_SRGB", 88: "B8G8R8X8_UNORM", 65: "A8_UNORM",
        61: "R8_UNORM", 49: "R8G8_UNORM", 51: "R8G8_SNORM", 115: "B4G4R4A4_UNORM", 85: "B5G6R5_UNORM",
        86: "B5G5R5A1_UNORM", 28: "R8G8B8A8_UNORM", 29: "R8G8B8A8_UNORM_SRGB", 10: "R16G16B16A16_FLOAT",
        2: "R32G32B32A32_FLOAT", 6: "R32G32B32_FLOAT", 56: "R16_UNORM", 98: "BC7_UNORM", 99: "BC7_UNORM_SRGB",
        95: "BC6H_UF16", 31: "R8G8B8A8_SNORM", 20: "R16G16B16A16_UNORM", 60: "R8_TYPELESS", 48: "R8G8_TYPELESS"}
chk = hist()
dxgi_map = collections.defaultdict(hist)
b54 = hist()
b55 = hist()
flag47 = hist()
flag9 = hist()
pixsize_eq = hist()
fmt25 = []
for info in per_bitmap:
    e, b = info["e"], info["b"]
    d = rd(fix_ptr + e["fix_off"], e["fix_len"])
    has_sec = info["sext"] is not None
    p_size, = struct.unpack_from("<i", d, 0)
    s_size, = struct.unpack_from("<i", d, 20)
    w, h = struct.unpack_from("<HH", d, 40)
    depth, levels, ttype, tflags = d[44], d[45], d[46], d[47]
    dxgi, = struct.unpack_from("<i", d, 48)
    f52, c53, u54, u55 = d[52], d[53], d[54], d[55]
    chk["def+0 == primary size (rest if secondary else full)"] += (p_size == (info["rest"] if has_sec else info["full"]))
    chk["def+20 == secondary size (mip0 if secondary else 0)"] += (s_size == (info["mip0"] if has_sec else 0))
    chk["def+40/42 == width/height"] += (w == b["w"] and h == b["h"])
    chk["def+44 == depth"] += (depth == b["depth"])
    chk["def+45 == mipmap count + 1"] += (levels == b["mips"] + 1)
    chk["def+46 == type"] += (ttype == b["type"])
    chk["def+52 == format"] += (f52 == b["fmt"])
    chk["def+53 == curve"] += (c53 == b["curve"])
    chk["def+54 == more flags (+14) low byte"] += (u54 == (b["more"] & 0xFF))
    chk["def+4..11, 16..19, 24..31, 36..39 zero"] += (d[4:12] == bytes(8) and d[16:20] == bytes(4) and d[24:32] == bytes(8) and d[36:40] == bytes(4))
    chk["def+56 d3d struct dword0 == 0 before fixup, dword1 == 0"] += (d[56:64] == bytes(8))
    chk["fixup[0] = (write 12, 0x40000000) ; secondary fixup = (write 32, 0x60000000); last = (write 56, 0x20000000)"] += (
        e["rf"][0] == (12, 0x40000000) and e["rf"][-1] == (56, 0x20000000) and (not has_sec or e["rf"][1] == (32, 0x60000000)))
    chk["definition fixup == (0x20000038, struct type 2 s_tag_d3d_texture)"] += (e["df"] == [(0x20000038, 2)])
    chk["segment offset of secondary page == 0 and secondary page usize == mip0"] += (not has_sec or (info["s"]["secondary_off"] == 0 and page(info["s"]["secondary_page"])["usize"] == info["mip0"]))
    chk["total"] += 1
    dxgi_map[(fmt_name(b["fmt"]), CURVES.get(b["curve"]))][(dxgi, DXGI.get(dxgi, "?"))] += 1
    b54[(b["more"], u54)] += 1
    b55[u55] += 1
    flag47[(tflags, has_sec)] += 1
    flag9[(b["flags"], has_sec, TYPES.get(b["type"]))] += 1
    pixsize_eq[("pix_size == full" if b["pix_size"] == info["full"] else ("pix_size == mip0" if b["pix_size"] == info["mip0"] else "other"))] += 1
    if b["fmt"] == 25:
        fmt25.append(f"{info['tag']['name']}[{info['bi']}] {b['w']}x{b['h']} type={TYPES.get(b['type'])} mips={b['mips']} curve={b['curve']} dxgi={dxgi} ({DXGI.get(dxgi, '?')}) primary size={p_size:#x} secondary size={s_size:#x} has_sec={has_sec} pix_size={b['pix_size']} flags={b['flags']:#x} more={b['more']}")
for k, v in chk.items():
    print(f"   {v:5d} / {chk['total']}  {k}")
print(" (format, curve) -> DXGI format at def+48:")
for k in sorted(dxgi_map, key=str):
    print(f"   {k}: {dict(dxgi_map[k])}")
print(" (bitmap more-flags +14, def+54):", dict(sorted(b54.items())))
print(" def+55:", dict(b55))
print(" (def+47 flags, has secondary page):", dict(sorted(flag47.items())))
print(" (bitmap flags +9, has secondary page, type):", dict(sorted(flag9.items(), key=str)))
print(" pixels size (+28) vs computed:", dict(pixsize_eq))
print(" format 25 bitmaps (not in per_bitmap; resolved here directly):")
for t in parsed:
    for bi, b in enumerate(t["bitmaps"]):
        if b["fmt"] != 25 or bi >= len(t["res"]):
            continue
        rid, _ = t["res"][bi]
        e = entry(rid & 0xFFFF)
        if e["seg_idx"] < 0 or ((e["salt"] << 16) | e["idx"]) != rid:
            print(f"   {t['name']}[{bi}] {b['w']}x{b['h']} unresolved")
            continue
        s = segment(e["seg_idx"])
        d = rd(fix_ptr + e["fix_off"], e["fix_len"])
        p_size, = struct.unpack_from("<i", d, 0)
        s_size, = struct.unpack_from("<i", d, 20)
        dxgi, = struct.unpack_from("<i", d, 48)
        pp = page(s["primary_page"])
        sp = page(s["secondary_page"]) if s["secondary_page"] >= 0 else None
        pix = b["w"] * b["h"] * (6 if b["type"] == 2 else max(1, b["depth"]))
        print(f"   {t['name']}[{bi}] {b['w']}x{b['h']}x{b['depth']} {TYPES.get(b['type'])} mips={b['mips']} curve={CURVES.get(b['curve'])} flags={b['flags']:#x} more={b['more']} pix_size={b['pix_size']} ; def: primary size={p_size:#x} secondary size={s_size:#x} dxgi={dxgi} ({DXGI.get(dxgi, '?')}) def+52..56={d[52:56].hex()} ; primary page {s['primary_page']} ({page_file(pp)}) usize={pp['usize']:#x} off={s['primary_off']:#x}; secondary {s['secondary_page']} usize={(sp['usize'] if sp else None)} ; bits/pixel if mip0 only = {8 * (s_size if sp else p_size) / pix:.3f}")
        if sp is None and s["primary_off"] == 0 and pp["usize"] < 0x20000:
            data = load_page(s["primary_page"])
            print(f"      first 32 bytes: {data[:32].hex(' ', 4)} ; zero bytes {data.count(0)}/{len(data)}")

print("\n== sequences / sprites sample ==")
shown_seq = 0
for t in parsed:
    if t["seqs"] and any(sp for (_, _, _, sp) in t["seqs"]) and shown_seq < 2:
        shown_seq += 1
        meta = t["meta"]
        seq_cnt, seq_ptr = block(meta + 84)
        print(f" {t['name']}: {seq_cnt} sequences, {len(t['bitmaps'])} bitmaps")
        for i in range(min(seq_cnt, 3)):
            sb = rd(seq_ptr + 64 * i, 64)
            sname = sb[:32].split(b"\0")[0].decode("latin1")
            first, cnt = struct.unpack_from("<hh", sb, 32)
            print(f"   seq[{i}] name={sname!r} first_submap={first} bitmap_count={cnt} bytes36..52={sb[36:52].hex(' ', 4)} sprites block={struct.unpack_from('<iI', sb, 52)} tail={sb[64 - 0:].hex()}")
            sp_cnt, sp_ptr = block(seq_ptr + 64 * i + 52)
            for j in range(min(sp_cnt, 2)):
                spb = rd(sp_ptr + 32 * j, 32)
                sub, = struct.unpack_from("<h", spb, 0)
                left, right, top, bottom, rx, ry = struct.unpack_from("<6f", spb, 8)
                print(f"      sprite[{j}] submap={sub} bytes2..8={spb[2:8].hex()} left={left:.4f} right={right:.4f} top={top:.4f} bottom={bottom:.4f} reg=({rx:.4f},{ry:.4f})")

# ======================================================================================
# Pass 3: array and cube layouts with raw / BC decoding
# ======================================================================================
print("\n== array / cube layout checks ==")


def bgra_decode(data, w, h):
    return [(data[i * 4 + 2], data[i * 4 + 1], data[i * 4]) for i in range(w * h)]


for info in per_bitmap:
    b = info["b"]
    if info["tag"]["name"] == "levels\\shared\\bitmaps\\nature\\water\\water_ripples" and info["bi"] == 0:
        w, h, faces = b["w"], b["h"], info["faces"]
        data = resource_bytes(info, "primary")
        m0 = mip_bytes(b["fmt"], w, h)
        m1 = mip_bytes(b["fmt"], w // 2, h // 2)
        big0 = bgra_decode(data[:m0], w, h)
        big1 = bgra_decode(data[m0:2 * m0], w, h)
        print(f" {info['tag']['name']}: {w}x{h}x{faces} A8R8G8B8 mips={b['mips']} (raw pixels; byte order in file B,G,R,A -> DXGI B8G8R8A8)")
        print(f"   slice0 mip0 first 16 bytes {data[:16].hex(' ', 4)} ; slice1 mip0 (if mip-major, at +{m0:#x}) first 16 bytes {data[m0:m0 + 16].hex(' ', 4)}")
        cands = {
            f"mip-major: slice0 mip1 at +{faces * m0:#x}": (big0, data[faces * m0:faces * m0 + m1]),
            f"mip-major: slice1 mip1 at +{faces * m0 + m1:#x}": (big1, data[faces * m0 + m1:faces * m0 + 2 * m1]),
            f"slice-major: slice0 mip1 at +{m0:#x}": (big0, data[m0:m0 + m1]),
        }
        for label, (big, blob) in cands.items():
            small = bgra_decode(blob, w // 2, h // 2)
            print(f"   {label}: MAE = {mae_downsample(big, w, h, small):.2f}")
    if info["tag"]["name"] == "levels\\multi\\riverworld\\riverworld_riverworld_cubemaps" and info["bi"] == 0:
        w, h = b["w"], b["h"]
        data = resource_bytes(info, "primary")
        m0 = mip_bytes(b["fmt"], w, h)
        m1 = mip_bytes(b["fmt"], w // 2, h // 2)
        m2 = mip_bytes(b["fmt"], w // 4, h // 4)
        print(f" {info['tag']['name']}[0]: cube {w}x{h} DXT5 mips={b['mips']} ; mip0 face = {m0:#x}, mip1 face = {m1:#x}, mip2 face = {m2:#x}; primary size def+0 = {struct.unpack_from('<i', rd(fix_ptr + info['e']['fix_off'], 4), 0)[0]:#x} = 6*(m0+m1+m2) = {6 * (m0 + m1 + m2):#x}")
        for face in range(6):
            big = bc3_colour_decode(data[face * m0:(face + 1) * m0], w, h)
            off_mm = 6 * m0 + face * m1
            small = bc3_colour_decode(data[off_mm:off_mm + m1], w // 2, h // 2)
            off_fm = face * (m0 + m1 + m2) + m0
            small2 = bc3_colour_decode(data[off_fm:off_fm + m1], w // 2, h // 2)
            print(f"   face {face}: mip-major candidate (+{off_mm:#x}) MAE = {mae_downsample(big, w, h, small):.2f} ; face-major candidate (+{off_fm:#x}) MAE = {mae_downsample(big, w, h, small2):.2f}")
        big1 = bc3_colour_decode(data[6 * m0:6 * m0 + m1], w // 2, h // 2)
        small = bc3_colour_decode(data[6 * m0 + 6 * m1:6 * m0 + 6 * m1 + m2], w // 4, h // 4)
        print(f"   face 0 mip2 at +{6 * m0 + 6 * m1:#x} vs downsampled face0 mip1: MAE = {mae_downsample(big1, w // 2, h // 2, small):.2f}")
        for face in range(6):
            px = bc3_colour_decode(data[face * m0:(face + 1) * m0], w, h)
            n = len(px)
            top = px[:w * 8]
            bot = px[-w * 8:]
            print(f"   face {face} mean RGB = ({sum(p[0] for p in px) / n:.0f}, {sum(p[1] for p in px) / n:.0f}, {sum(p[2] for p in px) / n:.0f}) ; top rows mean = {sum(sum(p) for p in top) / (3 * len(top)):.0f} ; bottom rows mean = {sum(sum(p) for p in bot) / (3 * len(bot)):.0f}")

for info in per_bitmap:
    b = info["b"]
    if b["fmt"] in (0, 1) and b["mips"] >= 6 and info["sext"] is not None:
        d = rd(fix_ptr + info["e"]["fix_off"], 4)
        aligned = sum((max(1, (b["w"] >> i)) * max(1, (b["h"] >> i)) + 3) & ~3 for i in range(1, b["mips"] + 1))
        print(f" {info['tag']['name']}[{info['bi']}] {fmt_name(b['fmt'])} {b['w']}x{b['h']} mips={b['mips']}: def+0 primary size = {struct.unpack('<i', d)[0]:#x}, computed mips 1..n unpadded = {info['rest']:#x}, 4-aligned per mip would be {aligned:#x}")
        break
