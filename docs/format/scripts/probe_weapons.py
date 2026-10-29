# Read-only probe of Halo 3 weapons (docs/format/weapons.md): every weap tag's
# models, magazines, triggers, barrels, projectile and damage numbers, zoom
# and holster marker; the markers on its render model; the Spartan's hand
# markers; and the scenario's weapon / equipment placements.
# Run: python -I probe_weapons.py [map name or path] [--all] [--markers]
# Never writes anything.
import math
import os
import sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map

args = [a for a in sys.argv[1:] if not a.startswith("--")]
m = Map(args[0] if args else "riverworld")
SHOW_ALL = "--all" in sys.argv
SHOW_MARKERS = "--markers" in sys.argv
S = m.meta(m.find("scnr"))


def short(name): return name.rsplit("\\", 1)[-1]


def render_model(obj):
    """object +0x34 -> hlmt, hlmt +0 -> mode."""
    h = m.ref(m.meta(obj) + 0x34)
    return None if h is None else m.ref(m.meta(h))


def quat_mul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz)


def rotate(q, v):
    x, y, z, w = q
    p = quat_mul(quat_mul(q, (v[0], v[1], v[2], 0.0)), (-x, -y, -z, w))
    return p[:3]


def nodes(mode):
    """(name, parent, world position, world rotation) per node, bind pose."""
    out = []
    for o in m.elements(m.meta(mode) + 0x30, 96):
        name, parent = m.sid_at(o), m.i16(o + 4)
        pos, q = m.f32n(o + 12, 3), m.f32n(o + 24, 4)
        if 0 <= parent < len(out):
            pp, pq = out[parent][2], out[parent][3]
            wp = tuple(a + b for a, b in zip(pp, rotate(pq, pos)))
            wq = quat_mul(pq, q)
        else:
            wp, wq = pos, q
        out.append((name, parent, wp, wq))
    return out


def markers(mode):
    ns = nodes(mode)
    out = []
    for g in m.elements(m.meta(mode) + 0x3C, 16):
        gname = m.sid_at(g)
        for mk in m.elements(g + 4, 36):
            nd = m.u8(mk + 2)
            pos, q = m.f32n(mk + 4, 3), m.f32n(mk + 16, 4)
            n = ns[nd] if nd < len(ns) else ("?", -1, (0, 0, 0), (0, 0, 0, 1))
            model = tuple(a + b for a, b in zip(n[2], rotate(n[3], pos)))
            out.append((gname, nd, n[0], pos, q, model))
    return out


def jpt(t):
    b = m.meta(t)
    return dict(radius=m.f32n(b, 2), lower=m.f32(b + 0x1C), upper=m.f32n(b + 0x20, 2))


def proj(t):
    b = m.meta(t)
    d = m.ref(b + 0x1F0)
    return dict(type=m.i16(b), range=m.f32(b + 0x22C), v0=m.f32(b + 0x23C), v1=m.f32(b + 0x240),
                damage=(short(m.names[d]), jpt(d)) if d is not None and m.cls(d) == "jpt!" else None)


placed = {}
for e in m.elements(S + 0x114, 168):
    pi = m.i16(e)
    pal = m.elements(S + 0x120, 16)
    if 0 <= pi < len(pal):
        t = m.ref(pal[pi])
        placed[t] = placed.get(t, 0) + 1

weaps = m.all("weap")
print(f"{len(weaps)} weap tags, {len(placed)} placed")
for w in weaps:
    if not SHOW_ALL and w not in placed:
        continue
    b = m.meta(w)
    mode = render_model(w)
    print(f"\n#{w} {m.names[w]}  object type {m.i16(b)}  class {m.sid_at(b + 0x3DC)} / {m.sid_at(b + 0x3E0)}"
          f"  holster {m.sid_at(b + 0x4D8) or '-'}  placed {placed.get(w, 0)}")
    print(f"   model {m.refname(b + 0x34)} -> mode {m.names[mode] if mode is not None else '-'}")
    print(f"   zoom levels {m.i16(b + 0x31E)} range {m.f32n(b + 0x320, 2)}")
    for e in m.elements(b + 0x3FC, 32):
        print(f"   first person {m.refname(e)}  anims {m.refname(e + 16)}")
    for e in m.elements(b + 0x424, 128):
        print(f"   magazine: loaded max {m.i16(e + 0xA)}, total initial {m.i16(e + 6)}, total max {m.i16(e + 8)},"
              f" reloaded {m.i16(e + 0x14)}")
    for k, e in enumerate(m.elements(b + 0x430, 144)):
        print(f"   trigger {k}: behaviour {m.i16(e + 6)} barrels {m.i16(e + 8)} / {m.i16(e + 0xA)}")
    for k, e in enumerate(m.elements(b + 0x43C, 308)):
        rps = m.f32n(e + 4, 2)
        spf = (m.i16(e + 0x1C), m.i16(e + 0x1E))
        err = [math.degrees(x) for x in m.f32n(e + 0x80, 2)]
        print(f"   barrel {k}: rounds/s {rps} shots/pull {spf} recovery {m.f32(e + 0x20):.2f}"
              f" magazine {m.i16(e + 0x28)} rounds/shot {m.i16(e + 0x2A)}"
              f" error {err[0]:.2f}..{err[1]:.2f} deg")
        p = m.ref(e + 0x98)
        if p is not None and m.cls(p) == "proj":
            print(f"      projectile {short(m.names[p])}: {proj(p)}")
    if mode is not None:
        mk = markers(mode)
        if SHOW_MARKERS:
            for g, nd, nn, pos, q, model in mk:
                print(f"   marker {g:20s} node {nd:2d} {nn:10s} local ({pos[0]:.4f}, {pos[1]:.4f}, {pos[2]:.4f})"
                      f" q ({q[0]:.3f}, {q[1]:.3f}, {q[2]:.3f}, {q[3]:.3f})")
        else:
            print("   markers:", ", ".join(sorted({g for g, *_ in mk})))
        print("   root node", nodes(mode)[0][0], nodes(mode)[0][2], nodes(mode)[0][3])

chief = m.find("mode", r"objects\characters\masterchief\mp_masterchief\mp_masterchief")
if chief is not None:
    print("\nSpartan hand / holster markers (local to node; model = bind pose):")
    for g, nd, nn, pos, q, model in markers(chief):
        if g in ("right_hand", "left_hand", "weapon_back", "weapon_thigh"):
            print(f"   {g:12s} node {nd:2d} {nn:8s} local ({pos[0]:.4f}, {pos[1]:.4f}, {pos[2]:.4f})"
                  f" q ({q[0]:.3f}, {q[1]:.3f}, {q[2]:.3f}, {q[3]:.3f})"
                  f" model ({model[0]:.4f}, {model[1]:.4f}, {model[2]:.4f})")

print("\nplacements (palette, position wu, yaw/pitch/roll deg, respawn s):")
for label, boff, poff, stride, resp in (("weapon", 0x114, 0x120, 168, 0x80), ("equipment", 0xFC, 0x108, 140, 0x64)):
    pal = [m.ref(o) for o in m.elements(S + poff, 16)]
    for k, e in enumerate(m.elements(S + boff, stride)):
        pi = m.i16(e)
        t = pal[pi] if 0 <= pi < len(pal) else None
        pos = m.f32n(e + 8, 3)
        ypr = [math.degrees(x) for x in m.f32n(e + 0x14, 3)]
        print(f"   {label:9s} {k:3d} {short(m.names[t]) if t is not None else '-':24s}"
              f" ({pos[0]:8.2f}, {pos[1]:8.2f}, {pos[2]:7.2f})"
              f" ({ypr[0]:7.1f}, {ypr[1]:6.1f}, {ypr[2]:6.1f}) {m.i16(e + resp):4d}"
              f"  type byte {m.u8(e + 0x3E)}")
