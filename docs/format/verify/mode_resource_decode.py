# Read-only decode of a render_model's vertex / index buffers on riverworld.map, to confirm
# that the render_model resource follows the same rules as the sbsp one (sbsp doc sections 4-7):
# strip index buffers, UInt16_N4 positions expanded with BoundingBoxes[0], skinned blend indices.
# Run:  python -I mode_resource_decode.py [tag index ...]     (default 173 and the mp_masterchief model)
import mmap, struct, sys, os, zlib, math

MAPS = os.environ.get("HALO3_MAPS", "")
MAP = os.path.join(MAPS, "riverworld.map")
WANT = [int(a) for a in sys.argv[1:]]

f = open(MAP, "rb")
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
def u8(o):  return m[o]
def i16(o): return struct.unpack_from("<h", m, o)[0]
def i32(o): return struct.unpack_from("<i", m, o)[0]
def u32(o): return struct.unpack_from("<I", m, o)[0]
def u64(o): return struct.unpack_from("<Q", m, o)[0]
def f32n(o, n): return struct.unpack_from("<%df" % n, m, o)
def cstr(o, maxlen=None):
    end = m.find(b"\0", o, o + maxlen if maxlen else len(m))
    return m[o:end].decode("latin-1")
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
namespaces = {}
start = ns_arr[0] & mask
for i in range(1, ns_count):
    namespaces[i] = (0, start); start += ns_arr[i] & mask
namespaces[0] = (ns_arr[0] & mask, start)
def string_id(v):
    idx = v & mask; ns = (v >> 19) & 0xFF
    while ns not in namespaces and ns > 0: ns -= 1
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
def block(o):
    cnt, p = i32(o), i32(o + 4)
    return cnt, (ptr32(p) if cnt > 0 else None)

zone = [i for i in range(tag_count) if tag_class(i) == "zone"][0]
Z = tag_meta(zone)
re_count, re_addr = block(Z + 0x64)
fixup_data = ptr32(i32(Z + 0x154))
play = [i for i in range(tag_count) if tag_class(i) == "play"][0]
P = tag_meta(play) if tags[play][2] != 0 else Z + 0x1C
shared_cnt, shared_addr = block(P + 0xC)
pages_cnt, pages_addr = block(P + 0x18)
segs_cnt, segs_addr = block(P + 0x3C)
page_cache = {}

def resource_bytes(rid):
    k = rid & 0xFFFF
    e = re_addr + 64*k
    seg_i = i16(e + 0x22)
    s = segs_addr + 16*seg_i
    prim, sec_, prim_off, sec_off_ = i16(s), i16(s + 2), i32(s + 4), i32(s + 8)
    page_i, seg_off = (sec_, sec_off_) if sec_ >= 0 else (prim, prim_off)
    pg = pages_addr + 88*page_i
    if i32(pg + 8) < 0 or i32(pg + 0xC) == 0:
        page_i, seg_off = prim, prim_off; pg = pages_addr + 88*page_i
    cache_i, data_off, comp, decomp = i16(pg + 4), i32(pg + 8), i32(pg + 0xC), i32(pg + 0x10)
    key = (cache_i, page_i)
    if key not in page_cache:
        if cache_i < 0:
            src = m; base = u32(0x4D0)
        else:
            name = os.path.basename(cstr(shared_addr + 264*cache_i, 32).replace("\\", os.sep))
            src = open(os.path.join(MAPS, name), "rb").read(); base = 0x4000
        raw = src[base + data_off: base + data_off + comp]
        page_cache[key] = raw if comp == decomp else zlib.decompress(raw, -15)
        print("   page %d (cache %d) data offset %d, compressed %d -> %d bytes" % (page_i, cache_i, data_off, comp, len(page_cache[key])))
    data = page_cache[key]
    print("   resource %d: segment %d, page %d, segment offset %d" % (k, seg_i, page_i, seg_off))
    return data[seg_off:], e

def unstrip(idx):
    tris = 0; degenerate = 0
    a = b = c = None
    for n, x in enumerate(idx, 1):
        a, b, c = b, c, x
        if n < 3: continue
        if a == b or a == c or b == c:
            degenerate += 1; continue
        tris += 1
    return tris, degenerate

def quat_to_mat(x, y, z, w):
    return [[1-2*(y*y+z*z), 2*(x*y-z*w),   2*(x*z+y*w)],
            [2*(x*y+z*w),   1-2*(x*x+z*z), 2*(y*z-x*w)],
            [2*(x*z-y*w),   2*(y*z+x*w),   1-2*(x*x+y*y)]]
def mat4(R, t):
    return [[R[0][0], R[0][1], R[0][2], t[0]], [R[1][0], R[1][1], R[1][2], t[1]], [R[2][0], R[2][1], R[2][2], t[2]], [0, 0, 0, 1]]
def mul(A, B): return [[sum(A[i][k]*B[k][j] for k in range(4)) for j in range(4)] for i in range(4)]
def apply(Mx, p): return tuple(Mx[r][0]*p[0] + Mx[r][1]*p[1] + Mx[r][2]*p[2] + Mx[r][3] for r in range(3))

def decode(idx):
    M = tag_meta(idx)
    print("\n== mode %d %r" % (idx, tag_names[idx]))
    nc, na = block(M + 48)
    nodes = []
    for n in range(nc):
        e = na + 96*n
        nodes.append(dict(name=string_id(i32(e)), parent=i16(e + 4), pos=f32n(e + 12, 3), rot=f32n(e + 24, 4)))
    worlds = []
    for n in nodes:
        x, y, z, w = n["rot"]
        local = mat4(quat_to_mat(x, y, z, w), n["pos"])
        worlds.append(local if n["parent"] < 0 else mul(worlds[n["parent"]], local))
    bbc, bba = block(M + 116)
    bb = f32n(bba + 4, 10)       # X Y Z U V min/max pairs
    def expand(px, py, pz):
        return (bb[0] + px*(bb[1]-bb[0]), bb[2] + py*(bb[3]-bb[2]), bb[4] + pz*(bb[5]-bb[4]))
    data, entry = resource_bytes(i32(M + 224))
    fo, fs = i32(entry + 0x14), i32(entry + 0x18)
    fx = fixup_data + fo
    vbN = i32(fx + fs - 24); ibN = i32(fx + fs - 12)
    fix_cnt, fix_addr = block(entry + 0x28)
    fixups = [u32(fix_addr + 8*k + 4) & 0x0FFFFFFF for k in range(fix_cnt)]
    sc, sa = block(M + 104)
    print("   vb %d ib %d fixups %d sections %d nodes %d; bbox X %s Y %s Z %s" % (vbN, ibN, fix_cnt, sc, nc, bb[0:2], bb[2:4], bb[4:6]))
    for s in range(min(sc, 3)):
        se = sa + 76*s
        vbi, ibi, node, vfmt, ifmt = i16(se + 24), i16(se + 40), u8(se + 45), u8(se + 46), u8(se + 48)
        vinfo = fx + 28*vbi; iinfo = fx + 40*vbN + 24*ibi
        vcount, vstride, vlen = i32(vinfo), i16(vinfo + 6), i32(vinfo + 8)
        iformat, ilen = i32(iinfo), i32(iinfo + 4)
        voff = fixups[vbi]; ioff = fixups[2*vbN + ibi]
        print("   section %d: node %d vfmt %d (resource fmt %d stride %d) %d verts @%d; ib %d format %d (%d u16) @%d; section ifmt byte %d" % (
            s, node, vfmt, i16(vinfo + 4), vstride, vcount, voff, ibi, iformat, ilen // 2, ioff, ifmt))
        # positions
        pts = []
        for v in range(vcount):
            o = voff + vstride*v
            px, py, pz, pw = struct.unpack_from("<4H", data, o)
            pts.append(expand(px/65535.0, py/65535.0, pz/65535.0))
        xs, ys, zs = zip(*pts)
        cen = (sum(xs)/len(xs), sum(ys)/len(ys), sum(zs)/len(zs))
        print("      decoded positions span x [%.3f, %.3f] y [%.3f, %.3f] z [%.3f, %.3f], centroid (%.3f, %.3f, %.3f)" % (
            min(xs), max(xs), min(ys), max(ys), min(zs), max(zs), cen[0], cen[1], cen[2]))
        # normals
        nlen = []
        for v in range(min(vcount, 200)):
            o = voff + vstride*v + 12
            nx, ny, nz, nw = struct.unpack_from("<4h", data, o)
            nlen.append(math.sqrt((nx/32767.0)**2 + (ny/32767.0)**2 + (nz/32767.0)**2))
        print("      normal length (first 200) min %.4f max %.4f" % (min(nlen), max(nlen)))
        if node != 255:
            wpos = (worlds[node][0][3], worlds[node][1][3], worlds[node][2][3])
            print("      attached to node %d %r whose bind-pose world origin is (%.3f, %.3f, %.3f)" % (node, nodes[node]["name"], wpos[0], wpos[1], wpos[2]))
            # if vertices were in node space, transforming the centroid by the node world matrix should land near the node
            tc = apply(worlds[node], cen)
            print("      centroid transformed by that node's world matrix = (%.3f, %.3f, %.3f)" % tc)
        if vfmt == 2:
            bad = 0; wsum_bad = 0; maxidx = 0
            for v in range(vcount):
                o = voff + vstride*v
                bi = struct.unpack_from("<4B", data, o + 0x24); bw = struct.unpack_from("<4B", data, o + 0x28)
                maxidx = max(maxidx, max(bi))
                if any(b >= nc for b, w in zip(bi, bw) if w > 0): bad += 1
                if abs(sum(bw) - 255) > 2: wsum_bad += 1
            print("      skinned: max blend index %d (nodes %d), vertices with a weighted index >= node count: %d, weight sums != 255: %d" % (maxidx, nc, bad, wsum_bad))
        # indices
        idx = struct.unpack_from("<%dH" % (ilen // 2), data, ioff)
        print("      indices: %d, max %d (< %d verts: %s)" % (len(idx), max(idx), vcount, max(idx) < vcount))
        smc, sma = block(se)
        for k in range(smc):
            p = sma + 16*k
            st, ln = struct.unpack_from("<HH", m, p + 4)
            tris, deg = unstrip(idx[st:st + ln])
            print("      submesh %d: strip slice [%d, +%d) -> %d triangles (%d degenerate skipped); as a list it would be %.1f triangles" % (k, st, ln, tris, deg, ln / 3))

if not WANT:
    WANT = [173] + [i for i in range(tag_count) if tag_class(i) == "mode"
                    and tag_names[i] == r"objects\characters\masterchief\mp_masterchief\mp_masterchief"]
for i in WANT:
    decode(i)
