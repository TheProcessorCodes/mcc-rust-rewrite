# Lists the scenario's trigger volumes (zone-sets.md 1: scnr +0x258, 68 bytes
# each: name string id, then forward, up, corner position and extents at +0x0C).
# Read-only. Run:  python -I probe_trigger_volumes.py [path\to\map]
# Default target is riverworld.map (Valhalla).
import mmap, struct, sys

MAP = sys.argv[1] if len(sys.argv) > 1 else (
    r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps\riverworld.map")

f = open(MAP, "rb")
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)

def i16(o): return struct.unpack_from("<h", m, o)[0]
def i32(o): return struct.unpack_from("<i", m, o)[0]
def u32(o): return struct.unpack_from("<I", m, o)[0]
def u64(o): return struct.unpack_from("<Q", m, o)[0]
def f32x3(o): return struct.unpack_from("<fff", m, o)
def cstr(o, maxlen=None):
    end = m.find(b"\0", o, o + maxlen if maxlen else len(m))
    if end < 0:
        end = o + (maxlen or 0)
    return m[o:end].decode("latin-1")
def fourcc(o):
    return m[o:o+4][::-1].decode("latin-1")

assert m[0:4] == b"daeh", m[0:4]
tag_count        = i32(0x20)
tagname_tbl_ptr  = u32(0x24)
tagname_idx_ptr  = u32(0x2C)
string_count     = i32(0x30)
string_tbl_ptr   = u32(0x34)
string_idx_ptr   = u32(0x3C)
ns_count         = i32(0x40)
ns_tbl_ptr       = u32(0x44)
virtual_base     = u64(0x2E0)
tag_index_vptr   = u64(0x2E8)
sec_off          = [u32(0x4CC + 4*i) for i in range(4)]
sections         = [(u32(0x4DC + 8*i), u32(0x4DC + 8*i + 4)) for i in range(4)]

def hdr(p):
    return (p + sec_off[0]) & 0xFFFFFFFF
tag_magic = virtual_base - ((sections[2][0] + sec_off[2]) & 0xFFFFFFFF)
def vaddr(v):
    return v - tag_magic
def ptr32(p):
    return vaddr((p & 0xFFFFFFFF) << 2)

str_idx = [i32(hdr(string_idx_ptr) + 4*i) for i in range(string_count)]
str_tbl = hdr(string_tbl_ptr)
strings = [cstr(str_tbl + o) if o >= 0 else None for o in str_idx]
INDEX_BITS, NS_BITS = 19, 8
ns_arr = [i32(hdr(ns_tbl_ptr) + 4*i) for i in range(ns_count)]
namespaces = {}
mask = (1 << INDEX_BITS) - 1
start = ns_arr[0] & mask
for i in range(1, ns_count):
    namespaces[i] = (0, start)
    start += ns_arr[i] & mask
namespaces[0] = (ns_arr[0] & mask, start)
def string_id(v):
    idx = v & mask
    ns = (v >> INDEX_BITS) & ((1 << NS_BITS) - 1)
    while ns not in namespaces and ns > 0:
        ns -= 1
    mn, st = namespaces[ns]
    real = idx if idx < mn else idx - mn + st
    return strings[real] if 0 <= real < len(strings) else f"<invalid 0x{v:08X}>"

ti = vaddr(tag_index_vptr)
class_count = i32(ti + 0)
class_tbl   = vaddr(u64(ti + 8))
tag_tbl     = vaddr(u64(ti + 24))
classes = [fourcc(class_tbl + 16*i) for i in range(class_count)]
tags = [(i16(tag_tbl + 8*i), i32(tag_tbl + 8*i + 4)) for i in range(tag_count)]
scnr = [i for i in range(tag_count) if tags[i][0] >= 0 and classes[tags[i][0]] == "scnr"]
assert len(scnr) == 1, scnr
S = ptr32(tags[scnr[0]][1])

cnt, p = i32(S + 0x258), i32(S + 0x258 + 4)
print(f"{MAP}: {cnt} trigger volumes")
for i in range(max(cnt, 0)):
    e = ptr32(p) + 68 * i
    name = string_id(u32(e))
    fwd, up, pos, ext = (f32x3(e + 0x0C + 12 * k) for k in range(4))
    print(f"{i:3d} {name:32s} corner ({pos[0]:8.2f},{pos[1]:8.2f},{pos[2]:8.2f}) "
          f"extents ({ext[0]:7.2f},{ext[1]:7.2f},{ext[2]:7.2f}) "
          f"fwd ({fwd[0]:.2f},{fwd[1]:.2f},{fwd[2]:.2f}) up ({up[0]:.2f},{up[1]:.2f},{up[2]:.2f})")

# Spawn markers: scenery placements (scnr +0xB4, 180 bytes; palette +0xC0 of
# 16-byte tag references) whose object lives under a "spawning" folder, as
# blam_geometry::scenario::spawn_kind picks them. Position at +0x08, yaw +0x14.
name_idx = [i32(hdr(tagname_idx_ptr) + 4*i) for i in range(tag_count)]
name_tbl = hdr(tagname_tbl_ptr)
def tag_name(i):
    return cstr(name_tbl + name_idx[i]) if 0 <= i < tag_count and name_idx[i] >= 0 else ""
pc, pp = i32(S + 0xC0), i32(S + 0xC0 + 4)
palette = []
for i in range(max(pc, 0)):
    tid = i32(ptr32(pp) + 16 * i + 12)
    palette.append(tag_name(struct.unpack("<h", struct.pack("<H", tid & 0xFFFF))[0]))
sc, sp = i32(S + 0xB4), i32(S + 0xB4 + 4)
n = 0
for i in range(max(sc, 0)):
    e = ptr32(sp) + 180 * i
    pal = i16(e)
    path = palette[pal].lower() if 0 <= pal < len(palette) else ""
    leaf = path.rsplit("\\", 1)[-1]
    if "spawning" not in path or leaf not in ("respawn_point", "respawn_point_invisible", "initial_spawn_point"):
        continue
    x, y, z = f32x3(e + 0x08)
    yaw = struct.unpack_from("<f", m, e + 0x14)[0]
    print(f"spawn {n:3d} {leaf:24s} {x:8.3f} {y:8.3f} {z:8.3f} yaw {yaw * 57.29578:7.1f}")
    n += 1
