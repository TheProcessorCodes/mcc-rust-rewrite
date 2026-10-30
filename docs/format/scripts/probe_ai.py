# Read-only probe of the scenario's AI blocks and the character / unit tags
# behind them (docs/format/ai.md). Never writes anything.
#
#   python -I probe_ai.py [map] [mode]
#
# map defaults to 120_halo. Modes:
#   summary      (default) block counts and strides, checked against the
#                distance between consecutive blocks' elements
#   squads       every squad: name, group, zone, cells, starting locations
#   survey       for every i16 slot of squads, cells and locations, the
#                distinct values seen (how the fields were told apart)
#   chars        the character palette with parents, unit, vitality,
#                perception and the unit's team and capsule
#   zones        AI zones, their firing positions and areas
import os
import struct
import sys
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map  # noqa: E402

MAP = sys.argv[1] if len(sys.argv) > 1 else "120_halo"
MODE = sys.argv[2] if len(sys.argv) > 2 else "summary"
m = Map(MAP)
S = m.meta(m.find("scnr"))

SQUAD_GROUPS, SQUADS, ZONES, CHARS = 0x378, 0x384, 0x390, 0x3A8
WEAPONS = 0x120


def name32(o):
    return m.cstr(o, 32)


def squads():
    for i, e in enumerate(m.elements(S + SQUADS, 64)):
        yield i, e


def cells(e):
    return m.elements(e + 0x30, 96)


def locations(c):
    return m.elements(c + 0x54, 136)


def summary():
    groups = m.elements(S + SQUAD_GROUPS, 40)
    sq = list(squads())
    cs = [c for _, e in sq for c in cells(e)]
    ls = [l for c in cs for l in locations(c)]
    zs = m.elements(S + ZONES, 64)
    fps = sum(m.block(z + 0x28)[0] for z in zs)
    print(f"{MAP}: {len(groups)} squad groups, {len(sq)} squads, {len(cs)} cells, {len(ls)} starting locations")
    print(f"       {len(zs)} AI zones with {fps} firing positions, {m.block(S + CHARS)[0]} characters")
    # Strides: consecutive firing-position and area blocks of the zones.
    for k in range(len(zs) - 1):
        n, a = m.block(zs[k] + 0x28)
        _, b = m.block(zs[k + 1] + 0x28)
        print(f"   zone {k}: firing positions {n} -> stride {(b - a) / n:.1f}")
        n, a = m.block(zs[k] + 0x34)
        _, b = m.block(zs[k + 1] + 0x34)
        print(f"   zone {k}: areas {n} -> stride {(b - a) / n:.1f}")
        if k > 2:
            break


def survey():
    sq = [e for _, e in squads()]
    cs = [c for e in sq for c in cells(e)]
    ls = [l for c in cs for l in locations(c)]
    for label, elems, base, size in (("squad", sq, 0x20, 32), ("cell", cs, 0, 96), ("location", ls, 0, 136)):
        print(f"== {label}: {len(elems)}")
        for off in range(base, base + size, 2):
            c = Counter(m.i16(e + off) for e in elems)
            if len(c) == 1 and 0 in c:
                continue
            top = ", ".join(f"{k}x{v}" for k, v in c.most_common(8))
            print(f"  +0x{off:02X} {len(c):4d} distinct: {top}")


def char_name(i):
    n, a = m.block(S + CHARS)
    if i < 0 or i >= n:
        return "-"
    t = m.ref(a + 16 * i)
    return m.names[t].split("\\")[-1] if t is not None else "-"


def weapon_name(i):
    n, a = m.block(S + WEAPONS)
    if i < 0 or i >= n:
        return "-"
    t = m.ref(a + 16 * i)
    return m.names[t].split("\\")[-1] if t is not None else "-"


def list_squads():
    groups = [name32(g) for g in m.elements(S + SQUAD_GROUPS, 40)]
    zones = [name32(z) for z in m.elements(S + ZONES, 64)]
    for i, e in squads():
        g, z = m.i16(e + 0x26), m.i16(e + 0x28)
        print(f"[{i:3}] {name32(e):<30} group {groups[g] if 0 <= g < len(groups) else '-':<24} zone {zones[z] if 0 <= z < len(zones) else '-'}")
        for c in cells(e):
            print(f"      cell {m.i16(c + 4)} x {char_name(m.i16(c + 8))} with {weapon_name(m.i16(c + 0xA))}")
            for l in locations(c):
                x, y, zz = m.f32n(l + 8, 3)
                print(f"         {m.sid_at(l + 4):<22} ({x:8.2f}, {y:8.2f}, {zz:7.2f}) bsp {m.i16(l + 0x16):3} yaw {m.f32(l + 0x18):6.2f}"
                      f" char {m.i16(l + 0x28)} weapon {m.i16(l + 0x2A)}")


def chain(t):
    out = []
    while t is not None and t not in out and len(out) < 8:
        out.append(t)
        t = m.ref(m.meta(t) + 4)
    return out


def first_block(ch, off):
    for t in ch:
        n, a = m.block(m.meta(t) + off)
        if n > 0:
            return a
    return None


def chars():
    n, a = m.block(S + CHARS)
    for i in range(n):
        t = m.ref(a + 16 * i)
        if t is None:
            continue
        ch = chain(t)
        unit = None
        for c in ch:
            unit = m.ref(m.meta(c) + 0x14) or m.ref(m.meta(c) + 0x24)
            if unit is not None:
                break
        print(f"[{i:2}] {m.names[t]}  <- {' <- '.join(m.names[c].split(chr(92))[-1] for c in ch[1:])}")
        v = first_block(ch, 0x78)
        p = first_block(ch, 0x90)
        g = first_block(ch, 0x6C)
        if g is not None:
            print(f"     actor type {m.i16(g + 4)}")
        if v is not None:
            print(f"     vitality body/shield {m.f32n(v + 4, 2)} legendary {m.f32n(v + 12, 2)}")
        if p is not None:
            print(f"     perception {['%.3g' % x for x in m.f32n(p + 4, 6)]}")
        if unit is not None:
            u = m.meta(unit)
            print(f"     unit {m.cls(unit)} {m.names[unit]}: team {m.i16(u + 0xFC)}", end="")
            if m.cls(unit) == "bipd":
                print(f", physics flags 0x{m.u32(u + 0x33C):X}, height {m.f32n(u + 0x340, 2)}, radius {m.f32(u + 0x348):.3f}")
            else:
                print()


def zones():
    for i, z in enumerate(m.elements(S + ZONES, 64)):
        print(f"[{i:2}] {name32(z):<22} flags {m.u32(z + 0x20)} bsp {m.i16(z + 0x24)}")
        for f in m.elements(z + 0x28, 40)[:3]:
            print(f"      fp {['%.2f' % x for x in m.f32n(f, 3)]} bsp {m.i16(f + 0xE)} flags {m.u16(f + 0x10)} area {m.i16(f + 0x16)} "
                  f"cluster {m.i16(f + 0x18)} normal {['%.3f' % x for x in m.f32n(f + 0x1C, 2)]}")
        for ar in m.elements(z + 0x34, 108)[:3]:
            print(f"      area {name32(ar):<12} centre {['%.2f' % x for x in m.f32n(ar + 0x24, 3)]} radius {m.f32(ar + 0x34):.2f} "
                  f"firing positions {m.u16(ar + 0x38)} +{m.u16(ar + 0x3A)}")


{"summary": summary, "squads": list_squads, "survey": survey, "chars": chars, "zones": zones}[MODE]()
