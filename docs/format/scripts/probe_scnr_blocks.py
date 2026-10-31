# Read-only check of the scenario (scnr) tag layout documented in
# docs/format/scenario-and-tag-reading.md against a real MCC Halo 3 U13 map.
# Run:  python -I scnr_riverworld.py [path\to\map]
# Never writes anything. Default target is riverworld.map (Valhalla).
import os, mmap, struct, sys, math

MAP = sys.argv[1] if len(sys.argv) > 1 else (
    os.path.join(os.environ["HALO3_MAPS"], "riverworld.map"))

f = open(MAP, "rb")
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)

def i16(o): return struct.unpack_from("<h", m, o)[0]
def i32(o): return struct.unpack_from("<i", m, o)[0]
def u32(o): return struct.unpack_from("<I", m, o)[0]
def u64(o): return struct.unpack_from("<Q", m, o)[0]
def f32(o): return struct.unpack_from("<f", m, o)[0]
def f32x3(o): return struct.unpack_from("<fff", m, o)
def cstr(o, maxlen=None):
    end = m.find(b"\0", o, o + maxlen if maxlen else len(m))
    if end < 0:
        end = o + (maxlen or 0)
    return m[o:end].decode("latin-1")
def fourcc(o):
    return m[o:o+4][::-1].decode("latin-1")   # stored little-endian: reverse to read "scnr"

# ---------------------------------------------------------------- header
assert m[0:4] == b"daeh", m[0:4]     # 'head' stored byte-reversed, like every fourcc in the file
tag_count        = i32(0x20)
tagname_tbl_ptr  = u32(0x24)
tagname_idx_ptr  = u32(0x2C)
string_count     = i32(0x30)
string_tbl_ptr   = u32(0x34)
string_idx_ptr   = u32(0x3C)
ns_count         = i32(0x40)
ns_tbl_ptr       = u32(0x44)
build            = cstr(0xA0, 32)
virtual_base     = u64(0x2E0)
tag_index_vptr   = u64(0x2E8)
sec_off          = [u32(0x4CC + 4*i) for i in range(4)]
sections         = [(u32(0x4DC + 8*i), u32(0x4DC + 8*i + 4)) for i in range(4)]

def hdr(p):      # header 32-bit pointer -> file offset (section 0)
    return (p + sec_off[0]) & 0xFFFFFFFF
tag_magic = virtual_base - ((sections[2][0] + sec_off[2]) & 0xFFFFFFFF)
def vaddr(v):    # 64-bit / expanded virtual address -> file offset (section 2)
    return v - tag_magic
def ptr32(p):    # shifted 32-bit tag-data pointer -> file offset
    return vaddr((p & 0xFFFFFFFF) << 2)

print(f"map            {MAP}")
print(f"build          '{build}'  file size 0x{len(m):X}")
print(f"tag count      {tag_count} (0x{tag_count:X})  string count {string_count} (0x{string_count:X})  namespaces {ns_count}")
print(f"virtual base   0x{virtual_base:X}  tag index vptr 0x{tag_index_vptr:X}  tag magic 0x{tag_magic:X}")
print(f"tag index file 0x{vaddr(tag_index_vptr):X}")

# ---------------------------------------------------------------- string ids
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

# ---------------------------------------------------------------- tag index
ti = vaddr(tag_index_vptr)
class_count   = i32(ti + 0)
class_tbl     = vaddr(u64(ti + 8))
tag_count2    = i32(ti + 16)
tag_tbl       = vaddr(u64(ti + 24))
assert tag_count2 == tag_count, (tag_count2, tag_count)
classes = [(fourcc(class_tbl + 16*i), fourcc(class_tbl + 16*i + 4), fourcc(class_tbl + 16*i + 8), i32(class_tbl + 16*i + 12))
           for i in range(class_count)]
tags = []
for i in range(tag_count):
    o = tag_tbl + 8*i
    tags.append((i16(o), i16(o+2), i32(o+4)))
name_idx = [i32(hdr(tagname_idx_ptr) + 4*i) for i in range(tag_count)]
name_tbl = hdr(tagname_tbl_ptr)
tag_names = [cstr(name_tbl + n) if n >= 0 else None for n in name_idx]
print(f"classes        {class_count}  class table file 0x{class_tbl:X}  tag table file 0x{tag_tbl:X}")

def tag_class(idx):   # class code of tag index idx, or None for the empty tag
    ci = tags[idx][0]
    return classes[ci][0] if ci >= 0 else None
def tag_meta(idx):    # file offset of tag data
    return ptr32(tags[idx][2])

def tagref(o):        # 16-byte TagReference -> (class fourcc, tag index or -1, tag name)
    cls = fourcc(o) if u32(o) != 0xFFFFFFFF else None
    tid = i32(o + 12)
    idx = struct.unpack("<h", struct.pack("<H", tid & 0xFFFF))[0]   # low 16 bits as signed short
    return cls, idx, (tag_names[idx] if idx >= 0 else None)
def block(o):         # 12-byte TagBlock -> (count, file offset of first element)
    cnt, p = i32(o), i32(o + 4)
    return cnt, (ptr32(p) if cnt > 0 else None)

scnr_list = [i for i in range(tag_count) if tag_class(i) == "scnr"]
assert len(scnr_list) == 1, scnr_list
scnr = scnr_list[0]
S = tag_meta(scnr)
print(f"scnr           tag index {scnr} (0x{scnr:X}) '{tag_names[scnr]}'  meta ptr 0x{tags[scnr][2]:08X} -> file 0x{S:X}")

# ---------------------------------------------------------------- scan every block header
# Walks the scnr root in 4-byte steps and prints every 12-byte TagBlock whose
# count is plausible and whose pointer lands inside the file, with the
# distance to the next block's elements (an upper bound on count * stride).
# Optional: argv[2] = how far to scan (hex), argv[3] = "offset:stride:n" to
# hex-dump n elements of one block.
SCNR_SIZE = int(sys.argv[2], 16) if len(sys.argv) > 2 else 0x800
found = []
for off in range(0, SCNR_SIZE, 4):
    cnt, p = i32(S + off), u32(S + off + 4)
    if not (0 < cnt < 100000) or p == 0:
        continue
    fo = ptr32(p)
    if not (0 < fo < len(m)):
        continue
    found.append((off, cnt, fo))
starts = sorted(x[2] for x in found)
for off, cnt, fo in found:
    nxt = [s for s in starts if s > fo]
    gap = (nxt[0] - fo) if nxt else 0
    print(f"+0x{off:04X}  count {cnt:6d}  at 0x{fo:X}  gap/count {gap / cnt:8.1f}")

def bsp_bounds():
    n, a = block(S + 0x18)
    out = []
    for i in range(n):
        _, idx, name = tagref(a + 108 * i)
        b = tag_meta(idx) + 0x40
        out.append((name, [struct.unpack_from("<ff", m, b + 8 * k) for k in range(3)]))
    return out

if len(sys.argv) > 3 and sys.argv[3].startswith("origin:"):
    # origin:<block hex>:<stride>: group a placement block by the i16 at
    # +0x3C (the object id's origin BSP) and print the position range of
    # each group next to that BSP's stated bounds.
    _, boff, stride = sys.argv[3].split(":")
    cnt, a = block(S + int(boff, 16))
    groups = {}
    for i in range(cnt):
        e = a + int(stride) * i
        if i16(e) < 0:
            continue
        groups.setdefault(i16(e + 0x3C), []).append(f32x3(e + 8))
    bounds = bsp_bounds()
    for g, ps in sorted(groups.items()):
        lo = [min(p[k] for p in ps) for k in range(3)]
        hi = [max(p[k] for p in ps) for k in range(3)]
        print(f"origin {g:3d}: {len(ps):5d} placed  x {lo[0]:8.1f}..{hi[0]:8.1f}  y {lo[1]:8.1f}..{hi[1]:8.1f}  z {lo[2]:7.1f}..{hi[2]:7.1f}")
        if 0 <= g < len(bounds):
            (bx, by, bz) = bounds[g][1]
            print(f"   bsp {bounds[g][0]}  x {bx[0]:8.1f}..{bx[1]:8.1f}  y {by[0]:8.1f}..{by[1]:8.1f}  z {bz[0]:7.1f}..{bz[1]:7.1f}")
    sys.exit(0)

if len(sys.argv) > 3 and sys.argv[3] == "clusters":
    # The i16s at +0x18..+0x2F of every cluster (sbsp + 0xC4, 280 bytes
    # each) of every BSP, counted: looks for per-cluster palette indices.
    n, a = block(S + 0x18)
    for i in range(n):
        _, idx, name = tagref(a + 108 * i)
        cn, ca = block(tag_meta(idx) + 0xC4)
        seen = {}
        for c in range(cn):
            e = ca + 280 * c
            key = tuple(struct.unpack_from("<12h", m, e + 0x18))
            seen[key] = seen.get(key, 0) + 1
        print(f"bsp {i} {name}: {cn} clusters")
        for c in range(min(cn, 4)):
            e = ca + 280 * c
            box = struct.unpack_from("<6f", m, e)
            print(f"   cluster {c}: x {box[0]:.1f}..{box[1]:.1f} y {box[2]:.1f}..{box[3]:.1f} z {box[4]:.1f}..{box[5]:.1f}  sky {struct.unpack_from('<b', m, e + 0x18)[0]}")
        for k, v in sorted(seen.items(), key=lambda kv: -kv[1]):
            print(f"   {v:3d} x {k}")
    sys.exit(0)

if len(sys.argv) > 3:
    boff, stride, n = sys.argv[3].split(":")
    boff, stride, n = int(boff, 16), int(stride, 0), int(n)
    cnt, a = block(S + boff)
    print(f"\nblock +0x{boff:X}: count {cnt}")
    for i in range(min(n, cnt)):
        e = a + stride * i
        raw = m[e:e + stride]
        words = [struct.unpack_from("<I", raw, j)[0] for j in range(0, stride - 3, 4)]
        floats = [struct.unpack_from("<f", raw, j)[0] for j in range(0, stride - 3, 4)]
        print(f"[{i}] " + " ".join(f"{w:08X}" for w in words))
        print("    " + " ".join(f"{x:8.3g}" for x in floats))
        print("    sid: " + " | ".join(string_id(w) if w and not str(string_id(w)).startswith("<") else "-" for w in words))
