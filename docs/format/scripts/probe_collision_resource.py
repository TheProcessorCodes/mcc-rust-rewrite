"""Probe the real collision BSP resource of an MCC Halo 3 map (read-only).

Walks: header -> tag index -> sbsp tag -> resource id -> zone (cache_file_resource_gestalt)
entry -> definition data (+ fixups) -> structure_bsp_tag_resources -> collision bsp arrays
(in the play / cache_file_resource_layout_table page data), then sanity-checks the arrays.

Offsets used here are the ones documented in docs/format/collision-bsp-research.md.
Run: python -I docs/format/scripts/probe_collision_resource.py [map path]
"""
import os
import struct
import sys
import zlib

MAPS = os.environ.get("HALO3_MAPS", "")
MAP = sys.argv[1] if len(sys.argv) > 1 else os.path.join(MAPS, "riverworld.map")

with open(MAP, "rb") as f:
    data = f.read()


def u8(o): return data[o]
def i8(o): return struct.unpack_from("<b", data, o)[0]
def i16(o): return struct.unpack_from("<h", data, o)[0]
def u16(o): return struct.unpack_from("<H", data, o)[0]
def i32(o): return struct.unpack_from("<i", data, o)[0]
def u32(o): return struct.unpack_from("<I", data, o)[0]
def i64(o): return struct.unpack_from("<q", data, o)[0]
def f32(o): return struct.unpack_from("<f", data, o)[0]


assert data[0:4] == b"daeh", data[0:4]  # 'head' stored little-endian
print("file", MAP, "size 0x%X" % len(data))
print("build", data[0xA0:0xC0].split(b"\0")[0].decode())

tag_count = i32(0x20)
tagname_table_ptr = u32(0x24)
tagname_index_ptr = u32(0x2C)
str_count = i32(0x30)
str_table_ptr = u32(0x34)
str_index_ptr = u32(0x3C)
virt_base = i64(0x2E0)
tag_index_ptr = i64(0x2E8)
secoff = [u32(0x4CC + 4 * i) for i in range(4)]
sections = [(u32(0x4DC + 8 * i), u32(0x4E0 + 8 * i)) for i in range(4)]
print("tag count", tag_count, "virt base 0x%X" % virt_base, "tag index ptr 0x%X" % tag_index_ptr)
print("section offsets", ["0x%08X" % s for s in secoff])
print("sections", [("0x%08X" % a, "0x%08X" % s) for a, s in sections])


def hdr_file(p):      # header (section 0, debug) pointer -> file offset
    return (p + secoff[0]) & 0xFFFFFFFF


tag_magic = (sections[2][0] + secoff[2]) & 0xFFFFFFFF


def tag_file(virt):   # section 2 virtual address -> file offset
    return virt - virt_base + tag_magic


def expand(p32):      # 32-bit shifted tag-data pointer -> file offset
    return tag_file((p32 & 0xFFFFFFFF) << 2)


res_base = secoff[1]  # section 1 (resources) file base; Reclaimer reads this u32 at header 0x4D0
print("tag magic 0x%08X, resource base 0x%08X (header 0x4D0 = 0x%08X)" % (tag_magic, res_base, u32(0x4D0)))

# strings (StringId U13: index 19 bits, namespace 8 bits, length 5 bits; see Reclaimer StringIdTranslator)
str_index_file = hdr_file(str_index_ptr)
str_table_file = hdr_file(str_table_ptr)
ns_count = i32(0x40)
ns_table_file = hdr_file(u32(0x44))
ns_counts = [i32(ns_table_file + 4 * i) & ((1 << 19) - 1) for i in range(ns_count)]
ns_start = {}
start = ns_counts[0]
for i in range(1, ns_count):
    ns_start[i] = (0, start)
    start += ns_counts[i]
ns_start[0] = (ns_counts[0], start)


def string_by_index(idx):
    off = i32(str_index_file + 4 * idx)
    end = data.index(b"\0", str_table_file + off)
    return data[str_table_file + off:end].decode("latin1")


def resolve_stringid(v):
    v &= 0xFFFFFFFF
    index = v & ((1 << 19) - 1)
    ns = (v >> 19) & 0xFF
    while ns not in ns_start and ns > 0:
        ns -= 1
    mn, st = ns_start[ns]
    real = index if index < mn else index - mn + st
    return string_by_index(real) if 0 <= real < str_count else "<bad %d>" % real


# tag index
ti = tag_file(tag_index_ptr)
class_count = i32(ti); class_table = i64(ti + 8); tcount = i32(ti + 16); tag_table = i64(ti + 24)
print("tag index @ file 0x%X: classes %d, tags %d" % (ti, class_count, tcount))
ct = tag_file(class_table); tt = tag_file(tag_table)
classes = [data[ct + 16 * i:ct + 16 * i + 4][::-1].decode("latin1") for i in range(class_count)]
tags = []
for i in range(tcount):
    ci = i16(tt + 8 * i); meta = u32(tt + 8 * i + 4)
    tags.append((ci, meta))
tn_index = hdr_file(tagname_index_ptr); tn_table = hdr_file(tagname_table_ptr)


def tag_name(i):
    off = i32(tn_index + 4 * i)
    if off < 0:
        return "<none>"
    end = data.index(b"\0", tn_table + off)
    return data[tn_table + off:end].decode("latin1")


def find_tags(code):
    return [i for i, (ci, meta) in enumerate(tags) if ci >= 0 and classes[ci] == code]


sbsps = find_tags("sbsp"); zones = find_tags("zone"); plays = find_tags("play")
print("sbsp tags", [(i, tag_name(i)) for i in sbsps])
print("zone", [(i, tag_name(i)) for i in zones], "play", [(i, tag_name(i)) for i in plays])

# ---- sbsp (MccHalo3U4+ offsets: Reclaimer Halo3/Config/ScenarioStructureBspTag.config.cs; Assembly Halo3MCC/sbsp.xml)
sb = expand(tags[sbsps[0]][1])
print("sbsp meta @ file 0x%X" % sb)
bounds = [(f32(sb + 64 + 8 * k), f32(sb + 64 + 8 * k + 4)) for k in range(3)]
print("sbsp world bounds", bounds)
rp1, rp2, rp3 = u32(sb + 596), u32(sb + 876), u32(sb + 908)
print("resource ids: @0x254 (596) = 0x%08X, @0x36C (876) = 0x%08X, @0x38C (908) = 0x%08X, @0x394 (pathfinding) = 0x%08X, @0x39C use_resource_items = %d"
      % (rp1, rp2, rp3, u32(sb + 0x394), i32(sb + 0x39C)))
inst_count = i32(sb + 0x1C0)
print("sbsp instanced geometry instances: count %d" % inst_count)
print("sbsp collision materials: count %d" % i32(sb + 0x28))
print("sbsp large structure surfaces @0x64: count %d; structure surface->triangle mapping @0x70: count %d" % (i32(sb + 0x64), i32(sb + 0x70)))

# ---- zone (cache_file_resource_gestalt), MccHalo3+ offsets: Reclaimer CacheFileResourceGestaltTag.config.cs
zn = expand(tags[zones[0]][1])
res_count = i32(zn + 100); res_ptr = expand(u32(zn + 104))
fixup_size = i32(zn + 328); fixup_ptr = expand(u32(zn + 340))
print("zone meta @ 0x%X: resource entries %d @ 0x%X; definition data %d bytes @ 0x%X" % (zn, res_count, res_ptr, fixup_size, fixup_ptr))
defs_count = i32(zn + 4); defs_ptr = expand(u32(zn + 8))
print("zone resource type definitions: %d" % defs_count)
for k in range(defs_count):
    e = defs_ptr + 0x1C * k
    sid = u32(e + 24)
    print("  type %2d guid %s name 0x%08X -> %r" % (k, data[e:e + 16].hex(), sid, resolve_stringid(sid)))
definition_data = data[fixup_ptr:fixup_ptr + fixup_size]


def resource_entry(rid):
    idx = rid & 0xFFFF
    e = res_ptr + 64 * idx
    fx_ptr = expand(u32(e + 44)); io_ptr = expand(u32(e + 56))
    return dict(
        index=idx, owner_class=data[e:e + 4][::-1].decode("latin1"), owner_tag=i32(e + 12) & 0xFFFF,
        salt=u16(e + 16), type_index=i8(e + 18), flags=u8(e + 19),
        def_offset=i32(e + 20), def_length=i32(e + 24), secondary_fixup_info=i32(e + 28),
        page_flags=i16(e + 32), segment=i16(e + 34), def_address=u32(e + 36),
        fixups=[(u32(fx_ptr + 8 * k), u32(fx_ptr + 8 * k + 4)) for k in range(i32(e + 40))],
        interops=[(u32(io_ptr + 8 * k), i32(io_ptr + 8 * k + 4)) for k in range(i32(e + 52))],
    )


for label, rid in (("sbsp+0x254", rp1), ("sbsp+0x36C", rp2), ("sbsp+0x38C", rp3), ("sbsp+0x394", u32(sb + 0x394))):
    e2 = resource_entry(rid)
    tname = resolve_stringid(u32(defs_ptr + 0x1C * e2["type_index"] + 24)) if e2["type_index"] >= 0 else "none"
    print("  %s = 0x%08X -> resource %d salt 0x%04X (entry salt 0x%04X) owner %s type %d %s" % (label, rid, e2["index"], rid >> 16, e2["salt"], e2["owner_class"], e2["type_index"], tname))

ent = resource_entry(rp3)
print("\ncollision resource entry (index %d, salt 0x%04X): owner %s tag %d, type %d (%s), flags 0x%02X, def data off 0x%X len 0x%X, 2nd fixup info 0x%X, page flags %d, segment %d, def address 0x%08X (type %d offset 0x%X)"
      % (ent["index"], ent["salt"], ent["owner_class"], ent["owner_tag"], ent["type_index"],
         resolve_stringid(u32(defs_ptr + 0x1C * ent["type_index"] + 24)) if ent["type_index"] >= 0 else "none",
         ent["flags"], ent["def_offset"], ent["def_length"],
         ent["secondary_fixup_info"], ent["page_flags"], ent["segment"], ent["def_address"], ent["def_address"] >> 29, ent["def_address"] & 0x1FFFFFFF))
print("fixups (%d), first 12:" % len(ent["fixups"]))
for bo, ad in ent["fixups"][:12]:
    print("  write 0x%08X (type %d off 0x%X) at def+0x%X" % (ad, ad >> 29, ad & 0x1FFFFFFF, bo))
types_seen = sorted(set(ad >> 29 for _, ad in ent["fixups"]))
print("  fixup address types seen: %s; all fixup block offsets inside definition data: %s"
      % (types_seen, all(bo + 4 <= ent["def_length"] for bo, _ in ent["fixups"])))
print("interops (%d): %s" % (len(ent["interops"]), ent["interops"][:8]))

defn = bytearray(definition_data[ent["def_offset"]:ent["def_offset"] + ent["def_length"]])
root_off = ent["def_address"] & 0x1FFFFFFF
raw_root = bytes(defn[root_off:root_off + 0x30])
for bo, ad in ent["fixups"]:
    struct.pack_into("<I", defn, bo, ad)

# ---- play (cache_file_resource_layout_table): Reclaimer CacheFileResourceLayoutTableTag.cs (MccHalo3 <= v < Halo3ODST: segments @60)
pl = expand(tags[plays[0]][1])
shared_count = i32(pl + 12); shared_ptr = expand(u32(pl + 16))
page_count = i32(pl + 24); page_ptr = expand(u32(pl + 28))
size_count = i32(pl + 36); size_ptr = expand(u32(pl + 40))
seg_count = i32(pl + 60); seg_ptr = expand(u32(pl + 64))
print("\nplay meta @ 0x%X: shared caches %d, pages %d, size groups %d, segments %d" % (pl, shared_count, page_count, size_count, seg_count))
for k in range(shared_count):
    e = shared_ptr + 264 * k
    print("  shared %d: %r u16@0x100 0x%04X u16@0x102 0x%04X u32@0x104 0x%X" % (k, data[e:e + 256].split(b"\0")[0].decode("latin1"), u16(e + 256), u16(e + 258), u32(e + 260)))


def read_page_bytes(pidx):
    p = page_ptr + 88 * pidx
    codec = i8(p + 3); shared = i16(p + 4); block = u32(p + 8); csize = u32(p + 12); usize = u32(p + 16)
    print("  page %d: codec %d shared %d block 0x%X compressed 0x%X uncompressed 0x%X" % (pidx, codec, shared, block, csize, usize))
    if shared < 0:
        src = data; off = res_base + block
    else:
        e = shared_ptr + 264 * shared
        name = os.path.basename(data[e:e + 256].split(b"\0")[0].decode("latin1").replace("\\", "/"))
        with open(os.path.join(os.path.dirname(MAP), name), "rb") as f:
            src = f.read()
        off = 16384 + block
        print("  (shared cache %s @ 0x%X)" % (name, off))
    print("  page file offset 0x%X" % off)
    comp = src[off:off + csize]
    if codec < 0 or csize == usize:
        return comp
    out = zlib.decompressobj(-15).decompress(comp)
    assert len(out) == usize, (len(out), usize)
    return out


seg = seg_ptr + 16 * ent["segment"]
req_page, opt_page, req_off, opt_off, req_size, opt_size = struct.unpack_from("<hhiihh", data, seg)
print("segment %d: required page %d offset 0x%X size idx %d; optional page %d offset 0x%X size idx %d" % (ent["segment"], req_page, req_off, req_size, opt_page, opt_off, opt_size))
primary = read_page_bytes(req_page)
if req_size >= 0:
    total = i32(size_ptr + 16 * req_size)
    primary = primary[req_off:req_off + total]
else:
    primary = primary[req_off:]
secondary = b""
if opt_page >= 0:
    secondary = read_page_bytes(opt_page)
    secondary = secondary[opt_off:opt_off + i32(size_ptr + 16 * opt_size)] if opt_size >= 0 else secondary[opt_off:]
print("primary data %d bytes, secondary %d bytes" % (len(primary), len(secondary)))

BUFS = {1: ("definition", defn), 2: ("data", primary), 3: ("secondary", secondary)}


def block(buf, off):
    count = struct.unpack_from("<i", buf, off)[0]; addr = struct.unpack_from("<I", buf, off + 4)[0]
    return count, addr >> 29, addr & 0x1FFFFFFF


def elems(buf, off, size, fmt):
    count, typ, o = block(buf, off)
    if count == 0:
        return []
    src = BUFS[typ][1]
    return [struct.unpack_from(fmt, src, o + size * k) for k in range(count)]


print("\nstructure_bsp_tag_resources root at def+0x%X; raw bytes before fixups: %s" % (root_off, raw_root.hex(" ")))
print("after fixups: %s" % bytes(defn[root_off:root_off + 0x30]).hex(" "))
names = ["collision_bsps", "large_collision_bsps", "instanced_geometry", "havok_data"]
root = {}
for k, n in enumerate(names):
    c, t, o = block(defn, root_off + 12 * k)
    root[n] = (c, t, o)
    print("  %-22s count %6d -> %s+0x%X" % (n, c, BUFS.get(t, ("?",))[0], o))


def dump_collision_geometry(buf, off, label, large=False):
    specs = [("bsp3d_nodes", 12 if large else 8), ("planes", 16), ("leaves", 8), ("bsp2d_references", 8 if large else 4),
             ("bsp2d_nodes", 20 if large else 16), ("surfaces", 16 if large else 12), ("edges", 24 if large else 12), ("vertices", 20 if large else 16)]
    print("\n%s (CollisionGeometry%s at 0x%X)" % (label, " large" if large else "", off))
    info = {}
    for k, (n, sz) in enumerate(specs):
        c, t, o = block(buf, off + 12 * k)
        info[n] = (c, t, o, sz)
        print("  %-18s count %7d elem %2d -> %s+0x%X" % (n, c, sz, BUFS.get(t, ("?",))[0], o))
    return info


def child(c):
    idx = c & 0x7FFFFF
    idx = (idx ^ (1 << 22)) - (1 << 22)
    return (c >> 23) & 1, idx


def walk_surfaces(S, E):
    polys = tris = bad = 0
    for i, s in enumerate(S):
        e = s[1]; loop = []; guard = 0
        while guard < 64:
            guard += 1
            ed = E[e]
            if ed[4] == i:
                loop.append(ed[0]); nxt = ed[2]
            elif ed[5] == i:
                loop.append(ed[1]); nxt = ed[3]
            else:
                bad += 1; break
            if nxt == s[1]:
                break
            e = nxt
        else:
            bad += 1
        if len(loop) >= 3:
            polys += 1; tris += len(loop) - 2
    return polys, tris, bad


cb = root["collision_bsps"]
cb_src = BUFS[cb[1]][1]
all_tris = 0
for b in range(cb[0]):
    base = cb[2] + 0x60 * b
    info = dump_collision_geometry(cb_src, base, "collision_bsps[%d]" % b)
    nv = info["vertices"][0]; ne = info["edges"][0]; ns = info["surfaces"][0]; npl = info["planes"][0]; nn = info["bsp3d_nodes"][0]; nl = info["leaves"][0]
    V = elems(cb_src, base + 12 * 7, 16, "<fffHh")
    E = elems(cb_src, base + 12 * 6, 12, "<HHHHHH")
    S = elems(cb_src, base + 12 * 5, 12, "<HHhhhBb")
    P = elems(cb_src, base + 12 * 1, 16, "<ffff")
    N = elems(cb_src, base + 12 * 0, 8, "<Q")
    L = elems(cb_src, base + 12 * 2, 8, "<BBHI")
    R2 = elems(cb_src, base + 12 * 3, 4, "<hh")
    N2 = elems(cb_src, base + 12 * 4, 16, "<fffhh")
    xs = [v[0] for v in V]; ys = [v[1] for v in V]; zs = [v[2] for v in V]
    print("  vertex bounds x[%.2f,%.2f] y[%.2f,%.2f] z[%.2f,%.2f] (sbsp bounds %s)" % (min(xs), max(xs), min(ys), max(ys), min(zs), max(zs), bounds))
    print("  vertex first-edge < edges: %s; sink values: %s" % (all(v[3] < ne for v in V), sorted(set(v[4] for v in V))[:10]))
    print("  edges: start/end < vertices %s; fwd/rev < edges %s; left/right surfaces < surfaces or 0xFFFF %s"
          % (all(e[0] < nv and e[1] < nv for e in E), all(e[2] < ne and e[3] < ne for e in E),
             all((e[4] < ns or e[4] == 0xFFFF) and (e[5] < ns or e[5] == 0xFFFF) for e in E)))
    print("  surfaces: plane(&0x7FFF) < planes %s (max raw plane 0x%X); first edge < edges %s; material range %s..%s; breakable set %s..%s / index %s..%s; flags seen %s; best-plane-vertex %s..%s"
          % (all((s[0] & 0x7FFF) < npl for s in S), max(s[0] for s in S), all(s[1] < ne for s in S), min(s[2] for s in S), max(s[2] for s in S),
             min(s[3] for s in S), max(s[3] for s in S), min(s[4] for s in S), max(s[4] for s in S), sorted(set(s[5] for s in S)), min(s[6] for s in S), max(s[6] for s in S)))
    print("  planes: |normal| in [%.4f, %.4f]" % (min((p[0] ** 2 + p[1] ** 2 + p[2] ** 2) ** .5 for p in P), max((p[0] ** 2 + p[1] ** 2 + p[2] ** 2) ** .5 for p in P)))
    ok = True; leafrefs = noderefs = neg = 0
    for n in N:
        v = n[0]; plane = v & 0xFFFF; back = (v >> 16) & 0xFFFFFF; front = (v >> 40) & 0xFFFFFF
        if plane >= npl:
            ok = False
        for c in (back, front):
            t, idx = child(c)
            if idx < 0:
                neg += 1; continue
            if t == 1:
                leafrefs += 1
                if idx >= nl:
                    ok = False
            else:
                noderefs += 1
                if idx >= nn:
                    ok = False
    print("  bsp3d nodes: packed u64 plane16|back24|front24 indices valid %s (node refs %d, leaf refs %d, negative (-1) refs %d)" % (ok, noderefs, leafrefs, neg))
    print("  leaves: flags seen %s; bsp2d ref count range %d..%d; first ref + count <= bsp2d refs %s" % (sorted(set(l[0] for l in L)), min(l[2] for l in L), max(l[2] for l in L), all(l[3] + l[2] <= len(R2) for l in L)))
    print("  bsp2d refs: plane idx (&0x7FFF) < planes %s; node idx < bsp2d nodes %s (min node idx %d)" % (all((r[0] & 0x7FFF) < npl for r in R2), all(r[1] < len(N2) for r in R2), min(r[1] for r in R2)))
    polys, tris, bad = walk_surfaces(S, E)
    all_tris += tris
    print("  surface edge-loop walk: %d polygons, %d triangles, %d bad loops" % (polys, tris, bad))
    # --- extra checks
    failing = [l for l in L if l[3] + l[2] > len(R2)]
    print("  leaves failing first+count<=refs: %d of %d; examples (flags,flags2,count,first): %s; max first ref 0x%X; leaves with count 0: %d"
          % (len(failing), len(L), failing[:4], max(l[3] for l in L), sum(1 for l in L if l[2] == 0)))
    alt = [l for l in L if l[2] > 0 and l[3] + l[2] > len(R2)]
    print("  ...of which have count>0: %d (so count==0 leaves carry a junk/-1 first index?)" % len(alt))
    print("  bsp2d refs with plane high bit 0x8000: %d of %d; bsp2d node plane |ij| in [%.4f, %.4f]"
          % (sum(1 for r in R2 if r[0] & 0x8000), len(R2), min((n2[0] ** 2 + n2[1] ** 2) ** .5 for n2 in N2), max((n2[0] ** 2 + n2[1] ** 2) ** .5 for n2 in N2)))
    neg_refs = [r for r in R2 if r[1] < 0]
    print("  bsp2d refs with negative node idx: %d of %d; (idx & 0x7FFF) < surfaces for all of them: %s"
          % (len(neg_refs), len(R2), all((r[1] & 0x7FFF) < ns for r in neg_refs)))
    kids = [c for n2 in N2 for c in (n2[3], n2[4])]
    negk = [c for c in kids if c < 0]
    print("  bsp2d node children: %d total, %d negative (surface refs, (c&0x7FFF)<surfaces: %s), non-negative < bsp2d nodes: %s"
          % (len(kids), len(negk), all((c & 0x7FFF) < ns for c in negk), all(c < len(N2) for c in kids if c >= 0)))
    hi = [s for s in S if s[0] & 0x8000]
    # test: does the surface plane high bit mean the polygon faces against the plane normal?
    agree = disagree = 0
    for i, s in enumerate(S):
        e = s[1]; loop = []; guard = 0
        while guard < 64:
            guard += 1; ed = E[e]
            if ed[4] == i: loop.append(ed[0]); nxt = ed[2]
            elif ed[5] == i: loop.append(ed[1]); nxt = ed[3]
            else: break
            if nxt == s[1]: break
            e = nxt
        if len(loop) < 3: continue
        a, b_, c = (V[loop[0]], V[loop[1]], V[loop[2]])
        u = (b_[0] - a[0], b_[1] - a[1], b_[2] - a[2]); w = (c[0] - a[0], c[1] - a[1], c[2] - a[2])
        nrm = (u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0])
        p = P[s[0] & 0x7FFF]
        dot = nrm[0] * p[0] + nrm[1] * p[1] + nrm[2] * p[2]
        if s[0] & 0x8000: dot = -dot
        if dot > 0: agree += 1
        elif dot < 0: disagree += 1
    print("  surfaces with plane high bit 0x8000: %d of %d; winding normal agrees with plane (flipped when bit set): %d agree, %d disagree" % (len(hi), len(S), agree, disagree))
    sizes = {}
    for i, s in enumerate(S):
        e = s[1]; n = 0; guard = 0
        while guard < 64:
            guard += 1; ed = E[e]
            if ed[4] == i: n += 1; nxt = ed[2]
            elif ed[5] == i: n += 1; nxt = ed[3]
            else: break
            if nxt == s[1]: break
            e = nxt
        sizes[n] = sizes.get(n, 0) + 1
    print("  polygon vertex-count histogram: %s" % dict(sorted(sizes.items())))
    print("  surfaces flagged two-sided: %d, invisible: %d, climbable: %d, breakable: %d, conveyor: %d, slip: %d, plane-negated(flag 0x80): %d"
          % tuple(sum(1 for s in S if s[5] & bit) for bit in (1, 2, 4, 8, 32, 64, 128)))

lb = root["large_collision_bsps"]
for b in range(lb[0]):
    dump_collision_geometry(BUFS[lb[1]][1], lb[2] + 0x60 * b, "large_collision_bsps[%d]" % b, large=True)

ig = root["instanced_geometry"]
ig_src = BUFS[ig[1]][1]
print("\ninstanced_geometry: %d definitions (sbsp has %d instances), element size 0xC4 assumed" % (ig[0], inst_count))
inst_tris = 0; inst_bad = 0
for d in range(ig[0]):
    base = ig[2] + 0xC4 * d
    checksum = struct.unpack_from("<i", ig_src, base)[0]
    sphere = struct.unpack_from("<ffff", ig_src, base + 4)
    ci = base + 20
    counts = [block(ig_src, ci + 12 * k)[0] for k in range(8)]
    render_bsp = block(ig_src, ci + 0x60)
    mopp = block(ig_src, ci + 0x60 + 12)
    brk = block(ig_src, ci + 0x60 + 24)
    small = block(ig_src, ci + 0x60 + 36)
    surf_planes = block(ig_src, ci + 0x60 + 48)
    planes_map = block(ig_src, ci + 0x60 + 60)
    mesh_index, compression_index = struct.unpack_from("<hh", ig_src, ci + 0x60 + 72)
    lm_scale = struct.unpack_from("<f", ig_src, ci + 0x60 + 76)[0]
    E = elems(ig_src, ci + 12 * 6, 12, "<HHHHHH")
    S = elems(ig_src, ci + 12 * 5, 12, "<HHhhhBb")
    polys, tris, bad = walk_surfaces(S, E)
    inst_tris += tris; inst_bad += bad
    if d < 6 or d == ig[0] - 1:
        print("  def %3d: checksum 0x%08X sphere %s; collision counts (nodes,planes,leaves,2dref,2dnode,surf,edge,vert) %s; render_bsp %d, mopp %d, breakable %d, small_surfaces_planes %d, surface_planes %d, planes(map) %d; mesh %d compression %d lm %.2f; tris %d"
              % (d, checksum & 0xFFFFFFFF, tuple(round(x, 2) for x in sphere), counts, render_bsp[0], mopp[0], brk[0], small[0], surf_planes[0], planes_map[0], mesh_index, compression_index, lm_scale, tris))
print("instanced geometry collision triangles (all definitions, before instancing): %d, bad loops %d" % (inst_tris, inst_bad))
hd = root["havok_data"]
print("havok_data: count %d at %s+0x%X" % (hd[0], BUFS.get(hd[1], ("?",))[0], hd[2]))
print("\nTOTAL cluster collision triangles: %d" % all_tris)
