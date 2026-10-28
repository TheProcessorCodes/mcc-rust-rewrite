"""Probe the MCC Halo 3 (U13) resource system on a .map file. Read-only.

usage: python -I probe_resources.py <maps dir> [map name]

Companion to docs/format/resources.md. Prints the zone/play tables, the
sbsp + Lbsp resource ids, the resolved page file offsets, inflates the pages
and shows the first bytes of the resource data. Every number here is an
EXPECTED VALUE for the Rust reader.
"""
import os
import struct
import sys
import zlib

MAPS = sys.argv[1]
NAME = sys.argv[2] if len(sys.argv) > 2 else "riverworld.map"
path = os.path.join(MAPS, NAME)
f = open(path, "rb")


def rd(off, n):
    f.seek(off)
    return f.read(n)


def i32(off): return struct.unpack("<i", rd(off, 4))[0]
def u32(off): return struct.unpack("<I", rd(off, 4))[0]
def i16(off): return struct.unpack("<h", rd(off, 2))[0]
def i64(off): return struct.unpack("<q", rd(off, 8))[0]


def hx(dct):
    return {k: (hex(v) if isinstance(v, int) else v) for k, v in dct.items()
            if not k.startswith("hash") and k not in ("raw", "rf_list", "df_list") and not k.startswith("page_data")}


# ---- header (U6+ layout) ----
virt_base = i64(0x2E0)
idx_ptr = i64(0x2E8)
sec_off = struct.unpack("<4I", rd(0x4CC, 16))
sec = [struct.unpack("<II", rd(0x4DC + 8 * i, 8)) for i in range(4)]


def hdr_addr(p):  # section 0 (debug) 32-bit pointer -> file offset
    return (p + sec_off[0]) & 0xFFFFFFFF


def tag_addr(v):  # 64-bit virtual address -> file offset
    return v - virt_base + ((sec[2][0] + sec_off[2]) & 0xFFFFFFFF)


def ptr32(p):  # shifted 32-bit tag-data pointer -> file offset (Reclaimer: ((long)(int)p) << 2, no 32-bit wrap)
    if p == 0:
        return -1
    if p >= 0x80000000:
        p -= 0x100000000
    return tag_addr(p << 2)


raw_base = sec_off[1]
print(f"virt_base={virt_base:#x} idx_ptr={idx_ptr:#x} raw_base(secoff[1])={raw_base:#x} u32@0x4D0={u32(0x4D0):#x}")

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


# ---- string ids (U13: index 19 bits, namespace 8 bits, length 5 bits; header namespace table) ----
str_count = i32(0x30)
str_tbl = hdr_addr(u32(0x34))
str_idx = hdr_addr(u32(0x3C))
ns_count = i32(0x40)
ns_tbl = hdr_addr(u32(0x44))
ns_arr = struct.unpack(f"<{ns_count}i", rd(ns_tbl, 4 * ns_count))
INDEX_BITS, NS_BITS = 19, 8
mask = (1 << INDEX_BITS) - 1
namespaces = {}
start = ns_arr[0] & mask
for n in range(1, ns_count):
    namespaces[n] = (0, start)  # (min, start)
    start += ns_arr[n] & mask
namespaces[0] = (ns_arr[0] & mask, start)


def string_id(v):
    index = v & mask
    ns = (v >> INDEX_BITS) & ((1 << NS_BITS) - 1)
    while ns not in namespaces and ns > 0:
        ns -= 1
    mn, st = namespaces[ns]
    si = index if index < mn else index - mn + st
    if si < 0 or si >= str_count:
        return f"<bad sid {v:#x}>"
    o = i32(str_idx + 4 * si)
    return "" if o < 0 else cstr(str_tbl + o)


zone_i = find("zone")[0]
play_i = find("play")[0]
zone_meta = ptr32(items[zone_i][1])
print(f"zone tag #{zone_i:#x} name={names[zone_i]!r} metaptr(shifted)={items[zone_i][1]:#x} -> file {zone_meta:#x}")
print(f"play tag #{play_i:#x} name={names[play_i]!r} metaptr(shifted)={items[play_i][1]:#x} (0 => Reclaimer reads layout table at zone+28)")
play_meta = zone_meta + 28 if items[play_i][1] == 0 else ptr32(items[play_i][1])
print(f"play meta @file {play_meta:#x}; zone+28 would be {zone_meta + 28:#x}")


def block(off):
    cnt, p = struct.unpack("<iI", rd(off, 8))
    return cnt, (ptr32(p) if cnt else -1)


print("\n-- zone (cache_file_resource_gestalt) bytes 0..100:")
print(rd(zone_meta, 100).hex(" ", 4))
for o in range(4, 100, 12):
    c, p = struct.unpack("<iI", rd(zone_meta + o, 8))
    print(f"   zone+{o:3d} (0x{o:02x}) as block: count={c} ptr={p:#x} -> file {ptr32(p) if c else -1:#x}")

cc_cnt, cc_ptr = block(play_meta + 0)
sc_cnt, sc_ptr = block(play_meta + 12)
pg_cnt, pg_ptr = block(play_meta + 24)
sg_cnt, sg_ptr = block(play_meta + 36)
seg_cnt, seg_ptr = block(play_meta + 60)
print(f"\n-- play (layout table) @file {play_meta:#x}: codecs={cc_cnt} @{cc_ptr:#x} shared_caches={sc_cnt} @{sc_ptr:#x}  pages={pg_cnt} @{pg_ptr:#x}  size_groups={sg_cnt} @{sg_ptr:#x}  segments={seg_cnt} @{seg_ptr:#x}")
print("   play+0..12:", rd(play_meta, 12).hex(" ", 4), " play+48..60:", rd(play_meta + 48, 12).hex(" ", 4), " play+72..84:", rd(play_meta + 72, 12).hex(" ", 4))
if cc_cnt:
    print(f"   compression codec[0] guid = {rd(cc_ptr, 16).hex()}")
shared = []
for i in range(sc_cnt):
    b = rd(sc_ptr + 264 * i, 264)
    fn = b[:256].split(b"\0")[0].decode("latin1")
    shared.append(fn)
    print(f"   shared[{i}] = {fn!r}  tail(256..264)={b[256:].hex(' ', 2)}")

re_cnt, re_ptr = block(zone_meta + 100)
fix_size = i32(zone_meta + 328)
fix_ptr = ptr32(u32(zone_meta + 340))
print(f"\n-- zone: resource_entries={re_cnt} @file {re_ptr:#x}; fixup data size={fix_size:#x} ({fix_size}) @file {fix_ptr:#x}; zone+328..348: {rd(zone_meta + 328, 20).hex(' ', 4)}")


def page(i):
    b = rd(pg_ptr + 88 * i, 88)
    salt, flags, codec, cache_idx, unk6, data_off, csize, usize, crc = struct.unpack_from("<HBbhhiiiI", b, 0)
    asset_cnt, unk86 = struct.unpack_from("<hh", b, 84)
    return dict(salt=salt, flags=flags, codec=codec, cache_idx=cache_idx, unk6=unk6, data_off=data_off, csize=csize,
                usize=usize, crc=crc, hash_all=b[24:44].hex(), hash_first=b[44:64].hex(), hash_last=b[64:84].hex(),
                asset_cnt=asset_cnt, unk86=unk86)


def segment(i):
    pp, sp, po, so, ps, ss = struct.unpack("<hhiihh", rd(seg_ptr + 16 * i, 16))
    return dict(primary_page=pp, secondary_page=sp, primary_off=po, secondary_off=so, primary_size_idx=ps, secondary_size_idx=ss)


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
    return dict(owner_cls=cls, owner_tag=tag_id, ident=(salt << 16) | i, idx=i, salt=salt, type_idx=b[18],
                flags=b[19], fix_off=fix_off, fix_len=fix_len, sec_fix_off=sec_fix_off, loc_type=loc_type,
                seg_idx=seg_idx, def_addr=def_addr, rf=(rf_cnt, ptr32(rf_ptr) if rf_cnt else -1),
                df=(df_cnt, ptr32(df_ptr) if df_cnt else -1), raw=b)


codecs = {}
cidx = {}
comp = 0
flagsh = {}
for i in range(pg_cnt):
    p = page(i)
    codecs[p["codec"]] = codecs.get(p["codec"], 0) + 1
    cidx[p["cache_idx"]] = cidx.get(p["cache_idx"], 0) + 1
    flagsh[p["flags"]] = flagsh.get(p["flags"], 0) + 1
    if p["csize"] != p["usize"]:
        comp += 1
print(f"   page codec byte histogram={codecs}  cache_idx histogram={cidx}  flags histogram={flagsh}  pages with csize!=usize: {comp}")
print("   page[0]:", hx(page(0)))
print("   page[1]:", hx(page(1)))
salts = [entry(i)["salt"] for i in range(re_cnt)]
print(f"   salt (u16 at entry+16): min={min(salts):#x} max={max(salts):#x} distinct={len(set(salts))} of {re_cnt}; first 8: {[hex(x) for x in salts[:8]]}")
types = {}
locs = {}
nib = {}
dnib = {}
segs_used = set()
for i in range(re_cnt):
    e = entry(i)
    k = (e["type_idx"], e["owner_cls"])
    types[k] = types.get(k, 0) + 1
    locs[e["loc_type"]] = locs.get(e["loc_type"], 0) + 1
    if e["seg_idx"] >= 0:
        segs_used.add(e["seg_idx"])
    for k2 in range(e["rf"][0]):
        a, b = struct.unpack("<iI", rd(e["rf"][1] + 8 * k2, 8))
        nib[b >> 28] = nib.get(b >> 28, 0) + 1
    for k2 in range(e["df"][0]):
        a, b = struct.unpack("<Ii", rd(e["df"][1] + 8 * k2, 8))
        dnib[a >> 28] = dnib.get(a >> 28, 0) + 1
print("   (type_idx, owner class) histogram:", dict(sorted(types.items())))
print("   loc_type (entry+32) histogram:", locs, " distinct segments referenced:", len(segs_used), "of", seg_cnt)
print("   resource-fixup value top-nibble histogram (all entries):", nib)
print("   definition-fixup address top-nibble histogram (all entries):", dnib)
# loc_type vs secondary page
lt = {}
for i in range(re_cnt):
    e = entry(i)
    if e["seg_idx"] < 0:
        continue
    s = segment(e["seg_idx"])
    p1 = page(s["primary_page"]) if s["primary_page"] >= 0 else None
    k = (e["loc_type"], s["secondary_page"] >= 0, (p1["cache_idx"] if p1 else None))
    lt[k] = lt.get(k, 0) + 1
print("   (loc_type, has secondary page, primary page cache_idx) histogram:", lt)

rt_cnt, rt_ptr = block(zone_meta + 4)
print(f"   zone+4 resource types: count={rt_cnt} @ {rt_ptr:#x} (28 bytes each: guid[16], 4 x i16, string id)")
for i in range(rt_cnt):
    b = rd(rt_ptr + 28 * i, 28)
    shorts = struct.unpack_from("<4h", b, 16)
    sid = struct.unpack_from("<I", b, 24)[0]
    print(f"      type[{i}] guid={b[:16].hex()} shorts={shorts} name={string_id(sid)!r} (sid {sid:#x})")
rs_cnt, rs_ptr = block(zone_meta + 16)
print(f"   zone+16 resource structure types: count={rs_cnt} @ {rs_ptr:#x} (20 bytes each: guid[16], string id)")
for i in range(rs_cnt):
    b = rd(rs_ptr + 20 * i, 20)
    sid = struct.unpack_from("<I", b, 16)[0]
    print(f"      struct[{i}] guid={b[:16].hex()} name={string_id(sid)!r} (sid {sid:#x})")

# ---- sbsp ----
sbsp_i = find("sbsp")[0]
sbsp_meta = ptr32(items[sbsp_i][1])
print(f"\n-- sbsp #{sbsp_i:#x} {names[sbsp_i]!r} meta @file {sbsp_meta:#x}")
for off in (596, 876, 908):
    v = u32(sbsp_meta + off)
    print(f"   sbsp+{off} (0x{off:x}) resource id = {v:#010x} -> index {v & 0xFFFF} salt {v >> 16:#x}")
lbsp = find("Lbsp", names[sbsp_i])
print(f"   Lbsp tags named like the sbsp: {[hex(x) for x in lbsp]}")
lbsp_i = lbsp[0]
lbsp_meta = ptr32(items[lbsp_i][1])
lb_sec_cnt, lb_sec_ptr = block(lbsp_meta + 308)
lb_res = u32(lbsp_meta + 428)
print(f"   Lbsp #{lbsp_i:#x} meta @file {lbsp_meta:#x}: bsp_index={i16(lbsp_meta + 2)} sections={lb_sec_cnt} @{lb_sec_ptr:#x} resource id @+428 = {lb_res:#010x} -> index {lb_res & 0xFFFF} salt {lb_res >> 16:#x}")

page_cache = {}


def load_page(pi):
    """Return (file name, file offset, inflated bytes) for page pi, applying the Reclaimer rule."""
    if pi in page_cache:
        return page_cache[pi]
    p = page(pi)
    fn = NAME if p["cache_idx"] < 0 else os.path.basename(shared[p["cache_idx"]].replace("\\", "/"))
    g = open(os.path.join(MAPS, fn), "rb")
    g.seek(0x4D0)
    base = struct.unpack("<I", g.read(4))[0]
    g.seek(0x4CC + 4)
    base2 = struct.unpack("<I", g.read(4))[0]
    off = base + p["data_off"]
    g.seek(off)
    comp = g.read(p["csize"])
    g.close()
    info = dict(file=fn, base=base, base_is_secoff1=(base == base2), off=off, first2=comp[:2].hex())
    if p["csize"] == p["usize"]:
        data = comp
        info["codec"] = "stored (csize == usize)"
    else:
        dobj = zlib.decompressobj(-15)
        data = dobj.decompress(comp)
        info["codec"] = f"raw deflate: inflated {len(data):#x} bytes, match usize={len(data) == p['usize']}, eof={dobj.eof}, unused={len(dobj.unused_data)}"
    info["crc_comp"] = zlib.crc32(comp) & 0xFFFFFFFF
    info["crc_uncomp"] = zlib.crc32(data) & 0xFFFFFFFF
    page_cache[pi] = (info, data)
    return page_cache[pi]


def dump_resource(label, rid, want_data=True):
    e = entry(rid & 0xFFFF)
    print(f"\n==== {label}: resource id {rid:#010x} ====")
    print("   entry raw:", e["raw"].hex(" ", 4))
    print("   entry:", hx(e))
    if e["ident"] != rid:
        print(f"   !! salt mismatch: entry salt {e['salt']:#x} vs reference salt {rid >> 16:#x} -> stale/dead reference (entry wiped), stop here")
        return e, b""
    ot = e["owner_tag"] & 0xFFFF
    print(f"   owner tag {e['owner_tag'] & 0xFFFFFFFF:#x} -> {names[ot]!r} [{classes[items[ot][0]]}]")
    s = segment(e["seg_idx"])
    print("   segment:", s)
    for which in ("primary", "secondary"):
        pi = s[f"{which}_page"]
        if pi < 0:
            print(f"   {which} page: none")
            continue
        p = page(pi)
        print(f"   {which} page[{pi}]:", hx(p))
        print(f"      hashes: all={p['hash_all']} first={p['hash_first']} last={p['hash_last']}")
        if not want_data:
            continue
        info, data = load_page(pi)
        print(f"   -> file {info['file']}: raw_base(u32@0x4D0)={info['base']:#x} (== secoff[1]: {info['base_is_secoff1']}) + data_off {p['data_off']:#x} = file offset {info['off']:#x}; compressed {p['csize']:#x} uncompressed {p['usize']:#x}")
        print(f"      first 2 compressed bytes {info['first2']} (78xx would be a zlib header); codec: {info['codec']}")
        print(f"      crc32(compressed)={info['crc_comp']:#010x} crc32(uncompressed)={info['crc_uncomp']:#010x} page.crc={p['crc']:#010x}")
        so = s[f"{which}_off"]
        print(f"      first 64 bytes of inflated page:            {data[:64].hex(' ', 4)}")
        print(f"      first 64 bytes at page+{so:#x} (segment start): {data[so:so + 64].hex(' ', 4)}")
        e["page_data_" + which] = data
    n = e["rf"][0]
    print(f"   resource fixups ({n}) [i] write-at(def offset) <- value(nibble, offset):")
    show = sorted(set(list(range(min(n, 4))) + list(range(max(0, n - 6), n))))
    e["rf_list"] = []
    for i in range(n):
        a, b = struct.unpack("<iI", rd(e["rf"][1] + 8 * i, 8))
        e["rf_list"].append((a, b))
        if i in show:
            print(f"      [{i}] at def+{a:#x} <- {b:#010x} (nibble {b >> 28}, offset {b & 0x0FFFFFFF:#x})")
    nibs = {}
    for a, b in e["rf_list"]:
        nibs[b >> 28] = nibs.get(b >> 28, 0) + 1
    print("      nibble histogram:", nibs)
    n = e["df"][0]
    print(f"   definition fixups ({n}) [i] address(nibble, offset) struct_type:")
    show = sorted(set(list(range(min(n, 3))) + list(range(max(0, n - 3), n))))
    e["df_list"] = []
    for i in range(n):
        a, b = struct.unpack("<Ii", rd(e["df"][1] + 8 * i, 8))
        e["df_list"].append((a, b))
        if i in show:
            print(f"      [{i}] {a:#010x} (nibble {a >> 28}, offset {a & 0x0FFFFFFF:#x}) struct_type={b}")
    print(f"   definition data: fixup_data+{e['fix_off']:#x}, len {e['fix_len']:#x} ({e['fix_len']}) -> file {fix_ptr + e['fix_off']:#x}; secondary fixup off {e['sec_fix_off']:#x}; def_addr(entry+36) {e['def_addr']:#010x}")
    d = rd(fix_ptr + e["fix_off"], e["fix_len"]) if e["fix_len"] > 0 else b""
    print("   definition data first 96 bytes:", d[:96].hex(" ", 4))
    print("   definition data last 48 bytes: ", d[-48:].hex(" ", 4))
    return e, d


e, d = dump_resource("Lbsp render geometry (what Reclaimer draws)", lb_res)
vbc = struct.unpack_from("<i", d, e["fix_len"] - 24)[0]
ibc = struct.unpack_from("<i", d, e["fix_len"] - 12)[0]
root = e["def_addr"] & 0x0FFFFFFF
print(f"\n   render_geometry_api_resource_definition root at def+{root:#x} (= fix_len-48: {root == e['fix_len'] - 48}), 48 bytes: {d[root:root + 48].hex(' ', 4)}")
print(f"   vertex buffers={vbc} index buffers={ibc}; 28*vbc={28 * vbc:#x}, +12*vbc={40 * vbc:#x}, +24*ibc={40 * vbc + 24 * ibc:#x}, +12*ibc={40 * vbc + 36 * ibc:#x} (root={root:#x})")
for i, (a, b) in enumerate(e["rf_list"]):
    if i in (0, vbc - 1, vbc, vbc + 1, 2 * vbc - 1, 2 * vbc, 2 * vbc + ibc - 1, 2 * vbc + ibc, 2 * vbc + 2 * ibc - 1, 2 * vbc + 2 * ibc, len(e["rf_list"]) - 1):
        print(f"      fixup[{i}] at def+{a:#x} <- {b:#010x}")
print("   vertex buffer definitions, full 28 bytes, first 3:")
for i in range(min(vbc, 3)):
    b28 = d[28 * i:28 * i + 28]
    print(f"      vb[{i}] {b28.hex(' ', 4)}  -> count={struct.unpack_from('<i', b28, 0)[0]} +4(i16)={struct.unpack_from('<h', b28, 4)[0]} +6(i16)={struct.unpack_from('<h', b28, 6)[0]} data_len={struct.unpack_from('<i', b28, 8)[0]} +12={struct.unpack_from('<I', b28, 12)[0]:#x} +16={struct.unpack_from('<I', b28, 16)[0]:#x} +20={struct.unpack_from('<I', b28, 20)[0]:#x} +24={struct.unpack_from('<I', b28, 24)[0]:#x}")
print("   s_tag_d3d_vertex_buffer structs (12 bytes), first 2:", [d[28 * vbc + 12 * i:28 * vbc + 12 * i + 12].hex(" ", 4) for i in range(2)])
ib_base = 40 * vbc
print("   index buffer definitions, full 24 bytes, first 3:")
for i in range(min(ibc, 3)):
    b24 = d[ib_base + 24 * i:ib_base + 24 * i + 24]
    print(f"      ib[{i}] {b24.hex(' ', 4)}  -> fmt={struct.unpack_from('<i', b24, 0)[0]} data_len={struct.unpack_from('<i', b24, 4)[0]}")
print("   s_tag_d3d_index_buffer structs (12 bytes), first 2:", [d[ib_base + 24 * ibc + 12 * i:ib_base + 24 * ibc + 12 * i + 12].hex(" ", 4) for i in range(2)])
pd = e.get("page_data_primary", b"")
seg = segment(e["seg_idx"])
so = seg["primary_off"]
print(f"   inflated page length {len(pd):#x}; segment primary offset {so:#x}")
for i in range(min(vbc, 2)):
    a, b = e["rf_list"][i]
    o = b & 0x0FFFFFFF
    print(f"   vb[{i}] fixup offset {o:#x}: bytes at page+{so:#x}+{o:#x} (segment-relative, Reclaimer): {pd[so + o:so + o + 32].hex(' ', 4)}")
    print(f"               bytes at page+{o:#x} (page-relative, for comparison):          {pd[o:o + 32].hex(' ', 4)}")
ib0 = e["rf_list"][2 * vbc][1] & 0x0FFFFFFF
print(f"   ib[0] fixup offset {ib0:#x}: bytes at page+{so:#x}+{ib0:#x}: {pd[so + ib0:so + ib0 + 32].hex(' ', 4)}")
print(f"   segment data length = sum(vb data_len)+sum(ib data_len) = {sum(struct.unpack_from('<i', d, 28 * i + 8)[0] for i in range(vbc)) + sum(struct.unpack_from('<i', d, ib_base + 24 * i + 4)[0] for i in range(ibc)):#x}; last fixup offset+len = {max((b & 0x0FFFFFFF) for a, b in e['rf_list'] if b >> 28 == 4):#x}+...")

print(f"\n   Lbsp sections ({lb_sec_cnt}), first 4 (SectionBlock 76 bytes: vb index i16@24, ib index i16@40, flags u8@44, node u8@45, vertex format u8@46, index format u8@48):")
for i in range(min(lb_sec_cnt, 4)):
    b = rd(lb_sec_ptr + 76 * i, 76)
    vbi = struct.unpack_from("<h", b, 24)[0]
    ibi = struct.unpack_from("<h", b, 40)[0]
    print(f"      section[{i}] vb_index={vbi} ib_index={ibi} flags={b[44]:#x} node={b[45]} vfmt={b[46]} ifmt={b[48]}")
    a, va = e["rf_list"][vbi]
    print(f"         vertex fixup[{vbi}] -> page offset {va & 0x0FFFFFFF:#x}; vb def count={struct.unpack_from('<i', d, 28 * vbi)[0]} data_len={struct.unpack_from('<i', d, 28 * vbi + 8)[0]:#x}")
    if ibi >= 0:
        a, ia = e["rf_list"][2 * vbc + ibi]
        print(f"         index fixup[{2 * vbc + ibi}] -> page offset {ia & 0x0FFFFFFF:#x}; ib def fmt={struct.unpack_from('<i', d, ib_base + 24 * ibi)[0]} data_len={struct.unpack_from('<i', d, ib_base + 24 * ibi + 4)[0]:#x}")

# which entries belong to the sbsp, and where in the sbsp meta are they referenced?
print()
print("-- all gestalt entries owned by the sbsp tag, and where the sbsp meta holds their 32-bit id (scan of sbsp meta, first 0x600 bytes):")
sb = rd(sbsp_meta, 0x600)
for j in range(re_cnt):
    e2 = entry(j)
    if (e2["owner_tag"] & 0xFFFF) == sbsp_i and e2["type_idx"] != 255:
        hits = [k for k in range(0, len(sb) - 3, 4) if struct.unpack_from("<I", sb, k)[0] == e2["ident"]]
        print(f"   entry {j} (0x{j:x}) id {e2['ident']:#010x} type {e2['type_idx']} ({string_id(struct.unpack_from('<I', rd(rt_ptr + 28 * e2['type_idx'], 28), 24)[0])}) seg {e2['seg_idx']} fix_len {e2['fix_len']:#x} -> found at sbsp+{[hex(h) for h in hits]}")
dump_resource("sbsp +596 resource (ResourcePointer1)", u32(sbsp_meta + 596), want_data=False)
dump_resource("sbsp +876 resource (ResourcePointer2)", u32(sbsp_meta + 876), want_data=False)
dump_resource("sbsp +908 resource (ResourcePointer3)", u32(sbsp_meta + 908), want_data=False)

# a page that lives in a shared cache, for the shared.map offset rule
for i in range(pg_cnt):
    p = page(i)
    if p["cache_idx"] >= 0 and p["csize"] > 0 and p["usize"] < 0x800000 and p["asset_cnt"] > 0:
        print(f"\n-- a small page in a shared cache: page[{i}] {hx(p)} -> {shared[p['cache_idx']]!r}")
        for j in range(re_cnt):
            e2 = entry(j)
            if e2["seg_idx"] >= 0:
                s2 = segment(e2["seg_idx"])
                if s2["primary_page"] == i or s2["secondary_page"] == i:
                    dump_resource(f"shared-cache example (entry {j}, owner {names[e2['owner_tag'] & 0xFFFF]!r} [{e2['owner_cls']}] type {e2['type_idx']})", e2["ident"])
                    break
        break
