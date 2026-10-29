# Read-only probe of an MCC Halo 2 scenario (docs/format/halo2.md 7): skies,
# scenery / crate / machine / control placements and the render models behind
# them, player starting locations (the multiplayer spawns), netgame flags and
# trigger volumes with the kill list.
# Run: python -I probe_h2_scenario.py [map]
import sys, os, collections
sys.path.insert(0, os.path.dirname(__file__))
from h2_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "coagulation")
sc = m.find("scnr")
a = m.addr(sc["meta"])
print("scenario", sc["name"])
print("skies", [m.tag_name(m.ref(o)) for o in m.elements(a + 0x8, 8)])


def render_model(obj_id):
    """object -> hlmt (object +0x34) -> mode (hlmt +0)."""
    mm, o = m.meta(obj_id)
    if o is None:
        return None, None
    hl = mm.ref(o + 0x34)
    mm2, h = m.meta(hl)
    if h is None:
        return m.tag_name(hl), None
    return m.tag_name(hl), mm2.ref(h)


def mode_info(mid):
    mm, o = m.meta(mid)
    if o is None:
        return "no data"
    secs = mm.elements(o + 36, 92)
    regions = mm.elements(o + 28, 16)
    tris = []
    for s in secs:
        tris.append((mm.u16(s), mm.u16(s + 4), mm.u16(s + 6), hex(mm.u32(s + 56)), mm.u32(s + 60), mm.u32(s + 68), mm.block(s + 72)[0]))
    perms = []
    for r in regions:
        for p in mm.elements(r + 8, 16):
            perms.append((mm.string(mm.u32(r)), mm.string(mm.u32(p)), [mm.i16(p + 4 + 2 * k) for k in range(6)]))
    return "%s regions %d sections %s perms %s shaders %d nodes %d" % (
        "shared" if mm is not m else "local", len(regions), tris[:4], perms[:3], mm.block(o + 96)[0], mm.block(o + 72)[0])


for title, off, size, pal in [("scenery", 0x50, 0x5C, 0x58), ("crates", 0x328, 0x4C, 0x330), ("machines", 0xA8, 0x48, 0xB0), ("controls", 0xB8, 0x44, 0xC0)]:
    palette = [m.ref(p) for p in m.elements(a + pal, 0x28)]
    items = m.elements(a + off, size)
    print("%s: %d placed, palette %d" % (title, len(items), len(palette)))
    for e in items:
        k = m.i16(e)
        name = m.tag_name(palette[k]) if 0 <= k < len(palette) else "?"
        print("  %-60s pos %s rot %s scale %.3g bsp %d" % (name, [round(x, 2) for x in m.vec(e + 8)],
              [round(x, 3) for x in m.vec(e + 0x14)], m.f32(e + 0x20), m.i16(e + 0x2C)))
    for p in palette:
        hl, md = render_model(p)
        print("   palette %s -> %s -> %s: %s" % (m.tag_name(p), hl, m.tag_name(md) if md else None, mode_info(md) if md else ""))

starts = m.elements(a + 0x100, 0x34)
print("player starting locations", len(starts))
kinds = collections.Counter()
for e in starts:
    kinds[(m.u16(e + 0x14), m.u16(e + 0x1C))] += 1
for e in starts[:6]:
    print("  pos %s facing %.1f team %d bsp %d game types %s spawn types %s" % ([round(x, 2) for x in m.vec(e)], m.f32(e + 0xC),
          m.u16(e + 0x10), m.i16(e + 0x12), [m.u16(e + 0x14 + 2 * k) for k in range(4)], [m.u16(e + 0x1C + 2 * k) for k in range(4)]))
print("  (game type 1, spawn type 0) counts", dict(kinds))
zs = [m.vec(e)[2] for e in starts]
print("  z range", min(zs), max(zs))
flags = m.elements(a + 0x118, 0x20)
fk = collections.Counter(m.u16(e + 0x10) for e in flags)
print("netgame flags", len(flags), dict(fk))
vols = m.elements(a + 0x108, 0x44)
print("trigger volumes", len(vols))
for e in vols:
    print("  %-30s fwd %s up %s pos %s ext %s" % (m.string(m.u32(e)), [round(x, 2) for x in m.vec(e + 0xC)], [round(x, 2) for x in m.vec(e + 0x18)],
          [round(x, 2) for x in m.vec(e + 0x24)], [round(x, 2) for x in m.vec(e + 0x30)]))
print("kill triggers", [m.i16(e) for e in m.elements(a + 0x230, 2)])
