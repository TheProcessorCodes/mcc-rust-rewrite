"""Read-only trace: scnr -> sLdT -> Lbsp -> zone resource entry -> play segment/page, for one MCC Halo 3 U13 .map.

Usage:  python -I check_lbsp_resource.py <path to riverworld.map>

Shows where the structure BSP render geometry (sections + vertex/index buffers) really lives in MCC
Halo 3 (the Lbsp tag's resource, not the sbsp's own Sections block), using Reclaimer's U13 offsets.
Never writes anything.
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
class_ptr = i64(data, ti + 8)
tag_ptr = i64(data, ti + 24)
cb = virt(class_ptr)
classes = [data[cb + 16 * i: cb + 16 * i + 4][::-1].decode('latin1') for i in range(class_count)]
tb = virt(tag_ptr)
tags = [(i16(data, tb + 8 * i), u32(data, tb + 8 * i + 4)) for i in range(tag_count)]
name_tbl = hdr_ptr(u32(data, 0x24))
name_idx = hdr_ptr(u32(data, 0x2C))


def cstr(off):
    return data[off: data.index(b'\0', off)].decode('latin1')


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
    assert len(found) == 1, (cls, found)
    return found[0]


scenario_path = cstr(0xE0)
scnr_i = [i for i, (ci, m) in enumerate(tags) if ci >= 0 and classes[ci] == 'scnr' and tag_name(i) == scenario_path][0]
scnr = meta_of(scnr_i)

# bsp 0 and its own Sections / BoundingBoxes counts
nb, bsp_tbl = block(scnr, 24)
bsp_ref = tagref(bsp_tbl, 0)
sbsp = meta_of(bsp_ref[1])
print('sbsp', bsp_ref, tag_name(bsp_ref[1]))
print('  sbsp.Sections @+756 count', block(sbsp, 756)[0], '| sbsp.BoundingBoxes @+768 count', block(sbsp, 768)[0],
      '| Clusters', block(sbsp, 196)[0], '| GeometryInstances', block(sbsp, 448)[0])
print('  sbsp.ResourcePointer1 @+596 =', hex(u32(data, sbsp + 596)), ' ResourcePointer2 @+876 =', hex(u32(data, sbsp + 876)),
      ' ResourcePointer3 @+908 =', hex(u32(data, sbsp + 908)))

# scenario lightmap (sLdT) -> LightmapRefs (TagBlock of 16-byte TagReferences at +4)
lm = tagref(scnr, 1708)
sldt = meta_of(lm[1])
print('sLdT', lm, tag_name(lm[1]))
n, p = block(sldt, 4)
print('  sLdT.LightmapRefs @+4 count', n)
lbsp_meta = None
for k in range(n):
    r = tagref(p + 16 * k, 0)
    if r is None:
        print('   ref', k, None)
        continue
    m = meta_of(r[1])
    bsp_index = i16(data, m + 2)
    nsec, _ = block(m, 308)
    rp = u32(data, m + 428)
    print(f'   ref {k}: {r} {tag_name(r[1])}  Lbsp.BspIndex @+2 = {bsp_index}  Lbsp.Sections @+308 count {nsec}'
          f'  Lbsp.ResourcePointer @+428 = {rp:#x} (resource index {rp & 0xFFFF})')
    if bsp_index == 0 and lbsp_meta is None:
        lbsp_meta = m

# Lbsp sections: SectionBlock 76 bytes (U13): Submeshes +0, Subsets +12, VertexBufferIndex i16 +24,
# IndexBufferIndex i16 +40, Flags u8 +44, NodeIndex u8 +45, VertexFormat u8 +46, IndexFormat u8 +48
nsec, sp = block(lbsp_meta, 308)
vf = Counter()
ifmt = Counter()
unindexed = 0
for s in range(nsec):
    o = sp + 76 * s
    vf[data[o + 46]] += 1
    ifmt[data[o + 48]] += 1
    if i16(data, o + 40) == -1 or (data[o + 44] & 0x10):
        unindexed += 1
print('  Lbsp sections', nsec, '| VertexFormat histogram', dict(vf),
      '| IndexFormat histogram (3=TriangleList,5=TriangleStrip)', dict(ifmt), '| unindexed', unindexed)
for s in list(range(3)) + [nsec - 1]:
    o = sp + 76 * s
    nsub, subp = block(o, 0)
    subs = [(i16(data, subp + 16 * j), u16(data, subp + 16 * j + 4), u16(data, subp + 16 * j + 6), u16(data, subp + 16 * j + 14)) for j in range(nsub)]
    more = '...' if nsub > 4 else ''
    print(f'   section {s}: Submeshes {nsub} (ShaderIndex, IndexStart, IndexLength, VertexCount)={subs[:4]}{more}'
          f' Subsets {block(o, 12)[0]} VB {i16(data, o + 24)} IB {i16(data, o + 40)} Flags {data[o + 44]:#x}'
          f' Node {data[o + 45]} VertexFormat {data[o + 46]} IndexFormat {data[o + 48]}')
total_tris_est = 0
for s in range(nsec):
    o = sp + 76 * s
    nsub, subp = block(o, 0)
    if data[o + 48] == 3:
        total_tris_est += sum(u16(data, subp + 16 * j + 6) for j in range(nsub)) // 3
print('  sum of submesh IndexLength/3 over TriangleList sections (clusters + instances, before placement):', total_tris_est)

# zone (cache_file_resource_gestalt): ResourceEntries @+100 (64-byte entries), FixupDataPointer @+340
zone = meta_of(global_tag('zone'))
nres, rp0 = block(zone, 100)
fixup_ptr = u32(data, zone + 340)
fixup_file = shifted(fixup_ptr)
print('zone: ResourceEntries @+100 count', nres, '| FixupDataSize @+328 =', i32(data, zone + 328),
      '| FixupDataPointer @+340 =', hex(fixup_ptr), '-> file', hex(fixup_file))
rp = u32(data, lbsp_meta + 428)
ridx = rp & 0xFFFF
e = rp0 + 64 * ridx
owner = tagref(e, 0)
fo = i32(data, e + 20)
fs = i32(data, e + 24)
loc = i16(data, e + 32)
seg = i16(data, e + 34)
nfix, fixp = block(e, 40)
ndef, _ = block(e, 52)
print(f'  entry[{ridx}]: Owner {owner} {tag_name(owner[1]) if owner else None} FixupOffset {fo} FixupSize {fs}'
      f' LocationType {loc} SegmentIndex {seg} ResourceFixups {nfix} ResourceDefinitionFixups {ndef}')
vb_count = i32(data, fixup_file + fo + fs - 24)
ib_count = i32(data, fixup_file + fo + fs - 24 + 12)
print(f'  fixup data tail: vertexBufferCount {vb_count} indexBufferCount {ib_count}'
      f'  (ResourceFixups vs 2*vb+ib+4 = {2 * vb_count + ib_count + 4})')
vbi = [(i32(data, fixup_file + fo + 28 * k), i32(data, fixup_file + fo + 28 * k + 8)) for k in range(vb_count)]
ib_base = fixup_file + fo + 28 * vb_count + 12 * vb_count
ibi = [(i32(data, ib_base + 24 * k), i32(data, ib_base + 24 * k + 4)) for k in range(ib_count)]
print('  VertexBufferInfo (VertexCount, DataLength) first 5:', vbi[:5], ' total vertices', sum(v for v, _ in vbi),
      ' total bytes', sum(d for _, d in vbi))
print('  IndexBufferInfo (IndexFormat, DataLength) first 5:', ibi[:5], ' total bytes', sum(d for _, d in ibi))
fix = [u32(data, fixp + 8 * k + 4) & 0x0FFFFFFF for k in range(nfix)]
ib0 = hex(fix[2 * vb_count]) if nfix > 2 * vb_count else None
print('  ResourceFixups masked offsets: first 4', [hex(x) for x in fix[:4]], ' index buffer 0 at fixups[2*vb+0] =', ib0)

# play (cache_file_resource_layout_table): SharedCaches @+12, Pages @+24, SizeGroups @+36, Segments @+60
play_i = global_tag('play')
play = meta_of(play_i)
if tags[play_i][1] == 0:
    play = zone + 28
nsh, shp = block(play, 12)
npg, pgp = block(play, 24)
nsg, sgp = block(play, 60)
print('play: SharedCaches', nsh, [cstr(shp + 264 * k) for k in range(nsh)], '| Pages', npg, '| Segments', nsg)
s = sgp + 16 * seg
pp, sp2, po, so = i16(data, s), i16(data, s + 2), i32(data, s + 4), i32(data, s + 8)
print(f'  segment[{seg}]: PrimaryPage {pp} SecondaryPage {sp2} PrimaryOffset {po} SecondaryOffset {so}')
for label, pi in (('primary', pp), ('secondary', sp2)):
    if pi < 0:
        continue
    g = pgp + 88 * pi
    print(f'  page[{pi}] ({label}): CacheIndex {i16(data, g + 4)} DataOffset {i32(data, g + 8):#x} CompressedSize {i32(data, g + 12)}'
          f' DecompressedSize {i32(data, g + 16)} DataChunkCount {i16(data, g + 84)}')
print('  data table address for local pages = u32 at file 1232 (0x4D0) =', hex(u32(data, 1232)), '; sectionOffset[1] =',
      hex(sec_off[1]), '; for shared.map pages Reclaimer uses 16384 (0x4000)')
