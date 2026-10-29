# Read-only check of the scenario (scnr) tag layout documented in
# docs/format/scenario-and-tag-reading.md against a real MCC Halo 3 U13 map.
# Run:  python -I scnr_riverworld.py [path\to\map]
# Never writes anything. Default target is riverworld.map (Valhalla).
import mmap, struct, sys, math

MAP = sys.argv[1] if len(sys.argv) > 1 else (
    r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps\riverworld.map")

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

# ---------------------------------------------------------------- LOD probe
# python -I probe_lod.py <map> perm <name part>     render model regions and permutations
# python -I probe_lod.py <map> obje <class> <name part> [len hex]   object header as floats
# python -I probe_lod.py <map> fade                  sbsp instance fade pixels (+0x6C), every BSP
MODE = sys.argv[2]


def words(o, n):
    for k in range(0, n, 16):
        ws = [u32(o + k + j) for j in range(0, 16, 4)]
        fs = [f32(o + k + j) for j in range(0, 16, 4)]
        hs = [i16(o + k + j) for j in range(0, 16, 2)]
        print(f"  +0x{k:03X}  " + " ".join(f"{w:08X}" for w in ws) + "   " + " ".join(f"{x:10.4g}" for x in fs) + "   " + " ".join(str(h) for h in hs))


if MODE == "perm":
    for t in range(tag_count):
        if tag_class(t) != "mode" or not tag_names[t] or sys.argv[3] not in tag_names[t]:
            continue
        b = tag_meta(t)
        print(tag_names[t])
        rn, ra = block(b + 12)
        for r in range(rn):
            ro = ra + 16 * r
            pn, pa = block(ro + 4)
            print(f" region {string_id(u32(ro))}: {pn} permutations")
            for p in range(min(pn, 4)):
                po = pa + 24 * p
                print(f"  perm {string_id(u32(po))}")
                words(po, 24)
        sn, sa = block(b + 0x68)
        print(f" sections (+0x68 block): {sn}")
elif MODE == "obje":
    cls, part = sys.argv[3], sys.argv[4]
    n = int(sys.argv[5], 16) if len(sys.argv) > 5 else 0x100
    for t in range(tag_count):
        if tag_class(t) != cls or not tag_names[t] or part not in tag_names[t]:
            continue
        print(tag_names[t])
        words(tag_meta(t), n)
        break
elif MODE == "fade":
    n, a = block(S + 0x18)
    for i in range(n):
        _, idx, name = tagref(a + 108 * i)
        b = tag_meta(idx)
        cn, ca = block(b + 0x1C0)
        seen = {}
        for k in range(cn):
            e = ca + 120 * k
            key = (struct.unpack_from("<H", m, e + 0x6C)[0], struct.unpack_from("<H", m, e + 0x6E)[0])
            seen[key] = seen.get(key, 0) + 1
        print(f"bsp {i} {name}: {cn} instances, (fade start, end) pixels: {sorted(seen.items())[:12]}")
