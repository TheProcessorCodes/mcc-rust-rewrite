# Shaders (docs/format/halo1.md 10): which offsets of each shader class
# hold bitmap references, and the fields around them for one example.
# python -I probe_h1_shaders.py [map] [shader path]
import os, struct, sys
from collections import Counter, defaultdict
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "bloodgulch")
SIZE = 0x400
offsets = defaultdict(Counter)
for cls in ["senv", "soso", "sotr", "schi", "scex", "swat", "sgla", "smet", "spla"]:
    tags = m.find(cls)
    for t in tags:
        o = m.data(t["id"])
        for k in range(0, SIZE, 4):
            if m.d[o + k:o + k + 4] == b"mtib" and m.tag(m.u32(o + k + 12)):
                offsets[cls][hex(k)] += 1
    print(cls, len(tags), "shaders; bitmap references at", dict(sorted(offsets[cls].items(), key=lambda x: int(x[1 - 1], 16))))
# Inside the blocks of schi / scex / sotr maps (stage maps).
for cls, at, size in [("schi", 0x54, 0xDC), ("scex", 0x54, 0xDC), ("sotr", 0x5C, 0x64), ("sotr", 0x50, 0x64)]:
    for t in m.find(cls)[:2]:
        o = m.data(t["id"])
        try:
            maps = m.block(o + at, size)
        except Exception:
            continue
        refs = []
        for e in maps[:4]:
            refs.append([hex(k) for k in range(0, size, 4) if m.d[e + k:e + k + 4] == b"mtib"])
        print(cls, t["name"], f"block at {at:#x}: {len(maps)} maps; bitmap refs in each at", refs)
if len(sys.argv) > 2:
    t = [t for t in m.tags if t["name"].lower() == sys.argv[2].lower() and t["cls"] in offsets][0]
    o = m.data(t["id"])
    print(t["cls"], t["name"])
    for k in range(0, 0x170, 4):
        print(f"{k:#05x} {m.u32(o + k):#010x} {m.f32(o + k):10.4f}", m.refname(o + k - 12) if m.d[o + k - 12:o + k - 8] == b"mtib" else "")
