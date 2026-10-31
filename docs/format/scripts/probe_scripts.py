# Read-only: decompiles the HaloScript in an MCC Halo 3 (U13) scenario to
# Lisp-like text, straight from the bytes (docs/format/halo-script.md).
# Run:  python -I probe_scripts.py <map> [substring]      (default 120_halo)
#       python -I probe_scripts.py <map> --stats          node kinds, types, functions
import os, struct, sys, collections
sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "120_halo")
arg = sys.argv[2] if len(sys.argv) > 2 else None
s = m.meta(m.find("scnr"))
# scnr +0x3D8: data reference (size, 8 bytes, pointer, 4 bytes) to the string data.
size, _, _, ptr = struct.unpack_from("<IIII", m.m, s + 0x3D8)
SD, SLEN = m.ptr32(ptr), size
# scnr +0x49C: syntax datums, 24 bytes each.
NN, NA = m.block(s + 0x49C)


def st(o):
    return m.cstr(SD + o, 512) if o < SLEN else f"<str {o:#x}>"


def node(d):
    o = NA + 24 * (d & 0xFFFF)
    salt, op, typ, fl, nxt, so, val, line = struct.unpack_from("<HHHHIIIH", m.m, o)
    assert salt == d >> 16, (hex(d), hex(salt))   # full datums carry the salt
    return op, typ, fl, nxt, so, val


# scnr +0x3EC scripts (52): name[32], i16 type, u16 return type, u32 root datum,
# parameters block (36: name[32], u16 type)
scripts = []
for o in m.elements(s + 0x3EC, 52):
    params = [(m.cstr(p, 32), m.i16(p + 32)) for p in m.elements(o + 0x28, 36)]
    scripts.append((m.cstr(o, 32), m.i16(o + 32), m.i16(o + 34), m.u32(o + 36), params))
# scnr +0x3F8 globals (40): name[32], u16 type, 2 bytes, u32 init datum
globs = [(m.cstr(o, 32), m.i16(o + 32), m.u32(o + 36)) for o in m.elements(s + 0x3F8, 40)]
TYPES = ["startup", "dormant", "continuous", "static", "command_script", "stub"]


def expr(d, params=()):
    op, typ, fl, nxt, so, val = node(d)
    if fl & 1 == 0:                       # a call: function (bit 1 clear) or script (bit 1 set)
        kids, c = [], val
        while c != 0xFFFFFFFF:
            kids.append(c)
            c = node(c)[3]
        name = st(node(kids[0])[4]) if fl & 2 == 0 else scripts[op][0]
        return "(" + " ".join([name] + [expr(k, params) for k in kids[1:]]) + ")"
    if fl & 0x10:                         # parameter reference: value = index
        return params[val][0] if val < len(params) else st(so)
    if fl & 4:                            # global reference: value = index, bit 15 = engine global
        return globs[val & 0x7FFF][0] if val & 0x8000 == 0 else st(so)
    if typ == 5:
        return "true" if val & 0xFF else "false"
    if typ == 6:
        return repr(struct.unpack("<f", struct.pack("<I", val))[0])
    if typ == 7:
        return str(struct.unpack("<h", struct.pack("<H", val & 0xFFFF))[0])
    if typ == 8:
        return str(struct.unpack("<i", struct.pack("<I", val))[0])
    if typ == 9:
        return '"' + st(so) + '"'
    if typ == 4:
        return "none"
    return st(so)


if arg == "--stats":
    kinds, types, funcs = collections.Counter(), collections.Counter(), collections.Counter()
    for i in range(NN):
        salt, op, typ, fl, nxt, so, val = struct.unpack_from("<HHHHIII", m.m, NA + 24 * i)
        if salt == 0:
            kinds["free"] += 1
            continue
        kinds[fl] += 1
        if fl == 9:
            types[typ] += 1
        if fl == 8:
            funcs[(op, st(node(val)[4]))] += 1
    print(len(scripts), "scripts", len(globs), "globals", NN, "slots", SLEN, "string bytes")
    print("node flags:", dict(kinds))
    print("constant types:", sorted(types.items()))
    for (op, name), n in sorted(funcs.items()):
        print(f"{op:5} {name:50} {n}")
    sys.exit()

for name, t, ret, root, params in scripts:
    src = f"(script {TYPES[t] if 0 <= t < 6 else t} {ret} {name}" \
        + "".join(f" ({p} {pt})" for p, pt in params) + "\n  " + expr(root, params) + ")"
    if not arg or arg in src:
        print(src)
