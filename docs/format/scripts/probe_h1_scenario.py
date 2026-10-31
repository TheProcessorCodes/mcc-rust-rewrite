# Scenario blocks (docs/format/halo1.md 4): skies, object placements and
# palettes, player starting locations, netgame flags and equipment,
# trigger volumes; each object's model reference.
# python -I probe_h1_scenario.py [map]
import math, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "bloodgulch")
sc = m.data(m.scenario_id)
print("skies", [m.refname(e) for e in m.block(sc + 0x30, 0x10)])
# (name, placements, element size, palette)
lists = [("scenery", 0x210, 0x48, 0x21C), ("bipeds", 0x228, 0x78, 0x234), ("vehicles", 0x240, 0x78, 0x24C),
         ("equipment", 0x258, 0x28, 0x264), ("weapons", 0x270, 0x5C, 0x27C), ("machines", 0x294, 0x40, 0x2A0),
         ("controls", 0x2AC, 0x40, 0x2B8), ("light fixtures", 0x2C4, 0x58, 0x2D0), ("sound scenery", 0x2DC, 0x28, 0x2E8)]
for name, at, size, pal in lists:
    palette = [m.ref(e) for e in m.block(sc + pal, 0x30)]
    items = m.block(sc + at, size)
    print(f"{name}: {len(items)} placed from a palette of {len(palette)}")
    for e in items[:2]:
        k = m.i16(e)
        tid = palette[k] if 0 <= k < len(palette) else None
        t = m.tag(tid) if tid is not None else None
        model = None
        if t:
            o = m.data(tid)
            model = m.refname(o + 0x28)
        print("   ", t and t["name"], t and t["cls"], "pos", [round(x, 2) for x in m.vec(e + 8)],
              "ypr", [round(x, 3) for x in m.vec(e + 0x14)], "model", model)
starts = m.block(sc + 0x354, 0x34)
print(f"player starting locations: {len(starts)}")
for e in starts[:4]:
    print("   pos", [round(x, 2) for x in m.vec(e)], "facing", round(m.f32(e + 0xC), 3), "team", m.i16(e + 0x10),
          "bsp", m.i16(e + 0x12), "types", struct.unpack_from("<4H", m.d, e + 0x14))
flags = m.block(sc + 0x378, 0x94)
print(f"netgame flags: {len(flags)}", sorted({m.u16(e + 0x10) for e in flags}))
for e in flags[:3]:
    print("   pos", [round(x, 2) for x in m.vec(e)], "facing", round(m.f32(e + 0xC), 3), "type", m.u16(e + 0x10), "team", m.u16(e + 0x12))
equip = m.block(sc + 0x384, 0x90)
print(f"netgame equipment: {len(equip)}")
for e in equip[:4]:
    print("   types", struct.unpack_from("<4H", m.d, e + 4), "pos", [round(x, 2) for x in m.vec(e + 0x40)],
          "facing", round(m.f32(e + 0x4C), 3), "collection", m.refname(e + 0x50))
start_eq = m.block(sc + 0x390, 0xCC)
print(f"starting equipment: {len(start_eq)}", [[m.refname(e + 0x3C + 16 * k) for k in range(6)] for e in start_eq[:2]])
tv = m.block(sc + 0x360, 0x60)
print(f"trigger volumes: {len(tv)}", [m.cstr(e + 4) for e in tv[:8]])
for e in tv[:2]:
    print("   ", [round(x, 3) for x in m.vec(e + 0x24, 15)])
bsps = m.block(sc + 0x5A4, 0x20)
print(f"structure bsps: {len(bsps)}")
