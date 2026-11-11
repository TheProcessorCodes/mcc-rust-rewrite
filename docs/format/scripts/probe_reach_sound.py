# Read-only probe of MCC Halo: Reach's sound tags (docs/format/sound.md 12):
# the sound gestalt's blocks and element sizes, a few `snd!` tags with their
# permutations looked up in sfx.fsb.info, each BSP's acoustics palette and
# how many clusters use each entry, the looping sounds those name (tracks and
# details), and the sound scenery placements.
# Run: python -I probe_reach_sound.py [map, default 20_sword_slayer] [--tracks]
#   --tracks: survey every looping sound's track slots, flags and fade times
# HALOREACH_MAPS names <MCC>\haloreach\maps. Never writes anything.
import os
import struct
import sys
from collections import Counter, defaultdict

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from reach_common import Map, MAPS  # noqa: E402

args = [a for a in sys.argv[1:] if not a.startswith("--")]
m = Map(args[0] if args else "20_sword_slayer")

# FMOD id -> (frames, path): 280-byte records as Halo 3's (sound.md 2).
INFO = {}
with open(os.path.join(os.path.dirname(MAPS), "fmod", "pc", "sfx.fsb.info"), "rb") as f:
    data = f.read()
for k in range(len(data) // 280):
    o = 280 * k
    fid, pcm, ch = struct.unpack_from("<III", data, o)
    INFO[fid] = (pcm // (2 * max(ch, 1)), data[o + 0x18:o + 0x118].split(b"\0")[0].decode("latin-1"))
print(f"sfx.fsb.info: {len(INFO)} samples")


def val(o):
    u, f = m.u32(o), m.f32(o)
    return f"{f:g}" if 1e-3 < abs(f) < 1e5 else f"{u:x}"


# The gestalt: every block in its first 0x100 bytes, element size from the
# gap to the next block's address.
ugh = m.meta(m.find("ugh!"))
blocks = []
for off in range(0, 0x100, 4):
    n, p = m.i32(ugh + off), m.u32(ugh + off + 4)
    if 0 < n < 200000 and p and m.u32(ugh + off + 8) == 0:
        fo = m.ptr32(p)
        if 0 < fo < len(m.m):
            blocks.append((fo, off, n))
blocks.sort()
print("ugh! blocks (offset, count, bytes to the next block / count):")
for i, (fo, off, n) in enumerate(blocks):
    gap = (blocks[i + 1][0] - fo) / n if i + 1 < len(blocks) else None
    print(f"  +{off:#x} {n} {gap}")

PLAYBACK = m.block(ugh + 0x10)[1]
NAMES = m.block(ugh + 0x28)[1]
PITCH = m.block(ugh + 0x4C)[1]
PERMS_N, PERMS = m.block(ugh + 0x58)
print("permutations whose +0x14 is a bank id:",
      sum(1 for q in range(PERMS_N) if m.u32(PERMS + 24 * q + 0x14) in INFO), "of", PERMS_N)


def sound(t):
    b = m.meta(t)
    pr, pb = m.i16(b + 6), m.i16(b + 0xE)
    out = [f"class {m.u8(b + 2)} ms {m.i32(b + 0x18)}"]
    if pb >= 0:
        e = PLAYBACK + 84 * pb
        out.append("playback " + " ".join(val(e + 4 * k) for k in range(14)))
    if pr >= 0:
        p = PITCH + 12 * pr
        first, count = m.u16(p + 8), (m.u16(p + 10) >> 4) & 0x3F
        for q in range(first, first + count):
            e = PERMS + 24 * q
            frames, path = INFO.get(m.u32(e + 0x14), (0, "?"))
            out.append(f"  {m.sid(m.u32(NAMES + 4 * m.u16(e)))} {frames / 48000:.2f}s {path}")
    return out


SLOTS = [(0x0C, "in"), (0x1C, "loop"), (0x2C, "out"), (0x3C, "alt loop"),
         (0x4C, "alt out"), (0x5C, "alt trans in"), (0x6C, "alt trans out")]


def looping(t):
    b = m.meta(t)
    print(f"  lsnd #{t} {m.names[t]}: flags {m.u32(b):#x} music time {m.f32(b + 4):g} "
          f"distance {m.f32(b + 0xC):g} / {m.f32(b + 0x10):g} class {m.i16(b + 0x18)}")
    for e in m.elements(b + 0x20, 0xB0):
        print(f"    track {m.sid_at(e)} flags {m.u32(e + 4):#x} gain {m.f32(e + 8):g} "
              f"fade in {m.f32(e + 0x7C):g} out {m.f32(e + 0x84):g}")
        for off, label in SLOTS:
            r = m.ref(e + off)
            if r is not None:
                print(f"      {label}: {m.names[r]}")
                for line in sound(r):
                    print("        " + line)
    for e in m.elements(b + 0x2C, 0x48):
        print(f"    detail {m.sid_at(e)} {m.refname(e + 4)} period {m.f32(e + 0x18):g}-{m.f32(e + 0x1C):g} "
              f"gain {m.f32(e + 0x20):g} flags {m.u32(e + 0x2C):#x} distance {m.f32(e + 0x40):g}-{m.f32(e + 0x44):g}")


scnr = m.meta(m.find("scnr"))
shown = set()
for bi, e in enumerate(m.elements(scnr + 0x50, 0xAC)):
    s = m.ref(e)
    if s is None:
        continue
    b = m.meta(s)
    clusters = m.elements(b + 0x138, 0x8C)
    use = Counter(m.i16(c + 0x1C) for c in clusters)
    print(f"bsp {bi} {m.names[s]}: {len(clusters)} clusters, {m.block(b + 0x64)[0]} leaf clusters (+0x64)")
    for k, a in enumerate(m.elements(b + 0x18C, 0x78)):
        print(f"  acoustics {k} {m.sid_at(a)}: {use.get(k, 0)} clusters | {m.refname(a + 4)} | "
              f"background {m.refname(a + 0x20)} | inside {m.refname(a + 0x30)}")
        for o in (0x20, 0x30):
            r = m.ref(a + o)
            if r is not None and r not in shown and use.get(k, 0):
                shown.add(r)
                looping(r)
    if use.get(-1):
        print(f"  no acoustics: {use[-1]} clusters")

palette = [m.ref(e) for e in m.elements(scnr + 0x1D4, 16)]
placed = m.elements(scnr + 0x1C8, 0x8C)
print(f"sound scenery: {len(placed)} placements (scnr +0x1C8, 0x8C each), palette {len(palette)} (+0x1D4)")
for off in (0x54, 0x58, 0x6C, 0x78, 0x84):
    print(f"  +{off:#x}", Counter(val(e + off) for e in placed).most_common(6))
for t in palette:
    if t is None:
        continue
    att = m.elements(m.meta(t) + 0x104, 0x20)
    print(f"  ssce {m.names[t]}: attachments {len(att)}", m.refname(att[0]) if att else "-")

sncl = m.meta(m.find("sncl"))
n, a = m.block(sncl)
print(f"sound classes: {n}, 0xD4 each")
for c in (4, 23, 32, 33, 35, 37):
    e = a + 0xD4 * c
    print(f"  class {c}: per tag {m.i16(e)} per object {m.i16(e + 2)} distances "
          f"{[round(m.f32(e + o), 1) for o in (0x38, 0x3C, 0x48, 0x54)]} gain {m.f32(e + 0x5C):g} / {m.f32(e + 0x60):g} "
          f"dialogue duck {m.f32(e + 0x80):g} fades {m.f32(e + 0x84):g} / {m.f32(e + 0x8C):g}")

if "--tracks" in sys.argv:
    slots, flags, fades = defaultdict(Counter), Counter(), Counter()
    for t in m.all("lsnd"):
        for e in m.elements(m.meta(t) + 0x20, 0xB0):
            flags[m.u32(e + 4)] += 1
            fades[(round(m.f32(e + 0x7C), 2), round(m.f32(e + 0x84), 2))] += 1
            for off, _ in SLOTS:
                r = m.ref(e + off)
                if r is not None and m.fourcc(e + off) == "snd!":
                    slots[off][m.names[r].split("\\")[-1]] += 1
    for off in sorted(slots):
        print(f"slot +{off:#x}", slots[off].most_common(6))
    print("track flags", flags.most_common())
    print("fade in / out", fades.most_common(8))
