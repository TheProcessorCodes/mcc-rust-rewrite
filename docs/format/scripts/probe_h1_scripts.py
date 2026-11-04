# Halo CE scenario scripts (docs/format/halo1.md 12): the script node
# table, string data, scripts and globals, decompiled to Lisp-like source.
# python -I probe_h1_scripts.py [map] [--grep TEXT] [--header]
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map

args = [a for a in sys.argv[1:]]
grep = None
if "--grep" in args:
    i = args.index("--grep")
    grep = args[i + 1]
    del args[i:i + 2]
header = "--header" in args
args = [a for a in args if a != "--header"]
m = Map(args[0] if args else "d40")
sc = m.data(m.scenario_id)


def data_field(o):
    """A 20-byte tag data field: size, flags, file offset, address, (unused)."""
    return m.u32(o), m.addr(m.u32(o + 0xC))


syn_size, syn = data_field(sc + 0x474)
str_size, strs = data_field(sc + 0x488)
strings = m.d[strs:strs + str_size]
sig = m.d.index(b"@t@d", syn)
hdr = syn
name = m.cstr(hdr)
maximum, esize = m.u16(hdr + 0x20), m.u16(hdr + 0x22)
count = m.u16(sig + 6)  # last index in use (next / last after the signature)
first = hdr + 0x38
if header:
    print("syntax data", hex(syn_size), "name", name, "max", maximum, "element", esize, "sig at", hex(sig - hdr),
          "u16s after sig", struct.unpack_from("<4H", m.d, sig + 4), "first element at", hex(first - hdr))
    print("max * size + 0x38 =", hex(maximum * esize + 0x38))


def node(i):
    o = first + esize * i
    salt, op, ty, flags, nxt, so, val = struct.unpack_from("<HHHHIII", m.d, o)
    return dict(salt=salt, op=op, ty=ty, flags=flags, next=nxt, string=so, value=val)


def s(off):
    e = strings.index(b"\0", off)
    return strings[off:e].decode("latin1")


scripts = []
for e in m.block(sc + 0x49C, 0x5C):
    scripts.append((m.cstr(e), m.u16(e + 0x20), m.u16(e + 0x22), m.u32(e + 0x24)))
globs = []
for e in m.block(sc + 0x4A8, 0x5C):
    globs.append((m.cstr(e), m.u16(e + 0x20), m.u32(e + 0x28)))

KINDS = ["startup", "dormant", "continuous", "static", "stub"]


def expr(i, depth=0):
    if i == 0xFFFFFFFF:
        return "<none>"
    n = node(i & 0xFFFF)
    if n["flags"] & 1 == 0:  # a call: value is the first child, the name node
        parts = []
        c = n["value"]
        while c != 0xFFFFFFFF:
            parts.append(expr(c, depth + 1))
            c = node(c & 0xFFFF)["next"]
        return "(" + " ".join(parts) + ")"
    ty = n["ty"]
    if n["flags"] & 4:
        g = n["value"] & 0xFFFF
        if n["value"] & 0x8000:
            return f"<engine global {n['value'] & 0x7FFF}>"
        return globs[g][0] if g < len(globs) else f"<global {g}>"
    if ty == 2:  # function name
        return s(n["string"])
    if ty == 5:
        return "true" if n["value"] & 0xFF else "false"
    if ty == 6:
        return repr(round(struct.unpack("<f", struct.pack("<I", n["value"]))[0], 4))
    if ty == 7:
        return str(struct.unpack("<h", struct.pack("<H", n["value"] & 0xFFFF))[0])
    if ty == 8:
        return str(struct.unpack("<i", struct.pack("<I", n["value"]))[0])
    if ty == 10:
        return s(n["string"])
    text = s(n["string"]) if n["string"] < len(strings) else "?"
    if ty == 9:
        return '"' + text + '"'
    return text if text else f"<t{ty} {n['value']:#x}>"


out = []
for g in globs:
    out.append(f"(global t{g[1]} {g[0]} {expr(g[2])})")
for nm, kind, rt, root in scripts:
    body = expr(root)
    k = KINDS[kind] if kind < len(KINDS) else f"type{kind}"
    out.append(f"(script {k} t{rt} {nm}\n  {body})")
for line in out:
    if grep is None or grep in line:
        print(line)
