# Read-only probe of MCC Halo 2 shader templates (`stem`, docs/format/halo2.md 8):
# the named parameters per category and the runtime pass / implementation /
# remapping tables that say which runtime bitmap and vertex constant each pass reads.
# Run: python -I probe_h2_templates.py [map] [template name filter]
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
from h2_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "coagulation")
flt = sys.argv[2] if len(sys.argv) > 2 else "tex_bump"

for t in m.tags:
    if t["cls"] != "stem" or t["id"] == 0xFFFFFFFF or flt not in t["name"]:
        continue
    mm, a = m.meta(t["id"])
    if a is None:
        continue
    print(t["name"], "shared" if mm is not m else "local")
    print("  properties", [(mm.u16(p), mm.string(mm.u32(p + 4))) for p in mm.elements(a + 0x10, 8)])
    for c in mm.elements(a + 0x18, 0xC):
        print("  category", mm.string(mm.u32(c)))
        for p in mm.elements(c + 4, 0x34):
            bm = mm.ref(p + 0x10)
            print("    %-28s type %d flags %x default %s scale %.3g" % (mm.string(mm.u32(p)), mm.u16(p + 0xC), mm.u16(p + 0xE),
                  m.tag_name(bm) if bm != 0xFFFFFFFF else "-", mm.f32(p + 0x30)))
    pps = mm.elements(a + 0x58, 0x28)
    if not pps:
        continue
    pp = pps[0]
    passes = mm.elements(pp + 0x10, 0xA)
    impls = mm.elements(pp + 0x18, 6)
    rem = mm.elements(pp + 0x20, 4)
    print("  passes", [(m.tag_name(mm.ref(x)).split("\\")[-1], hex(mm.u16(x + 8))) for x in passes])
    print("  impls", [tuple(hex(mm.u16(x + 2 * j)) for j in range(3)) for x in impls][:8])
    print("  remaps", [mm.u8(x + 3) for x in rem][:60])
