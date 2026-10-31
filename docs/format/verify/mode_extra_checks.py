# Read-only extra checks for the render_model gap fill in docs/format/scenario-and-tag-reading.md (riverworld.map).  Run: python -I mode_extra_checks.py
import os, mmap, struct, sys
MAP = os.path.join(os.environ["HALO3_MAPS"], "riverworld.map")
f = open(MAP, "rb"); m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
def u8(o):  return m[o]
def i16(o): return struct.unpack_from("<h", m, o)[0]
def u16(o): return struct.unpack_from("<H", m, o)[0]
def i32(o): return struct.unpack_from("<i", m, o)[0]
def u32(o): return struct.unpack_from("<I", m, o)[0]
def u64(o): return struct.unpack_from("<Q", m, o)[0]
def f32(o): return struct.unpack_from("<f", m, o)[0]
def f32n(o, n): return struct.unpack_from("<%df" % n, m, o)
def cstr(o, maxlen=None):
    end = m.find(b"\0", o, o + maxlen if maxlen else len(m)); return m[o:end].decode("latin-1")
def fourcc(o): return m[o:o+4][::-1].decode("latin-1")
tag_count = i32(0x20); tagname_tbl_ptr = u32(0x24); tagname_idx_ptr = u32(0x2C)
string_count = i32(0x30); string_tbl_ptr = u32(0x34); string_idx_ptr = u32(0x3C)
ns_count = i32(0x40); ns_tbl_ptr = u32(0x44)
virtual_base = u64(0x2E0); tag_index_vptr = u64(0x2E8)
sec_off = [u32(0x4CC + 4*i) for i in range(4)]
sections_tbl = [(u32(0x4DC + 8*i), u32(0x4DC + 8*i + 4)) for i in range(4)]
def hdr(p): return (p + sec_off[0]) & 0xFFFFFFFF
tag_magic = virtual_base - ((sections_tbl[2][0] + sec_off[2]) & 0xFFFFFFFF)
def vaddr(v): return v - tag_magic
def ptr32(p): return vaddr((p & 0xFFFFFFFF) << 2)
str_idx = [i32(hdr(string_idx_ptr) + 4*i) for i in range(string_count)]
str_tbl = hdr(string_tbl_ptr)
strings = [cstr(str_tbl + o) if o >= 0 else None for o in str_idx]
mask = (1 << 19) - 1
ns_arr = [i32(hdr(ns_tbl_ptr) + 4*i) for i in range(ns_count)]
namespaces = {}; start = ns_arr[0] & mask
for i in range(1, ns_count): namespaces[i] = (0, start); start += ns_arr[i] & mask
namespaces[0] = (ns_arr[0] & mask, start)
def string_id(v):
    idx = v & mask; ns = (v >> 19) & 0xFF
    while ns not in namespaces and ns > 0: ns -= 1
    mn, st = namespaces[ns]; real = idx if idx < mn else idx - mn + st
    return strings[real] if 0 <= real < len(strings) else None
ti = vaddr(tag_index_vptr)
class_count = i32(ti); class_tbl = vaddr(u64(ti + 8)); tag_tbl = vaddr(u64(ti + 24))
classes = [fourcc(class_tbl + 16*i) for i in range(class_count)]
tags = [(i16(tag_tbl + 8*i), i16(tag_tbl + 8*i + 2), i32(tag_tbl + 8*i + 4)) for i in range(tag_count)]
name_idx = [i32(hdr(tagname_idx_ptr) + 4*i) for i in range(tag_count)]
name_tbl = hdr(tagname_tbl_ptr)
tag_names = [cstr(name_tbl + n) if n >= 0 else None for n in name_idx]
def tag_class(i): return classes[tags[i][0]] if tags[i][0] >= 0 else None
def tag_meta(i): return ptr32(tags[i][2])
def tagref(o):
    idx = struct.unpack("<h", struct.pack("<H", i32(o + 12) & 0xFFFF))[0]
    return (fourcc(o) if u32(o) != 0xFFFFFFFF else None, idx, tag_names[idx] if idx >= 0 else None)
def block(o):
    cnt, p = i32(o), i32(o + 4); return cnt, (ptr32(p) if cnt > 0 else None)
def fmt(v, nd=4): return "(" + ", ".join(("%%.%df" % nd) % x for x in v) + ")"

modes = [i for i in range(tag_count) if tag_class(i) == "mode"]

print("== A. GeometryInstanceBlock (60 bytes) on decorator models with an instanced section")
for idx in modes:
    M = tag_meta(idx)
    inst_sec = i32(M + 28)
    if inst_sec < 0: continue
    gc, ga = block(M + 32); sc, sa = block(M + 104); nc, na = block(M + 48)
    se = sa + 76*inst_sec
    subsets = block(se + 12)[0]
    print("  tag %d %r: instSection %d of %d sections (that section has %d subsets, vfmt %d), instances %d, nodes %d" % (
        idx, tag_names[idx], inst_sec, sc, subsets, u8(se + 46), gc, nc))
    for k in range(min(gc, 6)):
        e = ga + 60*k
        rows = [f32n(e + 12, 3), f32n(e + 24, 3), f32n(e + 36, 3)]; t = f32n(e + 48, 3)
        print("     inst[%d] name %r node %d scale %.4f rows %s %s %s t %s" % (k, string_id(i32(e)), i32(e + 4), f32(e + 8), fmt(rows[0], 3), fmt(rows[1], 3), fmt(rows[2], 3), fmt(t, 3)))
    rc, ra = block(M + 12)
    for r in range(rc):
        pc, pa = block(ra + 16*r + 4)
        for p in range(pc):
            pe = pa + 24*p
            print("     region[%d] %r perm[%d] %r section %d count %d masks %s" % (r, string_id(i32(ra + 16*r)), p, string_id(i32(pe)), i16(pe + 4), i16(pe + 6), ["0x%X" % u32(pe + 8 + 4*j) for j in range(4)]))
    names_ok = sum(1 for k in range(gc) if string_id(i32(ga + 60*k)) is not None)
    print("     instance names decode under the 60-byte stride: %d/%d" % (names_ok, gc))

print("")
print("== B. NodeMaps vs Sections on models with UseLocalNodes")
for idx in modes:
    M = tag_meta(idx)
    nmc, nma = block(M + 176)
    if nmc == 0 and not (i32(M + 4) & 4): continue
    sc, sa = block(M + 104); nc, na = block(M + 48)
    maps = []
    for k in range(nmc):
        c, a = block(nma + 12*k); maps.append(list(m[a:a+c]) if a else [])
    secs = [(u8(sa + 76*s + 45), u8(sa + 76*s + 46)) for s in range(sc)]
    print("  tag %d %r flags 0x%X nodes %d sections %d (node,vfmt)=%s nodemaps %d %s" % (idx, tag_names[idx], i32(M + 4), nc, sc, secs, nmc, maps))

print("")
print("== C. mp_masterchief: first permutation per region vs the 'base' permutation; section details")
mc = [i for i in modes if tag_names[i] == r"objects\characters\masterchief\mp_masterchief\mp_masterchief"][0]
M = tag_meta(mc)
rc, ra = block(M + 12); sc, sa = block(M + 104); shc, sha = block(M + 72); nc, na = block(M + 48)
node_names = [string_id(i32(na + 96*n)) for n in range(nc)]
def section_info(s):
    se = sa + 76*s
    smc, sma = block(se); ssc, ssa = block(se + 12)
    vcount = sum(u16(sma + 16*k + 14) for k in range(smc))
    shaders = [tagref(sha + 36*i16(sma + 16*k))[2] for k in range(smc)]
    node = u8(se + 45)
    return "vb %d ib %d node %s vfmt %d submeshes %d subsets %d verts(sum) %d shaders %s" % (
        i16(se + 24), i16(se + 40), ("%d %r" % (node, node_names[node])) if node != 255 else "255 (skinned)", u8(se + 46), smc, ssc, vcount,
        [sh.split("\\")[-1] if sh else None for sh in shaders])
for r in range(rc):
    rn = string_id(i32(ra + 16*r)); pc, pa = block(ra + 16*r + 4)
    first = (string_id(i32(pa)), i16(pa + 4), i16(pa + 6))
    base = None
    for p in range(pc):
        pe = pa + 24*p
        if string_id(i32(pe)) == "base": base = (p, i16(pe + 4), i16(pe + 6))
    print("  region[%d] %-14r perms %2d  first=%r sec %d x%d   base=%s" % (r, rn, pc, first[0], first[1], first[2], base))
    if base and base[1] >= 0:
        for s in range(base[1], base[1] + base[2]):
            print("        section %d: %s" % (s, section_info(s)))
print("  section 0 (body/legacy): %s" % section_info(0))
mgc, mga = block(M + 60)
for g in range(mgc):
    e = mga + 16*g; gname = string_id(i32(e)); kc, ka = block(e + 4)
    if gname in ("head", "left_hand", "right_hand", "left_foot", "right_foot", "body", "weapon_back"):
        me = ka
        print("  marker %-12r node %2d %r region %d perm %d pos %s rot %s" % (gname, u8(me + 2), node_names[u8(me + 2)], u8(me), u8(me + 1), fmt(f32n(me + 4, 3)), fmt(f32n(me + 16, 4))))
nz = sum(1 for s in range(shc) if any(m[sha + 36*s + 16: sha + 36*s + 36]))
print("  shader entries with non-zero bytes 16..35: %d of %d" % (nz, shc))
print("")
print("== D. other biped-looking mode tags in riverworld")
for i in modes:
    if "characters" in (tag_names[i] or ""):
        Mi = tag_meta(i); print("  tag %d %r nodes %d sections %d regions %d" % (i, tag_names[i], block(Mi + 48)[0], block(Mi + 104)[0], block(Mi + 12)[0]))
