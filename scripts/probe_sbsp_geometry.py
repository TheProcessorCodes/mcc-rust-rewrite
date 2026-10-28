# Read-only probe of MCC Halo 3 (U13) sbsp render geometry on riverworld.map.
# Run: python -I probe_sbsp_geometry.py [map path]
# Writes nothing. Companion to docs/format/sbsp-render-geometry.md.
import struct, sys, os, math

MAPS = r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps"
path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(MAPS, "riverworld.map")
d = open(path, "rb").read()
print("file", path, "size 0x%X" % len(d))

def i32(o): return struct.unpack_from("<i", d, o)[0]
def u32(o): return struct.unpack_from("<I", d, o)[0]
def i16(o): return struct.unpack_from("<h", d, o)[0]
def u16(o): return struct.unpack_from("<H", d, o)[0]
def i64(o): return struct.unpack_from("<q", d, o)[0]
def f32(o): return struct.unpack_from("<f", d, o)[0]
def cstr(o, n=256):
    e = d.index(b"\0", o); return d[o:min(e, o + n)].decode("latin1")

# ---- header
secoff = [u32(0x4CC + 4 * i) for i in range(4)]
secs = [(u32(0x4DC + 8 * i), u32(0x4E0 + 8 * i)) for i in range(4)]
vbase = i64(0x2E0); idxptr = i64(0x2E8)
tagcount = i32(0x20); strcount = i32(0x30); nscount = i32(0x40)
print("sections", [hex(x) for x in secoff], [(hex(a), hex(s)) for a, s in secs])
magic2 = (secs[2][0] + secoff[2]) & 0xFFFFFFFF
def tagfile(v):  # virtual (64-bit or expanded) -> file offset (section 2)
    return v - vbase + magic2
def hdr(p, sec=0):  # header 32-bit pointer -> file offset
    return (p + secoff[sec]) & 0xFFFFFFFF
def tp(p):  # shifted 32-bit tag-data pointer -> file offset
    return tagfile((p << 2) & 0xFFFFFFFFFFFFFFFF) if p != 0 else -1
rawbase = u32(0x4D0)
print("vbase 0x%X idxptr 0x%X -> file 0x%X ; raw/section1 base (hdr 0x4D0) 0x%X" % (vbase, idxptr, tagfile(idxptr), rawbase))

# ---- strings
str_index = hdr(i32(0x3C)); str_table = hdr(i32(0x34)); ns_table = hdr(i32(0x44))
nsarr = [i32(ns_table + 4 * i) for i in range(nscount)]
INDEXBITS, NSBITS = 19, 8
mask = (1 << INDEXBITS) - 1
namespaces = {}
start = nsarr[0] & mask
for i in range(1, nscount):
    namespaces[i] = (0, start); start += nsarr[i] & mask
namespaces[0] = (nsarr[0] & mask, start)
def string_index(sid):
    idx = sid & mask; ns = (sid >> INDEXBITS) & ((1 << NSBITS) - 1)
    while ns not in namespaces and ns > 0: ns -= 1
    mn, st = namespaces[ns]
    return idx if idx < mn else idx - mn + st
def sidstr(sid):
    si = string_index(sid)
    if si < 0 or si >= strcount: return "<bad %08X>" % sid
    off = i32(str_index + 4 * si)
    return cstr(str_table + off) if off >= 0 else "<null>"

# ---- tag index
ti = tagfile(idxptr)
classcount = i32(ti); classptr = i64(ti + 8); tagcount2 = i32(ti + 16); tagptr = i64(ti + 24)
assert tagcount2 == tagcount
cf = tagfile(classptr); tf = tagfile(tagptr)
classes = []
for i in range(classcount):
    classes.append(d[cf + 16 * i:cf + 16 * i + 4][::-1].decode("latin1"))
nameidx = hdr(i32(0x2C)); names = hdr(i32(0x24))
tags = []  # (class, name, metafile)
for i in range(tagcount):
    ci = i16(tf + 8 * i); mp = u32(tf + 8 * i + 4)
    no = i32(nameidx + 4 * i)
    nm = cstr(names + no) if no >= 0 else None
    tags.append((classes[ci] if ci >= 0 else None, nm, tp(mp) if ci >= 0 else -1))
print("classes", classcount, "tags", tagcount)

def find(cls, name=None):
    return [i for i, t in enumerate(tags) if t[0] == cls and (name is None or t[1] == name)]

def block(o):  # TagBlock: count, shifted ptr, pad
    c = i32(o); p = u32(o + 4); return c, (tp(p) if c > 0 else -1)
def tagref(o):  # class id, 8 skip, tag id
    return d[o:o + 4][::-1].decode("latin1"), i32(o + 12)
def tagidx(tid): return tid & 0xFFFF if tid != -1 else -1

scen = cstr(0xE0)
scnr = find("scnr", scen)[0]; scnrm = tags[scnr][2]
print("scenario", scen, "tag", scnr, "meta file 0x%X" % scnrm)
sbsps = find("sbsp")
print("sbsp tags:", [(i, tags[i][1]) for i in sbsps])
sbc, sbp = block(scnrm + 24)
print("scnr.StructureBsps count", sbc)
bsp_list = []
for i in range(sbc):
    c, tid = tagref(sbp + 108 * i); bsp_list.append(tagidx(tid))
    print("  bsp", i, c, tagidx(tid), tags[tagidx(tid)][1])
ldc, ldt = tagref(scnrm + 1708); ldt = tagidx(ldt)
print("scnr.ScenarioLightmapReference", ldc, ldt, tags[ldt][1])
lrc, lrp = block(tags[ldt][2] + 4)
print("sLdT.LightmapRefs count", lrc)
lbsps = []
for i in range(lrc):
    c, tid = tagref(lrp + 16 * i); tid = tagidx(tid)
    if tid < 0:
        print("  ref", i, "null"); continue
    lm = tags[tid][2]
    print("  ref", i, c, tid, tags[tid][1], "BspIndex", i16(lm + 2), "Sections", block(lm + 308), "ResourcePointer 0x%08X" % u32(lm + 428))
    lbsps.append(tid)

# ---- sbsp (bsp 0)
bsp_index = 0
sb = bsp_list[bsp_index]; m = tags[sb][2]
print("\n=== sbsp", tags[sb][1], "meta file 0x%X" % m)
xb = (f32(m + 64), f32(m + 68)); yb = (f32(m + 72), f32(m + 76)); zb = (f32(m + 80), f32(m + 84))
print("world bounds X", xb, "Y", yb, "Z", zb)
clc, clp = block(m + 196); shc, shp = block(m + 208); gic, gip = block(m + 448)
sec_c, sec_p = block(m + 756); bb_c, bb_p = block(m + 768)
print("Clusters", clc, "Shaders", shc, "GeometryInstances", gic, "Sections(sbsp)", sec_c, "BoundingBoxes", bb_c)
print("ResourcePointer1 0x%08X ResourcePointer2 0x%08X ResourcePointer3 0x%08X" % (u32(m + 596), u32(m + 876), u32(m + 908)))
print("first shaders:")
for i in range(min(shc, 6)):
    c, tid = tagref(shp + 36 * i); print("  ", i, c, tagidx(tid), tags[tagidx(tid)][1] if tagidx(tid) >= 0 else None)
print("clusters (bounds, section index):")
cl_secs = []
for i in range(clc):
    o = clp + 280 * i; si = i16(o + 216); cl_secs.append(si)
    if i < 5: print("  ", i, [round(f32(o + 4 * k), 2) for k in range(6)], "section", si)
print("  cluster section indices:", cl_secs)
print("instances:")
inst_secs = []
for i in range(gic):
    o = gip + 120 * i
    scale = f32(o); mtx = [f32(o + 4 + 4 * k) for k in range(12)]; si = i16(o + 52); nm = sidstr(i32(o + 84))
    inst_secs.append(si)
    if i < 3:
        print("  inst", i, "scale", scale, "name", nm, "section", si)
        print("     rows", [[round(x, 5) for x in mtx[r * 3:r * 3 + 3]] for r in range(4)])
        print("     dwords 52..84:", " ".join("%08X" % u32(o + k) for k in range(52, 84, 4)))
        print("     floats 56..84:", [round(f32(o + k), 3) for k in range(56, 84, 4)])
        print("     dwords 88..120:", " ".join("%08X" % u32(o + k) for k in range(88, 120, 4)))
print("  instance section index range", min(inst_secs), max(inst_secs), "unique", len(set(inst_secs)))
allnames = [sidstr(i32(gip + 120 * i + 84)) for i in range(gic)]
skyish = [(i, n) for i, n in enumerate(allnames) if "sky" in n.lower() or "blocker" in n.lower()]
print("  sky/blocker-named instances:", skyish[:10])
print("  instance names sample:", allnames[:8])
print("bounding boxes (compression bounds):")
def bbox(i):
    o = bb_p + 44 * i; return [round(f32(o + 4 + 4 * k), 4) for k in range(10)], u32(o)
for i in range(min(bb_c, 3)):
    print("  bb", i, bbox(i))
print("  bb for cluster 0 section", cl_secs[0], bbox(cl_secs[0]))
print("  bb for instance 0 section", inst_secs[0], bbox(inst_secs[0]))
same = all(bbox(cl_secs[0])[0] == bbox(s)[0] for s in cl_secs)
print("  all cluster sections share identical bounds:", same)

# ---- Lbsp geometry
lb = [t for t in lbsps if i16(tags[t][2] + 2) == bsp_index]
lbm = tags[lb[0]][2]
lsec_c, lsec_p = block(lbm + 308); rp = u32(lbm + 428)
print("\n=== Lbsp", tags[lb[0]][1], "sections", lsec_c, "resource 0x%08X (index %d)" % (rp, rp & 0xFFFF))
def section(p, i):
    o = p + 76 * i
    sub_c, sub_p = block(o); ss_c, ss_p = block(o + 12)
    return dict(submeshes=(sub_c, sub_p), subsets=(ss_c, ss_p), vb=i16(o + 24), ib=i16(o + 40), flags=d[o + 44], node=d[o + 45], vfmt=d[o + 46], ifmt=d[o + 48], raw24_52=d[o + 24:o + 52].hex())
for i in range(min(lsec_c, 3)):
    print("  sec", i, section(lsec_p, i))
print("  sbsp-own sections, first 2:", [section(sec_p, i) for i in range(min(sec_c, 2))] if sec_c else None)
vfmts = {}
for i in range(lsec_c):
    s = section(lsec_p, i); vfmts[s["vfmt"]] = vfmts.get(s["vfmt"], 0) + 1
print("  vertex formats used:", vfmts)
ifmts = {}
for i in range(lsec_c):
    s = section(lsec_p, i); ifmts[s["ifmt"]] = ifmts.get(s["ifmt"], 0) + 1
print("  section IndexFormat bytes:", ifmts)
flagset = {}
for i in range(lsec_c):
    s = section(lsec_p, i); flagset[s["flags"]] = flagset.get(s["flags"], 0) + 1
print("  section flags:", flagset)

# ---- zone / resource gestalt
zone = find("zone")[0]; zm = tags[zone][2]
rec, rep = block(zm + 100); fsz = i32(zm + 328); fptr = tp(u32(zm + 340))
print("\n=== zone ResourceEntries", rec, "FixupDataSize", fsz, "FixupData file 0x%X" % fptr)
ri = rp & 0xFFFF
e = rep + 64 * ri
fo, fs = i32(e + 20), i32(e + 24); loc, seg = i16(e + 32), i16(e + 34)
rf_c, rf_p = block(e + 40); rdf_c, rdf_p = block(e + 52)
own = tagref(e)
print("entry", ri, "owner", own[0], tagidx(own[1]), "fixup off", fo, "size", fs, "loctype", loc, "segment", seg, "fixups", rf_c, "deffixups", rdf_c)
fix = [(u32(rf_p + 8 * k), u32(rf_p + 8 * k + 4)) for k in range(rf_c)]
print("  fixups (unknown, offset):", [("%08X" % a, "%08X" % b) for a, b in fix[:6]], "...")
print("  deffixups (first 4, last 2):", [("%08X" % u32(rdf_p + 8 * k), "%08X" % u32(rdf_p + 8 * k + 4)) for k in list(range(min(rdf_c, 4))) + list(range(max(0, rdf_c - 2), rdf_c))])
print("  fixups 2*vb.. (index buffer group, first 3):", [("%08X" % a, "%08X" % b) for a, b in fix[2 * i32(fd + fs - 24):2 * i32(fd + fs - 24) + 3]] if False else "see below")
fd = fptr + fo
vbcount = i32(fd + fs - 24); ibcount = i32(fd + fs - 12)
print("  definition tail (24 bytes):", d[fd + fs - 24:fd + fs].hex(), "-> vb", vbcount, "ib", ibcount)
expect = 28 * vbcount + 12 * vbcount + 24 * ibcount + 12 * ibcount + 48 + 24
print("  definition total size", fs, "= 28v+12v+24i+12i+48+24 ->", expect)
vbinfo = []
for k in range(vbcount):
    o = fd + 28 * k; vals = [i32(o + 4 * j) for j in range(7)]; vbinfo.append(vals)
    if k < 6: print("  vb", k, vals)
o2 = fd + 28 * vbcount
print("  12-byte structs after vb infos (first 2):", [d[o2 + 12 * k:o2 + 12 * k + 12].hex() for k in range(2)])
ibo = o2 + 12 * vbcount
ibinfo = []
for k in range(ibcount):
    o = ibo + 24 * k; vals = [i32(o + 4 * j) for j in range(6)]; ibinfo.append(vals)
    if k < 4: print("  ib", k, vals)
o3 = ibo + 24 * ibcount
print("  12-byte structs after ib infos (first 2):", [d[o3 + 12 * k:o3 + 12 * k + 12].hex() for k in range(2)])
print("  trailing 4x12:", [d[o3 + 12 * ibcount + 12 * k:o3 + 12 * ibcount + 12 * k + 12].hex() for k in range(4)])
print("  fixup count vs 2*vb+2*ib+?:", rf_c, 2 * vbcount + 2 * ibcount)
for k in range(min(vbcount, 6)):
    c, fmt, length = vbinfo[k][0], vbinfo[k][1], vbinfo[k][2]
    print("  vb", k, "count", c, "field1", fmt, "len", length, "len/count", (length / c) if c else None)

# ---- play / resource layout table
play = find("play")
pm = tags[play[0]][2] if play else tags[zone][2] + 28
shc_, shp_ = block(pm + 12); pgc, pgp = block(pm + 24); sgc, sgp = block(pm + 60)
print("\n=== play SharedCaches", shc_, "Pages", pgc, "Segments", sgc)
for k in range(shc_): print("  shared", k, cstr(shp_ + 264 * k, 32))
sgo = sgp + 16 * seg
pp, sp = i16(sgo), i16(sgo + 2); po, so = i32(sgo + 4), i32(sgo + 8)
print("segment", seg, "primary page", pp, "offset", po, "secondary page", sp, "offset", so)
usesec = sp >= 0
pg = sp if usesec else pp; pgoff = so if usesec else po
pgo = pgp + 88 * pg
cidx, doff, csz, dsz = i16(pgo + 4), i32(pgo + 8), i32(pgo + 12), i32(pgo + 16)
print("page", pg, "cacheindex", cidx, "dataoffset 0x%X" % doff, "compressed", csz, "decompressed", dsz)
if usesec and (doff < 0 or csz == 0):
    pg = pp; pgoff = po; pgo = pgp + 88 * pg
    cidx, doff, csz, dsz = i16(pgo + 4), i32(pgo + 8), i32(pgo + 12), i32(pgo + 16)
    print("  -> fell back to primary page", pg, cidx, hex(doff), csz, dsz)
if cidx >= 0:
    src = open(os.path.join(os.path.dirname(path), cstr(shp_ + 264 * cidx, 32)), "rb").read(); base = 16384
else:
    src = d; base = rawbase
import zlib
raw = src[base + doff: base + doff + csz]
if csz != dsz:
    print("  page is deflate-compressed: first bytes", raw[:4].hex(), "-> raw deflate (no zlib header)")
    page = zlib.decompressobj(-15).decompress(raw)
    print("  decompressed", len(page), "bytes (expected", dsz, ")")
else:
    page = raw
res = page[pgoff:]
print("resource data: %d bytes available from page offset %d (page file offset 0x%X)" % (len(res), pgoff, base + doff))

# ---- decode section vertices
def dec_u16n(b, o, n): return [struct.unpack_from("<H", b, o + 2 * k)[0] / 65535.0 for k in range(n)]
def dec_i16n(b, o, n): return [max(-1.0, struct.unpack_from("<h", b, o + 2 * k)[0] / 32767.0) for k in range(n)]
stride_of = {0: 44, 1: 36, 2: 44, 0xF: 20, 0x10: 8}
def decode_section(si, nverts=3):
    s = section(lsec_p, si)
    vbi, ibi = s["vb"], s["ib"]
    vinfo = vbinfo[vbi]; vcount, vlen = vinfo[0], vinfo[2]
    vaddr = fix[vbi][1] & 0x0FFFFFFF
    bbv, bbflags = bbox(si) if si < bb_c else ([0] * 10, None)
    print(" section", si, "(bounds index valid: %s)" % (si < bb_c), "vfmt 0x%02X vb %d (count %d len %d addr 0x%X) ib %d flags %02X node %d" % (s["vfmt"], vbi, vcount, vlen, vaddr, ibi, s["flags"], s["node"]))
    st = stride_of[s["vfmt"]]
    print("   stride check: len/count =", vlen / vcount if vcount else None, "expected", st, "| vb field1 =", vinfo[1], "field3..6 =", vinfo[3:])
    mn = [1e9] * 3; mx = [-1e9] * 3
    for v in range(vcount):
        o = vaddr + st * v
        if s["vfmt"] == 0:
            pos = list(struct.unpack_from("<3f", res, o)); uv = list(struct.unpack_from("<2f", res, o + 12)); nrm = dec_i16n(res, o + 0x14, 4)
        else:
            p = dec_u16n(res, o, 4); uv = dec_u16n(res, o + 8, 2); nrm = dec_i16n(res, o + 0xC, 4)
            pos = [bbv[0] + p[0] * (bbv[1] - bbv[0]), bbv[2] + p[1] * (bbv[3] - bbv[2]), bbv[4] + p[2] * (bbv[5] - bbv[4])]
            uv = [bbv[6] + uv[0] * (bbv[7] - bbv[6]), bbv[8] + uv[1] * (bbv[9] - bbv[8])]
        for k in range(3):
            mn[k] = min(mn[k], pos[k]); mx[k] = max(mx[k], pos[k])
        if v < nverts:
            print("   v%d pos %s uv %s nrm %s |n|=%.3f" % (v, [round(x, 3) for x in pos], [round(x, 3) for x in uv], [round(x, 3) for x in nrm[:3]], math.sqrt(sum(x * x for x in nrm[:3]))))
    print("   decoded position range min", [round(x, 2) for x in mn], "max", [round(x, 2) for x in mx])
    if ibi >= 0 and not (s["flags"] & 0x10):
        iinfo = ibinfo[ibi]; iaddr = fix[2 * vbcount + ibi][1] & 0x0FFFFFFF
        isz = 4 if vcount > 65535 else 2
        n = iinfo[1] // isz
        idx = list(struct.unpack_from("<%d%s" % (min(n, 12), "I" if isz == 4 else "H"), res, iaddr))
        allidx = struct.unpack_from("<%d%s" % (n, "I" if isz == 4 else "H"), res, iaddr)
        print("   ib %d format %d len %d (%d indices of %d bytes) addr 0x%X max index %d first:" % (ibi, iinfo[0], iinfo[1], n, isz, iaddr, max(allidx)), idx)
        sm_c, sm_p = s["submeshes"]
        tot = 0
        for k in range(sm_c):
            o = sm_p + 16 * k
            if k < 3: print("   submesh", k, "shader", i16(o), "start", u16(o + 4), "len", u16(o + 6), "subset", u16(o + 8), "n", u16(o + 10), "flags", d[o + 13], "verts", u16(o + 14))
            tot += u16(o + 6)
        print("   submesh index total", tot, "vs ib indices", n, "submeshes", sm_c)
        ss_c, ss_p = s["subsets"]
        print("   subsets", ss_c, "first:", [(u16(ss_p + 8 * k), u16(ss_p + 8 * k + 2), u16(ss_p + 8 * k + 4), u16(ss_p + 8 * k + 6)) for k in range(min(ss_c, 3))])
print("\n--- vertex format per section range")
runs = []
for i in range(lsec_c):
    s = section(lsec_p, i); vb_ = vbinfo[s["vb"]] if 0 <= s["vb"] < vbcount else None
    key = (s["vfmt"], vb_[1] & 0xFFFF if vb_ else None, (vb_[1] >> 16) if vb_ else None, s["flags"], s["ifmt"])
    if runs and runs[-1][0] == key: runs[-1][2] = i
    else: runs.append([key, i, i])
for key, a, b in runs:
    print("  sections %d..%d: section.vfmt 0x%02X | resource vb format %s stride %s | flags %02X | section ifmt %d" % (a, b, key[0], key[1], key[2], key[3], key[4]))
print("--- zero-vertex sections:", [i for i in range(lsec_c) if 0 <= section(lsec_p, i)["vb"] < vbcount and vbinfo[section(lsec_p, i)["vb"]][0] == 0])
print("--- Lbsp block at 320 (compression info of the Lbsp render geometry?):", block(lbm + 320), " sbsp block at 768:", (bb_c, bb_p))
lc_c, lc_p = block(lbm + 320)
if lc_c:
    print("    Lbsp comp[0]:", [round(f32(lc_p + 4 + 4 * k), 4) for k in range(10)], "flags", u32(lc_p))
    print("    Lbsp comp[76]:", [round(f32(lc_p + 44 * 76 + 4 + 4 * k), 4) for k in range(10)] if lc_c > 76 else None)
    print("    identical to sbsp BoundingBoxes for first 76:", all(d[lc_p + 44 * k:lc_p + 44 * (k + 1)] == d[bb_p + 44 * k:bb_p + 44 * (k + 1)] for k in range(min(lc_c, bb_c))))
print("\n=== decode cluster 0 section (sky/empty?)")
decode_section(cl_secs[0])
print("=== decode cluster 2 section (first with real bounds)")
decode_section(cl_secs[2])
print("=== decode instance 0 section")
decode_section(inst_secs[0])
ok = all(0 <= section(lsec_p, i)["vb"] < vbcount for i in range(lsec_c))
sk = sum(1 for i in range(lsec_c) if vbinfo[section(lsec_p, i)["vb"]][0] == 0) if ok else "n/a"
print("sections with zero-vertex buffers:", sk, "of", lsec_c)
print("sections with vb index out of range:", [i for i in range(lsec_c) if not (0 <= section(lsec_p, i)["vb"] < vbcount)])
print("vertex buffers with >65535 verts:", [k for k in range(vbcount) if vbinfo[k][0] > 65535])
ibf = {}
for k in range(ibcount): ibf[ibinfo[k][0]] = ibf.get(ibinfo[k][0], 0) + 1
print("index buffer formats in resource:", ibf)
# which sections are referenced by nobody
used = set(cl_secs) | set(inst_secs)
print("sections referenced by no cluster/instance:", [i for i in range(lsec_c) if i not in used][:20], "...")

print("\n=== extra checks")
print("sbsp Sections block ptr 0x%X vs Lbsp Sections block ptr 0x%X (same data: %s)" % (sec_p, lsec_p, sec_p == lsec_p))
print("sbsp BoundingBoxes ptr 0x%X vs Lbsp block@320 ptr 0x%X" % (bb_p, lc_p))
for label, off in (("ResourcePointer1@596", 596), ("ResourcePointer2@876", 876), ("ResourcePointer3@908", 908)):
    v = u32(m + off); ri2 = v & 0xFFFF
    if ri2 < rec:
        e2 = rep + 64 * ri2; o2_ = tagref(e2)
        print("  %s = 0x%08X index %d -> owner %s %d (%s) fixup size %d loctype %d segment %d" % (label, v, ri2, o2_[0], tagidx(o2_[1]), tags[tagidx(o2_[1])][1], i32(e2 + 24), i16(e2 + 32), i16(e2 + 34)))
    else:
        print("  %s = 0x%08X index %d out of range" % (label, v, ri2))
e2 = rep + 64 * ri; print("  Lbsp resource entry: ResourcePointer field at +16 = 0x%08X (should equal 0x%08X)" % (u32(e2 + 16), rp))
# instance unknown fields
same56 = all(i32(gip + 120 * i + 56) == i16(gip + 120 * i + 52) for i in range(gic))
vals54 = sorted(set(i16(gip + 120 * i + 54) for i in range(gic)))
vals90 = sorted(set(i16(gip + 120 * i + 90) for i in range(gic)))
vals88 = sorted(set(i16(gip + 120 * i + 88) for i in range(gic)))
vals92 = sorted(set(round(f32(gip + 120 * i + 92), 3) for i in range(gic)))
vals96 = sorted(set(i32(gip + 120 * i + 96) for i in range(gic)))
print("  instance i32@56 == i16@52 for all:", same56, "| i16@54 values", vals54, "| i16@88", vals88, "| i16@90", vals90, "| f32@92", vals92[:6], "| i32@96", vals96[:6])
ptrs = [u32(gip + 120 * i + 100) for i in range(gic)]
print("  instance u32@100: first 3 %s; stride between consecutive %s; expanded first -> file 0x%X" % (["%08X" % p for p in ptrs[:3]], sorted(set((ptrs[i + 1] - ptrs[i]) for i in range(gic - 1)))[:4], tp(ptrs[0])))
# sphere check: translation vs sphere centre
for i in range(3):
    o = gip + 120 * i
    print("  inst %d translation %s sphere centre %s radius %.3f" % (i, [round(f32(o + 40 + 4 * k), 3) for k in range(3)], [round(f32(o + 64 + 4 * k), 3) for k in range(3)], f32(o + 76)))
# scan sbsp meta for blocks with count 76 or 262 or 26
print("  blocks in sbsp meta (offset: count) with plausible pointer, counts of interest:")
for off in range(0, 1100, 4):
    c = i32(m + off); p = u32(m + off + 4)
    if c in (26, 64, 76, 102, 262) and p != 0:
        fo_ = tp(p)
        if 0 < fo_ < len(d): print("    @%d (0x%X): count %d -> file 0x%X" % (off, off, c, fo_))
c19 = tagref(shp + 36 * 19); print("  shader 19 (submesh flagged 8/IsWaterSurface in section 78):", c19[0], tags[tagidx(c19[1])][1])
c40 = tagref(shp + 36 * 40); print("  shader 40 (instance 0 submesh):", c40[0], tags[tagidx(c40[1])][1])
# fixup patch offsets: first vb fixup patch offset, first ib fixup patch offset
print("  fixup[0] patch offset 0x%X ; fixup[vb] patch offset 0x%X ; fixup[2vb] 0x%X ; fixup[2vb+ib] 0x%X ; last two %s" % (fix[0][0], fix[vbcount][0], fix[2 * vbcount][0], fix[2 * vbcount + ibcount][0], [("%08X" % a, "%08X" % b) for a, b in fix[-2:]]))
print("  expected: 20 (vb0 +20), 0x%X (12-byte structs), 0x%X (ib0 +8?), ..." % (28 * vbcount, 28 * vbcount + 12 * vbcount + 8))
# submesh/subset full check of section 0 subsets covering index range
s0 = section(lsec_p, inst_secs[0]); ss_c, ss_p = s0["subsets"]
print("  section 0 subsets sum of lengths:", sum(u16(ss_p + 8 * k + 2) for k in range(ss_c)), "vertex counts sum", sum(u16(ss_p + 8 * k + 6) for k in range(ss_c)))
# world-space check for instance 0: transform decoded vertex 0 by rotation rows + translation
o = gip; R = [[f32(o + 4 + 4 * (r * 3 + c)) for c in range(3)] for r in range(3)]; T = [f32(o + 40 + 4 * k) for k in range(3)]; S = f32(o)
v = [-0.144, -2.341, -5.944]
w = [S * sum(v[k] * R[k][c] for k in range(3)) + T[c] for c in range(3)]
print("  inst 0 vertex 0 world (row-vector convention):", [round(x, 3) for x in w])
