# Read-only probe of MCC Halo 3 (U13) render-method shaders on riverworld.map.
# Run: python -I probe_shaders.py [map path] [--rmdf | --rmt2 | shader tag name ...]
# Writes nothing. Companion to docs/format/shaders.md.
import struct, sys, os

MAPS = r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps"
args = list(sys.argv[1:])
path = args.pop(0) if args and args[0].lower().endswith(".map") else os.path.join(MAPS, "riverworld.map")
d = open(path, "rb").read()
print("file", path, "size 0x%X" % len(d))

def i32(o): return struct.unpack_from("<i", d, o)[0]
def u32(o): return struct.unpack_from("<I", d, o)[0]
def i16(o): return struct.unpack_from("<h", d, o)[0]
def u16(o): return struct.unpack_from("<H", d, o)[0]
def u8(o): return d[o]
def i64(o): return struct.unpack_from("<q", d, o)[0]
def f32(o): return struct.unpack_from("<f", d, o)[0]
def cstr(o, n=256):
    e = d.index(b"\0", o); return d[o:min(e, o + n)].decode("latin1")

# ---- header / address translation (container.md)
secoff = [u32(0x4CC + 4 * i) for i in range(4)]
secs = [(u32(0x4DC + 8 * i), u32(0x4E0 + 8 * i)) for i in range(4)]
vbase = i64(0x2E0); idxptr = i64(0x2E8)
tagcount = i32(0x20); strcount = i32(0x30); nscount = i32(0x40)
magic2 = (secs[2][0] + secoff[2]) & 0xFFFFFFFF
def tagfile(v): return v - vbase + magic2
def hdr(p, sec=0): return (p + secoff[sec]) & 0xFFFFFFFF
def tp(p): return tagfile((p << 2) & 0xFFFFFFFFFFFFFFFF) if p != 0 else -1

# ---- string ids
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
    if sid == 0: return "<none>"
    si = string_index(sid)
    if si < 0 or si >= strcount: return "<bad %08X>" % sid
    off = i32(str_index + 4 * si)
    return cstr(str_table + off) if off >= 0 else "<null>"

# ---- tag index
ti = tagfile(idxptr)
classcount = i32(ti); classptr = i64(ti + 8); tagptr = i64(ti + 24)
cf = tagfile(classptr); tf = tagfile(tagptr)
classes = [d[cf + 16 * i:cf + 16 * i + 4][::-1].decode("latin1") for i in range(classcount)]
nameidx = hdr(i32(0x2C)); names = hdr(i32(0x24))
tags = []  # (class, name, metafile)
for i in range(tagcount):
    ci = i16(tf + 8 * i); mp = u32(tf + 8 * i + 4)
    no = i32(nameidx + 4 * i)
    nm = cstr(names + no) if no >= 0 else None
    tags.append((classes[ci] if ci >= 0 else None, nm, tp(mp) if ci >= 0 else -1))

def block(o):
    c = i32(o); p = u32(o + 4); return c, (tp(p) if c > 0 else -1)
def tagref(o):
    return d[o:o + 4][::-1].decode("latin1"), i32(o + 12)
def tagidx(tid): return tid & 0xFFFF if tid != -1 else -1
def tagname(tid):
    i = tagidx(tid); return None if i < 0 else "%s:%s" % (tags[i][0], tags[i][1])
def find_name(name):
    r = [i for i, t in enumerate(tags) if t[1] == name and t[0] and t[0].startswith("rm")]
    return r[0] if r else None

# ---- rmdf
rmdf_cache = {}
def read_rmdf(ti):
    if ti in rmdf_cache: return rmdf_cache[ti]
    m = tags[ti][2]
    cc, cp = block(m + 16)
    cats = []
    for i in range(cc):
        e = cp + 24 * i
        name = sidstr(i32(e))
        oc, op = block(e + 4)
        opts = [sidstr(i32(op + 28 * j)) for j in range(oc)]
        cats.append((name, opts))
    rmdf_cache[ti] = cats
    return cats

def dump_rmdf(ti):
    m = tags[ti][2]
    print("\n=== rmdf", tags[ti][1], "tag", ti, "meta 0x%X" % m)
    print(" first 0x50 bytes:")
    for o in range(0, 0x50, 16):
        print("  +0x%02X %s" % (o, d[m + o:m + o + 16].hex(" ")))
    print(" +0x00 TagRef", tagref(m), tagname(tagref(m)[1]))
    for o in range(4, 0x50, 4):
        c, p = block(m + o)
        if 0 < c < 100 and p > 0 and p < len(d): print(" block at +0x%02X (%d) count %d file 0x%X" % (o, o, c, p))
    for ci, (name, opts) in enumerate(read_rmdf(ti)):
        print("  cat %2d %-22s (%d) %s" % (ci, name, len(opts), ", ".join(opts)))

def dump_rmdf_layout(ti):
    m = tags[ti][2]
    cc, cp = block(m + 16)
    print("\n rmdf", tags[ti][1], "category 0 raw 24 bytes:", d[cp:cp + 24].hex(" "))
    print("   +0 sid", sidstr(i32(cp)), " +4 block", block(cp + 4), " +16 tagref", tagref(cp + 16), tagname(tagref(cp + 16)[1]) if False else "")
    for ci in range(min(cc, 3)):
        e = cp + 24 * ci
        print("   cat", ci, sidstr(i32(e)), "+16..24:", d[e + 16:e + 24].hex(" "))
    oc, op = block(cp + 4)
    for j in range(min(oc, 3)):
        e = op + 28 * j
        print("   option", j, sidstr(i32(e)), "raw", d[e:e + 28].hex(" "))
        print("      +4 tagref", tagref(e + 4), tagname(tagref(e + 4)[1]), "+20 i32", i32(e + 20), "+24 i32", i32(e + 24))

# ---- rmt2
def read_rmt2(ti):
    m = tags[ti][2]
    ac, ap = block(m + 72); uc, up = block(m + 108)
    argsl = [sidstr(i32(ap + 4 * j)) for j in range(ac)]
    usages = [sidstr(i32(up + 4 * j)) for j in range(uc)]
    return argsl, usages

def dump_rmt2(ti):
    m = tags[ti][2]
    print("\n=== rmt2", tags[ti][1], "tag", ti, "meta 0x%X" % m)
    print(" first 0xA0 bytes:")
    for o in range(0, 0xA0, 16):
        print("  +0x%02X %s" % (o, d[m + o:m + o + 16].hex(" ")))
    print(" +0x00 TagRef", tagref(m), tagname(tagref(m)[1]))
    print(" +0x10 TagRef", tagref(m + 16), tagname(tagref(m + 16)[1]))
    print(" blocks at 4-byte offsets with plausible counts:")
    for o in range(0x20, 0xA0, 4):
        c, p = block(m + o)
        if 0 < c < 200 and p > 0 and p < len(d): print("  +0x%02X (%d) count %d file 0x%X" % (o, o, c, p))
    argsl, usages = read_rmt2(ti)
    print(" Arguments (+0x48):", argsl)
    print(" Usages    (+0x6C):", usages)
    for o in (0x3C, 0x54, 0x60, 0x78, 0x84, 0x90):
        c, p = block(m + o)
        if 0 < c < 200 and p > 0 and p < len(d):
            vals = [sidstr(i32(p + 4 * j)) for j in range(c)]
            print("  +0x%02X as StringIds:" % o, vals)

# ---- shader
def dump_shader(ti, verbose=True):
    cls, name, m = tags[ti]
    print("\n################ %s %s (tag %d, meta 0x%X)" % (cls, name, ti, m))
    rc, rt = tagref(m)
    rmdf_ti = tagidx(rt)
    print(" +0x00 rmdf ref:", rc, tagname(rt))
    oc, op = block(m + 16)
    opt_idx = [i16(op + 2 * j) for j in range(oc)]
    print(" +0x10 ShaderOptions count %d:" % oc, opt_idx)
    cats = read_rmdf(rmdf_ti)
    opts = {}
    for ci, oi in enumerate(opt_idx):
        if ci < len(cats) and 0 <= oi < len(cats[ci][1]):
            opts[cats[ci][0]] = cats[ci][1][oi]
            print("    %-22s -> %d = %s" % (cats[ci][0], oi, cats[ci][1][oi]))
        else:
            print("    category %d option index %d out of range" % (ci, oi))
    if verbose:
        for o in range(0x1C, 0x28, 4): print(" +0x%02X i32 %d (0x%08X)" % (o, i32(m + o), u32(m + o)))
    pc, pp = block(m + 40)
    print(" +0x28 ShaderProperties count %d file 0x%X" % (pc, pp))
    if verbose:
        for o in range(0x34, 0x40, 4): print(" +0x%02X i32 %d (0x%08X)" % (o, i32(m + o), u32(m + o)))
    for pi in range(pc):
        p = pp + 132 * pi
        tc, tt = tagref(p)
        print("  props[%d] +0x00 template:" % pi, tc, tagname(tt))
        mc, mp = block(p + 16); dc, dp = block(p + 28)
        print("  props[%d] +0x10 ShaderMaps count %d file 0x%X ; +0x1C TilingData count %d file 0x%X" % (pi, mc, mp, dc, dp))
        if verbose:
            print("  props[%d] rest of the 132-byte entry from +0x28:" % pi)
            for o in range(0x28, 132, 12):
                c, q = block(p + o)
                extra = ""
                if 0 < c < 500 and q > 0 and q < len(d): extra = "  <- TagBlock? count %d file 0x%X" % (c, q)
                print("    +0x%02X %s%s" % (o, d[p + o:p + o + 12].hex(" "), extra))
        argsl, usages = read_rmt2(tagidx(tt))
        print("  template arguments:", argsl)
        print("  template usages   :", usages)
        til = [(f32(dp + 16 * j), f32(dp + 16 * j + 4), f32(dp + 16 * j + 8), f32(dp + 16 * j + 12)) for j in range(dc)]
        print("  shader maps (28 bytes each):")
        for j in range(mc):
            e = mp + 28 * j
            bc, bt = tagref(e)
            raw = d[e + 16:e + 28].hex(" ")
            use = usages[j] if j < len(usages) else "<no usage>"
            print("    map %2d usage %-22s bitmap %-70s tail(+16..27) %s" % (j, use, tagname(bt), raw))
        print("  tiling data (float4):")
        for j, t in enumerate(til):
            print("    [%2d] %-26s %r" % (j, argsl[j] if j < len(argsl) else "<no argument>", tuple(round(x, 5) for x in t)))
        print("  RESOLVED usage -> bitmap, xform (scale x, scale y, offset x, offset y):")
        for j, use in enumerate(usages):
            bt = tagref(mp + 28 * j)[1] if j < mc else -1
            k = argsl.index(use) if use in argsl else -1
            xf = til[k] if 0 <= k < len(til) else (1.0, 1.0, 0.0, 0.0)
            tidx = u8(mp + 28 * j + 22) if j < mc else None
            print("    %-22s -> %-60s tilingArg=%2d tilingIndexByte=%s xform=%s" % (use, tagname(bt), k, tidx, tuple(round(x, 4) for x in xf)))
        print("  non-texture arguments:")
        for k, a in enumerate(argsl):
            if a not in usages and k < len(til): print("    %-26s %s" % (a, tuple(round(x, 4) for x in til[k])))
    return opts


# ---- survey of every render-method tag in the map
RM_CLASSES = ("rmsh", "rmtr", "rmfl", "rmw ", "rmd ", "rmhg", "rmcs", "rmct", "rmzo", "rmbk", "rmlv", "rmb ", "rmc ", "rmp ", "rmsk")
def shader_summary(ti):
    cls, name, m = tags[ti]
    rt = tagref(m)[1]; rmdf_ti = tagidx(rt)
    oc, op = block(m + 16)
    opt_idx = [i16(op + 2 * j) for j in range(oc)]
    cats = read_rmdf(rmdf_ti) if rmdf_ti >= 0 else []
    opts = {}
    for ci, oi in enumerate(opt_idx):
        if ci < len(cats) and 0 <= oi < len(cats[ci][1]): opts[cats[ci][0]] = cats[ci][1][oi]
    pc, pp = block(m + 40)
    maps = []
    tmpl = None
    if pc > 0:
        p = pp
        tt = tagref(p)[1]; tmpl = tagidx(tt)
        mc, mp = block(p + 16); dc, dp = block(p + 28)
        argsl, usages = read_rmt2(tmpl)
        for j, use in enumerate(usages):
            bt = tagref(mp + 28 * j)[1] if j < mc else -1
            k = argsl.index(use) if use in argsl else -1
            tb = u8(mp + 28 * j + 22) if j < mc else None
            xf = (f32(dp + 16 * k), f32(dp + 16 * k + 4), f32(dp + 16 * k + 8), f32(dp + 16 * k + 12)) if 0 <= k < dc else (1.0, 1.0, 0.0, 0.0)
            maps.append((use, tagidx(bt), k, tb, xf))
        return dict(cls=cls, name=name, rmdf=rmdf_ti, opts=opts, nopts=oc, ncats=len(cats), nprops=pc, tmpl=tmpl,
                    nmaps=mc, nusages=len(usages), ntiling=dc, nargs=len(argsl), maps=maps)
    return dict(cls=cls, name=name, rmdf=rmdf_ti, opts=opts, nopts=oc, ncats=len(cats), nprops=pc, tmpl=None, maps=maps)

def classify(cls, opts, name):
    blend = opts.get("blend_mode", "opaque"); at = opts.get("alpha_test", "none")
    c = cls.strip()
    if c == "rmw": cat = "water"
    elif c == "rmtr": cat = "terrain"
    elif c == "rmfl": cat = "foliage"
    elif c == "rmd":
        cat = {"multiply": "decal_multiply", "double_multiply": "decal_multiply", "opaque": "decal_opaque"}.get(blend,
              "decal_additive" if blend in ("additive", "add_src_times_dstalpha", "add_src_times_srcalpha", "multiply_add", "maximum") else "decal_blend")
    elif c in ("rmsh", "rmcs", "rmhg", "rmtd"):
        if blend in ("additive", "add_src_times_dstalpha", "add_src_times_srcalpha", "maximum", "multiply_add"): cat = "additive"
        elif blend in ("multiply", "double_multiply"): cat = "multiply"
        elif blend in ("alpha_blend", "pre_multiplied_alpha"): cat = "alpha_blend"
        else: cat = "alpha_test" if at != "none" else "opaque"
    else: cat = "special"
    import re as _re
    if _re.search(r"(^|\\)(invisible|sky_?portal|portal|z_only|zonly|black)(\\|_|$)", name.lower()): cat = "special"
    return cat

def survey():
    from collections import Counter
    rms = [i for i, t in enumerate(tags) if t[0] in RM_CLASSES]
    print("render-method tags:", len(rms), Counter(tags[i][0] for i in rms))
    nprops = Counter(); mism = []; tbad = []; blend = Counter(); atest = Counter(); mapcount = Counter(); optcount_ok = 0
    for i in rms:
        s = shader_summary(i)
        nprops[s["nprops"]] += 1
        if s["nopts"] == s["ncats"]: optcount_ok += 1
        else: print("  option count != category count:", s["cls"], s["name"], s["nopts"], s["ncats"])
        if s["tmpl"] is not None:
            if s["nmaps"] != s["nusages"] or s["ntiling"] != s["nargs"]:
                mism.append((s["cls"], s["name"], s["nmaps"], s["nusages"], s["ntiling"], s["nargs"]))
            for (use, bt, k, tb, xf) in s["maps"]:
                if (k if k >= 0 else 255) != tb: tbad.append((s["name"], use, k, tb))
                if bt < 0: mapcount["null bitmap"] += 1
                else: mapcount["bitmap"] += 1
        blend[(s["cls"], s["opts"].get("blend_mode"))] += 1
        atest[(s["cls"], s["opts"].get("alpha_test"))] += 1
    print("ShaderProperties counts:", dict(nprops))
    print("option count == category count for %d of %d" % (optcount_ok, len(rms)))
    print("ShaderMaps/Usages or TilingData/Arguments count mismatches:", len(mism))
    for x in mism[:20]: print("   ", x)
    print("tiling-index byte != arguments.index(usage):", len(tbad), tbad[:10])
    print("shader map bitmaps:", dict(mapcount))
    print("blend_mode by class:"); [print("   ", k, v) for k, v in sorted(blend.items(), key=str)]
    print("alpha_test by class:"); [print("   ", k, v) for k, v in sorted(atest.items(), key=str)]
    # the sbsp's shaders, classified
    scen = cstr(0xE0)
    scnr = [i for i, t in enumerate(tags) if t[0] == "scnr" and t[1] == scen][0]
    sbc, sbp = block(tags[scnr][2] + 24)
    sb = tagidx(tagref(sbp)[1]); m = tags[sb][2]
    shc, shp = block(m + 208)
    print("\nsbsp", tags[sb][1], "Shaders", shc)
    cats = Counter()
    rows = []
    for i in range(shc):
        tid = tagidx(tagref(shp + 36 * i)[1])
        if tid < 0: rows.append((i, None, None, None)); continue
        s = shader_summary(tid)
        cat = classify(s["cls"], s["opts"], s["name"])
        cats[cat] += 1
        base = [mp for mp in s["maps"] if mp[0] in ("base_map", "base_map_m_0", "watercolor_texture", "foam_texture")]
        rows.append((i, s["cls"], s["name"], cat, s["opts"].get("blend_mode"), s["opts"].get("alpha_test"), [(b[0], tags[b[1]][1].split("\\")[-1] if b[1] >= 0 else None, tuple(round(x, 3) for x in b[4])) for b in base]))
    for r in rows: print("  ", r)
    print("categories:", dict(cats))

def rmdf_detail():
    for ti, t in enumerate(tags):
        if t[0] != "rmdf": continue
        m = t[2]
        print("\n=== rmdf", t[1], "tag", ti)
        print("  +0x34 TagRef", tagref(m + 0x34), tagname(tagref(m + 0x34)[1]))
        print("  +0x44 TagRef", tagref(m + 0x44), tagname(tagref(m + 0x44)[1]))
        c2, p2 = block(m + 0x1C); c3, p3 = block(m + 0x28)
        print("  +0x1C block count", c2, "first 64 bytes:", d[p2:p2 + 64].hex(" ") if p2 > 0 else None)
        print("  +0x28 block count", c3, "first 64 bytes:", d[p3:p3 + 64].hex(" ") if p3 > 0 else None)
        cc, cp = block(m + 16)
        for ci in range(cc):
            e = cp + 24 * ci
            oc, op = block(e + 4)
            print("  cat %2d %-20s +16 i32 %d  +20 sid %s" % (ci, sidstr(i32(e)), i32(e + 16), sidstr(i32(e + 20))))
            for j in range(oc):
                o = op + 28 * j
                print("      opt %2d %-40s +4 %s  +20 i32 %d  +24 sid %s" % (j, sidstr(i32(o)), tagname(tagref(o + 4)[1]), i32(o + 20), sidstr(i32(o + 24))))
        if t[1] in (r"shaders\shader",): pass

def whois(idxs):
    for x in idxs:
        print("tag 0x%04X %d ->" % (x, x), tags[x][0], tags[x][1])

DEFAULT = [r"levels\multi\riverworld\shaders\riverworld_rock",
           r"levels\multi\riverworld\shaders\riverworld_ground",
           r"levels\multi\riverworld\shaders\riverworld_tree_leafa",
           r"levels\multi\riverworld\shaders\riverworld_water_rough",
           r"levels\multi\riverworld\shaders\forerunner\glass_wall"]
if "--survey" in args:
    survey(); sys.exit()
if "--rmdf-detail" in args:
    rmdf_detail(); whois([0x1C9F, 0x1CEC, 0x1CFD, 0x1C2A]); sys.exit()
if "--rmdf" in args:
    for i, t in enumerate(tags):
        if t[0] == "rmdf": dump_rmdf(i)
    dump_rmdf_layout([i for i, t in enumerate(tags) if t[0] == "rmdf" and t[1] == r"shaders\shader"][0])
    sys.exit()
if "--rmt2" in args:
    for n in DEFAULT:
        ti = find_name(n); m = tags[ti][2]; pc, pp = block(m + 40); tt = tagref(pp)[1]
        dump_rmt2(tagidx(tt))
    sys.exit()
names = [a for a in args if not a.startswith("--")] or DEFAULT
for n in names:
    ti = find_name(n)
    if ti is None: print("NOT FOUND", n); continue
    dump_shader(ti)
