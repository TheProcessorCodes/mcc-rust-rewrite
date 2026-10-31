# The structure BSP's collision BSP (docs/format/halo1.md 7): block sizes,
# the winged-edge walk of each surface, and its winding against the
# surface's plane.
# python -I probe_h1_collision.py [map]
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "bloodgulch")
for b in m.bsps():
    s = b.sbsp
    mats = b.block(s + 0xA4, 0x14)
    print("collision materials:", [(m.refname(e), m.u16(e + 0x12)) for e in mats])
    for c in b.block(s + 0xB0, 0x60):
        sizes = [12, 16, 8, 8, 20, 12, 24, 16]
        names = ["3d nodes", "planes", "leaves", "2d refs", "2d nodes", "surfaces", "edges", "vertices"]
        bl = {n: b.block(c + 12 * k, z) for k, (n, z) in enumerate(zip(names, sizes))}
        print({n: len(v) for n, v in bl.items()})
        planes, surfaces, edges, verts = bl["planes"], bl["surfaces"], bl["edges"], bl["vertices"]
        E = lambda e: struct.unpack_from("<6i", m.d, edges[e])
        tris = corrupt = agree = flipped = 0
        flags = {}
        for si, so in enumerate(surfaces):
            plane, first, fl, brk, mat = m.i32(so), m.i32(so + 4), m.u8(so + 8), m.i8(so + 9), m.i16(so + 10)
            flags[fl] = flags.get(fl, 0) + 1
            pts, e = [], first
            for _ in range(len(edges)):
                a, z, fwd, rev, left, right = E(e)
                if left == si:
                    pts.append(m.vec(verts[a])); e = fwd
                elif right == si:
                    pts.append(m.vec(verts[z])); e = rev
                else:
                    corrupt += 1; pts = []; break
                if e == first:
                    break
            if len(pts) < 3:
                continue
            n = m.vec(planes[plane & 0x7FFFFFFF])
            if plane < 0:
                flipped += 1
                n = [-x for x in n]
            for k in range(1, len(pts) - 1):
                p0, p1, p2 = pts[0], pts[k], pts[k + 1]
                ab = [p1[j] - p0[j] for j in range(3)]
                ac = [p2[j] - p0[j] for j in range(3)]
                cr = (ab[1] * ac[2] - ab[2] * ac[1], ab[2] * ac[0] - ab[0] * ac[2], ab[0] * ac[1] - ab[1] * ac[0])
                tris += 1
                agree += sum(cr[j] * n[j] for j in range(3)) > 0
        print(f"  {tris} triangles, {corrupt} corrupt loops, {flipped} surfaces on a flipped plane; "
              f"fan winding agrees with the plane: {agree / max(tris, 1):.4f}; surface flags {flags}")
