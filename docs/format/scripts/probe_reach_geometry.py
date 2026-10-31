# Read-only probe: MCC Reach render geometry (docs/format/reach.md 4, 5, 7).
# For a map: each structure BSP's lightmap-BSP-data (Lbsp) meshes (+0x7C,
# 0x5C each: parts +0, vertex buffers +0x18 (8 x i16), index buffer +0x28,
# flags u16 +0x2C, rigid node +0x2E, vertex type +0x2F, index type +0x32),
# compression info (+0x88, 0x34 each) and geometry resource (+0x10C); the
# clusters' mesh index (sbsp +0x138, 0x8C each, +0x40); the instances in
# the structure_bsp_cache_file_tag_resources resource (sbsp +0x520; root
# block +0x48, 0x9C each: definition +0x34, flags +0x36, mesh +0x38); and
# the render models of the placed objects (object +0x64 -> hlmt +0 ->
# mode: sections +0x68 (0x5C), bounds +0x74 (0x34), resource +0xF8).
# Checks that every vertex buffer's size is count x stride, that every
# index fits its vertex buffer, and prints the (vertex type, buffer type,
# stride) pairs seen. Use:  python -I probe_reach_geometry.py [map]
import collections
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from reach_common import Map  # noqa: E402


def fixed_definition(m, rid):
    zone = m.meta(m.find("zone"))
    n, first = m.block(zone + 0x64)
    e = first + 64 * (rid & 0xFFFF)
    info, d, f, data = m.resource(rid)
    d = bytearray(d)
    for o in m.elements(e + 0x28, 8):
        at, addr = m.u32(o), m.u32(o + 4)
        d[at:at + 4] = struct.pack("<I", addr)
    return m.u32(e + 0x24), bytes(d), f, data


def geometry(m, rid):
    """Vertex buffers (count, type, stride, offset) and index buffers (format, length, offset)."""
    _, d, f, data = fixed_definition(m, rid)
    nvb = struct.unpack_from("<I", d, len(d) - 24)[0]
    nib = struct.unpack_from("<I", d, len(d) - 12)[0]
    vbs = []
    for i in range(nvb):
        count, kind, stride, length = struct.unpack_from("<IHHI", d, 28 * i)
        vbs.append((count, kind, stride, length, f[i]))
    ib_base = 40 * nvb
    ibs = []
    for j in range(nib):
        fmt, _, length = struct.unpack_from("<iiI", d, ib_base + 28 * j)
        ibs.append((fmt, length, f[2 * nvb + j]))
    return vbs, ibs, data


def check_meshes(m, meshes, vbs, ibs, data, pairs, problems, what):
    for k, s in enumerate(meshes):
        vb = m.i16(s + 0x18)
        ib = m.i16(s + 0x28)
        vtype = m.u8(s + 0x2F)
        if vb < 0:
            continue
        count, kind, stride, length, off = vbs[vb]
        pairs[(what, vtype, kind, stride)] += 1
        if count * stride != length:
            problems.append(f"{what} mesh {k}: {count} x {stride} != {length}")
        if ib >= 0:
            fmt, ilen, ioff = ibs[ib]
            width = 4 if count > 0xFFFF else 2
            idx = struct.unpack_from("<%d%s" % (ilen // width, "I" if width == 4 else "H"), data, ioff)
            if idx and max(idx) >= count:
                problems.append(f"{what} mesh {k}: index {max(idx)} >= {count} vertices")
            for p in m.elements(s, 0x18):
                start, n = m.u32(p + 4), m.u32(p + 8)
                if start + n > len(idx):
                    problems.append(f"{what} mesh {k}: part runs past {len(idx)} indices")


name = sys.argv[1] if len(sys.argv) > 1 else "20_sword_slayer"
m = Map(name)
scnr = m.meta(m.find("scnr", m.cstr(0xE0)))
sldt = m.meta(m.ref(scnr + 0x708))
lbsps = m.elements(sldt + 4, 0x20)
pairs = collections.Counter()
problems = []
for b, bo in enumerate(m.elements(scnr + 0x50, 0xAC)):
    si = m.ref(bo)
    s = m.meta(si)
    li = m.ref(lbsps[b]) if b < len(lbsps) else None
    if li is None or m.block(m.meta(li) + 0x7C)[0] == 0:
        print(f"bsp {b} {m.names[si]}: no lightmap geometry")
        continue
    lb = m.meta(li)
    meshes = m.elements(lb + 0x7C, 0x5C)
    vbs, ibs, data = geometry(m, m.u32(lb + 0x10C))
    check_meshes(m, meshes, vbs, ibs, data, pairs, problems, "bsp")
    clusters = [m.i16(c + 0x40) for c in m.elements(s + 0x138, 0x8C)]
    root, d, f, _ = fixed_definition(m, m.u32(s + 0x520))
    r = root & 0x1FFFFFFF
    n, addr = struct.unpack_from("<II", d, r + 0x48)
    inst = [struct.unpack_from("<hHhh", d, (addr & 0x1FFFFFFF) + 0x9C * i + 0x34) for i in range(n)]
    names = m.block(s + 0x258)[0]
    print(f"bsp {b} {m.names[si]}: {len(meshes)} meshes, {len(vbs)} vertex / {len(ibs)} index buffers, "
          f"{len(clusters)} clusters (meshes {min(clusters)}..{max(clusters)}), {n} instances ({names} names), "
          f"instance meshes {min((i[2] for i in inst), default=-1)}..{max((i[2] for i in inst), default=-1)}, "
          f"render-only {sum(1 for i in inst if i[1] & 2)}")
seen = set()
models = 0
for blk, pal in ((0xFC, 0x108), (0x180, 0x18C), (0x1B0, 0x1BC), (0x600, 0x60C)):
    for p in m.elements(scnr + pal, 0x10):
        obj = m.ref(p)
        if obj is None:
            continue
        hlmt = m.ref(m.meta(obj) + 0x64)
        if hlmt is None or m.cls(hlmt) != "hlmt":
            continue
        mode = m.ref(m.meta(hlmt))
        if mode is None or mode in seen:
            continue
        seen.add(mode)
        mo = m.meta(mode)
        rid = m.u32(mo + 0xF8)
        if rid in (0, 0xFFFFFFFF):
            continue
        vbs, ibs, data = geometry(m, rid)
        check_meshes(m, m.elements(mo + 0x68, 0x5C), vbs, ibs, data, pairs, problems, "model")
        models += 1
print(f"{models} render models")
for k, v in sorted(pairs.items()):
    print("  (what, vertex type, buffer type, stride)", k, "x", v)
print(f"{len(problems)} problems", problems[:10])
