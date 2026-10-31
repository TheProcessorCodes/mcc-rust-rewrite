# Read-only probe of looping sounds (lsnd, docs/format/sound.md 5): the header
# words, every track's 144 bytes as words, the sound each reference names and its
# permutations' lengths from sfx.fsb.info, and the detail sounds.
# Run: python -I probe_lsnd.py [map] [name filter, default \music\] [--raw]
# Never writes anything.
import os
import struct
import sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map, MAPS

args = [a for a in sys.argv[1:] if not a.startswith("--")]
m = Map(args[0] if args else "120_halo")
FILTER = (args[1] if len(args) > 1 else "\\music\\").lower()
RAW = "--raw" in sys.argv

UGH = m.meta(m.find("ugh!"))
PITCH = m.block(UGH + 0x40)[1]
PERMS = m.block(UGH + 0x4C)[1]
NAMES = m.block(UGH + 0x28)[1]

# FMOD id -> frames (sfx.fsb.info: 280-byte records, +0x04 PCM bytes, +0x08 channels).
INFO = {}
info_path = os.path.join(os.path.dirname(MAPS), "fmod", "pc", "sfx.fsb.info")
with open(info_path, "rb") as f:
    data = f.read()
for k in range(len(data) // 280):
    o = 280 * k
    fid, pcm, ch = struct.unpack_from("<III", data, o)
    loop_end = struct.unpack_from("<I", data, o + 0x14)[0]
    INFO[fid] = (pcm // (2 * max(ch, 1)), loop_end)


def perms(t):
    b = m.meta(t)
    pr = m.i16(b + 6)
    if pr < 0:
        return []
    p = PITCH + 12 * pr
    first, count = m.u16(p + 8), (m.u16(p + 10) >> 4) & 0x3F
    out = []
    for q in range(first, first + count):
        e = PERMS + 20 * q
        name = m.sid(m.u32(NAMES + 4 * m.u16(e)))
        frames, loop_end = INFO.get(m.u32(e + 16), (0, 0))
        out.append(f"{name} {frames / 48000:.2f}s" + (f" loop_end {loop_end}" if loop_end else ""))
    return out


SLOTS = [(0x14, "in"), (0x24, "loop"), (0x34, "out"), (0x44, "alt loop"),
         (0x54, "alt out"), (0x68, "alt trans in"), (0x78, "alt trans out")]

for t in m.all("lsnd"):
    if FILTER not in m.names[t].lower():
        continue
    b = m.meta(t)
    words = [m.u32(b + 4 * k) for k in range(16)]
    print(f"#{t} {m.names[t]}")
    print("  header", " ".join(f"{w:08x}" for w in words[:10]),
          f"| +0x0C {m.f32(b + 0x0C):g} +0x10 {m.f32(b + 0x10):g} class {m.i16(b + 0x24)}")
    for e in m.elements(b + 0x28, 144):
        print(f"  track {m.sid_at(e)} flags {m.u32(e + 4):#x} gain {m.f32(e + 8):g} fade in {m.f32(e + 12):g} out {m.f32(e + 16):g}")
        if RAW:
            for row in range(0, 144, 16):
                print("    +%02X " % row + " ".join(f"{m.u32(e + row + 4 * k):08x}" for k in range(4)))
        for off, label in SLOTS:
            r = m.ref(e + off)
            if r is None:
                continue
            print(f"    {label:13} {m.names[r]}")
            for p in perms(r):
                print(f"      {p}")
        print(f"    +0x64 {m.u32(e + 0x64):#x} +0x88 {m.u32(e + 0x88):#x} +0x8C {m.u32(e + 0x8C):#x}")
    for e in m.elements(b + 0x34, 60):
        print(f"  detail {m.sid_at(e)} {m.refname(e + 4)}")
