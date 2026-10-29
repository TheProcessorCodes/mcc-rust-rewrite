# Read-only probe of an MCC Halo 2 structure BSP's collision (docs/format/halo2.md 6):
# the BSP's own collision BSP and each instanced geometry definition's, walked
# as winged-edge loops into triangles. Prints counts, flags and bounds.
# Run: python -I probe_h2_collision.py [map]
import sys, collections
sys.path.insert(0, __import__("os").path.dirname(__file__))
from h2_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "coagulation")


def walk(at):
    """at: offset of the nine collision blocks (3D nodes .. vertices)."""
    surfaces = m.elements(at + 0x28, 8)
    edges = m.elements(at + 0x30, 12)
    verts = m.elements(at + 0x38, 16)
    tris, corrupt, flags = [], 0, collections.Counter()
    for s, so in enumerate(surfaces):
        first, fl, mat = m.u16(so + 2), m.u8(so + 4), m.i16(so + 6)
        flags[fl] += 1
        loop, e = [], first
        for _ in range(len(edges)):
            eo = edges[e]
            start, end, fwd, rev, left, right = [m.u16(eo + 2 * k) for k in range(6)]
            if left == s:
                v, nxt = start, fwd
            elif right == s:
                v, nxt = end, rev
            else:
                corrupt += 1; loop = []; break
            loop.append(m.vec(verts[v]))
            if nxt == first:
                break
            e = nxt
        for k in range(1, len(loop) - 1):
            tris.append((loop[0], loop[k], loop[k + 1], fl, mat))
    return dict(nodes=m.block(at)[0], planes=m.block(at + 8)[0], leaves=m.block(at + 0x10)[0],
                surfaces=len(surfaces), edges=len(edges), vertices=len(verts), triangles=tris,
                corrupt=corrupt, flags=flags)


t = m.find("sbsp")
b = m.addr(t["meta"])
mats = m.elements(b + 0xC, 0x14)
print("collision materials", len(mats))
for k, mo in enumerate(mats[:40]):
    print("  %2d %s  global %d" % (k, m.tag_name(m.ref(mo + 0xC)) if m.ref(mo + 0xC) != 0xFFFFFFFF else m.tag_name(m.ref(mo)), m.i16(mo + 8)))
for k, at in enumerate(m.elements(b + 0x14, 0x40)):
    r = walk(at)
    pts = [p for t3 in r["triangles"] for p in t3[:3]]
    lo = [min(p[i] for p in pts) for i in range(3)]
    hi = [max(p[i] for p in pts) for i in range(3)]
    print("bsp collision %d: %d nodes %d planes %d leaves %d surfaces %d edges %d vertices -> %d triangles, %d corrupt loops"
          % (k, r["nodes"], r["planes"], r["leaves"], r["surfaces"], r["edges"], r["vertices"], len(r["triangles"]), r["corrupt"]))
    print("  flags", dict(r["flags"]), "bounds", [round(x, 2) for x in lo], [round(x, 2) for x in hi])
    up = sum(1 for (a, b2, c, f, _) in r["triangles"]
             if ((b2[0]-a[0])*(c[1]-a[1]) - (b2[1]-a[1])*(c[0]-a[0])) > 0)
    print("  triangles whose normal has +Z:", up)
total = 0
defs = m.elements(b + 0x138, 0xC8)
per_def = []
for k, g in enumerate(defs):
    r = walk(g + 0x70)
    per_def.append(len(r["triangles"]))
    if r["corrupt"]:
        print("  def %d corrupt %d" % (k, r["corrupt"]))
insts = m.elements(b + 0x140, 0x58)
for io in insts:
    total += per_def[m.i16(io + 0x34)]
print("instanced definitions", len(defs), "triangles per definition", per_def)
print("instances", len(insts), "instanced collision triangles placed", total)
for io in insts[:4]:
    print("  inst %s def %d scale %.3f fwd %s pos %s flags %x" % (m.string(m.u32(io + 0x50)), m.i16(io + 0x34), m.f32(io),
          [round(x, 3) for x in m.vec(io + 4)], [round(x, 2) for x in m.vec(io + 0x28)], m.u16(io + 0x36)))
