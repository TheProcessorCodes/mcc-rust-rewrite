# Read-only probe of a structure BSP's cluster portals (docs/format/sbsp-portals.md).
# Run:  python -I probe_sbsp_portals.py <map> root|cluster|dump|portals <bsp> [arg]
# Never writes anything. The header and tag-index code is the same as probe_scnr_blocks.py.
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

# ---------------------------------------------------------------- sbsp probe
# python -I probe_sbsp_portals.py <map> root|cluster|dump|portals <bsp> [arg]
MODE = sys.argv[2] if len(sys.argv) > 2 else "root"
BSP = int(sys.argv[3]) if len(sys.argv) > 3 else 0
n, a = block(S + 0x18)
_, idx, name = tagref(a + 108 * BSP)
B = tag_meta(idx)
print("bsp", BSP, name, hex(B))


def scan(base, size):
    found = []
    for off in range(0, size, 4):
        cnt, p = i32(base + off), u32(base + off + 4)
        if not (0 < cnt < 1000000) or p == 0:
            continue
        fo = ptr32(p)
        if not (0 < fo < len(m)):
            continue
        found.append((off, cnt, fo))
    return found


if MODE == "root":
    found = scan(B, 0x400)
    starts = sorted(x[2] for x in found)
    for off, cnt, fo in found:
        nxt = [s for s in starts if s > fo]
        gap = (nxt[0] - fo) if nxt else 0
        print(f"+0x{off:04X}  count {cnt:6d}  at 0x{fo:X}  gap/count {gap / cnt:8.1f}")
elif MODE == "cluster":
    cn, ca = block(B + 0xC4)
    for c in range(min(cn, int(sys.argv[4]) if len(sys.argv) > 4 else 3)):
        e = ca + 280 * c
        print("cluster", c)
        for off, cnt, fo in scan(e, 280):
            print(f"   +0x{off:03X} count {cnt} at 0x{fo:X} first i16s {struct.unpack_from('<8h', m, fo)}")
elif MODE == "portals":
    # Portal (sbsp + 0x7C, 40 bytes): i16 back cluster, i16 front cluster,
    # i32 plane, centroid, radius, u32 flags, vertex block (12-byte points).
    pn, pa = block(B + 0x7C)
    cn, ca = block(B + 0xC4)
    show = int(sys.argv[4]) if len(sys.argv) > 4 else 8
    flags = {}
    worst = 0.0
    for p in range(pn):
        e = pa + 40 * p
        back, front, plane = i16(e), i16(e + 2), i32(e + 4)
        c = f32x3(e + 8)
        r = f32(e + 0x14)
        fl = u32(e + 0x18)
        flags[fl] = flags.get(fl, 0) + 1
        vn, va = block(e + 0x1C)
        vs = [f32x3(va + 12 * k) for k in range(vn)]
        mean = [sum(v[k] for v in vs) / max(vn, 1) for k in range(3)]
        far = max((math.dist(v, c) for v in vs), default=0.0)
        worst = max(worst, abs(far - r) / max(r, 1e-3))
        if p < show:
            print(f"portal {p}: back {back} front {front} plane {plane} centroid {tuple(round(x, 2) for x in c)} mean {tuple(round(x, 2) for x in mean)} radius {r:.2f} (farthest vertex {far:.2f}) flags {fl:#x} {vn} vertices")
    print(f"{pn} portals, flags {flags}, worst |farthest vertex - radius| / radius {worst:.3f}")
    # Every cluster's portal list (+0x3C, i16 indices) against the portals'
    # own back / front clusters.
    bad = 0
    for ci in range(cn):
        e = ca + 280 * ci
        k, ka = block(e + 0x3C)
        listed = [i16(ka + 2 * j) for j in range(k)] if k > 0 else []
        for p in listed:
            pe = pa + 40 * p
            if ci not in (i16(pe), i16(pe + 2)):
                bad += 1
        own = [p for p in range(pn) if ci in (i16(pa + 40 * p), i16(pa + 40 * p + 2))]
        if sorted(listed) != own:
            bad += 1
            if bad < 6:
                print(f"cluster {ci}: lists {sorted(listed)} but portals touching it are {own}")
    print(f"{cn} clusters, {bad} mismatches between cluster portal lists and portal sides")
elif MODE == "leaves":
    # sbsp + 0x34: one byte per collision BSP leaf, the leaf's cluster.
    ln, la = block(B + 0x34)
    cn, _ = block(B + 0xC4)
    hist = {}
    for k in range(ln):
        v = struct.unpack_from("<b", m, la + k)[0]
        hist[v] = hist.get(v, 0) + 1
    print(f"{ln} leaves, {cn} clusters, cluster byte histogram {sorted(hist.items())}")
elif MODE == "air":
    # Every BSP: the atmosphere palette at sbsp + 0x94 (8 bytes: string id
    # name, i16 index into the skya settings) and the clusters' bytes at
    # +0x18 (sky) and +0x19 (atmosphere palette index), counted.
    n, a = block(S + 0x18)
    for i in range(n):
        _, idx, name = tagref(a + 108 * i)
        b = tag_meta(idx)
        pn, pa = block(b + 0x94)
        palette = [(string_id(u32(pa + 8 * k)), i16(pa + 8 * k + 4)) for k in range(pn)]
        cn, ca = block(b + 0xC4)
        seen = {}
        for c in range(cn):
            e = ca + 280 * c
            key = (struct.unpack_from("<b", m, e + 0x18)[0], struct.unpack_from("<b", m, e + 0x19)[0])
            seen[key] = seen.get(key, 0) + 1
        print(f"bsp {i} {name}: palette {palette}")
        print(f"   (sky, atmosphere) x clusters: {sorted(seen.items())}")
elif MODE == "dump":
    boff, stride, k = sys.argv[4].split(":")
    boff, stride, k = int(boff, 16), int(stride, 0), int(k)
    cnt, a2 = block(B + boff)
    print(f"block +0x{boff:X}: count {cnt}")
    for i in range(min(k, cnt)):
        e = a2 + stride * i
        raw = m[e:e + stride]
        print(f"[{i}] " + " ".join(f"{struct.unpack_from('<I', raw, j)[0]:08X}" for j in range(0, stride - 3, 4)))
        print("    " + " ".join(f"{struct.unpack_from('<f', raw, j)[0]:8.3g}" for j in range(0, stride - 3, 4)))
        print("    h " + " ".join(f"{struct.unpack_from('<h', raw, j)[0]}" for j in range(0, min(stride, 16) - 1, 2)))
        sids = [string_id(struct.unpack_from('<I', raw, j)[0]) for j in range(0, stride - 3, 4)]
        print("    sid: " + " | ".join(x if x and not x.startswith("<") else "-" for x in sids))
