# Structure BSP render geometry (docs/format/halo1.md 5, 6): the BSP chunk
# header, lightmaps and their materials, the shared vertex buffer, the
# surfaces' winding against the vertex normals, lightmap texcoords.
# python -I probe_h1_sbsp.py [map]
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "bloodgulch")
for bi, b in enumerate(m.bsps()):
    s = b.sbsp
    vsize, vstart = m.u32(b.start + 4), m.u32(b.start + 8)
    print(f"bsp {bi} {m.tag(b.id)['name']}: chunk {b.start:#x}+{b.size:#x} at {b.base:#x}; "
          f"vertices {vsize:#x} bytes at file {vstart:#x}; lightmap bitmap {m.refname(s)}")
    surf = b.block(s + 0xF8, 6)
    lms = b.block(s + 0x104, 0x20)
    tris = agree = 0
    lmuv = [9, 9, -9, -9]
    no_lm = 0
    for l in lms:
        page = m.i16(l)
        for e in b.block(l + 0x14, 0x100):
            first, count = m.i32(e + 0x14), m.i32(e + 0x18)
            nv, voff = m.u32(e + 0xB4), m.u32(e + 0xB8)
            nl, loff = m.u32(e + 0xC8), m.u32(e + 0xCC)
            assert nl in (0, nv), (nl, nv)
            if page < 0 or nl == 0:
                no_lm += count
            V = lambda k: vstart + voff + 56 * k
            for t in surf[first:first + count]:
                i = struct.unpack_from("<3H", m.d, t)
                p = [m.vec(V(k)) for k in i]
                n = [m.vec(V(k) + 12) for k in i]
                ab = [p[1][j] - p[0][j] for j in range(3)]
                ac = [p[2][j] - p[0][j] for j in range(3)]
                cr = (ab[1] * ac[2] - ab[2] * ac[1], ab[2] * ac[0] - ab[0] * ac[2], ab[0] * ac[1] - ab[1] * ac[0])
                nn = [sum(x[j] for x in n) for j in range(3)]
                tris += 1
                if sum(cr[j] * nn[j] for j in range(3)) > 0:
                    agree += 1
            for k in range(nl):
                u, v = m.vec(vstart + loff + 20 * k + 12, 2)
                lmuv = [min(lmuv[0], u), min(lmuv[1], v), max(lmuv[2], u), max(lmuv[3], v)]
    print(f"  {len(lms)} lightmaps, {tris} triangles of {len(surf)} surfaces; "
          f"counter-clockwise (a,b,c) agrees with normals: {agree / max(tris, 1):.4f}; "
          f"(a,c,b) agrees: {1 - agree / max(tris, 1):.4f}")
    print(f"  triangles without a lightmap page: {no_lm}; lightmap texcoords u {lmuv[0]:.3f}..{lmuv[2]:.3f} v {lmuv[1]:.3f}..{lmuv[3]:.3f}")
    print(f"  clusters {m.u32(s + 0x134)}, collision materials {m.u32(s + 0xA4)}, collision bsps {m.u32(s + 0xB0)}")
    # Clusters (+0x134, 0x68 each): the sky index (i16 at +0, -1: none);
    # the size is checked by the blocks inside each cluster (+0x28, +0x34,
    # +0x44, +0x50, +0x5C portals) pointing into the chunk.
    clusters = b.block(s + 0x134, 0x68)
    inside = all(m.u32(c + o) == 0 or b.base <= m.u32(c + o + 4) < b.base + b.size
                 for c in clusters for o in (0x28, 0x34, 0x44, 0x50, 0x5C))
    skies = {}
    for c in clusters:
        skies[m.i16(c)] = skies.get(m.i16(c), 0) + 1
    print(f"  clusters by sky index {skies}; inner blocks inside the chunk: {inside}")
