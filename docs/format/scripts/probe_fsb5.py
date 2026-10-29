"""Probe MCC Halo 3's FMOD sound bank (FSB5) and its .fsb.info sidecar.

python -I docs/format/scripts/probe_fsb5.py "<halo3 dir>" [bank] [mode]

mode:
  header        the FSB5 header and the first samples (default)
  stats         histograms of rate, channels, setup-header CRC, chunk types
  setups        where the bank's setup CRCs sit in halo3.dll's table (the packets are
                rebuilt by crates/blam_sound, sound.md 3.2)
  info N        info record N and sample N side by side
  grep TEXT     info records whose path contains TEXT

Read-only. Written from public format facts (docs/format/sound.md), no GPL code.
"""
import os
import struct
import sys
import zlib

RATES = {1: 8000, 2: 11000, 3: 11025, 4: 16000, 5: 22050, 6: 24000, 7: 32000, 8: 44100, 9: 48000}


def read_fsb(path):
    f = open(path, "rb")
    head = f.read(0x3C)
    magic, ver, n, shsize, nsize, dsize, mode = struct.unpack_from("<4s6I", head)
    assert magic == b"FSB5", magic
    flags = struct.unpack_from("<I", head, 0x1C)[0]
    hsize = 0x3C if ver == 1 else 0x40
    f.seek(hsize)
    sh = f.read(shsize)
    names = f.read(nsize)
    data_base = hsize + shsize + nsize
    samples = []
    p = 0
    for i in range(n):
        v = struct.unpack_from("<Q", sh, p)[0]
        p += 8
        s = {
            "rate": RATES.get((v >> 1) & 0xF, -1),
            "channels": 2 if (v >> 5) & 1 else 1,
            "offset": ((v >> 6) & 0x0FFFFFFF) * 16,
            "frames": v >> 34,
            "chunks": [],
        }
        more = v & 1
        while more:
            c = struct.unpack_from("<I", sh, p)[0]
            p += 4
            more = c & 1
            size = (c >> 1) & 0xFFFFFF
            ctype = c >> 25
            s["chunks"].append((ctype, sh[p:p + size]))
            p += size
        for ctype, body in s["chunks"]:
            if ctype == 1:
                s["channels"] = body[0]
            elif ctype == 2:
                s["rate"] = struct.unpack_from("<I", body)[0]
            elif ctype == 3:
                s["loop"] = struct.unpack_from("<II", body)
            elif ctype == 11:
                s["crc"] = struct.unpack_from("<I", body)[0]
                s["seek"] = (len(body) - 4) // 8
        samples.append(s)
    for i, s in enumerate(samples):
        end = samples[i + 1]["offset"] if i + 1 < n else dsize
        s["size"] = end - s["offset"]
    return {
        "version": ver, "count": n, "sample_headers": shsize, "names": nsize,
        "data": dsize, "mode": mode, "flags": flags, "data_base": data_base,
        "samples": samples, "hsize": hsize,
    }


def read_info(path):
    d = open(path, "rb").read()
    out = []
    for p in range(0, len(d), 280):
        a, b, c, e, f, g = struct.unpack_from("<6I", d, p)
        name = d[p + 24:p + 280].split(b"\0")[0].decode("latin-1")
        out.append((a, b, c, e, f, g, name))
    return out


def main():
    root = sys.argv[1]
    bank = sys.argv[2] if len(sys.argv) > 2 else "sfx"
    mode = sys.argv[3] if len(sys.argv) > 3 else "header"
    fsb_path = os.path.join(root, "fmod", "pc", bank + ".fsb")
    fsb = read_fsb(fsb_path)
    info = read_info(fsb_path + ".info")
    if mode == "header":
        print({k: (hex(v) if isinstance(v, int) else v) for k, v in fsb.items() if k != "samples"})
        print("info records", len(info))
        for i in range(8):
            s = fsb["samples"][i]
            print(i, {k: v for k, v in s.items() if k != "chunks"}, [c[0] for c in s["chunks"]], info[i])
    elif mode == "stats":
        from collections import Counter
        print("rate", Counter(s["rate"] for s in fsb["samples"]))
        print("channels", Counter(s["channels"] for s in fsb["samples"]))
        print("chunk types", Counter(c[0] for s in fsb["samples"] for c in s["chunks"]))
        print("crc", Counter(hex(s.get("crc", 0)) for s in fsb["samples"]).most_common())
        print("loops", sum(1 for s in fsb["samples"] if "loop" in s))
        print("info fields 1..5", Counter((r[2], r[3], r[4], r[5]) for r in info).most_common(10))
        same = sum(1 for s, r in zip(fsb["samples"], info) if r[1] == s["frames"])
        print("info[1] == frames", same, "of", len(info))
        uniq = len(set(r[0] for r in info))
        print("distinct info[0]", uniq)
    elif mode == "setups":
        dll = open(os.path.join(root, "halo3.dll"), "rb").read()
        for c in sorted(set(s.get("crc") for s in fsb["samples"])):
            p = dll.find(struct.pack("<I", c))
            print(hex(c), "row at dll+%#x" % p, dll[p - 16:p + 16].hex(" ", 4))
    elif mode == "info":
        i = int(sys.argv[4])
        s = fsb["samples"][i]
        print({k: v for k, v in s.items() if k != "chunks"}, info[i])
    elif mode == "grep":
        t = sys.argv[4].lower()
        for i, r in enumerate(info):
            if t in r[6].lower():
                s = fsb["samples"][i]
                print(i, hex(r[0]), r[1:6], r[6], s["rate"], s["channels"], s["frames"], s.get("loop"))


if __name__ == "__main__":
    main()
