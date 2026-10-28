"""Read-only check of scnr / sbsp counts and bounds straight from an MCC Halo 3 U13 .map.

Usage:  python -I check_scnr_sbsp_counts.py <path to riverworld.map>

Uses only the container facts already verified for U13 (header 0x4000, section tables,
shifted 32-bit tag pointers) plus the U13 field offsets Reclaimer uses for scnr and sbsp.
Prints the numbers the Rust reader must reproduce. Never writes anything.
"""
import struct, sys

def u32(b, o): return struct.unpack_from('<I', b, o)[0]
def i32(b, o): return struct.unpack_from('<i', b, o)[0]
def i16(b, o): return struct.unpack_from('<h', b, o)[0]
def i64(b, o): return struct.unpack_from('<q', b, o)[0]
def f32(b, o): return struct.unpack_from('<f', b, o)[0]

path = sys.argv[1]
data = open(path, 'rb').read()
assert data[:4] == b'daeh', 'not a Halo cache (expected bytes 64 61 65 68)'
assert i32(data, 4) == 13, 'not header version 13'

sec_off = [u32(data, 0x4CC + 4*i) for i in range(4)]
sec = [(u32(data, 0x4DC + 8*i), u32(data, 0x4E0 + 8*i)) for i in range(4)]
virtual_base = i64(data, 0x2E0)
index_ptr = i64(data, 0x2E8)
tag_count = i32(data, 0x20)
tagdata_file_base = (sec[2][0] + sec_off[2]) & 0xFFFFFFFF

def hdr_ptr(p):           # header (section 0) pointer -> file offset
    return (p + sec_off[0]) & 0xFFFFFFFF
def virt(v):              # 64-bit virtual tag-data address -> file offset
    return v - virtual_base + tagdata_file_base
def shifted(p):           # 32-bit shifted tag-data pointer -> file offset
    return virt((p & 0xFFFFFFFF) << 2)

ti = virt(index_ptr)
class_count = i32(data, ti); class_ptr = i64(data, ti + 8)
tcount = i32(data, ti + 16); tag_ptr = i64(data, ti + 24)
assert tcount == tag_count

classes = []
cb = virt(class_ptr)
for i in range(class_count):
    cid = data[cb + 16*i: cb + 16*i + 4]
    classes.append(cid[::-1].decode('latin1'))

tb = virt(tag_ptr)
tags = []
for i in range(tcount):
    ci = i16(data, tb + 8*i); meta = u32(data, tb + 8*i + 4)
    tags.append((ci, meta))

# tag names
name_tbl = hdr_ptr(u32(data, 0x24)); name_idx = hdr_ptr(u32(data, 0x2C))
def tag_name(i):
    off = i32(data, name_idx + 4*i)
    if off < 0: return None
    e = data.index(b'\0', name_tbl + off)
    return data[name_tbl + off: e].decode('latin1')

scenario_path = data[0xE0: data.index(b'\0', 0xE0)].decode('latin1')
build = data[0xA0: data.index(b'\0', 0xA0)].decode('latin1')
print('build', repr(build), 'scenario', scenario_path, 'tags', tcount, 'classes', class_count)

def find_tag(cls, name):
    for i, (ci, meta) in enumerate(tags):
        if ci >= 0 and classes[ci] == cls and tag_name(i) == name:
            return i, shifted(meta)
    raise SystemExit(f'tag {cls} {name} not found')

def block(meta, off):
    """TagBlock at meta+off -> (count, file offset of first element)"""
    cnt = i32(data, meta + off); ptr = u32(data, meta + off + 4)
    return cnt, (shifted(ptr) if cnt else None)

def tagref(meta, off):
    """TagReference -> (class code, tag index) or None"""
    cid = data[meta + off: meta + off + 4][::-1].decode('latin1')
    tid = i32(data, meta + off + 12)
    return None if tid == -1 else (cid, tid & 0xFFFF)

scnr_i, scnr = find_tag('scnr', scenario_path)
print('scnr tag index', scnr_i, 'meta file offset', hex(scnr))
U13 = dict(StructureBsps=24, Skies=52, ObjectNames=168, Scenery=180, SceneryPalette=192,
           Machines=312, MachinePalette=324, Controls=360, ControlPalette=372,
           Crates=1468, CratePalette=1480)
for k, o in U13.items():
    cnt, _ = block(scnr, o)
    print(f'  scnr.{k:<15} @+{o:<5} (0x{o:03X}) count {cnt}')
lm = tagref(scnr, 1708)
print('  scnr.ScenarioLightmapReference @+1708 (0x6AC):', lm, tag_name(lm[1]) if lm else None)

# structure bsps: 108-byte entries, TagReference at +0
nb, bsp_tbl = block(scnr, 24)
for b in range(nb):
    ref = tagref(bsp_tbl + 108*b, 0)
    name = tag_name(ref[1])
    sb = shifted(tags[ref[1]][1])
    print(f'bsp {b}: {ref} {name} meta file offset {sb:#x}')
    xb = (f32(data, sb+64), f32(data, sb+68)); yb = (f32(data, sb+72), f32(data, sb+76)); zb = (f32(data, sb+80), f32(data, sb+84))
    print('   XBounds @+64', xb, 'YBounds @+72', yb, 'ZBounds @+80', zb)
    for k, o in dict(Clusters=196, Shaders=208, GeometryInstances=448, Sections=756, BoundingBoxes=768).items():
        cnt, p = block(sb, o)
        print(f'   sbsp.{k:<17} @+{o:<4} (0x{o:03X}) count {cnt}')
    # cluster section indices (280-byte clusters, SectionIndex at +216)
    nc, cp = block(sb, 196)
    secs = [i16(data, cp + 280*i + 216) for i in range(nc)]
    print('   cluster SectionIndex list:', secs)
    # geometry instances: 120-byte, scale +0, 4x3 matrix +4, section +52, name string id +84
    ni, ip = block(sb, 448)
    nonunit = sum(1 for i in range(ni) if abs(f32(data, ip + 120*i) - 1.0) > 1e-4)
    print('   instances with TransformScale != 1:', nonunit, 'of', ni)
    first = ip
    m = [f32(data, first + 4 + 4*k) for k in range(12)]
    print('   instance[0]: scale', f32(data, first), 'section', i16(data, first + 52), 'matrix rows', [m[0:3], m[3:6], m[6:9]], 'translation', m[9:12])
# skies
ns, sp = block(scnr, 52)
for s in range(ns):
    ref = tagref(sp + 20*s, 0)
    print('sky', s, ref, tag_name(ref[1]))
# palettes (16-byte entries, TagReference at 0)
for pal, pl_off in (('SceneryPalette', 192), ('CratePalette', 1480), ('MachinePalette', 324), ('ControlPalette', 372)):
    n, p = block(scnr, pl_off)
    names = []
    for i in range(n):
        r = tagref(p + 16*i, 0)
        names.append(tag_name(r[1]) if r else None)
    print(pal, n, names[:6], '...' if n > 6 else '')
