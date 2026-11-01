# Read-only: an effect (effe) tag and what it reaches, as in docs/format/effects.md:
# events (0x44 bytes), parts (0x60), particle systems (0x5C), emitters (0x2F0) with
# their properties and function blobs, the particles (prt3) with their render method
# options and bitmaps, particle physics (pmov), contrails (cntl) and lights (ligh).
# Run:  python -I probe_effects.py [map] [effe tag path]
#       python -I probe_effects.py 120_halo --census     function kinds and segment shapes
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "120_halo")
arg = sys.argv[2] if len(sys.argv) > 2 else r"levels\solo\120_halo\fx\trench_run\plate_explosion\plate_explosion"

EMITTER = [(0x88, "radius"), (0xA8, "angle"), (0xC8, "axis angle"), (0xE8, "starting count"),
           (0x108, "max count"), (0x128, "rate"), (0x148, "lifespan"), (0x1C0, "velocity"),
           (0x1E0, "rotation"), (0x200, "rotation rate"), (0x220, "size"), (0x240, "scale"),
           (0x260, "tint"), (0x280, "alpha"), (0x2A0, "black point")]
PARTICLE = [(0x7C, "aspect"), (0x9C, "colour"), (0xBC, "intensity"), (0xDC, "alpha"),
            (0x100, "frame"), (0x120, "animation rate"), (0x140, "palette")]
CONTRAIL = [(0x24, "rate"), (0x44, "lifespan"), (0x9C, "size"), (0x180, "colour"),
            (0x1A0, "alpha"), (0x1C0, "intensity"), (0x1E0, "black point")]
SEGMENT_VALUES = {4: 2, 7: 4, 10: 7}


def blob(a):
    """A property's function blob: (bytes offset, size) or None."""
    size = m.u32(a + 4)
    if size == 0 or m.u32(a + 16) == 0:
        return None
    return m.ptr32(m.u32(a + 16)), size


def curve(p):
    n = m.u32(p)
    p += 4
    segs = []
    for _ in range(n):
        shape = m.u32(p) & 0xFF
        k = SEGMENT_VALUES.get(shape)
        if k is None:
            return segs, p, f"unknown shape {shape}"
        end = m.f32(p + 4)
        vals = [m.f32(p + 8 + 4 * i) for i in range(k)]
        segs.append((shape, end, vals))
        p += 8 + 4 * k
    return segs, p, None


def describe(a):
    b = m.m[a:a + 4]
    head = f"in {b[0]:2} range {b[1]:2}"
    loc = blob(a)
    if loc is None:
        return head + " | (empty)"
    d, size = loc
    kind, flags, colours = m.u8(d), m.u8(d + 1), m.u8(d + 2)
    ranged = flags & 1
    if colours:
        cs = " ".join(f"{m.u32(d + o) & 0xFFFFFF:06x}" for o in (4, 8, 12, 16))
        body = f"{colours}-colour [{cs}]"
    else:
        body = f"[{m.f32(d + 4):.4g} .. {m.f32(d + 8):.4g}]"
    text = f"{head} | kind {kind}{' ranged' if ranged else ''} {body}"
    if kind == 8:
        p = d + 0x20
        for c in range(2 if ranged else 1):
            segs, p, err = curve(p)
            text += f" | curve {c}: " + "; ".join(
                f"to {e:.3g}: " + " ".join(f"{v:.4g}" for v in vals[:4]) for _, e, vals in segs)
            if err:
                text += f" ({err})"
    return text


def render_method(rm):
    rmdf = m.ref(rm)
    cats = m.elements(m.meta(rmdf) + 0x10, 24)
    opts = []
    for k, oa in enumerate(m.elements(rm + 0x10, 2)):
        if k >= len(cats):
            break
        chosen = m.i16(oa)
        names = m.elements(cats[k] + 4, 28)
        opts.append(f"{m.sid_at(cats[k])}={m.sid_at(names[chosen]) if 0 <= chosen < len(names) else chosen}")
    maps = []
    props = m.elements(rm + 0x28, 132)
    if props:
        t = m.meta(m.ref(props[0]))
        usages = [m.sid_at(x) for x in m.elements(t + 0x6C, 4)]
        for u, x in zip(usages, m.elements(props[0] + 0x10, 28)):
            maps.append(f"{u}: {m.refname(x)}")
    return opts, maps


def movement(at, pad):
    for mv in m.elements(at, 0x18):
        print(f"{pad}movement kind {m.u16(mv)} mask {m.u32(mv + 0x10):#x}")
        for p in m.elements(mv + 4, 0x24):
            print(f"{pad}  parameter {m.u32(p)}: {describe(p + 4)}")


def particle(i, pad):
    o = m.meta(i)
    print(f"{pad}prt3 {m.names[i]}: appearance {m.u32(o + 0x10):#x} billboard {m.u16(o + 0x14)} "
          f"centre offset {m.f32(o + 0x20):.3g} animation flags {m.u32(o + 0xFC):#x}")
    opts, maps = render_method(o + 0x3C)
    print(f"{pad}  options {' '.join(opts)}")
    for x in maps:
        print(f"{pad}  {x}")
    for off, name in PARTICLE:
        print(f"{pad}  {name:15} {describe(o + off)}")
    for a in m.elements(o + 4, 0x14):
        print(f"{pad}  attachment {m.refname(a)} trigger {m.u8(a + 0x10)}")


def effect(i, pad="", seen=None):
    seen = set() if seen is None else seen
    o = m.meta(i)
    print(f"{pad}effe {m.names[i]}: flags {m.u32(o):#x} loop start {m.i16(o + 0x18)} "
          f"locations {[m.sid_at(l) for l in m.elements(o + 0x20, 0xC)]} looping sound {m.refname(o + 0x38)}")
    if i in seen:
        return
    seen.add(i)
    for e in m.elements(o + 0x2C, 0x44):
        d, u = m.f32n(e + 0x10, 2), m.f32n(e + 0x18, 2)
        print(f"{pad}  event {m.sid_at(e)}: skip {m.f32(e + 0xC):.2f} delay {d[0]:.3g}-{d[1]:.3g} s "
              f"duration {u[0]:.3g}-{u[1]:.3g} s")
        for p in m.elements(e + 0x20, 0x60):
            v = m.f32n(p + 0x20, 9)
            print(f"{pad}    part {m.refname(p + 0x10)} location {m.i16(p + 4)} velocity {v[0]:.3g}-{v[1]:.3g} "
                  f"yaw/pitch {v[2]:.3g}/{v[3]:.3g} cone {v[4]:.3g} spin {v[5]:.3g}-{v[6]:.3g}")
            r = m.ref(p + 0x10)
            if r is not None and m.cls(r) == "effe":
                effect(r, pad + "      ", seen)
            if r is not None and m.cls(r) == "ligh":
                lo = m.meta(r)
                print(f"{pad}      light size {m.f32(lo + 8):.3g} duration {m.f32(lo + 0x70):.3g} s")
                print(f"{pad}        colour    {describe(lo + 0x24)}")
                print(f"{pad}        intensity {describe(lo + 0x48)}")
            if r is not None and m.cls(r) == "cntl":
                for c in m.elements(m.meta(r), 0x26C):
                    opts, maps = render_method(c + 0x130)
                    print(f"{pad}      contrail {m.sid_at(c)}: {' '.join(opts)}; {'; '.join(maps)}")
                    for off, name in CONTRAIL:
                        print(f"{pad}        {name:12} {describe(c + off)}")
        for s in m.elements(e + 0x38, 0x5C):
            print(f"{pad}    particle system {m.refname(s + 4)} location {m.i32(s + 0x14)} "
                  f"local {m.u32(s + 0x18)} sort bias {m.i16(s + 0x20)} flags {m.u16(s + 0x22):#x}")
            for em in m.elements(s + 0x4C, 0x2F0):
                print(f"{pad}      emitter {m.sid_at(em)} shape {m.u8(em + 4)} bounds {m.f32(em + 8):.3g} "
                      f"physics {m.refname(em + 0x168)}")
                for off, name in EMITTER:
                    print(f"{pad}        {name:15} {describe(em + off)}")
                movement(em + 0x17C, pad + "        ")
            r = m.ref(s + 4)
            if r is not None:
                particle(r, pad + "      ")


def census():
    from collections import Counter
    kinds, shapes = Counter(), Counter()
    for i in m.all("effe"):
        o = m.meta(i)
        for e in m.elements(o + 0x2C, 0x44):
            for s in m.elements(e + 0x38, 0x5C):
                for em in m.elements(s + 0x4C, 0x2F0):
                    for off, _ in EMITTER:
                        loc = blob(em + off)
                        if loc is None:
                            continue
                        d, _ = loc
                        kinds[(m.u8(d), m.u8(d + 1) & 1, m.u8(d + 2))] += 1
                        if m.u8(d) == 8:
                            p = d + 0x20
                            for _ in range(2 if m.u8(d + 1) & 1 else 1):
                                segs, p, err = curve(p)
                                for sh, _, _ in segs:
                                    shapes[sh] += 1
                                if err:
                                    shapes[err] += 1
    print("function (kind, ranged, colours):", dict(kinds.most_common()))
    print("curve segment shapes:", dict(shapes))


if arg == "--census":
    census()
else:
    effect(m.find("effe", arg))
