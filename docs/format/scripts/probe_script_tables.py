# Read-only: the scenario tables scripts name things in (docs/format/halo-script.md 5, 6):
# cutscene flags (scnr +0x428, 28 bytes), point sets (scnr +0x41C -> +0x00, 52 bytes; points 60),
# editor folders (scnr +0x614, 260 bytes), and the flags / folder fields of scenery placements.
# Run:  python -I probe_script_tables.py [map]
import os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "120_halo")
s = m.meta(m.find("scnr"))
flags = m.elements(s + 0x428, 28)
print(len(flags), "cutscene flags; first:")
for e in flags[:5]:
    print("  ", m.sid_at(e), m.f32n(e + 4, 3), "facing", m.f32n(e + 16, 2))
n, data = m.block(s + 0x41C)
sets = m.elements(data, 52) if data else []
print(len(sets), "point sets; first:")
for e in sets[:4]:
    pts = m.elements(e + 32, 60)
    print("  ", m.cstr(e, 32), "bsp", m.i16(e + 44), len(pts), "points",
          [(m.cstr(p, 32), [round(x, 1) for x in m.f32n(p + 0x20, 3)], m.i16(p + 0x2E)) for p in pts[:2]])
folders = [(m.i32(o), m.cstr(o + 4, 256)) for o in m.elements(s + 0x614, 260)]
print(len(folders), "folders:", [f[1] for f in folders[:14]])
sc = m.elements(s + 0xB4, 180)
print("scenery (flags +0x04, folder +0x42) counts:",
      sorted(collections.Counter((m.u32(o + 4), m.i16(o + 0x42)) for o in sc).items()))
