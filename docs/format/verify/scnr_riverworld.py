# Read-only check of the scenario (scnr) tag layout documented in
# docs/format/scenario-and-tag-reading.md against a real MCC Halo 3 U13 map.
# Run:  python -I scnr_riverworld.py [path\to\map]
# Never writes anything. Default target is riverworld.map (Valhalla).
import os, mmap, struct, sys, math

MAP = sys.argv[1] if len(sys.argv) > 1 else (
    os.path.join(os.environ["HALO3_MAPS"], "riverworld.map"))

f = open(MAP, "rb")
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)

def i16(o): return struct.unpack_from("<h", m, o)[0]
def i32(o): return struct.unpack_from("<i", m, o)[0]
def u32(o): return struct.unpack_from("<I", m, o)[0]
def u64(o): return struct.unpack_from("<Q", m, o)[0]
def f32(o): return struct.unpack_from("<f", m, o)[0]
def f32x3(o): return struct.unpack_from("<fff", m, o)
def cstr(o, maxlen=None):
    end = m.find(b"\0", o, o + maxlen if maxlen else len(m))
    if end < 0:
        end = o + (maxlen or 0)
    return m[o:end].decode("latin-1")
def fourcc(o):
    return m[o:o+4][::-1].decode("latin-1")   # stored little-endian: reverse to read "scnr"

# ---------------------------------------------------------------- header
assert m[0:4] == b"daeh", m[0:4]     # 'head' stored byte-reversed, like every fourcc in the file
tag_count        = i32(0x20)
tagname_tbl_ptr  = u32(0x24)
tagname_idx_ptr  = u32(0x2C)
string_count     = i32(0x30)
string_tbl_ptr   = u32(0x34)
string_idx_ptr   = u32(0x3C)
ns_count         = i32(0x40)
ns_tbl_ptr       = u32(0x44)
build            = cstr(0xA0, 32)
virtual_base     = u64(0x2E0)
tag_index_vptr   = u64(0x2E8)
sec_off          = [u32(0x4CC + 4*i) for i in range(4)]
sections         = [(u32(0x4DC + 8*i), u32(0x4DC + 8*i + 4)) for i in range(4)]

def hdr(p):      # header 32-bit pointer -> file offset (section 0)
    return (p + sec_off[0]) & 0xFFFFFFFF
tag_magic = virtual_base - ((sections[2][0] + sec_off[2]) & 0xFFFFFFFF)
def vaddr(v):    # 64-bit / expanded virtual address -> file offset (section 2)
    return v - tag_magic
def ptr32(p):    # shifted 32-bit tag-data pointer -> file offset
    return vaddr((p & 0xFFFFFFFF) << 2)

print(f"map            {MAP}")
print(f"build          '{build}'  file size 0x{len(m):X}")
print(f"tag count      {tag_count} (0x{tag_count:X})  string count {string_count} (0x{string_count:X})  namespaces {ns_count}")
print(f"virtual base   0x{virtual_base:X}  tag index vptr 0x{tag_index_vptr:X}  tag magic 0x{tag_magic:X}")
print(f"tag index file 0x{vaddr(tag_index_vptr):X}")

# ---------------------------------------------------------------- string ids
str_idx = [i32(hdr(string_idx_ptr) + 4*i) for i in range(string_count)]
str_tbl = hdr(string_tbl_ptr)
strings = [cstr(str_tbl + o) if o >= 0 else None for o in str_idx]
INDEX_BITS, NS_BITS = 19, 8
ns_arr = [i32(hdr(ns_tbl_ptr) + 4*i) for i in range(ns_count)]
namespaces = {}
mask = (1 << INDEX_BITS) - 1
start = ns_arr[0] & mask
for i in range(1, ns_count):
    namespaces[i] = (0, start)
    start += ns_arr[i] & mask
namespaces[0] = (ns_arr[0] & mask, start)
def string_id(v):
    idx = v & mask
    ns = (v >> INDEX_BITS) & ((1 << NS_BITS) - 1)
    while ns not in namespaces and ns > 0:
        ns -= 1
    mn, st = namespaces[ns]
    real = idx if idx < mn else idx - mn + st
    return strings[real] if 0 <= real < len(strings) else f"<invalid 0x{v:08X}>"

# ---------------------------------------------------------------- tag index
ti = vaddr(tag_index_vptr)
class_count   = i32(ti + 0)
class_tbl     = vaddr(u64(ti + 8))
tag_count2    = i32(ti + 16)
tag_tbl       = vaddr(u64(ti + 24))
assert tag_count2 == tag_count, (tag_count2, tag_count)
classes = [(fourcc(class_tbl + 16*i), fourcc(class_tbl + 16*i + 4), fourcc(class_tbl + 16*i + 8), i32(class_tbl + 16*i + 12))
           for i in range(class_count)]
tags = []
for i in range(tag_count):
    o = tag_tbl + 8*i
    tags.append((i16(o), i16(o+2), i32(o+4)))
name_idx = [i32(hdr(tagname_idx_ptr) + 4*i) for i in range(tag_count)]
name_tbl = hdr(tagname_tbl_ptr)
tag_names = [cstr(name_tbl + n) if n >= 0 else None for n in name_idx]
print(f"classes        {class_count}  class table file 0x{class_tbl:X}  tag table file 0x{tag_tbl:X}")

def tag_class(idx):   # class code of tag index idx, or None for the empty tag
    ci = tags[idx][0]
    return classes[ci][0] if ci >= 0 else None
def tag_meta(idx):    # file offset of tag data
    return ptr32(tags[idx][2])

def tagref(o):        # 16-byte TagReference -> (class fourcc, tag index or -1, tag name)
    cls = fourcc(o) if u32(o) != 0xFFFFFFFF else None
    tid = i32(o + 12)
    idx = struct.unpack("<h", struct.pack("<H", tid & 0xFFFF))[0]   # low 16 bits as signed short
    return cls, idx, (tag_names[idx] if idx >= 0 else None)
def block(o):         # 12-byte TagBlock -> (count, file offset of first element)
    cnt, p = i32(o), i32(o + 4)
    return cnt, (ptr32(p) if cnt > 0 else None)

scnr_list = [i for i in range(tag_count) if tag_class(i) == "scnr"]
assert len(scnr_list) == 1, scnr_list
scnr = scnr_list[0]
S = tag_meta(scnr)
print(f"scnr           tag index {scnr} (0x{scnr:X}) '{tag_names[scnr]}'  meta ptr 0x{tags[scnr][2]:08X} -> file 0x{S:X}")

# ---------------------------------------------------------------- scnr blocks (U13 offsets)
OFF = dict(structure_bsps=24, skies=52, object_names=168, scenery=180, scenery_palette=192,
           machines=312, machine_palette=324, controls=360, control_palette=372,
           crates=1468, crate_palette=1480, lightmap_ref=1708)
ELEM = dict(structure_bsps=108, skies=20, object_names=36, scenery=180, machines=136, controls=120, crates=176, palette=16)

print("\n== structure_bsps @ +24 (elem 108, sbsp ref @ +0)")
n, a = block(S + OFF["structure_bsps"])
print(f"   count {n}  elements at file 0x{a:X}" if a else f"   count {n}")
for i in range(n):
    e = a + ELEM["structure_bsps"]*i
    print(f"   [{i}] {tagref(e)}")

print("\n== skies @ +52 (elem 20, ref @ +0)")
n, a = block(S + OFF["skies"])
print(f"   count {n}")
for i in range(n):
    print(f"   [{i}] {tagref(a + 20*i)}")

print("\n== object_names @ +168 (elem 36: name[32] i16 type @32 i16 placement @34)")
on_count, on_addr = block(S + OFF["object_names"])
object_names = []
for i in range(on_count):
    e = on_addr + 36*i
    object_names.append((cstr(e, 32), i16(e+32), i16(e+34)))
print(f"   count {on_count}")
for i in range(min(on_count, 5)):
    print(f"   [{i}] {object_names[i]}")

print("\n== scenario_lightmap reference @ +1708")
print(f"   {tagref(S + OFF['lightmap_ref'])}")

def read_palette(o):
    n, a = block(o)
    return [tagref(a + ELEM["palette"]*i) for i in range(n)]

def read_placements(o, size):
    n, a = block(o)
    out = []
    for i in range(n):
        e = a + size*i
        out.append(dict(palette=i16(e), name=i16(e+2), pos=f32x3(e+8), rot=f32x3(e+20), scale=f32(e+32),
                        variant=i32(e+84), file=e))
    return out, a

def obje_chain(tidx):
    """obje -> hlmt -> mode, using ObjectTag/ModelTag offsets. Returns dict."""
    if tidx < 0:
        return None
    o = tag_meta(tidx)
    objtype = i16(o)
    variant = i32(o + 48)
    model = tagref(o + 52)
    out = dict(obje=tag_names[tidx], cls=tag_class(tidx), type=objtype, default_variant=string_id(variant), hlmt=model, mode=None)
    if model[1] >= 0:
        h = tag_meta(model[1])
        out["mode"] = tagref(h + 0)
        out["coll"] = tagref(h + 16)
        out["phmo"] = tagref(h + 48)
    return out

def check_kind(kind, size, pal_off, pl_off, expect_cls, expect_type):
    print(f"\n== {kind}: placements @ +{pl_off} (elem {size}), palette @ +{pal_off} (elem 16)")
    pal = read_palette(S + pal_off)
    pls, a = read_placements(S + pl_off, size)
    if a:
        print(f"   palette count {len(pal)}  placement count {len(pls)}  placements at file 0x{a:X}")
    else:
        print(f"   palette count {len(pal)}  placement count {len(pls)}")
    bad = [p for p in pls if not (0 <= p['palette'] < len(pal)) or not (-1 <= p['name'] < on_count) or not (0 <= p['scale'] <= 10)
           or any(math.isnan(x) or abs(x) > 1e5 for x in p['pos'] + p['rot'])]
    print(f"   plausibility (palette idx in range, name idx in [-1,{on_count}), scale 0..10, finite pos/rot): {len(pls)-len(bad)} ok, {len(bad)} bad")
    for p in bad[:3]:
        print(f"      BAD file 0x{p['file']:X} palette {p['palette']} name {p['name']} pos {p['pos']} rot {p['rot']} scale {p['scale']}")
    chains = [obje_chain(t[1]) if t[1] >= 0 else None for t in pal]
    with_mode = sum(1 for c in chains if c and c["mode"] and c["mode"][1] >= 0)
    cls_ok = all(t[0] == expect_cls for t in pal if t[1] >= 0)
    types = sorted({c["type"] for c in chains if c})
    print(f"   palette entries with valid tag {sum(1 for t in pal if t[1]>=0)}, class all '{expect_cls}': {cls_ok}, obje.ObjectType values seen {types} (expect [{expect_type}])")
    print(f"   palette entries whose obje->hlmt->mode chain resolves: {with_mode}/{len(pal)}")
    hl_ok = all(c["hlmt"][0] == "hlmt" for c in chains if c and c["hlmt"][1] >= 0)
    md_ok = all(c["mode"][0] == "mode" for c in chains if c and c["mode"] and c["mode"][1] >= 0)
    print(f"   all model refs are class hlmt: {hl_ok};  all render model refs are class mode: {md_ok}")
    placed = sum(1 for p in pls if 0 <= p['palette'] < len(pal) and chains[p['palette']] and chains[p['palette']]['mode'] and chains[p['palette']]['mode'][1] >= 0)
    print(f"   placements with a resolvable render model (HaloExport 'nodes'): {placed}")
    xref = sum(1 for i, (nm, ty, pi) in enumerate(object_names) if ty == expect_type and 0 <= pi < len(pls) and pls[pi]['name'] == i)
    named = sum(1 for (nm, ty, pi) in object_names if ty == expect_type)
    print(f"   object_names of type {expect_type}: {named}, of which placement[idx].name_index points back: {xref}")
    tilted = [p for p in pls if abs(p['rot'][1]) > 0.01 or abs(p['rot'][2]) > 0.01]
    print(f"   placements with non-zero pitch or roll: {len(tilted)}")
    for p in tilted[:3]:
        nm = pal[p['palette']][2] if 0 <= p['palette'] < len(pal) else None
        print(f"      file 0x{p['file']:X} {nm}  pos {tuple(round(x,3) for x in p['pos'])}  rot rad {tuple(round(x,4) for x in p['rot'])}  scale {p['scale']}")
    return pal, pls, chains

sc_pal, sc_pl, sc_ch = check_kind("scenery", ELEM["scenery"], OFF["scenery_palette"], OFF["scenery"], "scen", 6)
print("\n   first 5 scenery placements:")
for i in range(min(5, len(sc_pl))):
    p = sc_pl[i]
    deg = tuple(round(math.degrees(r), 3) for r in p['rot'])
    nm = object_names[p['name']][0] if p['name'] >= 0 else None
    var = string_id(p['variant']) if p['variant'] != 0 else ''
    print(f"   [{i}] file 0x{p['file']:X} palette {p['palette']} -> {sc_pal[p['palette']][2]}")
    print(f"        name idx {p['name']} ({nm!r})  pos {tuple(round(x,4) for x in p['pos'])} wu")
    print(f"        rot(yaw,pitch,roll) rad {tuple(round(x,5) for x in p['rot'])} = deg {deg}  scale {p['scale']}  variant 0x{p['variant']:08X} '{var}'")
print("\n   scenery palette (first 8) with obje->hlmt->mode chain:")
for i, c in enumerate(sc_ch[:8]):
    if c:
        print(f"   [{i}] {c['cls']} {c['obje']}  type {c['type']}  variant '{c['default_variant']}'")
        print(f"        hlmt {c['hlmt'][2]}")
        print(f"        mode {c['mode'][2] if c['mode'] else None}")
    else:
        print(f"   [{i}] (null palette entry)")

cr_pal, cr_pl, cr_ch = check_kind("crates",   ELEM["crates"],   OFF["crate_palette"],   OFF["crates"],   "bloc", 10)
ma_pal, ma_pl, ma_ch = check_kind("machines", ELEM["machines"], OFF["machine_palette"], OFF["machines"], "mach", 7)
co_pal, co_pl, co_ch = check_kind("controls", ELEM["controls"], OFF["control_palette"], OFF["controls"], "ctrl", 8)

hist = {}
for (_, ty, _) in object_names:
    hist[ty] = hist.get(ty, 0) + 1
print("\n== object_names type histogram:", sorted(hist.items()))
