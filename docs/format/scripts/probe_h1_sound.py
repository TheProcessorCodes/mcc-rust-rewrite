# Halo CE sound in MCC (docs/format/halo1.md 13): the `snd!` and `lsnd`
# tags in the map, the FMOD banks in halo1\sound\pc and their `.lst.bin`
# indexes. Read-only.
#
# python -I probe_h1_sound.py raw TAGNAME [map]       hex of a tag's data
# python -I probe_h1_sound.py lsnd NAMEPART [map]     looping sounds' tracks
# python -I probe_h1_sound.py snd NAMEPART [map]      sounds' pitch ranges and permutations
# python -I probe_h1_sound.py lst BANK [grep]         a .lst.bin index (BANK: sounds_music, ...)
# python -I probe_h1_sound.py fsb BANK [first [n]]    FSB5 header and sample headers
# python -I probe_h1_sound.py find NAMEPART [map]     which bank and samples a sound's permutations are
#
# HALO1_MAPS: the maps folder; the sound folder is its sibling `sound\pc`.
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map, MAPS

SOUND = os.path.join(os.path.dirname(os.path.normpath(MAPS)), "sound", "pc")
BANKS = ["sounds_music", "sounds_dialogs", "sounds_adpcm", "sounds_stream", "sounds_debug"]


def hexdump(m, o, n):
    for r in range(0, n, 16):
        print("  %4x  %s" % (r, " ".join("%08x" % m.u32(o + r + k) for k in range(0, 16, 4))))


def lst(bank):
    """A .lst.bin: u32 version (1), u32 count, then per entry u32 length
    (with the NUL), the path, u32 first sample, u32 sample count."""
    d = open(os.path.join(SOUND, "lst", bank + ".lst.bin"), "rb").read()
    ver, n = struct.unpack_from("<II", d, 0)
    o = 8
    out = []
    for _ in range(n):
        ln, = struct.unpack_from("<I", d, o)
        path = d[o + 4:o + 4 + ln].split(b"\0")[0].decode("latin1")
        first, count = struct.unpack_from("<II", d, o + 4 + ln)
        out.append((path, first, count))
        o += 12 + ln
    assert o == len(d), (o, len(d))
    return ver, out


RATES = {1: 8000, 2: 11000, 3: 11025, 4: 16000, 5: 22050, 6: 24000, 7: 32000, 8: 44100, 9: 48000}


def fsb(bank):
    f = open(os.path.join(SOUND, bank + ".fsb"), "rb")
    h = f.read(0x3C)
    magic, ver, n, shs, nts, ds, codec = struct.unpack_from("<4sIIIIII", h, 0)
    heads = f.read(shs)
    samples = []
    o = 0
    for _ in range(n):
        v, = struct.unpack_from("<Q", heads, o)
        o += 8
        more = v & 1
        rate = RATES.get((v >> 1) & 0xF, (v >> 1) & 0xF)
        ch = 2 if (v >> 5) & 1 else 1
        off = ((v >> 6) & ((1 << 28) - 1)) * 16
        frames = v >> 34
        chunks = []
        while more:
            c, = struct.unpack_from("<I", heads, o)
            more = c & 1
            size = (c >> 1) & 0xFFFFFF
            kind = c >> 25
            chunks.append((kind, heads[o + 4:o + 4 + size]))
            o += 4 + size
        samples.append(dict(rate=rate, ch=ch, off=off, frames=frames, chunks=chunks))
    names = f.read(nts)
    if nts:
        for i, s in enumerate(samples):
            no, = struct.unpack_from("<I", names, 4 * i)
            s["name"] = names[no:names.index(b"\0", no)].decode("latin1")
    return dict(magic=magic, ver=ver, n=n, shs=shs, nts=nts, ds=ds, codec=codec, flags=struct.unpack_from("<I", h, 0x1C)[0],
                data=0x3C + shs + nts, samples=samples)


# --- tags ---------------------------------------------------------------

def lsnd(m, o):
    print("  flags %#x  zero-scale period %.2f  one-scale period %.2f  max distance? %s" % (
        m.u32(o), m.f32(o + 4), m.f32(o + 0x10), m.f32(o + 0x20)))
    for i, e in enumerate(m.block(o + 0x3C, 0xA0)):
        print("  track %d: flags %#x gain %.2f fade in %.2f fade out %.2f" % (i, m.u32(e), m.f32(e + 4), m.f32(e + 8), m.f32(e + 0xC)))
        for name, at in (("start", 0x30), ("loop", 0x40), ("end", 0x50), ("alt loop", 0x80), ("alt end", 0x90)):
            print("    %-8s %s" % (name, m.refname(e + at)))
    for i, e in enumerate(m.block(o + 0x48, 0x68)):
        print("  detail %d: %s period %.1f-%.1f gain %.2f-%.2f flags %#x yaw %.2f..%.2f pitch %.2f..%.2f distance %.1f..%.1f" % (
            i, m.refname(e), m.f32(e + 0x10), m.f32(e + 0x14), m.f32(e + 0x18), m.f32(e + 0x1C), m.u32(e + 0x20),
            m.f32(e + 0x50), m.f32(e + 0x54), m.f32(e + 0x58), m.f32(e + 0x5C), m.f32(e + 0x60), m.f32(e + 0x64)))


def snd(m, o):
    print("  flags %#x class %d rate %d distance %.2f..%.2f skip %.2f pitch %.2f..%.2f gain %.2f encoding %d compression %d" % (
        m.u32(o), m.u16(o + 4), m.u16(o + 6), m.f32(o + 8), m.f32(o + 0xC), m.f32(o + 0x10), m.f32(o + 0x14),
        m.f32(o + 0x18), m.f32(o + 0x28), m.u16(o + 0x6C), m.u16(o + 0x6E)))
    for i, pr in enumerate(m.block(o + 0x98, 0x48)):
        perms = m.block(pr + 0x3C, 0x7C)
        print("  pitch range %d %r natural %.2f bend %.2f..%.2f actual %d, %d permutations" % (
            i, m.cstr(pr), m.f32(pr + 0x20), m.f32(pr + 0x24), m.f32(pr + 0x28), m.i16(pr + 0x2C), len(perms)))
        for j, p in enumerate(perms):
            print("    %2d %-16r skip %.2f gain %.2f comp %d next %d | data size %d flags %#x off %#x | %s" % (
                j, m.cstr(p), m.f32(p + 0x20), m.f32(p + 0x24), m.u16(p + 0x28), m.i16(p + 0x2A),
                m.u32(p + 0x40), m.u32(p + 0x44), m.u32(p + 0x48),
                " ".join("%08x" % m.u32(p + k) for k in range(0x2C, 0x40, 4))))


def main():
    a = sys.argv[1:]
    cmd = a[0]
    if cmd == "lst":
        ver, entries = lst(a[1])
        print("version", ver, "entries", len(entries), "samples", sum(c for _, _, c in entries))
        for p, f, c in entries:
            if len(a) < 3 or a[2].lower() in p.lower():
                print("%6d %4d %s" % (f, c, p))
        return
    if cmd == "fsb":
        b = fsb(a[1])
        print({k: (hex(v) if isinstance(v, int) else v) for k, v in b.items() if k != "samples"})
        first = int(a[2]) if len(a) > 2 else 0
        n = int(a[3]) if len(a) > 3 else 10
        for i in range(first, min(first + n, len(b["samples"]))):
            s = b["samples"][i]
            print(i, s.get("name"), s["rate"], s["ch"], hex(s["off"]), s["frames"], [(k, len(c), c[:8].hex()) for k, c in s["chunks"]])
        from collections import Counter
        print("rates", Counter(s["rate"] for s in b["samples"]), "channels", Counter(s["ch"] for s in b["samples"]),
              "chunk kinds", Counter(k for s in b["samples"] for k, _ in s["chunks"]))
        return
    m = Map(a[2] if len(a) > 2 else "d40")
    want = a[1].lower()
    if cmd == "raw":
        t = [t for t in m.tags if t["name"].lower() == want][0]
        hexdump(m, m.addr(t["data"]), int(a[3], 16) if len(a) > 3 else 0x100)
        return
    if cmd in ("lsnd", "snd"):
        cls = "lsnd" if cmd == "lsnd" else "snd!"
        for t in m.tags:
            if t["cls"] == cls and want in t["name"].lower():
                print("#%d %s" % (t["index"], t["name"]))
                (lsnd if cmd == "lsnd" else snd)(m, m.addr(t["data"]))
        return
    if cmd == "check":
        # Every snd! of the map: found in a bank by path? Does the sample
        # count equal the pitch ranges' actual permutation counts, and does
        # sample first+k carry permutation k's name?
        from collections import Counter
        idx, banks = {}, {}
        for b in BANKS:
            try:
                for p, f, c in lst(b)[1]:
                    idx.setdefault(p.lower(), []).append((b, f, c))
                banks[b] = fsb(b)
            except FileNotFoundError:
                pass
        stats = Counter()
        for t in m.tags:
            if t["cls"] != "snd!":
                continue
            o = m.addr(t["data"])
            hits = idx.get(t["name"].lower())
            if not hits:
                stats["not in any bank"] += 1
                if want != "-" and want in t["name"].lower():
                    print("missing", t["name"])
                continue
            b, first, count = hits[0]
            stats["in " + b] += 1
            names = []
            for pr in m.block(o + 0x98, 0x48):
                actual = m.i16(pr + 0x2C)
                perms = m.block(pr + 0x3C, 0x7C)
                names += [m.cstr(p) for p in perms[:actual]]
            stats["count matches" if len(names) == count else "count differs"] += 1
            got = [banks[b]["samples"][first + k].get("name", "") for k in range(count)]
            ok = [a.lower() == g.lower() for a, g in zip(names, got)]
            stats["names match" if all(ok) and ok else "names differ"] += 1
            if (not all(ok) or len(names) != count) and want in t["name"].lower():
                print(t["name"], b, first, count, names[:6], got[:6])
            if len(hits) > 1:
                stats["in several banks"] += 1
        print(dict(stats))
        return
    if cmd == "find":
        idx = {}
        for b in BANKS:
            try:
                for p, f, c in lst(b)[1]:
                    idx.setdefault(p.lower(), []).append((b, f, c))
            except FileNotFoundError:
                pass
        for t in m.tags:
            if t["cls"] == "snd!" and want in t["name"].lower():
                print("#%d %s -> %s" % (t["index"], t["name"], idx.get(t["name"].lower())))


if __name__ == "__main__":
    main()
