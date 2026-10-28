"""Read-only: reproduce HaloExport's World / instance triangle counts from tag data alone (no glTF needed).

Usage:  python -I check_triangle_sums.py <path to riverworld.map>

World triangles  = sum over cluster sections of (submesh IndexLength / 3), skipping submeshes whose
                   shader reference is null (HaloExport skips segments with no material).
Instance tris    = same sum per instance section, added once per geometry instance placement, skipping
                   instances whose section has no vertex buffer data (Reclaimer yields a null mesh).
Degenerate triangles (two equal indices) are also dropped by HaloExport, so the figure here is an upper
bound that should match exactly when there are none.  Never writes anything.
"""
import struct
import sys
from collections import Counter


def u32(b, o): return struct.unpack_from('<I', b, o)[0]
def i32(b, o): return struct.unpack_from('<i', b, o)[0]
def i16(b, o): return struct.unpack_from('<h', b, o)[0]
def u16(b, o): return struct.unpack_from('<H', b, o)[0]
def i64(b, o): return struct.unpack_from('<q', b, o)[0]


path = sys.argv[1]
data = open(path, 'rb').read()
assert data[:4] == b'daeh' and i32(data, 4) == 13
sec_off = [u32(data, 0x4CC + 4 * i) for i in range(4)]
sec = [(u32(data, 0x4DC + 8 * i), u32(data, 0x4E0 + 8 * i)) for i in range(4)]
virtual_base = i64(data, 0x2E0)
index_ptr = i64(data, 0x2E8)
tag_count = i32(data, 0x20)
tagdata_file_base = (sec[2][0] + sec_off[2]) & 0xFFFFFFFF


def hdr_ptr(p): return (p + sec_off[0]) & 0xFFFFFFFF
def virt(v): return v - virtual_base + tagdata_file_base
def shifted(p): return virt((p & 0xFFFFFFFF) << 2)


ti = virt(index_ptr)
class_count = i32(data, ti)
cb = virt(i64(data, ti + 8))
tb = virt(i64(data, ti + 24))
classes = [data[cb + 16 * i: cb + 16 * i + 4][::-1].decode('latin1') for i in range(class_count)]
tags = [(i16(data, tb + 8 * i), u32(data, tb + 8 * i + 4)) for i in range(tag_count)]
name_tbl = hdr_ptr(u32(data, 0x24))
name_idx = hdr_ptr(u32(data, 0x2C))


def cstr(off): return data[off: data.index(b'\0', off)].decode('latin1')


def tag_name(i):
    off = i32(data, name_idx + 4 * i)
    return None if off < 0 else cstr(name_tbl + off)


def meta_of(i): return shifted(tags[i][1])


def block(meta, off):
    cnt = i32(data, meta + off)
    ptr = u32(data, meta + off + 4)
    return cnt, (shifted(ptr) if cnt else None)


def tagref(meta, off):
    tid = i32(data, meta + off + 12)
    return None if tid == -1 else (data[meta + off: meta + off + 4][::-1].decode('latin1'), tid & 0xFFFF)


def global_tag(cls):
    found = [i for i, (ci, m) in enumerate(tags) if ci >= 0 and classes[ci] == cls]
    assert len(found) == 1
    return found[0]


scenario_path = cstr(0xE0)
scnr_i = [i for i, (ci, m) in enumerate(tags) if ci >= 0 and classes[ci] == 'scnr' and tag_name(i) == scenario_path][0]
scnr = meta_of(scnr_i)
nb, bsp_tbl = block(scnr, 24)
bsp_ref = tagref(bsp_tbl, 0)
sbsp = meta_of(bsp_ref[1])

# shaders of the sbsp (36-byte ShaderBlock, TagReference at 0)
nsh, shp = block(sbsp, 208)
shader_valid = [tagref(shp + 36 * k, 0) is not None for k in range(nsh)]
print('sbsp shaders', nsh, 'null shader refs', shader_valid.count(False))

# lightmap bsp data (U13): Sections @+308, ResourcePointer @+428
lm = tagref(scnr, 1708)
sldt = meta_of(lm[1])
n, p = block(sldt, 4)
lbsp = None
for k in range(n):
    r = tagref(p + 16 * k, 0)
    if r and i16(data, meta_of(r[1]) + 2) == 0:
        lbsp = meta_of(r[1])
nsec, sp = block(lbsp, 308)

# vertex buffer infos from the zone fixup data, to know which sections have no data
zone = meta_of(global_tag('zone'))
nres, rp0 = block(zone, 100)
fixup_file = shifted(u32(data, zone + 340))
ridx = u32(data, lbsp + 428) & 0xFFFF
e = rp0 + 64 * ridx
fo = i32(data, e + 20)
fs = i32(data, e + 24)
vb_count = i32(data, fixup_file + fo + fs - 24)
vb_vertex_count = [i32(data, fixup_file + fo + 28 * k) for k in range(vb_count)]


def section_tris(s):
    """(triangles with a material, triangles without, vertex count of its vertex buffer)"""
    o = sp + 76 * s
    nsub, subp = block(o, 0)
    vbi = i16(data, o + 24)
    vcount = vb_vertex_count[vbi] if 0 <= vbi < vb_count else 0
    with_mat = without = 0
    for j in range(nsub):
        si = i16(data, subp + 16 * j)
        tris = u16(data, subp + 16 * j + 6) // 3
        if 0 <= si < nsh and shader_valid[si]:
            with_mat += tris
        else:
            without += tris
    return with_mat, without, vcount


# clusters: 280-byte blocks, SectionIndex at +216
nc, cp = block(sbsp, 196)
world = 0
empty_clusters = []
for c in range(nc):
    s = i16(data, cp + 280 * c + 216)
    w, wo, vc = section_tris(s)
    if vc == 0 or (w + wo) == 0:
        empty_clusters.append((c, s, vc))
    world += w if vc else 0
print(f'clusters {nc}: World triangles (sum of IndexLength/3 with a material, non-empty sections) = {world}; '
      f'clusters with no mesh data = {len(empty_clusters)} {empty_clusters}')

# geometry instances: 120-byte blocks, SectionIndex at +52, Name string id at +84
ni, ip = block(sbsp, 448)
inst_tris = 0
placed = 0
skipped = []
per_section = Counter()
for i in range(ni):
    s = i16(data, ip + 120 * i + 52)
    w, wo, vc = section_tris(s)
    if vc == 0 or (w + wo) == 0:
        skipped.append((i, s, vc, hex(u32(data, ip + 120 * i + 84))))
        continue
    inst_tris += w
    placed += 1
    per_section[s] += 1
print(f'geometry instances {ni}: placed {placed}, skipped {len(skipped)} (index, section, vertex count, name string id) {skipped}')
print(f'  instance triangles summed over placements (with a material) = {inst_tris}; distinct sections used = {len(per_section)}')
