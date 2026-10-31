# Read-only: an object's damage sections (hlmt +0x88 -> +0x68) and its
# collision model (hlmt +0x10 coll: regions -> permutations -> bsps), as in
# docs/format/halo-script.md 7 and 8.
# Run:  python -I probe_plate_damage.py [map] [scen tag path]
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "120_halo")
path = sys.argv[2] if len(sys.argv) > 2 else "objects\\levels\\solo\\120_halo\\plates\\plate_flat\\plate_flat"
i = m.find("scen", path)
o = m.meta(i)
h = m.ref(o + 0x34)
ho = m.meta(h)
print("scen", path, "\n  hlmt", m.names[h])
print("  render", m.refname(ho), "| coll", m.refname(ho + 0x10), "| jmad", m.refname(ho + 0x20),
      "| phmo", m.refname(ho + 0x30))

n, info = m.block(ho + 0x88)
print("damage info elements:", n)
for sec in m.elements(info + 0x68, 44):
    print("  section", m.sid_at(sec), "flags", hex(m.u32(sec + 4)), "vitality", m.f32(sec + 8))
    for r in m.elements(sec + 0x0C, 0x88):
        print("    response type", m.u16(r), "effect", m.refname(r + 0x10), "| second", m.refname(r + 0x20),
              "| markers", m.sid_at(r + 0x58), m.sid_at(r + 0x5C), "| +0x60", m.f32(r + 0x60))

c = m.ref(ho + 0x10)
if c is not None:
    co = m.meta(c)
    for reg in m.elements(co + 0x20, 16):
        for perm in m.elements(reg + 4, 40):
            for b in m.elements(perm + 4, 100):
                g = b + 4                               # the 96-byte collision geometry
                counts = [m.block(g + off)[0] for off in (0x00, 0x0C, 0x18, 0x24, 0x30, 0x3C, 0x48, 0x54)]
                print("  coll region", m.sid_at(reg), "perm", m.sid_at(perm), "node", m.i16(b),
                      "| nodes planes leaves 2d-refs 2d-nodes surfaces edges vertices =", counts)
                verts = [m.f32n(v, 3) for v in m.elements(g + 0x54, 16)]
                lo = [round(min(v[k] for v in verts), 3) for k in range(3)]
                hi = [round(max(v[k] for v in verts), 3) for k in range(3)]
                print("    box", lo, hi)
