# Read-only probe of an MCC Halo 2 multiplayer map's weapons
# (docs/format/halo2.md 11): the scenario's netgame equipment (+0x120) and
# starting equipment (+0x128), the item collections (`itmc`) they name, and
# each weapon's numbers (`weap`), its render model's nodes and markers.
# Run: python -I probe_h2_weapons.py [map]
import sys, os, collections
sys.path.insert(0, os.path.dirname(__file__))
from h2_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "lockout")
sc = m.find("scnr")
a = m.addr(sc["meta"])


def tag(mm, o):
    """The datum of the tag reference at o, if it names a tag."""
    t = mm.u32(o + 4)
    if t == 0xFFFFFFFF or (t & 0xFFFF) >= len(m.tags):
        return None
    return t if m.tags[t & 0xFFFF]["id"] == t else None


def collection_item(itmc):
    """itmc +0x00: permutations (0x10: weight f32, item reference +0x04, variant sid +0x0C)."""
    mm, o = m.meta(itmc)
    if o is None:
        return None
    perms = mm.elements(o, 0x10)
    return tag(mm, perms[0] + 4) if perms else None


print("scenario", sc["name"])
classes = collections.Counter()
for k, e in enumerate(m.elements(a + 0x120, 0x90)):
    itmc = tag(m, e + 0x58)
    item = collection_item(itmc) if itmc else None
    cls = m.tags[item & 0xFFFF]["cls"] if item else "-"
    classes[cls] += 1
    print("%2d %-50s %-4s class %d respawn %3d s flag %d at %s ypr %s" % (
        k, m.tags[itmc & 0xFFFF]["name"] if itmc else "(none)", cls, m.u8(e + 0x14), m.i16(e + 0xE), m.i16(e + 0x12),
        [round(x, 2) for x in m.vec(e + 0x40)], [round(x, 3) for x in m.vec(e + 0x4C)]))
print("netgame equipment by item class:", dict(classes))
for e in m.elements(a + 0x128, 0x9C):
    print("starting equipment:", [m.tags[t & 0xFFFF]["name"] for t in (tag(m, e + 0x3C + 8 * j) for j in range(6)) if t])


def weapon(datum):
    mm, o = m.meta(datum)
    name = m.tags[datum & 0xFFFF]["name"]
    zoom = (mm.i16(o + 0x1FE), mm.f32(o + 0x200), mm.f32(o + 0x204))
    mags = [(mm.i16(e + 6), mm.i16(e + 8), mm.i16(e + 0xA), mm.i16(e + 0x14)) for e in mm.elements(o + 0x2C0, 0x5C)]
    trig = [(mm.i16(e + 6), mm.i16(e + 8), mm.i16(e + 0xA)) for e in mm.elements(o + 0x2C8, 0x40)]
    bars = []
    for e in mm.elements(o + 0x2D0, 0xEC):
        p = tag(mm, e + 0x8C)
        proj = None
        if p:
            pm, po = mm.meta(p)
            proj = (round(pm.f32(po + 0x16C), 1), round(pm.f32(po + 0x17C), 1), round(pm.f32(po + 0x180), 1))
        bars.append(dict(rps=(round(mm.f32(e + 4), 2), round(mm.f32(e + 8), 2)), shots=(mm.i16(e + 0x1C), mm.i16(e + 0x1E)),
                         recovery=round(mm.f32(e + 0x20), 3), mag=mm.i16(e + 0x28), per_shot=mm.i16(e + 0x2A),
                         error=(round(mm.f32(e + 0x70), 4), round(mm.f32(e + 0x74), 4), round(mm.f32(e + 0x78), 4)),
                         proj=proj))
    # object +0x34 -> hlmt -> mode (+0x00)
    hm, ho = mm.meta(tag(mm, o + 0x34))
    mode = tag(hm, ho)
    md, mo = hm.meta(mode)
    nodes = []
    for e in md.elements(mo + 0x48, 0x60):
        nodes.append((md.string(md.u32(e)), md.i16(e + 4), [round(x, 3) for x in md.vec(e + 0xC)], [round(x, 3) for x in md.vec(e + 0x18, 4)]))
    markers = []
    for g in md.elements(mo + 0x58, 0xC):
        for k in md.elements(g + 4, 0x24):
            markers.append((md.string(md.u32(g)), md.i16(k + 2), [round(x, 3) for x in md.vec(k + 4)]))
    print(name)
    print("   zoom", zoom, "magazines", mags, "triggers", trig)
    for b in bars:
        print("   barrel", b)
    print("   nodes", nodes)
    print("   markers", [(n, nd, p) for n, nd, p in markers if n in ("primary_trigger", "left_hand", "right_hand")])


seen = set()
for e in m.elements(a + 0x120, 0x90):
    itmc = tag(m, e + 0x58)
    item = collection_item(itmc) if itmc else None
    if item and m.tags[item & 0xFFFF]["cls"] == "weap" and item not in seen:
        seen.add(item)
        weapon(item)
