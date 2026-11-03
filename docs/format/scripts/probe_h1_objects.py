# Halo CE placed objects (docs/format/halo1.md 13 to 15): the BSPs each
# object stands in (checked against scenery's own mask at +0x20), each
# object tag's collision model (coll nodes, one BSP per permutation) put on
# its render model's nodes (and which way the node rotations go), and the
# devices' numbers (devi, mach, the placements' device flags).
# python -I probe_h1_objects.py [map]   (default d40)
import math, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "d40")
sc = m.data(m.scenario_id)
names = [m.cstr(e) for e in m.block(sc + 0x204, 0x24)]

# --- 13. Which BSPs an object stands in: the collision BSP's 3D tree. ---
trees = []
for b in m.bsps():
    cb = b.block(b.sbsp + 0xB0, 0x60)
    if not cb:
        trees.append(([], []))
        continue
    c = cb[0]
    trees.append(([struct.unpack_from("<iii", m.d, o) for o in b.block(c, 12)],
                  [struct.unpack_from("<4f", m.d, o) for o in b.block(c + 12, 16)]))


def inside(tree, p):
    nodes, planes = tree
    if not nodes:
        return True
    n = 0
    for _ in range(len(nodes) + 1):
        plane, back, front = nodes[n]
        a, b, c, d = planes[plane]
        child = front if a * p[0] + b * p[1] + c * p[2] - d >= 0 else back
        if child == -1:
            return False
        if child < 0:
            return True
        n = child
    return False


def mask(p):
    return sum(1 << k for k, t in enumerate(trees) if inside(t, p))


lists = [("scenery", 0x210, 0x48, 0x21C), ("machines", 0x294, 0x40, 0x2A0), ("controls", 0x2AC, 0x40, 0x2B8)]
agree = total = 0
for kind, at, size, pal in lists:
    for e in m.block(sc + at, size):
        if kind == "scenery":
            total += 1
            agree += m.u16(e + 0x20) == mask(m.vec(e + 8))
print(f"scenery BSP mask (+0x20) equals the point test on {agree} of {total}")

# --- 14. Collision models on the render model's nodes. ---
def qrot(q, v):
    x, y, z, w = q
    ix = w * v[0] + y * v[2] - z * v[1]
    iy = w * v[1] + z * v[0] - x * v[2]
    iz = w * v[2] + x * v[1] - y * v[0]
    iw = -x * v[0] - y * v[1] - z * v[2]
    return (ix * w - iw * x - iy * z + iz * y, iy * w - iw * y - iz * x + ix * z, iz * w - iw * z - ix * y + iy * x)


def qmul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz)


def node_frames(mod, conjugate):
    nodes = m.block(mod + 0xB8, 0x9C)
    out = []
    for n in nodes:
        parent, t, q = m.i16(n + 0x24), m.vec(n + 0x28), m.vec(n + 0x34, 4)
        if conjugate:
            q = (-q[0], -q[1], -q[2], q[3])
        if 0 <= parent < len(out):
            pt, pq = out[parent][1]
            r = qrot(pq, t)
            out.append((m.cstr(n), ((pt[0] + r[0], pt[1] + r[1], pt[2] + r[2]), qmul(pq, q))))
        else:
            out.append((m.cstr(n), (t, q)))
    return out


def render_points(mod):
    pts = []
    for g in m.block(mod + 0xD0, 0x30):
        for part in m.block(g + 0x24, 0x84):
            n, off = m.u32(part + 0x58), m.u32(part + 0x64)
            pts += [m.vec(m.model_data + off + 68 * k) for k in range(0, n, max(1, n // 200))]
    return pts


seen = set()
for cls in ("scen", "mach", "ctrl"):
    for t in m.find(cls):
        o = m.data(t["id"])
        mod, coll = m.ref(o + 0x28), m.ref(o + 0x70)
        if not m.tag(mod) or not m.tag(coll) or coll in seen:
            continue
        seen.add(coll)
        mo, co = m.data(mod), m.data(coll)
        regions = [(m.cstr(r), [m.cstr(p) for p in m.block(r + 0x48, 0x40)]) for r in m.block(co + 0x240, 0x54)]
        nodes = m.block(co + 0x28C, 0x40)
        rp = render_points(mo)
        err = []
        for conj in (False, True):
            frames = dict(node_frames(mo, conj))
            s = c = 0
            for cn in nodes:
                bs = m.block(cn + 0x34, 0x60)
                if not bs or m.cstr(cn) not in frames or not rp:
                    continue
                tt, q = frames[m.cstr(cn)]
                for v in m.block(bs[0] + 0x54, 16)[::3]:
                    p = qrot(q, m.vec(v))
                    p = (p[0] + tt[0], p[1] + tt[1], p[2] + tt[2])
                    s += min(math.dist(p, r) for r in rp)
                    c += 1
            err.append(round(s / max(c, 1), 3))
        print(f"{t['name'].split(chr(92))[-1]}: regions {regions}, nodes",
              [(m.cstr(n), m.i16(n + 0x20), m.u32(n + 0x34)) for n in nodes],
              f"mean distance to the drawn model: rotations as stored {err[0]}, conjugated {err[1]}")

# --- 15. Devices. ---
print("machine tags: type (+0x290: 0 door, 1 platform, 2 gear), flags (+0x292), door open time (+0x294),",
      "devi power / position transition times (+0x180, +0x188)")
for t in m.find("mach"):
    o = m.data(t["id"])
    print("   ", t["name"].split(chr(92))[-1], m.u16(o + 0x290), hex(m.u16(o + 0x292)), round(m.f32(o + 0x294), 2),
          round(m.f32(o + 0x180), 2), round(m.f32(o + 0x188), 2))
for kind, at, size, pal in lists[1:]:
    palette = [m.ref(e) for e in m.block(sc + pal, 0x30)]
    for i, e in enumerate(m.block(sc + at, size)):
        p = m.vec(e + 8)
        bs = mask(p)
        if not bs & 0xE0 and len(sys.argv) < 3:
            continue
        ni = m.i16(e + 2)
        print(kind, i, names[ni] if ni >= 0 else "-", m.tag(palette[m.i16(e)])["name"].split(chr(92))[-1],
              "not placed", hex(m.u16(e + 4)), "at", [round(x, 1) for x in p], "BSPs", hex(bs),
              "power/position group", m.i16(e + 0x28), m.i16(e + 0x2A), "device flags", hex(m.u32(e + 0x2C)),
              "machine flags", hex(m.u32(e + 0x30)))
