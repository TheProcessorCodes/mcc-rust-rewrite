"""Probe MCC Halo 3's dialogue: the language banks' .info sidecars and a map's mission
dialogue tag (mdlg). docs/format/sound.md section 11.

python -I docs/format/scripts/probe_dialogue.py "<halo3 dir>" [map] [line filter]

Prints, per language bank: record count, duplicate ids, ids shared with sfx.fsb, whether
the ids match the English bank's in order, how many ids are djb2 of the record's own path,
path prefixes and channel counts. Then the map's mdlg (default 120_halo): the line and
variant layout, counts of speakers and effects, and each line (matching the filter) with
its speaker, effect, snd! tag and the take's length in the English bank.

Read-only. Written from our own probing, no GPL code.
"""
import os
import struct
import sys
from collections import Counter

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map  # noqa: E402

LANGUAGES = ["english", "french", "german", "italian", "mexican", "spanish"]


def info(path):
    b = open(path, "rb").read()
    out = []
    for i in range(len(b) // 280):
        r = b[i * 280:(i + 1) * 280]
        rid, pcm, ch, _, _, loop = struct.unpack_from("<6I", r)
        out.append((rid, pcm, ch, loop, r[24:].split(b"\0")[0].decode("latin-1")))
    return out


def djb2(s):
    h = 5381
    for c in s.encode("latin-1"):
        h = (h * 33 + c) & 0xFFFFFFFF
    return h


def own_id(path):
    p = path.split("\\", 2)[2] if path.count("\\") >= 2 else path  # drop data*\sound\
    return djb2(p.rsplit(".", 1)[0])


halo3 = sys.argv[1]
map_name = sys.argv[2] if len(sys.argv) > 2 else "120_halo"
line_filter = sys.argv[3] if len(sys.argv) > 3 else ""
pc = os.path.join(halo3, "fmod", "pc")

sfx_ids = {r[0] for r in info(os.path.join(pc, "sfx.fsb.info"))}
english = info(os.path.join(pc, "english.fsb.info"))
# English length by source path under data\sound\ without the extension.
seconds = {r[4].split("\\", 2)[2].rsplit(".", 1)[0]: r[1] / (max(r[2], 1) * 2) / 48000 for r in english}
for lang in LANGUAGES:
    recs = info(os.path.join(pc, lang + ".fsb.info"))
    ids = [r[0] for r in recs]
    prefixes = Counter(r[4].split("\\", 1)[0] for r in recs)
    print(
        f"{lang}: {len(recs)} records, {len(ids) - len(set(ids))} duplicate ids, "
        f"{len(set(ids) & sfx_ids)} shared with sfx, same ids as english in order: "
        f"{ids == [r[0] for r in english]}, djb2 of own path under data*\\sound\\: {sum(own_id(r[4]) == r[0] for r in recs)}, "
        f"prefixes {dict(prefixes)}, channels {dict(Counter(r[2] for r in recs))}"
    )

m = Map(map_name)
for t in m.all("mdlg"):
    b = m.meta(t)
    lines = m.elements(b, 20)
    print(f"\nmdlg {m.names[t]}: {len(lines)} lines of 20 bytes")
    speakers, effects, variants = Counter(), Counter(), Counter()
    for ln in lines:
        vs = m.elements(ln + 4, 24)
        variants[len(vs)] += 1
        effects[m.sid_at(ln + 16) or "-"] += 1
        for v in vs:
            speakers[m.sid_at(v)] += 1
    print(f"variants per line {dict(variants)}; speakers {dict(speakers)}; line effects {dict(effects)}")
    for ln in lines:
        name = m.sid_at(ln)
        if line_filter not in name:
            continue
        for v in m.elements(ln + 4, 24):
            snd = m.ref(v + 4)
            length = "-"
            if snd is not None:
                # snd! + 0x14: the longest permutation, ms (sound.md 4.1).
                length = f"tag {m.i32(m.meta(snd) + 0x14)} ms"
                path = m.names[snd].split("\\", 1)[1] if "\\" in m.names[snd] else m.names[snd]
                # The take is the permutation named like the tag (one per line here).
                if path in seconds:
                    length += f", english {seconds[path]:.3f} s"
            print(
                f"  {name:<10} {m.sid_at(v):<4} line fx {m.sid_at(ln + 16) or '-':<16} "
                f"variant fx {m.sid_at(v + 20) or '-':<14} {m.refname(v + 4)}  [{length}]"
            )
