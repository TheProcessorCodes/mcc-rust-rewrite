# Read-only probe of a Halo 3 map's sound classes (docs/format/sound.md 9): every
# `sncl` class's non-zero words (floats where they look like floats), the sounds
# and looping sounds that use each class, and the `snmx` sound mix's floats.
# Run: python -I probe_sound_classes.py [map name or path]
# Never writes anything.
import os
import sys
from collections import defaultdict
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "120_halo")


def words(base, size):
    out = []
    for k in range(size // 4):
        w = m.u32(base + 4 * k)
        if w == 0:
            continue
        f = m.f32(base + 4 * k)
        out.append(f"{4 * k:02X}:{round(f, 3) if 1e-6 < abs(f) < 1e6 else hex(w)}")
    return " ".join(out)


snd_by = defaultdict(list)
for t in m.all("snd!"):
    snd_by[m.u8(m.meta(t) + 2)].append(m.names[t])
lsnd_by = defaultdict(list)
for t in m.all("lsnd"):
    lsnd_by[m.i16(m.meta(t) + 0x24)].append(m.names[t])

t = m.find("sncl")
print(f"sncl {m.names[t]}: element 0x98 bytes")
for i, c in enumerate(m.elements(m.meta(t), 0x98)):
    print(f"class {i}: gain range {m.f32(c + 0x44):g} / {m.f32(c + 0x48):g} dB  [{words(c, 0x98)}]")
    names = [n.replace("sound\\", "") for n in snd_by[i][:3] + lsnd_by[i][:2]]
    if names:
        print(f"    {len(snd_by[i])} snd!, {len(lsnd_by[i])} lsnd: {', '.join(names)}")

t = m.find("snmx")
if t is not None:
    print(f"\nsnmx {m.names[t]}: {words(m.meta(t), 0x90)}")
