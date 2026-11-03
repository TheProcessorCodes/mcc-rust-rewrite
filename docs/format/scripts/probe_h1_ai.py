# Halo CE's AI (docs/format/halo1.md 14, 15): the scenario's encounters,
# squads, platoons, starting and firing positions; the actor variant palette
# (actv -> actr, bipd, weap) with the firing numbers; the units' size, team
# and vitality (bipd, coll); weapons' triggers, projectiles and damage
# (weap, proj, jpt!); a model's nodes and markers (mod2); an animation
# graph's animations and how their frames are laid out (antr).
# python -I probe_h1_ai.py [map] [encounter prefix]   (HALO1_MAPS = maps folder)
import math, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "d40")
prefix = sys.argv[2] if len(sys.argv) > 2 else "enc6_1"
BS = chr(92)
sc = m.data(m.scenario_id)


def tag_at(o, cls=None):
    tid = m.ref(o)
    t = m.tag(tid) if (tid & 0xFFFF) < len(m.tags) else None
    return t if t and (cls is None or t["cls"] == cls) else None


# Scenario +0x420: the actor variant palette, 0x10 each.
palette = [tag_at(e, "actv") for e in m.block(sc + 0x420, 0x10)]
print("actor variants:", len(palette))
for i, t in enumerate(palette):
    if not t:
        continue
    v = m.data(t["id"])
    actr, unit, weap = tag_at(v + 0x4, "actr"), tag_at(v + 0x14), tag_at(v + 0x64, "weap")
    a = m.data(actr["id"]) if actr else None
    print(f"  {i:2} {t['name']}: type {m.u16(a + 0x14) if a else '?'} unit {unit and unit['name']} weapon {weap and weap['name']}")
    print(f"      vision {m.f32(a + 0x18):.0f} wu, hearing {m.f32(a + 0x4C):.0f} wu; fires to {m.f32(v + 0x74):.0f} wu at {m.f32(v + 0x78):.1}/s,"
          f" error {m.f32(v + 0x7C):.3f}, combat range {m.f32(v + 0x9C):.1f}..{m.f32(v + 0xA0):.1f},"
          f" bursts {m.f32(v + 0xE0):.2f}..{m.f32(v + 0xE4):.2f} s every {m.f32(v + 0xE8):.2f}..{m.f32(v + 0xEC):.2f} s;"
          f" body {m.f32(v + 0x200):.2f} shield {m.f32(v + 0x204):.1f}")

# Scenario +0x42C: encounters, 0xB0 each.
for e in m.block(sc + 0x42C, 0xB0):
    name = m.cstr(e)
    if not name.startswith(prefix):
        continue
    flags = m.u32(e + 0x20)
    bsp = m.u16(e + 0x2A) if flags & 0x40 else m.u16(e + 0x7E)
    squads = m.block(e + 0x80, 0xE8)
    platoons = [m.cstr(p) for p in m.block(e + 0x8C, 0xAC)]
    fps = m.block(e + 0x98, 0x18)
    print(f"{name}: flags {flags:#x} team {m.u16(e + 0x24)} bsp {bsp}, {len(squads)} squads, platoons {platoons}, {len(fps)} firing positions")
    for s in squads:
        locs = m.block(s + 0xD0, 0x1C)
        print(f"  {m.cstr(s)}: actor {m.i16(s + 0x20)} platoon {m.i16(s + 0x22)} counts {m.i16(s + 0x7C)}/{m.i16(s + 0x7E)}, {len(locs)} starts")
        for l in locs[:3]:
            print(f"     at {[round(x, 2) for x in m.vec(l)]} facing {m.f32(l + 0xC):.2f} cluster {m.i16(l + 0x10)}"
                  f" actor {m.i16(l + 0x18)} command list {m.i16(l + 0x1A)}")

# A unit, its collision model, its weapon.
unit = m.find("bipd", "characters" + BS + "floodcombat elite" + BS + "floodcombat elite")[0]
u = m.data(unit["id"])
coll = m.data(m.ref(u + 0x70))
print(f"{unit['name']}: team {m.u16(u + 0x180)} model {m.refname(u + 0x28)} graph {m.refname(u + 0x38)}"
      f" melee {m.refname(u + 0x288)}; camera {m.f32(u + 0x400):.2f} height {m.f32(u + 0x424):.2f} radius {m.f32(u + 0x42C):.2f};"
      f" body {m.f32(coll + 0x8):.0f} shield {m.f32(coll + 0xCC):.0f}")
weap = m.find("weap", "weapons" + BS + "assault rifle" + BS + "assault rifle")[0]
w = m.data(weap["id"])
for t in m.block(w + 0x4FC, 0x114):
    p = m.data(m.ref(t + 0x94))
    d = m.data(m.ref(p + 0x224))
    print(f"{weap['name']}: {m.f32(t + 4):.1f}..{m.f32(t + 8):.1f} rounds/s, {m.i16(t + 0x6E)} per shot,"
          f" error {m.f32(t + 0x7C):.3f}..{m.f32(t + 0x80):.3f}; {m.refname(t + 0x94)} at {m.f32(p + 0x1E4) * 30:.0f} wu/s,"
          f" damage {m.f32(d + 0x1D0):.0f} {m.f32(d + 0x1D4):.0f}..{m.f32(d + 0x1D8):.0f}")

# A model's nodes (rotations stored conjugated) and markers.
mode = m.data(m.ref(u + 0x28))
nodes = m.block(mode + 0xB8, 0x9C)


def conj(q):
    return (-q[0], -q[1], -q[2], q[3])


def qmul(a, b):
    ax, ay, az, aw = a; bx, by, bz, bw = b
    return (aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz)


def rotate(q, v):
    r = qmul(qmul(q, (v[0], v[1], v[2], 0.0)), conj(q))
    return r[:3]


world = []
for k, n in enumerate(nodes):
    parent = m.i16(n + 0x24)
    q = conj(m.vec(n + 0x34, 4))
    t = m.vec(n + 0x28)
    if parent < 0:
        world.append((q, t))
    else:
        pq, pt = world[parent]
        world.append((qmul(pq, q), tuple(a + b for a, b in zip(pt, rotate(pq, t)))))
for k in range(len(nodes)):
    name = m.cstr(nodes[k])
    if name in ("bip01 pelvis", "bip01 head", "bip01 l foot", "bip01 r foot", "bip01 r hand"):
        print(f"  node {name}: model space {[round(x, 3) for x in world[k][1]]}")
for mk in m.block(mode + 0xAC, 0x40):
    for i in m.block(mk + 0x34, 0x20)[:1]:
        if m.cstr(mk) in ("right hand", "head"):
            print(f"  marker {m.cstr(mk)} on node {m.u8(i + 2)} at {[round(x, 3) for x in m.vec(i + 4)]}")

# The animation graph: per animation, frame size = 8 per rotated node + 12
# per translated + 4 per scaled; frames interleave them node by node.
g = m.data(m.ref(u + 0x38))
anims = m.block(g + 0x74, 0xB4)
print(f"graph: {m.u32(g + 0x68)} nodes, {len(anims)} animations")
for a in anims[:8]:
    n = m.u16(a + 0x2C)
    bits = lambda o: m.u32(o) | m.u32(o + 4) << 32
    rot, tr, scl = bits(a + 0x6C), bits(a + 0x5C), bits(a + 0x7C)
    size = sum(8 * (rot >> k & 1) + 12 * (tr >> k & 1) + 4 * (scl >> k & 1) for k in range(n))
    info = m.u16(a + 0x26)
    first = m.addr(m.u32(a + 0xAC))
    q = [x / 32767 for x in struct.unpack_from("<4h", m.d, first)] if rot & 1 else None
    rq = f"; node 0 rotation |q| {math.sqrt(sum(x * x for x in q)):.3f}" if q else ""
    print(f"  {m.cstr(a)}: type {m.u16(a + 0x20)}, {m.u16(a + 0x22)} frames of {m.u16(a + 0x24)} bytes (flags say {size}),"
          f" frame info {info} ({m.u32(a + 0x48)} bytes), compressed at {m.u32(a + 0x88)},"
          f" default {m.u32(a + 0x8C)} bytes, frames {m.u32(a + 0xA0)} bytes" + rq)
