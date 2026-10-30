# Halo 2 sound (docs/format/halo2.md section 11), read-only. Prints Lockout's
# background sounds and sound scenery, then checks that every Xbox ADPCM
# chunk of the map's and shared.map's sound gestalt (ugh!) is whole blocks
# with valid headers in the file the sound's flags pick:
# sounds_remastered.dat for flag 0x800, else sounds_neutral.dat.
# Run: python -I probe_h2_sound.py [map]
import collections, mmap, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h2_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "lockout")

scnr = m.addr(m.find("scnr")["meta"])
for bsp in m.elements(scnr + 0x210, 0x44):
    sb, so = m.meta(m.ref(bsp + 0x10))
    clusters = collections.Counter(sb.i16(c + 0x72) for c in sb.elements(so + 0x9C, 0xB0))
    for k, e in enumerate(sb.elements(so + 0xD4, 0x64)):
        print("background", k, sb.cstr(e)[:32], sb.tag_name(sb.ref(e + 0x20)), "clusters", clusters[k])
palette = m.elements(scnr + 0xE0, 0x28)
for e in m.elements(scnr + 0xD8, 0x50):
    print("sound scenery", m.tag_name(m.ref(palette[m.i16(e)])), "at", tuple(round(v, 2) for v in m.vec(e + 8)))

DIR = os.path.dirname(m.path)
handles = {k: open(os.path.join(DIR, "sounds_%s.dat" % k), "rb") for k in ("neutral", "remastered")}
files = {k: mmap.mmap(h.fileno(), 0, access=mmap.ACCESS_READ) for k, h in handles.items()}
for mm, label in ((m, m.tags[0]["name"]), (m.shared(), "shared.map")):
    ugh = mm.addr([t for t in mm.tags if t["cls"] == "ugh!" and t["meta"]][0]["meta"])
    codecs = [(mm.u8(c), mm.u8(c + 1), mm.u8(c + 2)) for c in mm.elements(ugh, 3)]
    chunks = mm.elements(mm.elements(ugh + 0x60, 8)[0], 0x10)  # language 0
    ranges, perms = mm.elements(ugh + 0x28, 0xC), mm.elements(ugh + 0x30, 0x10)
    result = collections.Counter()
    for t in mm.tags:
        if t["cls"] != "snd!" or not t["meta"]:
            continue
        a = mm.addr(t["meta"])
        flags, codec = mm.u16(a), codecs[mm.i8(a + 3)]
        if codec[2] != 1 or flags & 0x80:  # Xbox ADPCM, language-neutral
            continue
        channels = {0: 1, 1: 2, 3: 4}[codec[1]]
        block = 36 * channels
        data = files["remastered" if flags & 0x800 else "neutral"]
        first = mm.i16(a + 8)
        for r in ranges[first:first + mm.i8(a + 0xA)]:
            for q in perms[mm.i16(r + 8):mm.i16(r + 8) + mm.i16(r + 0xA)]:
                info = mm.elements(q + 8, 8)
                if not info:
                    continue
                c0, n = mm.i16(info[0] + 4), mm.i16(info[0] + 6)
                for e in chunks[c0:c0 + n]:
                    off, size, pcm = mm.u32(e), mm.u32(e + 4) & 0xFFFFFF, mm.u32(e + 8)
                    d = data[off:off + size]
                    ok = size % block == 0 and size // block * 64 * 2 * channels == pcm and all(
                        d[b + 4 * k + 2] <= 88 and d[b + 4 * k + 3] == 0
                        for b in range(0, size, block) for k in range(channels))
                    result[ok] += 1
    print(label, "Xbox ADPCM chunks valid / invalid:", result[True], "/", result[False])
