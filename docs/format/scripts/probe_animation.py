# Probe for MCC Halo 3 (U13) model animation graphs (`jmad`), read-only.
# Walks a jmad tag: skeleton nodes, animations, the mode -> weapon class ->
# weapon type -> action graph, resource groups; decodes an animation's codec
# data into per-node local transforms and compares frame 0 of a base
# animation with the render model's bind pose. Layouts: docs/format/animation.md.
#
#   python -I docs/format/scripts/probe_animation.py 120_halo "objects\characters\floodcombat_human\floodcombat_human"
#   python -I docs/format/scripts/probe_animation.py 120_halo <jmad name> --anim "combat:rifle:move_front" [--frame N]
#   python -I docs/format/scripts/probe_animation.py 120_halo --codecs     (codec histogram over every jmad)
import math
import os
import struct
import sys
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map  # noqa: E402
from h3_resources import Resources  # noqa: E402

ROOT_NODES, ROOT_ANIMS, ROOT_MODES, ROOT_GROUPS = 0x14, 0x50, 0x5C, 0xF8
ANIM_SIZE, MODE_SIZE, CLASS_SIZE, TYPE_SIZE, MEMBER_SIZE = 136, 40, 28, 52, 48


def anim_header(m, a):
    return {
        "name": m.sid_at(a),
        "weight": m.f32(a + 4),
        "loop_frame": m.i16(a + 8),
        "playback_flags": m.u16(a + 10),
        "blend_screen": m.i8(a + 12),
        "desired_compression": m.u8(a + 13),
        "current_compression": m.u8(a + 14),
        "nodes": m.u8(a + 15),
        "frames": m.i16(a + 16),
        "type": m.u8(a + 18),
        "movement": m.u8(a + 19),
        "production_flags": m.u16(a + 20),
        "internal_flags": m.u16(a + 22),
        "node_checksum": m.u32(a + 24),
        "checksum": m.u32(a + 28),
        "prev_variant": m.i16(a + 36),
        "next_variant": m.i16(a + 38),
        "group": m.i16(a + 40),
        "member": m.i16(a + 42),
    }


def member(R, m, jmad_meta, h):
    groups = m.elements(jmad_meta + ROOT_GROUPS, 12)
    if h["group"] < 0 or h["group"] >= len(groups):
        return None, None
    e = R.entry(m.u32(groups[h["group"]] + 4))
    if not e["salt_ok"] or e["segment"] < 0:
        return None, None
    d = R.definition(e)
    o = h["member"] * MEMBER_SIZE
    mem = {
        "checksum": struct.unpack_from("<I", d, o + 4)[0],
        "frames": struct.unpack_from("<h", d, o + 8)[0],
        "nodes": d[o + 10],
        "movement": d[o + 11],
        "static_flags": d[o + 12],
        "animated_flags": d[o + 13],
        "movement_size": struct.unpack_from("<H", d, o + 14)[0],
        "pill_size": struct.unpack_from("<H", d, o + 16)[0],
        "static_size": struct.unpack_from("<H", d, o + 18)[0],
        "uncompressed_size": struct.unpack_from("<I", d, o + 20)[0],
        "animated_size": struct.unpack_from("<I", d, o + 24)[0],
        "data_size": struct.unpack_from("<I", d, o + 28)[0],
        "address": struct.unpack_from("<I", d, o + 40)[0],
    }
    page, seg = R.data(e)
    if page is None:
        return mem, None
    start = seg + (mem["address"] & 0x1FFFFFFF)
    return mem, page[start:start + mem["data_size"]]


def q16(b, o):
    x, y, z, w = struct.unpack_from("<4h", b, o)
    q = [x / 32767.0, y / 32767.0, z / 32767.0, w / 32767.0]
    n = math.sqrt(sum(c * c for c in q)) or 1.0
    return [c / n for c in q]


def codec(b, base, frames):
    """Decodes one codec block at `base` into ([rot tracks], [trans tracks], [scale tracks]),
    each track a list of `frames` values."""
    kind, nr, nt, ns = b[base], b[base + 1], b[base + 2], b[base + 3]
    u = lambda o: struct.unpack_from("<I", b, base + o)[0]
    rots, trans, scales = [], [], []
    if kind == 1:  # static: one value per track
        t_off, s_off = u(12), u(16)
        rots = [[q16(b, base + 32 + 8 * i)] * frames for i in range(nr)]
        trans = [[list(struct.unpack_from("<3f", b, base + t_off + 12 * i))] * frames for i in range(nt)]
        scales = [[struct.unpack_from("<f", b, base + s_off + 4 * i)[0]] * frames for i in range(ns)]
    elif kind in (2, 3, 8):  # every frame stored, track after track
        t_off, s_off, rs, ts, ss = u(12), u(16), u(20), u(24), u(28)
        rsize = 16 if kind == 8 else 8
        n = rs // rsize
        for i in range(nr):
            tr = []
            for f in range(n):
                o = base + 32 + rs * i + rsize * f
                if kind == 8:
                    q = list(struct.unpack_from("<4f", b, o))
                    l = math.sqrt(sum(c * c for c in q)) or 1.0
                    tr.append([c / l for c in q])
                else:
                    tr.append(q16(b, o))
            rots.append(tr)
        trans = [[list(struct.unpack_from("<3f", b, base + t_off + ts * i + 12 * f)) for f in range(ts // 12)] for i in range(nt)]
        scales = [[struct.unpack_from("<f", b, base + s_off + ss * i + 4 * f)[0] for f in range(ss // 4)] for i in range(ns)]
    elif kind in (4, 5, 6, 7):  # keyframes: per track (count, offset) into key frame indices and values
        word = kind in (5, 7)
        offs = [u(12 + 4 * k) for k in range(8)]
        th, sh, rk, tk, sk, rd, td, sd = offs
        heads = [(32 + 16, nr, rk, rd, 8), (th, nt, tk, td, 12), (sh, ns, sk, sd, 4)]
        out = []
        for hoff, count, koff, doff, vsize in heads:
            tracks = []
            for i in range(count):
                hv = u(hoff + 4 * i)
                kc, ko = hv & 0xFFF, hv >> 12
                keys = []
                for k in range(kc):
                    if word:
                        fr = struct.unpack_from("<H", b, base + koff + 2 * (ko + k))[0]
                    else:
                        fr = b[base + koff + ko + k]
                    vo = base + doff + vsize * (ko + k)
                    if vsize == 8:
                        v = q16(b, vo)
                    elif vsize == 12:
                        v = list(struct.unpack_from("<3f", b, vo))
                    else:
                        v = struct.unpack_from("<f", b, vo)[0]
                    keys.append((fr, v))
                tracks.append(interp_keys(keys, frames, vsize))
            out.append(tracks)
        rots, trans, scales = out
    else:
        raise ValueError(f"codec {kind}")
    return kind, rots, trans, scales


def lerp(a, b, t):
    if isinstance(a, float):
        return a + (b - a) * t
    return [x + (y - x) * t for x, y in zip(a, b)]


def nlerp(a, b, t):
    if sum(x * y for x, y in zip(a, b)) < 0:
        b = [-y for y in b]
    q = lerp(a, b, t)
    n = math.sqrt(sum(c * c for c in q)) or 1.0
    return [c / n for c in q]


def interp_keys(keys, frames, vsize):
    out = []
    k = 0
    for f in range(frames):
        while k + 1 < len(keys) and keys[k + 1][0] <= f:
            k += 1
        f0, v0 = keys[k]
        if k + 1 < len(keys) and f > f0:
            f1, v1 = keys[k + 1]
            t = (f - f0) / (f1 - f0)
            out.append(nlerp(v0, v1, t) if vsize == 8 else lerp(v0, v1, t))
        else:
            out.append(v0)
    return out


def bits(b, o, words):
    v = 0
    for k in range(words):
        v |= struct.unpack_from("<I", b, o + 4 * k)[0] << (32 * k)
    return v


def decode(mem, data, nodes):
    """-> {node: {"r": [...], "t": [...], "s": [...]}} plus movement frames."""
    frames = mem["frames"]
    words = (nodes + 31) // 32
    pose = {}
    sd, ad = mem["static_size"], mem["animated_size"]
    flag_at = sd + ad
    sflags = [bits(data, flag_at + 4 * words * k, words) for k in range(3)] if mem["static_flags"] else [0, 0, 0]
    flag_at += mem["static_flags"]
    aflags = [bits(data, flag_at + 4 * words * k, words) for k in range(3)] if mem["animated_flags"] else [0, 0, 0]
    flag_at += mem["animated_flags"]
    kinds = []
    for which, (start, size, flags) in enumerate(((0, sd, sflags), (sd, ad, aflags))):
        if size == 0:
            continue
        kind, rots, trans, scales = codec(data, start, frames)
        kinds.append(kind)
        for ch, tracks, fl in (("r", rots, flags[0]), ("t", trans, flags[1]), ("s", scales, flags[2])):
            nodes_with = [n for n in range(nodes) if fl >> n & 1]
            assert len(nodes_with) == len(tracks), (ch, which, len(nodes_with), len(tracks))
            for n, tr in zip(nodes_with, tracks):
                pose.setdefault(n, {})[ch] = tr
    move = []
    msize = {1: 8, 2: 12, 3: 16}.get(mem["movement"], 0)
    if msize:
        for f in range(mem["movement_size"] // msize):
            move.append(struct.unpack_from("<%df" % (msize // 4), data, flag_at + msize * f))
    return kinds, pose, move


def render_nodes(m, mode):
    out = []
    for o in m.elements(m.meta(mode) + 48, 96):
        out.append((m.sid_at(o), m.i16(o + 4), m.f32n(o + 12, 3), m.f32n(o + 24, 4)))
    return out


def main():
    map_name = sys.argv[1]
    m = Map(map_name)
    R = Resources(m)
    if "--codecs" in sys.argv:
        st, an, fails = Counter(), Counter(), Counter()
        for j in m.all("jmad"):
            meta = m.meta(j)
            for a in m.elements(meta + ROOT_ANIMS, ANIM_SIZE):
                h = anim_header(m, a)
                mem, data = member(R, m, meta, h)
                if data is None:
                    fails["no data"] += 1
                    continue
                try:
                    kinds, _, _ = decode(mem, data, mem["nodes"])
                except Exception as e:  # noqa: BLE001
                    fails[str(e)[:50]] += 1
                    continue
                if mem["static_size"]:
                    st[kinds[0]] += 1
                if mem["animated_size"]:
                    an[kinds[-1]] += 1
        print("static codecs", dict(st))
        print("animated codecs", dict(an))
        print("not decoded", dict(fails))
        return
    name = sys.argv[2]
    j = m.find("jmad", name)
    meta = m.meta(j)
    nodes = [m.sid_at(o) for o in m.elements(meta + ROOT_NODES, 32)]
    anims = m.elements(meta + ROOT_ANIMS, ANIM_SIZE)
    print(f"jmad {j} {name}: {len(nodes)} nodes, {len(anims)} animations, "
          f"{m.block(meta + ROOT_MODES)[0]} modes, {m.block(meta + ROOT_GROUPS)[0]} resource groups, "
          f"parent {m.refname(meta)}")
    if "--anim" not in sys.argv:
        for mo in m.elements(meta + ROOT_MODES, MODE_SIZE):
            for wc in m.elements(mo + 4, CLASS_SIZE):
                for wt in m.elements(wc + 4, TYPE_SIZE):
                    acts = [f"{m.sid_at(x)}={m.i16(x + 6)}" + (f"@g{m.i16(x + 4)}" if m.i16(x + 4) >= 0 else "")
                            for x in m.elements(wt + 4, 8)]
                    ovs = [m.sid_at(x) for x in m.elements(wt + 16, 8)]
                    print(f"  {m.sid_at(mo)}:{m.sid_at(wc)}:{m.sid_at(wt)} actions {acts} overlays {ovs}")
        return
    want = sys.argv[sys.argv.index("--anim") + 1]
    frame = int(sys.argv[sys.argv.index("--frame") + 1]) if "--frame" in sys.argv else 0
    a = anims[int(want)] if want.isdigit() else next(x for x in anims if m.sid_at(x) == want)
    h = anim_header(m, a)
    mem, data = member(R, m, meta, h)
    print(h)
    print(mem)
    kinds, pose, move = decode(mem, data, len(nodes))
    print("codecs", kinds, "movement frames", len(move), move[:3])
    # The render model next to the graph (same tag path), for the bind pose.
    mode = m.find("mode", name)
    rn = render_nodes(m, mode) if mode is not None else []
    for n, nm in enumerate(nodes):
        p = pose.get(n, {})
        r = p.get("r", [None])[min(frame, len(p.get("r", [None])) - 1)]
        t = p.get("t", [None])[min(frame, len(p.get("t", [None])) - 1)]
        s = p.get("s", [None])[min(frame, len(p.get("s", [None])) - 1)]
        bind = next((x for x in rn if x[0] == nm), None)
        dot = abs(sum(x * y for x, y in zip(r, bind[3]))) if r and bind else None
        dt = math.dist(t, bind[2]) if t and bind else None
        fmt = lambda v: "-" if v is None else ("%.3f" % v if isinstance(v, float) else "(" + ",".join("%.3f" % c for c in v) + ")")
        print(f"  {n:3} {nm:24} r {fmt(r)} t {fmt(t)} s {fmt(s)}   |q.bind| {fmt(dot)} |t-bind| {fmt(dt)}")


if __name__ == "__main__":
    main()
