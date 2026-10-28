# Read-only scan of every MCC Halo 3 (U13) .map for sbsp submeshes whose shader reference is null.
# Run: python -I scan_null_shaders.py [maps folder or single .map]
# Writes nothing. Companion to docs/format/sbsp-render-geometry.md, gap fill "null shader reference".
#
# Reclaimer rule being tested (Blam\Common\TagReference.cs): TagId = (short)(raw & 0xFFFF);
# the reference is null when TagId < 0. Halo3Common.GetMaterials yields a null material for such a
# shader; GetMeshes assigns Material = materials.ElementAtOrDefault(submesh.ShaderIndex), so an
# out-of-range or negative ShaderIndex also gives null.
import struct, sys, os, glob

MAPS = r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps"
arg = sys.argv[1] if len(sys.argv) > 1 else MAPS
paths = [arg] if arg.lower().endswith(".map") else sorted(glob.glob(os.path.join(arg, "*.map")))


def scan(path):
    d = open(path, "rb").read()

    def i32(o): return struct.unpack_from("<i", d, o)[0]
    def u32(o): return struct.unpack_from("<I", d, o)[0]
    def i16(o): return struct.unpack_from("<h", d, o)[0]
    def u16(o): return struct.unpack_from("<H", d, o)[0]
    def i64(o): return struct.unpack_from("<q", d, o)[0]
    def cstr(o, n=256):
        e = d.index(b"\0", o); return d[o:min(e, o + n)].decode("latin1")

    if d[:4] != b"daeh":  # 'head' stored little-endian: bytes 64 61 65 68
        return "not a cache file"
    build = cstr(0xA0, 32)
    secoff = [u32(0x4CC + 4 * i) for i in range(4)]
    secs = [(u32(0x4DC + 8 * i), u32(0x4E0 + 8 * i)) for i in range(4)]
    vbase = i64(0x2E0); idxptr = i64(0x2E8); tagcount = i32(0x20)
    magic2 = (secs[2][0] + secoff[2]) & 0xFFFFFFFF

    def tagfile(v): return v - vbase + magic2
    def hdr(p): return (p + secoff[0]) & 0xFFFFFFFF
    def tp(p): return tagfile((p << 2) & 0xFFFFFFFFFFFFFFFF) if p != 0 else -1

    if tagcount <= 0 or idxptr == 0:
        return "build %s: no tag index" % build
    ti = tagfile(idxptr)
    classcount = i32(ti); classptr = i64(ti + 8); tagptr = i64(ti + 24)
    cf = tagfile(classptr); tf = tagfile(tagptr)
    classes = [d[cf + 16 * i:cf + 16 * i + 4][::-1].decode("latin1") for i in range(classcount)]
    nameidx = hdr(i32(0x2C)); names = hdr(i32(0x24))
    tags = []
    for i in range(tagcount):
        ci = i16(tf + 8 * i); mp = u32(tf + 8 * i + 4)
        no = i32(nameidx + 4 * i)
        nm = cstr(names + no) if no >= 0 else None
        tags.append((classes[ci] if 0 <= ci < classcount else None, nm, tp(mp) if ci >= 0 else -1))

    def find(cls, name=None):
        return [i for i, t in enumerate(tags) if t[0] == cls and (name is None or t[1] == name)]

    def block(o):
        c = i32(o); p = u32(o + 4); return c, (tp(p) if c > 0 else -1)

    def tagref(o):  # raw class id, raw tag id, Reclaimer TagId (signed low 16 bits)
        raw_cls = u32(o); raw_tid = i32(o + 12)
        tid16 = struct.unpack("<h", struct.pack("<H", raw_tid & 0xFFFF))[0]
        return raw_cls, raw_tid, tid16

    sbsps = find("sbsp")
    if not sbsps:
        return "build %s: %d tags, no sbsp" % (build, tagcount)

    # scnr -> StructureBsps order and sLdT -> Lbsp list (for the Lbsp-by-BspIndex rule)
    scen = cstr(0xE0)
    scnr = find("scnr", scen)
    bsp_order, lbsp_by_index = {}, {}
    if scnr:
        scnrm = tags[scnr[0]][2]
        sbc, sbp = block(scnrm + 0x18)
        for i in range(sbc):
            _, _, t16 = tagref(sbp + 108 * i)
            if t16 >= 0: bsp_order[t16] = i
        _, _, ldt = tagref(scnrm + 0x6AC)
        if ldt >= 0 and tags[ldt][0] == "sLdT":
            lrc, lrp = block(tags[ldt][2] + 4)
            for i in range(lrc):
                _, _, lt = tagref(lrp + 16 * i)
                if lt >= 0 and tags[lt][2] >= 0:
                    lbsp_by_index[i16(tags[lt][2] + 2)] = lt

    lines = ["build %s: %d tags, %d sbsp" % (build, tagcount, len(sbsps))]
    for s in sbsps:
        m = tags[s][2]
        if m < 0: continue
        shc, shp = block(m + 0xD0)
        nulls = []
        for i in range(shc):
            raw_cls, raw_tid, t16 = tagref(shp + 36 * i)
            if t16 < 0:
                nulls.append((i, raw_cls, raw_tid))
        # which Sections block does Reclaimer read: the Lbsp's (by BspIndex, then by name), else the sbsp's
        bi = bsp_order.get(s)
        lb = lbsp_by_index.get(bi) if bi is not None else None
        if lb is None:
            cands = find("Lbsp", tags[s][1]); lb = cands[0] if cands else None
        if lb is not None:
            secc, secp = block(tags[lb][2] + 0x134); src = "Lbsp %d" % lb
        else:
            secc, secp = block(m + 0x2F4); src = "sbsp own"
        own_c, own_p = block(m + 0x2F4)
        same = (own_c, own_p) == (secc, secp)
        # placements per section
        clc, clp = block(m + 0xC4); inc, inp = block(m + 0x1C0)
        placed = {}
        for c in range(clc):
            si = i16(clp + 280 * c + 216); placed[si] = placed.get(si, 0) + 1
        for n in range(inc):
            si = i16(inp + 120 * n + 52); placed[si] = placed.get(si, 0) + 1
        nullset = {n[0] for n in nulls}

        def scan_sections(secc, secp):
            bad = []  # (section, submesh, shaderIndex, reason, tris_unplaced, tris_placed, vb)
            for si in range(secc):
                sec = secp + 76 * si
                vbi = i16(sec + 0x18)
                smc, smp = block(sec)
                for k in range(smc):
                    sm = smp + 16 * k
                    shi = i16(sm); ilen = u16(sm + 6)
                    reason = None
                    if shi < 0 or shi >= shc: reason = "out of range"
                    elif shi in nullset: reason = "null ref"
                    if reason:
                        bad.append((si, k, shi, reason, ilen // 3, (ilen // 3) * placed.get(si, 0), vbi))
            return bad

        bad_sub = scan_sections(secc, secp)
        tot_sub = sum(i32(secp + 76 * si) for si in range(secc))
        line = "  sbsp %d %s: shaders %d, null refs %d; sections %d (%s%s), submeshes %d, bad submeshes %d" % (
            s, tags[s][1], shc, len(nulls), secc, src, "" if same else ", differs from sbsp+0x2F4", tot_sub, len(bad_sub))
        if not same:  # also test the sbsp's own copy of the Sections block
            own_bad = scan_sections(own_c, own_p)
            line += "; sbsp-own sections %d, bad submeshes %d" % (own_c, len(own_bad))
            for (si, k, shi, reason, t, tpl, vbi) in own_bad:
                bad_sub.append((si, k, shi, reason + " (sbsp-own block)", t, tpl, vbi))
        lines.append(line)
        for (i, raw_cls, raw_tid) in nulls:
            lines.append("    null shader[%d]: class bytes %08X (%r) raw tag id %08X" % (
                i, raw_cls, struct.pack("<I", raw_cls)[::-1], raw_tid & 0xFFFFFFFF))
        for (si, k, shi, reason, t, tpl, vbi) in bad_sub:
            lines.append("    section %d submesh %d ShaderIndex %d (%s): %d tris unplaced, %d placed, vb %d" % (
                si, k, shi, reason, t, tpl, vbi))
    return "\n".join(lines)


for p in paths:
    try:
        print(os.path.basename(p) + ": " + scan(p))
    except Exception as e:  # keep going over the folder
        print(os.path.basename(p) + ": ERROR " + repr(e))
