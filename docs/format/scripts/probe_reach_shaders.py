# Read-only probe: MCC Reach render method tags (rmsh, rmtr, rmfl, ...;
# docs/format/reach.md 9). Same chain as Halo 3 (shaders.md) at Reach's
# offsets: the definition (rmdf) reference at +0, the chosen options
# (2 bytes each) at +0x20, the properties block at +0x38 (0xB4 each:
# template rmt2 at +0, textures at +0x10 (24 each: bitmap reference at +0,
# tiling constant index byte at +0x15), float constants at +0x1C (16 each)).
# The rmdf's categories (+0x10, 24 each: name, options block of 28-byte
# entries) name the options; the rmt2's float constant names (+0x48) and
# texture names (+0x6C) name the constants and textures.
# Use:  python -I probe_reach_shaders.py [map] [how many]
import collections
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from reach_common import Map  # noqa: E402

name = sys.argv[1] if len(sys.argv) > 1 else "20_sword_slayer"
show = int(sys.argv[2]) if len(sys.argv) > 2 else 3
m = Map(name)
classes = collections.Counter()
bad = 0
shown = collections.Counter()
for t in range(len(m.tags)):
    c = m.cls(t)
    if c is None or not c.startswith("rm") or c in ("rmdf", "rmt2", "rmop"):
        continue
    s = m.meta(t)
    classes[c] += 1
    rmdf = m.ref(s)
    if rmdf is None or m.cls(rmdf) != "rmdf":
        bad += 1
        continue
    cats = m.elements(m.meta(rmdf) + 0x10, 24)
    options = []
    for i, o in enumerate(m.elements(s + 0x20, 2)):
        if i >= len(cats):
            break
        chosen = m.i16(o)
        opts = m.elements(cats[i] + 4, 28)
        options.append((m.sid_at(cats[i]), m.sid_at(opts[chosen]) if 0 <= chosen < len(opts) else "?"))
    props = m.elements(s + 0x38, 0xB4)
    if not props:
        bad += 1
        continue
    p = props[0]
    rmt2 = m.ref(p)
    if rmt2 is None or m.cls(rmt2) != "rmt2":
        bad += 1
        continue
    t2 = m.meta(rmt2)
    usages = [m.sid_at(o) for o in m.elements(t2 + 0x6C, 4)]
    args = [m.sid_at(o) for o in m.elements(t2 + 0x48, 4)]
    maps = m.elements(p + 0x10, 24)
    consts = m.elements(p + 0x1C, 16)
    if shown[c] < show:
        shown[c] += 1
        print(f"{c} {m.names[t]}")
        print("   options:", ", ".join(f"{a}={b}" for a, b in options))
        for u, o in zip(usages, maps):
            k = m.u8(o + 0x15)
            xf = m.f32n(consts[k], 4) if k < len(consts) else None
            print(f"   {u}: {m.refname(o)} tiling {xf}")
        print("   constants:", [(a, tuple(round(x, 3) for x in m.f32n(o, 4))) for a, o in zip(args, consts)][:8])
print(f"{name}: {dict(classes)}; {bad} shaders whose chain does not resolve")
