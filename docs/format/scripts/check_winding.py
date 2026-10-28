"""Read-only: which side of a decoded sbsp triangle (i0, i1, i2) is the front?

Usage:  python -I check_winding.py [path to riverworld.map]

For every cluster section and every instance section of BSP 0 this decodes the vertex positions and
the Int16_N4 vertex normals, walks each submesh's triangle list (IndexFormat 3) and measures
    sign( cross(p1 - p0, p2 - p0) . (n0 + n1 + n2) )
in Halo space (world units, Z up).  A positive dot product means the index order is counter-clockwise
when seen from the side the normal points to (right-hand rule), i.e. the normal side is the front.
Also counts geometry instances whose 3x3 rotation has a negative determinant (mirrored placements),
because those are the only triangles whose winding flips when transformed to world space.
Never writes anything.  Companion to docs/format/sbsp-render-geometry.md "Gap fill" section.
"""
import os
import struct
import sys
import zlib

MAPS = r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps"
_positional = [a for i, a in enumerate(sys.argv[1:], 1) if not a.startswith("--") and sys.argv[i - 1] != "--detail"]
path = _positional[0] if _positional else os.path.join(MAPS, "riverworld.map")
d = open(path, "rb").read()
print("file", path, "size 0x%X" % len(d))


def i32(o): return struct.unpack_from("<i", d, o)[0]
def u32(o): return struct.unpack_from("<I", d, o)[0]
def i16(o): return struct.unpack_from("<h", d, o)[0]
def u16(o): return struct.unpack_from("<H", d, o)[0]
def i64(o): return struct.unpack_from("<q", d, o)[0]
def f32(o): return struct.unpack_from("<f", d, o)[0]
def cstr(o, n=256):
    e = d.index(b"\0", o); return d[o:min(e, o + n)].decode("latin1")


# ---- header / address translation (see container.md)
assert d[:4] == b"daeh" and i32(4) == 13
secoff = [u32(0x4CC + 4 * i) for i in range(4)]
secs = [(u32(0x4DC + 8 * i), u32(0x4E0 + 8 * i)) for i in range(4)]
vbase = i64(0x2E0); idxptr = i64(0x2E8); tagcount = i32(0x20)
magic2 = (secs[2][0] + secoff[2]) & 0xFFFFFFFF
def tagfile(v): return v - vbase + magic2
def hdr(p): return (p + secoff[0]) & 0xFFFFFFFF
def tp(p): return tagfile((p << 2) & 0xFFFFFFFFFFFFFFFF) if p != 0 else -1
rawbase = u32(0x4D0)

# ---- tag index
ti = tagfile(idxptr)
classcount = i32(ti); cf = tagfile(i64(ti + 8)); tf = tagfile(i64(ti + 24))
classes = [d[cf + 16 * i:cf + 16 * i + 4][::-1].decode("latin1") for i in range(classcount)]
nameidx = hdr(i32(0x2C)); names = hdr(i32(0x24))
tags = []
for i in range(tagcount):
    ci = i16(tf + 8 * i); mp = u32(tf + 8 * i + 4); no = i32(nameidx + 4 * i)
    tags.append((classes[ci] if ci >= 0 else None, cstr(names + no) if no >= 0 else None, tp(mp) if ci >= 0 else -1))
def find(cls, name=None): return [i for i, t in enumerate(tags) if t[0] == cls and (name is None or t[1] == name)]
def block(o):
    c = i32(o); p = u32(o + 4); return c, (tp(p) if c > 0 else -1)
def tagref(o): return d[o:o + 4][::-1].decode("latin1"), i32(o + 12)
def tagidx(tid): return tid & 0xFFFF if tid != -1 else -1

# ---- scnr -> sbsp 0 -> Lbsp
scnrm = tags[find("scnr", cstr(0xE0))[0]][2]
sbc, sbp = block(scnrm + 24)
bsp_index = 0
sb = tagidx(tagref(sbp + 108 * bsp_index)[1]); m = tags[sb][2]
print("sbsp", sb, tags[sb][1])
ldt = tagidx(tagref(scnrm + 1708)[1])
lrc, lrp = block(tags[ldt][2] + 4)
lbm = None
for i in range(lrc):
    tid = tagidx(tagref(lrp + 16 * i)[1])
    if tid >= 0 and i16(tags[tid][2] + 2) == bsp_index:
        lbm = tags[tid][2]; print("Lbsp", tid, tags[tid][1])
if lbm is None:  # fallback: Lbsp with the sbsp's name
    lbm = tags[find("Lbsp", tags[sb][1])[0]][2]

clc, clp = block(m + 196); shc, shp = block(m + 208); gic, gip = block(m + 448)
bb_c, bb_p = block(m + 768)
lsec_c, lsec_p = block(lbm + 308); rp = u32(lbm + 428)
cl_secs = [i16(clp + 280 * i + 216) for i in range(clc)]
inst = []
for i in range(gic):
    o = gip + 120 * i
    R = [[f32(o + 4 + 4 * (r * 3 + c)) for c in range(3)] for r in range(3)]
    T = [f32(o + 40 + 4 * k) for k in range(3)]
    inst.append((f32(o), R, T, i16(o + 52)))
print("clusters", clc, "instances", gic, "sections", lsec_c, "bounding boxes", bb_c)


def det3(R):
    return (R[0][0] * (R[1][1] * R[2][2] - R[1][2] * R[2][1])
            - R[0][1] * (R[1][0] * R[2][2] - R[1][2] * R[2][0])
            + R[0][2] * (R[1][0] * R[2][1] - R[1][1] * R[2][0]))


dets = [det3(R) for _, R, _, _ in inst]
neg = [(i, round(dt, 4)) for i, dt in enumerate(dets) if dt < 0]
print("instance det(R): min %.4f max %.4f ; mirrored (det < 0): %d %s" % (min(dets), max(dets), len(neg), neg[:10]))

# ---- resource bytes (zone + play, see sbsp-render-geometry.md section 4)
zm = tags[find("zone")[0]][2]
rec, rep = block(zm + 100); fptr = tp(u32(zm + 340))
ri = rp & 0xFFFF; e = rep + 64 * ri
fo, fs = i32(e + 20), i32(e + 24); seg = i16(e + 34)
rf_c, rf_p = block(e + 40)
fix = [u32(rf_p + 8 * k + 4) & 0x0FFFFFFF for k in range(rf_c)]
fd = fptr + fo
vbcount = i32(fd + fs - 24); ibcount = i32(fd + fs - 12)
vbinfo = [[i32(fd + 28 * k + 4 * j) for j in range(7)] for k in range(vbcount)]
ibo = fd + 40 * vbcount
ibinfo = [[i32(ibo + 24 * k + 4 * j) for j in range(6)] for k in range(ibcount)]
play = find("play")
pm = tags[play[0]][2] if play else zm + 28
shc_, shp_ = block(pm + 12); pgc, pgp = block(pm + 24); sgc, sgp = block(pm + 60)
sgo = sgp + 16 * seg
pp, sp = i16(sgo), i16(sgo + 2); po, so = i32(sgo + 4), i32(sgo + 8)
pg, pgoff = (sp, so) if sp >= 0 else (pp, po)
pgo = pgp + 88 * pg
cidx, doff, csz, dsz = i16(pgo + 4), i32(pgo + 8), i32(pgo + 12), i32(pgo + 16)
if sp >= 0 and (doff < 0 or csz == 0):
    pg, pgoff = pp, po; pgo = pgp + 88 * pg
    cidx, doff, csz, dsz = i16(pgo + 4), i32(pgo + 8), i32(pgo + 12), i32(pgo + 16)
if cidx >= 0:
    src = open(os.path.join(os.path.dirname(path), cstr(shp_ + 264 * cidx, 32)), "rb").read(); base = 16384
else:
    src = d; base = rawbase
raw = src[base + doff: base + doff + csz]
page = zlib.decompressobj(-15).decompress(raw) if csz != dsz else raw
res = page[pgoff:]
print("resource: page %d, %d bytes from page offset %d" % (pg, len(res), pgoff))


# ---- decode
def section(i):
    o = lsec_p + 76 * i
    return dict(sub=block(o), vb=i16(o + 24), ib=i16(o + 40), flags=d[o + 44], vfmt=d[o + 46], ifmt=d[o + 48])


def bbox(i):
    o = bb_p + 44 * i; return [f32(o + 4 + 4 * k) for k in range(6)]


def decode_vertices(s, si):
    vbi = s["vb"]; vcount = vbinfo[vbi][0]; vaddr = fix[vbi]
    st = {0: 44, 1: 36}[s["vfmt"]]
    bb = bbox(si) if si < bb_c else None
    P = []; N = []
    for v in range(vcount):
        o = vaddr + st * v
        if s["vfmt"] == 0:
            pos = struct.unpack_from("<3f", res, o); no = o + 0x14
        else:
            q = struct.unpack_from("<4H", res, o)
            pos = tuple(bb[2 * k] + q[k] / 65535.0 * (bb[2 * k + 1] - bb[2 * k]) for k in range(3))
            no = o + 0xC
        n = struct.unpack_from("<4h", res, no)
        N.append(tuple(max(-1.0, c / 32767.0) for c in n[:3])); P.append(pos)
    return P, N


def triangles(s):
    """triangle list indices of every submesh, in stored order"""
    vbi = s["vb"]; ibi = s["ib"]; vcount = vbinfo[vbi][0]
    assert ibi >= 0 and not (s["flags"] & 0x10), "unindexed section not handled"
    assert ibinfo[ibi][0] == 3, "not a triangle list: %d" % ibinfo[ibi][0]
    isz = 4 if vcount > 65535 else 2
    n = ibinfo[ibi][1] // isz
    idx = struct.unpack_from("<%d%s" % (n, "I" if isz == 4 else "H"), res, fix[2 * vbcount + ibi])
    sub_c, sub_p = s["sub"]
    for k in range(sub_c):
        o = sub_p + 16 * k
        start, length, shader = u16(o + 4), u16(o + 6), i16(o)
        sl = idx[start:start + length]
        for t in range(0, length - 2, 3):
            yield shader, sl[t], sl[t + 1], sl[t + 2]


def measure(si, P, N, R=None):
    """returns (agree, disagree, zero_cross, zero_normal, degenerate)"""
    agree = disagree = zc = zn = dg = 0
    for _, a, b, c in triangles(section(si)):
        if a == b or b == c or a == c:
            dg += 1; continue
        pa, pb, pc = P[a], P[b], P[c]
        e1 = (pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2])
        e2 = (pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2])
        fn = (e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0])
        vn = tuple(N[a][k] + N[b][k] + N[c][k] for k in range(3))
        if sum(x * x for x in fn) <= 1e-14: zc += 1; continue
        if sum(x * x for x in vn) <= 1e-8: zn += 1; continue
        if R is not None:  # rotate both into world space (checks that a proper rotation keeps the sign)
            fn = tuple(sum(fn[k] * R[k][c_] for k in range(3)) for c_ in range(3))
            vn = tuple(sum(vn[k] * R[k][c_] for k in range(3)) for c_ in range(3))
        dot = sum(fn[k] * vn[k] for k in range(3))
        if dot > 0: agree += 1
        elif dot < 0: disagree += 1
        else: zc += 1
    return agree, disagree, zc, zn, dg


def up_down(si, P):
    """near-horizontal triangles (|unit face normal z| > 0.65): how many face +Z (up) vs -Z (down)"""
    up = down = 0
    for _, a, b, c in triangles(section(si)):
        pa, pb, pc = P[a], P[b], P[c]
        e1 = (pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]); e2 = (pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2])
        fn = (e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0])
        L2 = sum(x * x for x in fn)
        if L2 <= 1e-14: continue
        nz = fn[2] / L2 ** 0.5
        if nz > 0.65: up += 1
        elif nz < -0.65: down += 1
    return up, down


tot = [0, 0, 0, 0, 0]; ud = [0, 0]
print("\n--- cluster sections (s_world_vertex, float positions, Int16_N4 normal at +0x14)")
for c, si in enumerate(cl_secs):
    s = section(si)
    if s["vb"] < 0 or vbinfo[s["vb"]][0] == 0:
        print("  cluster %2d section %3d: no vertex buffer" % (c, si)); continue
    P, N = decode_vertices(s, si)
    r = measure(si, P, N); u = up_down(si, P)
    for k in range(5): tot[k] += r[k]
    ud[0] += u[0]; ud[1] += u[1]
    print("  cluster %2d section %3d: %6d verts  CCW-from-normal %6d  CW %5d  zero-cross %d zero-normal %d degenerate %d | flat-ish faces: up %d down %d" % (c, si, len(P), *r, *u))
print("  clusters total: agree %d disagree %d zero-cross %d zero-normal %d degenerate %d -> agreement %.4f"
      % (*tot, tot[0] / max(1, tot[0] + tot[1])))
print("  clusters: near-horizontal triangles whose (p1-p0)x(p2-p0) points up (+Z): %d ; down (-Z): %d" % tuple(ud))

print("\n--- instance sections (s_rigid_vertex, UInt16_N4 positions expanded with BoundingBoxes[section], normal at +0xC)")
itot = [0, 0, 0, 0, 0]
seen = {}
worst = []
for si in sorted(set(s_ for *_, s_ in inst)):
    s = section(si)
    if s["vb"] < 0 or vbinfo[s["vb"]][0] == 0:
        print("  section %3d: no vertex buffer" % si); continue
    P, N = decode_vertices(s, si)
    r = measure(si, P, N)
    seen[si] = r
    for k in range(5): itot[k] += r[k]
    if r[1]: worst.append((si, r))
print("  %d instance sections decoded in local space: agree %d disagree %d zero-cross %d zero-normal %d degenerate %d -> agreement %.4f"
      % (len(seen), *itot, itot[0] / max(1, itot[0] + itot[1])))
print("  instance sections with any CW triangle (section, (agree, disagree, zc, zn, dg)):", worst[:12])

# the same count but summed over placements, rotating fn and vn by each instance's R (as a renderer would see them)
ptot = [0, 0, 0, 0, 0]
for k, (S, R, T, si) in enumerate(inst):
    s = section(si)
    if s["vb"] < 0 or vbinfo[s["vb"]][0] == 0: continue
    if k < 3 or dets[k] < 0:
        P, N = decode_vertices(s, si)
        r = measure(si, P, N, R)
        print("  placement %3d section %3d det %+.3f: agree %d disagree %d (positions and normals both rotated by R)" % (k, si, dets[k], r[0], r[1]))

print("\nHaloExport-style figure over clusters + instance sections: %.4f" %
      ((tot[0] + itot[0]) / max(1, tot[0] + itot[0] + tot[1] + itot[1])))

# ---- optional: --detail <section>  prints every CW triangle of that section with its normals
if "--detail" in sys.argv:
    si = int(sys.argv[sys.argv.index("--detail") + 1])
    s = section(si); P, N = decode_vertices(s, si)
    users = [k for k, (_, _, _, s_) in enumerate(inst) if s_ == si]
    print("\n--- detail of section %d (used by %d placements %s)" % (si, len(users), users[:6]))
    import math
    for shader, a, b, c in triangles(s):
        pa, pb, pc = P[a], P[b], P[c]
        e1 = (pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]); e2 = (pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2])
        fn = (e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0])
        L = math.sqrt(sum(x * x for x in fn)) or 1.0
        fnu = tuple(x / L for x in fn)
        dots = [sum(fnu[k] * N[v][k] for k in range(3)) for v in (a, b, c)]
        if sum(dots) < 0:
            sref = tagref(shp + 36 * shader); sname = tags[tagidx(sref[1])][1] if tagidx(sref[1]) >= 0 else None
            print("  tri (%d,%d,%d) shader %d %s area %.2e" % (a, b, c, shader, sname, L / 2))
            print("     p0 %s p1 %s p2 %s" % tuple([round(x, 3) for x in p] for p in (pa, pb, pc)))
            print("     face normal %s ; vertex normals %s dots %s" % ([round(x, 3) for x in fnu], [[round(x, 3) for x in N[v]] for v in (a, b, c)], [round(x, 3) for x in dots]))
