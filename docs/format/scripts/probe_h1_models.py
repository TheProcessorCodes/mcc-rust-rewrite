# gbxmodel (mod2) geometry (docs/format/halo1.md 9): regions, permutations,
# parts, the model vertex and index areas, the strip winding against the
# vertex normals, how many vertices use nodes other than the first.
# python -I probe_h1_models.py [map]
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "bloodgulch")
indices_at = m.model_data + m.vertex_size
print(f"model data at {m.model_data:#x}: vertices {m.vertex_size:#x} bytes, indices after them, {m.model_size:#x} in all")
total = agree = 0
for t in m.find("mod2"):
    o = m.data(t["id"])
    nodes = m.block(o + 0xB8, 0x9C)
    geoms = m.block(o + 0xD0, 0x30)
    regions = m.block(o + 0xC4, 0x4C)
    other_node = verts = 0
    for g in geoms:
        for p in m.block(g + 0x24, 0x84):
            ni, ioff = m.u32(p + 0x48), m.u32(p + 0x4C)
            nv, voff = m.u32(p + 0x58), m.u32(p + 0x64)
            V = m.model_data + voff
            strip = [i for i in struct.unpack_from("<%dH" % (ni + 2), m.d, indices_at + ioff) if i != 0xFFFF]
            assert max(strip) < nv, (t["name"], max(strip), nv)
            for k in range(nv):
                n0, n1, w0, w1 = struct.unpack_from("<2h2f", m.d, V + 68 * k + 56)
                verts += 1
                if n0 != 0 or (w1 > 0 and n1 != 0):
                    other_node += 1
            for k in range(len(strip) - 2):
                a, b, c = strip[k:k + 3]
                if a == b or b == c or a == c:
                    continue
                if k % 2:
                    b, c = c, b
                P = [struct.unpack_from("<3f", m.d, V + 68 * i) for i in (a, b, c)]
                N = [struct.unpack_from("<3f", m.d, V + 68 * i + 12) for i in (a, b, c)]
                ab = [P[1][j] - P[0][j] for j in range(3)]
                ac = [P[2][j] - P[0][j] for j in range(3)]
                cr = (ab[1] * ac[2] - ab[2] * ac[1], ab[2] * ac[0] - ab[0] * ac[2], ab[0] * ac[1] - ab[1] * ac[0])
                total += 1
                agree += sum(cr[j] * sum(n[j] for n in N) for j in range(3)) > 0
    if len(nodes) > 1 and other_node:
        print(f"  {t['name']}: {len(nodes)} nodes, {other_node} of {verts} vertices on another node; "
              f"flags {m.u32(o):#x}; node 1 at {[round(x, 3) for x in m.vec(nodes[1] + 0x28)]}")
print(f"strip triangles (even k: a,b,c; odd: a,c,b) counter-clockwise against normals: {agree}/{total} = {agree / max(total, 1):.4f}")
