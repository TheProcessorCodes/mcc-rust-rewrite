# Halo CE effects (docs/format/halo1.md 13): an `effe` and everything it
# reaches -- events, parts (sounds, lights, damage, debris, child effects),
# particles (`part`), particle systems (`pctl`), lights (`ligh`), particle
# physics (`pphy`) and the bitmaps' sequences.
# python -I probe_h1_effects.py [map] [effect path | --list | --census | --hex PATH [SIZE]]
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map

args = sys.argv[1:]
m = Map(args[0] if args else "d40")
rest = args[1:]


def hd(o, n, indent=""):
    for k in range(0, n, 16):
        w = struct.unpack_from("<4I", m.d, o + k)
        f = struct.unpack_from("<4f", m.d, o + k)
        print(indent + "%04x" % k, " ".join("%08x" % x for x in w), "  ", " ".join("%10.4g" % x for x in f))


def name(tid):
    t = m.tag(tid) if (tid & 0xFFFF) < len(m.tags) else None
    return "%s %s" % (t["cls"], t["name"]) if t else "-"


def ref(o):
    return m.ref(o)


def bounds(o):
    return "%.3g..%.3g" % m.vec(o, 2)


def colour(o):
    a, r, g, b = m.vec(o, 4)
    return "argb(%.2f %.2f %.2f %.2f)" % (a, r, g, b)


FUNCTIONS = ["linear", "early", "very early", "late", "very late", "cosine"]


def light(tid, ind):
    o = m.data(tid)
    print(ind + "ligh flags %#x radius %.3g radius modifier %s falloff angle %.3g cutoff angle %.3g" % (
        m.u32(o), m.f32(o + 4), bounds(o + 8), m.f32(o + 0x10), m.f32(o + 0x14)))
    print(ind + "  colour lower %s upper %s (interpolation flags %#x)" % (colour(o + 0x38), colour(o + 0x48), m.u32(o + 0x34)))
    print(ind + "  gel map %s" % name(ref(o + 0x64)))
    print(ind + "  lens flare %s" % name(ref(o + 0xAC)))
    print(ind + "  duration %.3g ticks, falloff function %s" % (
        m.f32(o + 0xF4), FUNCTIONS[m.u16(o + 0xFA)] if m.u16(o + 0xFA) < 6 else m.u16(o + 0xFA)))


def bitmap_sequences(tid, ind):
    o = m.data(tid)
    imgs = m.block(o + 0x60, 0x30)
    seqs = m.block(o + 0x54, 0x40)
    fmt = [m.u16(i + 0xC) for i in imgs]
    sizes = [(m.u16(i + 4), m.u16(i + 6)) for i in imgs]
    print(ind + "bitm %d images %s formats %s; type %d; %d sequences" % (
        len(imgs), sorted(set(sizes))[:4], sorted(set(fmt)), m.u16(o), len(seqs)))
    for k, s in enumerate(seqs[:6]):
        sprites = m.block(s + 0x34, 0x20)
        sp = [(m.i16(p), m.vec(p + 8, 4)) for p in sprites[:2]]
        print(ind + "  seq %d %r first bitmap %d count %d sprites %d %s" % (
            k, m.cstr(s), m.i16(s + 0x20), m.i16(s + 0x22), len(sprites), sp))


def physics(tid, ind):
    o = m.data(tid)
    print(ind + "pphy %s: flags %#x density %.4g air friction %.4g water friction %.4g surface friction %.4g elasticity %.4g;"
          " mass %.4g water buoyancy %.4g air buoyancy %.4g" % (
              name(tid), m.u32(o), m.f32(o + 0x20), m.f32(o + 0x24), m.f32(o + 0x28), m.f32(o + 0x2C), m.f32(o + 0x30),
              m.f32(o + 4), m.f32(o + 8), m.f32(o + 0xC)))


BLEND = ["alpha blend", "multiply", "double multiply", "add", "subtract", "component min", "component max",
         "alpha-multiply add"]
ORIENT = ["screen facing", "parallel to direction", "perpendicular to direction"]


def shader(o, ind):
    """The particle shader fields shared by `part` (+0xD4) and pctl particle states (+0xDC)."""
    b = m.u16(o + 6)
    print(ind + "shader flags %#x blend %s fade mode %d map flags %#x; secondary map %s" % (
        m.u16(o + 4), BLEND[b] if b < 8 else b, m.u16(o + 8), m.u16(o + 0xA), name(ref(o + 0x28))))


def particle(tid, ind):
    o = m.data(tid)
    print(ind + "part flags %#x" % m.u32(o))
    print(ind + "  bitmap %s, physics %s, foot effect %s" % (name(ref(o + 4)), name(ref(o + 0x14)), name(ref(o + 0x24))))
    print(ind + "  lifespan %s s, fade in %.3g s, fade out %.3g s" % (bounds(o + 0x38), m.f32(o + 0x40), m.f32(o + 0x44)))
    print(ind + "  collision effect %s, death effect %s" % (name(ref(o + 0x48)), name(ref(o + 0x58))))
    print(ind + "  minimum size %.3g, radius animation %s, animation rate %s, contact deterioration %.3g, fade start/end size %.3g %.3g" % (
        m.f32(o + 0x68), bounds(o + 0x74), bounds(o + 0x80), m.f32(o + 0x88), m.f32(o + 0x8C), m.f32(o + 0x90)))
    print(ind + "  sequences: first %d, initial %d, looping %d, final %d; orientation %s" % (
        m.i16(o + 0x98), m.i16(o + 0x9A), m.i16(o + 0x9C), m.i16(o + 0x9E),
        ORIENT[m.u16(o + 0xAC)] if m.u16(o + 0xAC) < 3 else m.u16(o + 0xAC)))
    shader(o + 0xD4, ind + "  ")
    if "-v" in rest:
        hd(o, 0x164, ind + "    ")
    if m.tag(ref(o + 4)):
        bitmap_sequences(ref(o + 4), ind + "  ")
    if m.tag(ref(o + 0x14)):
        physics(ref(o + 0x14), ind + "  ")


def system(tid, ind):
    o = m.data(tid)
    print(ind + "pctl %s: physics %s, update physics %d, constants %s" % (
        name(tid), name(ref(o + 0x38)), m.u16(o + 0x48), [round(m.f32(k), 4) for k in m.block(o + 0x50, 4)]))
    for ty in m.block(o + 0x5C, 0x80):
        print(ind + "  type %r: flags %#x initial count %d render mode %d radius %.3g creation physics %d physics flags %#x constants %s" % (
            m.cstr(ty), m.u32(ty + 0x20), m.i16(ty + 0x24), m.u16(ty + 0x28), m.f32(ty + 0x2C), m.u16(ty + 0x54),
            m.u32(ty + 0x58), [round(m.f32(k), 4) for k in m.block(ty + 0x5C, 4)]))
        for st in m.block(ty + 0x68, 0xC0):
            print(ind + "    state %r: duration %s transition %s; multipliers %s; creation %d constants %s" % (
                m.cstr(st), bounds(st + 0x20), bounds(st + 0x28), " ".join("%.3g" % x for x in m.vec(st + 0x34, 9)),
                m.u16(st + 0xB0), [round(m.f32(k), 4) for k in m.block(st + 0xB4, 4)]))
            if "-v" in rest:
                hd(st, 0xC0, ind + "        ")
        for ps in m.block(ty + 0x74, 0x178):
            print(ind + "    particle state %r: duration %s transition %s bitmap %s sequence %d" % (
                m.cstr(ps), bounds(ps + 0x20), bounds(ps + 0x28), name(ref(ps + 0x30)), m.i16(ps + 0x40)))
            print(ind + "      scale %s wu/pixel, animation rate %s, rotation rate %s, colour 1 %s colour 2 %s, radius multiplier %.3g" % (
                bounds(ps + 0x48), bounds(ps + 0x50), bounds(ps + 0x58), colour(ps + 0x60), colour(ps + 0x70), m.f32(ps + 0x80)))
            shader(ps + 0xDC, ind + "      ")
            print(ind + "      creation %d constants %s" % (m.u16(ps + 0x168), [round(m.f32(k), 4) for k in m.block(ps + 0x16C, 4)]))
            if m.tag(ref(ps + 0x84)):
                physics(ref(ps + 0x84), ind + "      ")
            if m.tag(ref(ps + 0x30)):
                bitmap_sequences(ref(ps + 0x30), ind + "      ")
            if "-v" in rest:
                hd(ps, 0x178, ind + "        ")


seen = set()


def effect(tid, ind=""):
    if tid in seen:
        print(ind + "(again) " + name(tid))
        return
    seen.add(tid)
    o = m.data(tid)
    print(ind + "effe %s: flags %#x loop start %d stop %d" % (name(tid), m.u32(o), m.i16(o + 4), m.i16(o + 6)))
    locs = [m.cstr(l) for l in m.block(o + 0x28, 0x20)]
    print(ind + "  locations %s" % locs)
    for ei, e in enumerate(m.block(o + 0x34, 0x44)):
        print(ind + "  event %d: skip %.3g delay %s duration %s" % (ei, m.f32(e + 4), bounds(e + 8), bounds(e + 0x10)))
        for p in m.block(e + 0x2C, 0x68):
            what = ref(p + 0x18)
            print(ind + "    part %s: create in %d %d location %d flags %#x" % (
                name(what), m.u16(p), m.u16(p + 2), m.i16(p + 4), m.u16(p + 6)))
            print(ind + "      velocity %s cone %.3g angular %s radius modifier %s A %#x B %#x" % (
                bounds(p + 0x40), m.f32(p + 0x48), bounds(p + 0x4C), bounds(p + 0x54), m.u32(p + 0x60), m.u32(p + 0x64)))
            t = m.tag(what)
            if t and t["cls"] == "pctl" and what not in seen:
                seen.add(what)
                system(what, ind + "      ")
            if t and t["cls"] == "ligh":
                light(what, ind + "      ")
            elif t and t["cls"] == "effe":
                effect(what, ind + "      ")
        for q in m.block(e + 0x38, 0xE8):
            what = ref(q + 0x54)
            print(ind + "    particle %s: create in %d %d mode %d location %d" % (
                name(what), m.u16(q), m.u16(q + 2), m.u16(q + 4), m.i16(q + 8)))
            print(ind + "      direction (yaw pitch) %.3g %.3g offset %s direction vector? %s" % (
                m.f32(q + 0xC), m.f32(q + 0x10), "%.3g %.3g %.3g" % m.vec(q + 0x14), "%.3g %.3g %.3g" % m.vec(q + 0x20)))
            print(ind + "      flags %#x distribution %d count %d..%d distribution radius %s" % (
                m.u32(q + 0x64), m.u16(q + 0x68), m.i16(q + 0x6C), m.i16(q + 0x6E), bounds(q + 0x70)))
            print(ind + "      velocity %s cone %.3g angular %s radius %s" % (
                bounds(q + 0x84), m.f32(q + 0x8C), bounds(q + 0x90), bounds(q + 0xA0)))
            print(ind + "      tint %s .. %s" % (colour(q + 0xB0), colour(q + 0xC0)))
            print(ind + "      A %#x B %#x" % (m.u32(q + 0xE0), m.u32(q + 0xE4)))
            if "-v" in rest:
                hd(q, 0xE8, ind + "        ")
            if m.tag(what) and m.tag(what)["cls"] == "part" and what not in seen:
                seen.add(what)
                particle(what, ind + "      ")


if "--list" in rest:
    for t in m.find("effe"):
        print(t["index"], t["name"])
elif "--pctl" in rest:
    for t in m.find("pctl"):
        system(t["id"], "")
elif "--hex" in rest:
    i = rest.index("--hex")
    path = rest[i + 1]
    size = int(rest[i + 2], 16) if len(rest) > i + 2 else 0x100
    t = [t for t in m.tags if t["name"].lower() == path.lower()][0]
    print(t["cls"], t["name"])
    hd(m.addr(t["data"]), size)
else:
    paths = [a for a in rest if not a.startswith("-")] or [r"effects\explosions\large explosion"]
    for p in paths:
        for t in m.find("effe", p):
            effect(t["id"])
