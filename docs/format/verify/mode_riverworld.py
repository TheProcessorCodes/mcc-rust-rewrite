# Read-only check of the render_model (mode) tag layout documented in
# docs/format/scenario-and-tag-reading.md ("Gap fill: render_model") against a real
# MCC Halo 3 U13 map.  Run:  python -I mode_riverworld.py [path\to\map] [tag index ...]
# Default: riverworld.map, tags 173 (mp_spawn_point) and the mp_masterchief render model.
# Never writes anything.
import os, mmap, struct, sys, math

args = list(sys.argv[1:])
MAP = args.pop(0) if args and args[0].lower().endswith(".map") else (
    os.path.join(os.environ["HALO3_MAPS"], "riverworld.map"))
WANT = [int(a) for a in args]

f = open(MAP, "rb")
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)

def u8(o):  return m[o]
def i16(o): return struct.unpack_from("<h", m, o)[0]
def u16(o): return struct.unpack_from("<H", m, o)[0]
def i32(o): return struct.unpack_from("<i", m, o)[0]
def u32(o): return struct.unpack_from("<I", m, o)[0]
def u64(o): return struct.unpack_from("<Q", m, o)[0]
def f32(o): return struct.unpack_from("<f", m, o)[0]
def f32n(o, n): return struct.unpack_from("<%df" % n, m, o)
def cstr(o, maxlen=None):
    end = m.find(b"\0", o, o + maxlen if maxlen else len(m))
    if end < 0:
        end = o + (maxlen or 0)
    return m[o:end].decode("latin-1")
def fourcc(o): return m[o:o+4][::-1].decode("latin-1")

# ---------------------------------------------------------------- header / tag index / string ids
assert m[0:4] == b"daeh"
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
INDEX_BITS, NS_BITS = 19, 8
mask = (1 << INDEX_BITS) - 1
ns_arr = [i32(hdr(ns_tbl_ptr) + 4*i) for i in range(ns_count)]
namespaces = {}
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
    cls = fourcc(o) if u32(o) != 0xFFFFFFFF else None
    idx = struct.unpack("<h", struct.pack("<H", i32(o + 12) & 0xFFFF))[0]
    return cls, idx, (tag_names[idx] if idx >= 0 else None)
def block(o):
    cnt, p = i32(o), i32(o + 4)
    return cnt, (ptr32(p) if cnt > 0 else None)

print("map %s" % MAP)
print("build '%s'  tags %d  tag magic 0x%X" % (cstr(0xA0, 32), tag_count, tag_magic))

# ---------------------------------------------------------------- zone (resource gestalt) for owner checks
zone = [i for i in range(tag_count) if tag_class(i) == "zone"]
assert len(zone) == 1
Z = tag_meta(zone[0])
re_count, re_addr = block(Z + 0x64)
def resource_entry(rid):
    k = rid & 0xFFFF
    if not (0 <= k < re_count):
        return None
    e = re_addr + 64*k
    return dict(index=k, owner=tagref(e), fixup_off=i32(e + 0x14), fixup_size=i32(e + 0x18),
                segment=i16(e + 0x22), fixups=block(e + 0x28)[0])

# ---------------------------------------------------------------- matrix helpers (column-vector 4x4 as nested lists)
def quat_to_mat(x, y, z, w):
    # standard rotation matrix for column vectors (v' = R v)
    return [[1-2*(y*y+z*z), 2*(x*y-z*w),   2*(x*z+y*w)],
            [2*(x*y+z*w),   1-2*(x*x+z*z), 2*(y*z-x*w)],
            [2*(x*z-y*w),   2*(y*z+x*w),   1-2*(x*x+y*y)]]
def mat4(R, t):
    return [[R[0][0], R[0][1], R[0][2], t[0]],
            [R[1][0], R[1][1], R[1][2], t[1]],
            [R[2][0], R[2][1], R[2][2], t[2]],
            [0, 0, 0, 1]]
def mul(A, B):
    return [[sum(A[i][k]*B[k][j] for k in range(4)) for j in range(4)] for i in range(4)]
def rowvec_3x4_to_col4x4(v):
    # 12 floats in Reclaimer ReadMatrix3x4 order (row-vector rows M1x..M4x) -> column-vector 4x4 = transpose
    return [[v[0], v[3], v[6], v[9]],
            [v[1], v[4], v[7], v[10]],
            [v[2], v[5], v[8], v[11]],
            [0, 0, 0, 1]]
def max_dev_identity(A):
    return max(abs(A[i][j] - (1.0 if i == j else 0.0)) for i in range(4) for j in range(4))
def fmt(v, nd=4): return "(" + ", ".join(("%%.%df" % nd) % x for x in v) + ")"

# ---------------------------------------------------------------- the render_model dump
def read_nodes(nodes_cnt, nodes_addr):
    out = []
    for n in range(nodes_cnt):
        e = nodes_addr + 96*n
        out.append(dict(file=e, name=string_id(i32(e)), parent=i16(e+4), child=i16(e+6), sibling=i16(e+8),
                        pad10=i16(e+10), pos=f32n(e+12, 3), rot=f32n(e+24, 4), inv_scale=f32(e+40),
                        inv=f32n(e+44, 12), dist=f32(e+92)))
    return out

def check_nodes(nodes):
    """Verifies tree indices, unit quaternions, dist = |pos|, and that InverseTransform is the
    inverse of the bind-pose world transform built from the local chain. Returns (ok, msgs, worlds)."""
    msgs = []; ok = True
    n = len(nodes)
    worlds = [None]*n
    for i, nd in enumerate(nodes):
        if not (-1 <= nd["parent"] < n and -1 <= nd["child"] < n and -1 <= nd["sibling"] < n):
            ok = False; msgs.append("node %d: tree index out of range" % i)
        if nd["child"] >= 0 and nodes[nd["child"]]["parent"] != i:
            ok = False; msgs.append("node %d: first child %d has parent %d" % (i, nd["child"], nodes[nd["child"]]["parent"]))
        if nd["sibling"] >= 0 and nodes[nd["sibling"]]["parent"] != nd["parent"]:
            ok = False; msgs.append("node %d: sibling %d has a different parent" % (i, nd["sibling"]))
        qn = math.sqrt(sum(c*c for c in nd["rot"]))
        if abs(qn - 1) > 1e-3:
            ok = False; msgs.append("node %d: quaternion norm %.5f" % (i, qn))
        pl = math.sqrt(sum(c*c for c in nd["pos"]))
        expect = 0.0 if nd["parent"] < 0 else pl          # root nodes store 0, not |pos|
        if abs(expect - nd["dist"]) > 1e-3 * max(1, pl):
            ok = False; msgs.append("node %d: dist %.5f != expected %.5f (|pos| %.5f)" % (i, nd["dist"], expect, pl))
        if nd["parent"] >= i:
            ok = False; msgs.append("node %d: parent %d not before child" % (i, nd["parent"]))
    worst = 0.0
    for i, nd in enumerate(nodes):
        x, y, z, w = nd["rot"]
        local = mat4(quat_to_mat(x, y, z, w), nd["pos"])          # column vectors: translate after rotate
        worlds[i] = local if nd["parent"] < 0 else mul(worlds[nd["parent"]], local)
        inv = rowvec_3x4_to_col4x4(nd["inv"])
        dev = max_dev_identity(mul(worlds[i], inv))
        worst = max(worst, dev)
        if dev > 2e-3:
            ok = False; msgs.append("node %d %r: world*InverseTransform deviates from identity by %.4f" % (i, nd["name"], dev))
    msgs.append("max |world*InverseTransform - I| over %d nodes = %.2e" % (n, worst))
    return ok, msgs, worlds

def dump_mode(idx, full=True):
    M = tag_meta(idx)
    print("")
    print("=" * 100)
    print("mode tag %d %r  meta ptr 0x%08X -> file 0x%X" % (idx, tag_names[idx], tags[idx][2], M))
    name_v = i32(M + 0); flags = i32(M + 4)
    print("  +0   Name       0x%08X -> %r" % (name_v, string_id(name_v)))
    print("  +4   Flags      0x%08X  (bit2 UseLocalNodes = %s)" % (flags, bool(flags & 4)))
    def show(off, label):
        c, a = block(M + off)
        print("  +%-4d %-14s count %d%s" % (off, label, c, ("  -> file 0x%X" % a) if a else ""))
        return c, a
    regions = show(12, "Regions")
    print("  +28  InstancedGeometrySectionIndex %d" % i32(M + 28))
    ginst = show(32, "GeometryInst.")
    nodes_b = show(48, "Nodes")
    mg = show(60, "MarkerGroups")
    sh = show(72, "Shaders")
    sec = show(104, "Sections")
    bb = show(116, "BoundingBoxes")
    nm = show(176, "NodeMaps")
    rid = i32(M + 224); re = resource_entry(rid)
    if re:
        print("  +224 ResourcePointer 0x%08X -> resource index %d: owner %s %d (%s), fixup off %d size %d, segment %d, fixups %d" % (
            rid, rid & 0xFFFF, re["owner"][0], re["owner"][1], "THIS TAG" if re["owner"][1] == idx else re["owner"][2],
            re["fixup_off"], re["fixup_size"], re["segment"], re["fixups"]))
    else:
        print("  +224 ResourcePointer 0x%08X -> no zone entry" % rid)

    print("  -- regions (16 bytes: StringId name, TagBlock permutations @4); permutations (24 bytes: name, i16 section, i16 count, 4 x i32 masks)")
    perm_blocks = []
    for r in range(regions[0]):
        e = regions[1] + 16*r
        pc, pa = block(e + 4)
        perm_blocks.append((pc, pa))
        print("     region[%d] %r permutations %d" % (r, string_id(i32(e)), pc))
        for p in range(pc):
            pe = pa + 24*p
            masks = [u32(pe + 8), u32(pe + 12), u32(pe + 16), u32(pe + 20)]
            if full or p < 4:
                print("        perm[%d] %r section %d count %d masks %s" % (p, string_id(i32(pe)), i16(pe+4), i16(pe+6), ["0x%X" % x for x in masks]))
    adj = [(perm_blocks[r+1][1] - perm_blocks[r][1], perm_blocks[r][0]*24) for r in range(len(perm_blocks)-1) if perm_blocks[r][1] and perm_blocks[r+1][1]]
    if adj:
        good = sum(1 for d, e in adj if d == e)
        print("     consecutive regions' permutation blocks are %s bytes apart; count*24 = %s  -> %d/%d match" % ([d for d, e in adj][:6], [e for d, e in adj][:6], good, len(adj)))

    nodes = read_nodes(nodes_b[0], nodes_b[1]) if nodes_b[1] else []
    print("  -- nodes (96 bytes): name, parent, first child, next sibling, +10 i16, position, rotation xyzw, inverse scale, inverse 3x4 @44, distance @92")
    for i, nd in enumerate(nodes):
        if full or i < 3:
            print("     [%2d] %-22r parent %3d child %3d sibling %3d +10=%d pos %s rot %s invS %.4f dist %.4f" % (
                i, nd["name"], nd["parent"], nd["child"], nd["sibling"], nd["pad10"], fmt(nd["pos"]), fmt(nd["rot"]), nd["inv_scale"], nd["dist"]))
    if nodes:
        ok, msgs, worlds = check_nodes(nodes)
        print("     node checks %s: %s" % ("OK" if ok else "FAILED", "; ".join(msgs[-3:])))
        if full:
            print("     inverse 3x4 of node 0 (12 floats, ReadMatrix3x4 order M11..M13 M21..M23 M31..M33 M41..M43): %s" % fmt(nodes[0]["inv"], 3))
            xs = [w[0][3] for w in worlds]; ys = [w[1][3] for w in worlds]; zs = [w[2][3] for w in worlds]
            print("     bind-pose node origins span x [%.3f, %.3f] y [%.3f, %.3f] z [%.3f, %.3f] wu" % (min(xs), max(xs), min(ys), max(ys), min(zs), max(zs)))

    print("  -- marker groups (16 bytes: name, TagBlock markers @4); markers (36 bytes: u8 region, u8 perm, u8 node, pos @4, quat @16, scale @32)")
    for g in range(mg[0]):
        e = mg[1] + 16*g
        mc, ma = block(e + 4)
        if full or g < 6:
            print("     group[%d] %r markers %d" % (g, string_id(i32(e)), mc))
            for k in range(mc if full else min(mc, 2)):
                me = ma + 36*k
                print("        marker[%d] region %d perm %d node %d +3=%d pos %s rot %s scale %.3f" % (
                    k, u8(me), u8(me+1), u8(me+2), u8(me+3), fmt(f32n(me+4, 3)), fmt(f32n(me+16, 4)), f32(me+32)))

    print("  -- shaders (36 bytes: TagReference @0)")
    for s in range(sh[0]):
        e = sh[1] + 36*s
        if full or s < 4:
            print("     shader[%d] %s  bytes 16..35: %s" % (s, tagref(e), m[e+16:e+36].hex()))

    print("  -- sections (76 bytes): submeshes @0, subsets @12, vb index @24, ib index @40, flags @44, node @45, vertex fmt @46, index fmt @48")
    for s in range(sec[0]):
        e = sec[1] + 76*s
        sm = block(e); ss = block(e + 12)
        if full or s < 6:
            print("     section[%d] submeshes %d subsets %d vb %d (+26..39 %s) ib %d ib2 %d flags 0x%02X node %d vfmt %d +47=%d ifmt %d" % (
                s, sm[0], ss[0], i16(e+24), m[e+26:e+40].hex(), i16(e+40), i16(e+42), u8(e+44), u8(e+45), u8(e+46), u8(e+47), u8(e+48)))
            for k in range(min(sm[0], 3)):
                p = sm[1] + 16*k
                print("        submesh[%d] shader %d idxStart %d idxLen %d subset %d+%d flags 0x%02X verts %d" % (
                    k, i16(p), u16(p+4), u16(p+6), u16(p+8), u16(p+10), u8(p+13), u16(p+14)))

    print("  -- bounding boxes (56 bytes): u32 @0, X @4, Y @12, Z @20, U @28, V @36, 12 bytes @44")
    for b in range(bb[0]):
        e = bb[1] + 56*b
        print("     bbox[%d] +0=0x%08X X %s Y %s Z %s U %s V %s tail %s (as floats %s)" % (
            b, u32(e), fmt(f32n(e+4, 2)), fmt(f32n(e+12, 2)), fmt(f32n(e+20, 2)), fmt(f32n(e+28, 2)), fmt(f32n(e+36, 2)),
            m[e+44:e+56].hex(), fmt(f32n(e+44, 3), 3)))

    print("  -- node maps (12 bytes: TagBlock of bytes)")
    for k in range(nm[0]):
        e = nm[1] + 12*k
        c, a = block(e)
        if full or k < 6:
            print("     nodemap[%d] count %d: %s" % (k, c, list(m[a:a+c]) if a else []))
    return nodes

# ---------------------------------------------------------------- stride survey over every mode tag
def survey():
    modes = [i for i in range(tag_count) if tag_class(i) == "mode"]
    print("")
    print("== survey of all %d mode tags: node checks under the 96-byte stride, permutation adjacency under the 24-byte stride" % len(modes))
    node_ok = node_bad = 0; perm_ok = perm_bad = perm16_ok = 0
    local_nodes = with_nodemaps = with_instances = owner_ok = name_ok = multi_perm = node_total = 0
    max_nodes = (0, None)
    bad_list = []
    vfmts = {}
    for i in modes:
        M = tag_meta(i)
        if string_id(i32(M)) is not None:
            name_ok += 1
        if i32(M + 4) & 4:
            local_nodes += 1
        if block(M + 176)[0] > 0:
            with_nodemaps += 1
        if i32(M + 28) >= 0:
            with_instances += 1
        re = resource_entry(i32(M + 224))
        if re and re["owner"][1] == i:
            owner_ok += 1
        nc, na = block(M + 48)
        node_total += nc
        if nc > max_nodes[0]:
            max_nodes = (nc, i)
        if na:
            ok, msgs, _ = check_nodes(read_nodes(nc, na))
            if ok:
                node_ok += 1
            else:
                node_bad += 1; bad_list.append((i, msgs[:2]))
        rc, ra = block(M + 12)
        ptrs = []
        for r in range(rc):
            pc, pa = block(ra + 16*r + 4)          # Permutations TagBlock sits at region + 4
            ptrs.append((pc, pa))
            if pc > 1:
                multi_perm += 1
        if (i32(M + 4) & 4) or block(M + 176)[0] > 0 or i32(M + 28) >= 0:
            nmc, nma = block(M + 176)
            maps = []
            for k in range(min(nmc, 4)):
                c, a = block(nma + 12*k)
                maps.append(list(m[a:a+c]) if a else [])
            print("   special: tag %d %r flags 0x%X nodes %d nodemaps %d instSection %d instances %d first maps %s" % (
                i, tag_names[i], i32(M + 4), nc, nmc, i32(M + 28), block(M + 32)[0], maps))
        for r in range(len(ptrs) - 1):
            if ptrs[r][1] and ptrs[r+1][1]:
                d = ptrs[r+1][1] - ptrs[r][1]
                if d == ptrs[r][0]*24:
                    perm_ok += 1
                else:
                    perm_bad += 1
                if d == ptrs[r][0]*16:
                    perm16_ok += 1
        sc, sa = block(M + 104)
        for s in range(sc):
            v = u8(sa + 76*s + 46)
            vfmts[v] = vfmts.get(v, 0) + 1
    print("   names decode: %d/%d   resource owner == tag: %d/%d" % (name_ok, len(modes), owner_ok, len(modes)))
    print("   node checks OK: %d, failed: %d  (total nodes %d, largest rig %d nodes = tag %s %r)" % (
        node_ok, node_bad, node_total, max_nodes[0], max_nodes[1], tag_names[max_nodes[1]] if max_nodes[1] is not None else None))
    for i, msgs in bad_list[:5]:
        print("      failed: tag %d %r: %s" % (i, tag_names[i], msgs))
    print("   consecutive-region permutation blocks: %d are exactly count*24 apart, %d are not; %d would fit count*16   (regions with >1 permutation: %d)" % (
        perm_ok, perm_bad, perm16_ok, multi_perm))
    print("   models with UseLocalNodes flag: %d; with NodeMaps: %d; with instanced geometry section: %d" % (local_nodes, with_nodemaps, with_instances))
    print("   section vertex formats seen (fmt: count): %s" % sorted(vfmts.items()))

if not WANT:
    WANT = [173] + [i for i in range(tag_count) if tag_class(i) == "mode"
                    and tag_names[i] == r"objects\characters\masterchief\mp_masterchief\mp_masterchief"]
for idx in WANT:
    dump_mode(idx, full=True)
survey()
