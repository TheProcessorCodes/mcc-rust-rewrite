# Read-only: lists the palette and the placements of scenario object blocks
# (scenery, bipeds, vehicles, equipment, weapons, machines, controls,
# crates), with each placement's palette tag, position and rotation.
# Run:  python -I probe_scnr_objects.py <map path> [block hex:palette hex:stride ...]
# Default blocks are the MCC Halo 3 U13 ones (see
# docs/format/scenario-and-tag-reading.md 2 and 6). Never writes anything.
import math
import mmap
import struct
import sys

MAP = sys.argv[1]
f = open(MAP, "rb")
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)


def i16(o): return struct.unpack_from("<h", m, o)[0]
def i32(o): return struct.unpack_from("<i", m, o)[0]
def u32(o): return struct.unpack_from("<I", m, o)[0]
def u64(o): return struct.unpack_from("<Q", m, o)[0]
def f32x3(o): return struct.unpack_from("<fff", m, o)


def cstr(o, maxlen=256):
    end = m.find(b"\0", o, o + maxlen)
    return m[o:end if end >= 0 else o + maxlen].decode("latin-1")


def fourcc(o): return m[o:o + 4][::-1].decode("latin-1")


tag_count = i32(0x20)
tagname_tbl_ptr, tagname_idx_ptr = u32(0x24), u32(0x2C)
virtual_base, tag_index_vptr = u64(0x2E0), u64(0x2E8)
sec_off = [u32(0x4CC + 4 * i) for i in range(4)]
sections = [(u32(0x4DC + 8 * i), u32(0x4DC + 8 * i + 4)) for i in range(4)]
def hdr(p): return (p + sec_off[0]) & 0xFFFFFFFF
tag_magic = virtual_base - ((sections[2][0] + sec_off[2]) & 0xFFFFFFFF)
def vaddr(v): return v - tag_magic
def ptr32(p): return vaddr((p & 0xFFFFFFFF) << 2)


ti = vaddr(tag_index_vptr)
class_tbl = vaddr(u64(ti + 8))
tag_tbl = vaddr(u64(ti + 24))
classes = [fourcc(class_tbl + 16 * i) for i in range(i32(ti))]
tags = [(i16(tag_tbl + 8 * i), i32(tag_tbl + 8 * i + 4)) for i in range(tag_count)]
names = [cstr(hdr(tagname_tbl_ptr) + i32(hdr(tagname_idx_ptr) + 4 * i)) for i in range(tag_count)]


def tagref(o):
    idx = struct.unpack("<h", struct.pack("<H", i32(o + 12) & 0xFFFF))[0]
    if idx < 0:
        return None
    return f"{classes[tags[idx][0]]} {names[idx]}"


def block(o):
    cnt = i32(o)
    return cnt, (ptr32(u32(o + 4)) if cnt > 0 else None)


scnr = next(i for i in range(tag_count) if tags[i][0] >= 0 and classes[tags[i][0]] == "scnr")
S = ptr32(tags[scnr][1])

DEFAULT = [
    ("scenery", 0xB4, 0xC0, 180),
    ("0xE4", 0xE4, 0xF0, 168),
    ("0xFC", 0xFC, 0x108, 140),
    ("0x114", 0x114, 0x120, 168),
    ("machines", 0x138, 0x144, 136),
    ("controls", 0x168, 0x174, 120),
    ("crates", 0x5BC, 0x5C8, 176),
]
blocks = DEFAULT
if len(sys.argv) > 2:
    blocks = []
    for a in sys.argv[2:]:
        b, p, s = a.split(":")
        blocks.append((a, int(b, 16), int(p, 16), int(s)))

for label, boff, poff, stride in blocks:
    pc, pa = block(S + poff)
    palette = [tagref(pa + 16 * i) for i in range(pc)]
    cnt, a = block(S + boff)
    print(f"== {label}: {cnt} placed (+0x{boff:X}, stride {stride}), palette {pc} (+0x{poff:X})")
    for i, p in enumerate(palette):
        print(f"   palette {i:3d}: {p}")
    for i in range(cnt):
        e = a + stride * i
        pi = i16(e)
        pos = f32x3(e + 8)
        yaw, pitch, roll = (math.degrees(x) for x in f32x3(e + 0x14))
        pname = palette[pi].rsplit("\\", 1)[-1] if 0 <= pi < len(palette) and palette[pi] else "-"
        print(f"   [{i:3d}] {pi:3d} {pname:32s} ({pos[0]:8.2f}, {pos[1]:8.2f}, {pos[2]:7.2f})"
              f"  ypr ({yaw:7.1f}, {pitch:6.1f}, {roll:6.1f})  name {i16(e + 2)}")
