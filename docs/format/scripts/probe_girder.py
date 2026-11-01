# Read-only: the Warthog run's tunnel girder crate (bloc): its model's
# damage sections with every instant response dumped, and its physics model
# (phmo): the top-level fields and blocks, to find mass, friction,
# restitution and any constraints the `fall` response breaks.
# Run:  python -I probe_girder.py [map] [bloc tag path]
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "120_halo")
path = sys.argv[2] if len(sys.argv) > 2 else "objects\\levels\\solo\\120_halo\\plates\\tunnel_girder\\tunnel_girder"
i = m.find("bloc", path)
o = m.meta(i)
print("bloc", path)
for off, kind, what in m.scan(o, 0x200):
    print(f"  obj +0x{off:03X} {kind} {what}")
h = m.ref(o + 0x34)
ho = m.meta(h)
print("hlmt", m.names[h], "render", m.refname(ho), "| coll", m.refname(ho + 0x10), "| jmad", m.refname(ho + 0x20),
      "| phmo", m.refname(ho + 0x30))
for off, kind, what in m.scan(ho, 0x180):
    print(f"  hlmt +0x{off:03X} {kind} {what}")

def words(a, n):
    return " ".join(f"{m.u32(a + 4 * k):08X}" for k in range(n))

def floats(a, n):
    return " ".join(f"{m.f32(a + 4 * k):.3g}" for k in range(n))

n, info = m.block(ho + 0x88)
for sec in m.elements(info + 0x68, 44):
    print("section", m.sid_at(sec), "flags", hex(m.u32(sec + 4)), "vitality", m.f32(sec + 8),
          "| +0x18..", words(sec + 0x18, 5))
    for r in m.elements(sec + 0x0C, 0x88):
        print("  response")
        for k in range(0, 0x88, 16):
            print(f"    +0x{k:02X} {words(r + k, 4)}   | {floats(r + k, 4)}")
        for off, kind, what in m.scan(r, 0x88):
            print(f"    +0x{off:02X} {kind} {what}")
        for k in range(0, 0x88, 4):
            s = m.sid_at(r + k)
            if s and not s.startswith("<sid"):
                print(f"    +0x{k:02X} string id {s}")

p = m.ref(ho + 0x30)
if p is not None:
    po = m.meta(p)
    print("phmo", m.names[p])
    for k in range(0, 0x40, 16):
        print(f"  +0x{k:02X} {words(po + k, 4)}   | {floats(po + k, 4)}")
    for off, kind, what in m.scan(po, 0x200):
        print(f"  phmo +0x{off:03X} {kind} {what}")

    # Each top-level block: the gap to the next block's data hints at its
    # stride; the first element's words, floats and string ids.
    blocks = [(off, *m.block(po + off)) for off, kind, _ in m.scan(po, 0x200) if kind == "block"]
    for off, cnt, at in blocks:
        print(f"phmo block +0x{off:03X} count {cnt}")
        for k in range(0, 0xC0, 16):
            print(f"    +0x{k:02X} {words(at + k, 4)}   | {floats(at + k, 4)}")
        sids = [(k, m.sid_at(at + k)) for k in range(0, 0xC0, 4)]
        sids = [(k, s) for k, s in sids if s and not s.startswith("<sid") and m.u32(at + k) > 0x100]
        print("    string ids:", sids)
