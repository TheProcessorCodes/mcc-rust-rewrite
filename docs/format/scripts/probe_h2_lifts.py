# Read-only probe of MCC Halo 2 gravity lifts and other pushing volumes
# (docs/format/halo2.md 11): placed objects whose model (hlmt +0x20) names a
# physics model (phmo) with phantom types, the shapes that use them, and the
# render model markers that aim the push.
# Run: python -I probe_h2_lifts.py [map ...]   (default lockout; "all" scans
# every map in the folder)
import sys, os, math
sys.path.insert(0, os.path.dirname(__file__))
from h2_common import Map, MAPS

# (title, placements, element size, palette) in the scenario (halo2.md 4).
LISTS = [("scenery", 0x50, 0x5C, 0x58), ("machines", 0xA8, 0x48, 0xB0),
         ("controls", 0xB8, 0x44, 0xC0), ("crates", 0x328, 0x4C, 0x330)]
# phmo shape blocks: (name, offset, element size); phantom type i8 at +0x1E.
SHAPES = [("spheres", 0x48, 0xA0), ("pills", 0x58, 0x60), ("boxes", 0x60, 0xB0),
          ("polyhedra", 0x70, 0x120)]


def quat_x(q):
    """The marker's forward (+X) turned by quaternion (i, j, k, w)."""
    x, y, z, w = q
    return (1 - 2 * (y * y + z * z), 2 * (x * y + w * z), 2 * (x * z - w * y))


def probe(m):
    sc = m.find("scnr")
    a = m.addr(sc["meta"])
    found = 0
    for title, off, size, pal in LISTS:
        palette = [m.ref(p) for p in m.elements(a + pal, 0x28)]
        for k, e in enumerate(m.elements(a + off, size)):
            pi = m.i16(e)
            if not 0 <= pi < len(palette):
                continue
            obj = palette[pi]
            mo, o = m.meta(obj)
            if o is None:
                continue
            mh, h = m.meta(mo.ref(o + 0x34))
            if h is None:
                continue
            mp, p = m.meta(mh.ref(h + 0x20))
            if p is None:
                continue
            types = mp.elements(p + 0x28, 0x68)
            if not types:
                continue
            found += 1
            print("%s %d %s at %s angles %s" % (title, k, m.tag_name(obj), [round(x, 3) for x in m.vec(e + 8)],
                                                [round(x, 3) for x in m.vec(e + 0x14)]))
            for t, te in enumerate(types):
                print("  phantom type %d: flags %#x marker %r dead radius %g center %g/%g axis %g/%g direction %g/%g" % (
                    t, mp.u32(te), mp.string(mp.u32(te + 8)), mp.f32(te + 0x1C), mp.f32(te + 0x20), mp.f32(te + 0x24),
                    mp.f32(te + 0x28), mp.f32(te + 0x2C), mp.f32(te + 0x30), mp.f32(te + 0x34)))
            for name, so, ss in SHAPES:
                for i, s in enumerate(mp.elements(p + so, ss)):
                    ph = mp.i8(s + 0x1E)
                    if ph < 0:
                        continue
                    line = "  %s %d %r phantom %d" % (name, i, mp.string(mp.u32(s)), ph)
                    if name == "polyhedra":
                        line += " box half %s centre %s, %d vertices" % (
                            [round(x, 3) for x in mp.vec(s + 0x40)], [round(x, 3) for x in mp.vec(s + 0x50)], mp.i32(s + 0x100))
                    elif name == "pills":
                        line += " radius %g from %s to %s" % (
                            mp.f32(s + 0x38), [round(x, 3) for x in mp.vec(s + 0x40)], [round(x, 3) for x in mp.vec(s + 0x50)])
                    elif name == "boxes":
                        line += " half %s rows %s at %s" % (
                            [round(x, 3) for x in mp.vec(s + 0x40)],
                            [[round(x, 3) for x in mp.vec(s + r)] for r in (0x70, 0x80, 0x90)],
                            [round(x, 3) for x in mp.vec(s + 0xA0)])
                    elif name == "spheres":
                        line += " radius %g at %s" % (mp.f32(s + 0x38), [round(x, 3) for x in mp.vec(s + 0x90)])
                    print(line)
            # Marker groups in the render model (mode +0x58, 12 bytes; markers
            # 36 bytes: node i8 +2, translation +4, rotation i j k w +0x10).
            mr, r = m.meta(mh.ref(h))
            if r is None:
                continue
            for g in mr.elements(r + 0x58, 12):
                for mk in mr.elements(g + 4, 36):
                    q = mr.vec(mk + 0x10, 4)
                    print("  marker %r node %d at %s forward %s" % (mr.string(mr.u32(g)), mr.i8(mk + 2),
                          [round(x, 3) for x in mr.vec(mk + 4)], [round(x, 3) for x in quat_x(q)]))
    return found


names = sys.argv[1:] or ["lockout"]
if names == ["all"]:
    names = sorted(f[:-4] for f in os.listdir(MAPS) if f.endswith(".map") and f not in ("shared.map", "single_player_shared.map", "mainmenu.map"))
for n in names:
    print("==", n)
    try:
        print("  %d objects with phantoms" % probe(Map(n)))
    except Exception as e:
        print("  failed:", e)
