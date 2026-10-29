# Read-only probe of MCC Halo 2 shader tags (docs/format/halo2.md 8): the
# template, the named parameters, and the runtime ("postprocess") bitmaps and
# vertex constants with the first pass's remapping.
# Run: python -I probe_h2_shaders.py [map] [name filter]
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
from h2_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "coagulation")
flt = sys.argv[2] if len(sys.argv) > 2 else ""
TYPES = {0: "bitmap", 1: "value", 2: "color", 3: "switch"}


def short_ref(mm, o):
    """A 4-byte tag reference: the datum only."""
    return mm.u32(o)


def template_passes(stem_id):
    mm, a = m.meta(stem_id)
    if a is None:
        return []
    out = []
    props = mm.elements(a + 88, 0x30)  # size not needed beyond first
    if not props:
        return out
    p = mm.addr(mm.u32(a + 88 + 4))
    passes = mm.elements(p + 16, 10)
    impls = mm.elements(p + 24, 6)
    remaps = mm.elements(p + 32, 4)
    for po in passes:
        pid = mm.ref(po)
        first_impl = mm.u16(po + 8) & 0xFF  # see doc: low byte index? printed raw too
        raw = mm.u16(po + 8)
        out.append((mm.tag_name(pid) if mm.tag_by_id(pid) else m.tag_name(pid), raw))
    return out, impls, remaps, mm


for t in m.tags:
    if t["cls"] != "shad" or t["id"] == 0xFFFFFFFF or flt not in t["name"]:
        continue
    mm, a = m.meta(t["id"])
    if a is None:
        print(t["name"], "(no data)"); continue
    tmpl = mm.ref(a)
    print("%s%s\n  template %s" % (t["name"], " [shared]" if mm is not m else "", m.tag_name(tmpl)))
    for po in mm.elements(a + 0x18, 0x28):
        ty = mm.u16(po + 4)
        bm = mm.ref(po + 8)
        print("    param %-24s %-6s %s value %.3g colour %s" % (mm.string(mm.u32(po)), TYPES.get(ty, ty),
              m.tag_name(bm) if bm != 0xFFFFFFFF else "-", mm.f32(po + 0x10), [round(x, 3) for x in mm.vec(po + 0x14)]))
    for pp in mm.elements(a + 0x20, 0x7C):
        bms = mm.elements(pp + 4, 0xC)
        vcs = mm.elements(pp + 0x14, 0x10)
        print("    runtime template %s" % m.tag_name(short_ref(mm, pp)))
        for k, bo in enumerate(bms):
            print("      bitmap %d %s" % (k, m.tag_name(short_ref(mm, bo))))
        print("      vertex constants", " | ".join(" ".join("%.3g" % x for x in mm.vec(vo, 4)) for vo in vcs))
        print("      pixel constants", [hex(mm.u32(x)) for x in mm.elements(pp + 0xC, 4)])
        print("      lods", [(hex(mm.u32(x)), mm.u16(x + 4)) for x in mm.elements(pp + 0x1C, 6)],
              "layers", [mm.u16(x) for x in mm.elements(pp + 0x24, 2)],
              "passes", [mm.u16(x) for x in mm.elements(pp + 0x2C, 2)],
              "impls", [tuple(mm.u16(x + 2 * j) for j in range(5)) for x in mm.elements(pp + 0x34, 0xA)])
