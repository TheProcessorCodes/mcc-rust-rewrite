# Read-only probe: MCC Reach bitmap tags (docs/format/reach.md 8).
# For every bitm tag of a map: the Bitmap Data block at +0x7C (56 bytes per
# bitmap: width +0, height +2, depth +4, type +6, format +8, mipmaps +0x10,
# curve +0x11), the resource block at +0xA8 (8 bytes, resource id first),
# and the resource definition (72 bytes: Halo 3's texture description with
# four more bytes at +0x30: width +0x28, height +0x2A, levels +0x2D, DXGI
# format +0x34, Halo format +0x38, curve +0x39). Prints the (Halo format, DXGI format) pairs seen and how many
# bitmaps disagree with their definition.
# Use:  python -I probe_reach_bitmaps.py [map]
import collections
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from reach_common import Map  # noqa: E402

name = sys.argv[1] if len(sys.argv) > 1 else "20_sword_slayer"
m = Map(name)
pairs = collections.Counter()
kinds = collections.Counter()
mismatch = 0
total = 0
no_resource = 0
for t in m.all("bitm"):
    base = m.meta(t)
    datas = m.elements(base + 0x7C, 0x38)
    res = m.elements(base + 0xA8, 8)
    for k, d in enumerate(datas):
        total += 1
        w, h, depth = m.u16(d), m.u16(d + 2), m.u8(d + 4)
        kind, fmt, mips, curve = m.u8(d + 6), m.u16(d + 8), m.u8(d + 0x10), m.u8(d + 0x11)
        kinds[kind] += 1
        if k >= len(res):
            no_resource += 1
            continue
        rid = m.u32(res[k])
        if rid in (0, 0xFFFFFFFF):
            no_resource += 1
            continue
        _, definition, _, _ = m.resource(rid, data=False)
        if len(definition) < 0x3A:
            no_resource += 1
            continue
        dw, dh = struct.unpack_from("<HH", definition, 0x28)
        levels = definition[0x2D]
        dxgi = struct.unpack_from("<i", definition, 0x34)[0]
        dfmt = definition[0x38]
        pairs[(fmt, dxgi)] += 1
        if (dw, dh) != (w, h) or dfmt != fmt or levels != mips + 1:
            mismatch += 1
print(f"{name}: {total} bitmaps, {no_resource} without a resource, {mismatch} disagree with their definition")
print("types:", dict(kinds))
for (fmt, dxgi), n in sorted(pairs.items()):
    print(f"  format {fmt:3d}  DXGI {dxgi:3d}  x{n}")
