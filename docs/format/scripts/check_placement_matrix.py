"""Read-only: rebuild glTF node matrices from raw .map bytes and compare with HaloExport's scene.gltf.

Usage:  python -I check_placement_matrix.py <riverworld.map> <halo-exports/riverworld/scene.gltf>

Verifies the matrix conventions documented in docs/haloexport-notes.md:
  * scenery/crate placement: xf = S(scale or 1) * R(quat from yaw/pitch/roll) * T(position), row-vector matrices
    quat = Qx(roll) * Q(-y)(pitch) * Qz(yaw) with the Hamilton product (System.Numerics operator *)
  * bsp instance: raw 4x3 matrix (3 basis rows + translation row) with M44 = 1, then
    S(TransformScale) * S(decomposed scale) * R(decomposed rotation) * T(translation)
  * glTF matrix G = C^-1 * M * C with C = diag-ish map (x, y, z) -> 3.048 * (x, z, -y), written row-major
Never writes anything.
"""
import json
import math
import struct
import sys

S = 3.048


def u32(b, o): return struct.unpack_from('<I', b, o)[0]
def i32(b, o): return struct.unpack_from('<i', b, o)[0]
def i16(b, o): return struct.unpack_from('<h', b, o)[0]
def i64(b, o): return struct.unpack_from('<q', b, o)[0]
def f32(b, o): return struct.unpack_from('<f', b, o)[0]


# ---- 4x4 row-vector matrix helpers (lists of 16, row-major, like System.Numerics field order)
def mat_mul(a, b):
    return [sum(a[r * 4 + k] * b[k * 4 + c] for k in range(4)) for r in range(4) for c in range(4)]


def mat_scale(s): return [s, 0, 0, 0, 0, s, 0, 0, 0, 0, s, 0, 0, 0, 0, 1]
def mat_trans(x, y, z): return [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, z, 1]


def quat_axis_angle(ax, ang):
    h = ang / 2
    s = math.sin(h)
    return (ax[0] * s, ax[1] * s, ax[2] * s, math.cos(h))     # (x, y, z, w)


def quat_mul(a, b):   # Hamilton product a*b (System.Numerics operator *)
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw * bx + bw * ax + (ay * bz - az * by),
            aw * by + bw * ay + (az * bx - ax * bz),
            aw * bz + bw * az + (ax * by - ay * bx),
            aw * bw - (ax * bx + ay * by + az * bz))


def mat_from_quat(q):  # System.Numerics Matrix4x4.CreateFromQuaternion (row-vector)
    x, y, z, w = q
    xx, yy, zz = x * x, y * y, z * z
    xy, wz, xz, wy, yz, wx = x * y, w * z, x * z, w * y, y * z, w * x
    return [1 - 2 * (yy + zz), 2 * (xy + wz), 2 * (xz - wy), 0,
            2 * (xy - wz), 1 - 2 * (xx + zz), 2 * (yz + wx), 0,
            2 * (xz + wy), 2 * (yz - wx), 1 - 2 * (xx + yy), 0,
            0, 0, 0, 1]


C = [S, 0, 0, 0, 0, 0, -S, 0, 0, S, 0, 0, 0, 0, 0, 1]
Cinv = [1 / S, 0, 0, 0, 0, 0, 1 / S, 0, 0, -1 / S, 0, 0, 0, 0, 0, 1]
assert max(abs(v - e) for v, e in zip(mat_mul(Cinv, C), [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1])) < 1e-12


def to_gltf(m): return mat_mul(mat_mul(Cinv, m), C)


def euler_to_quat(yaw, pitch, roll):
    return quat_mul(quat_mul(quat_axis_angle((1, 0, 0), roll), quat_axis_angle((0, -1, 0), pitch)), quat_axis_angle((0, 0, 1), yaw))


# ---- map reading
mp, gp = sys.argv[1], sys.argv[2]
data = open(mp, 'rb').read()
g = json.load(open(gp))
assert data[:4] == b'daeh' and i32(data, 4) == 13
sec_off = [u32(data, 0x4CC + 4 * i) for i in range(4)]
sec = [(u32(data, 0x4DC + 8 * i), u32(data, 0x4E0 + 8 * i)) for i in range(4)]
virtual_base = i64(data, 0x2E0)
tagdata_file_base = (sec[2][0] + sec_off[2]) & 0xFFFFFFFF


def hdr_ptr(p): return (p + sec_off[0]) & 0xFFFFFFFF
def virt(v): return v - virtual_base + tagdata_file_base
def shifted(p): return virt((p & 0xFFFFFFFF) << 2)


ti = virt(i64(data, 0x2E8))
cb = virt(i64(data, ti + 8))
tb = virt(i64(data, ti + 24))
tag_count = i32(data, 0x20)
classes = [data[cb + 16 * i: cb + 16 * i + 4][::-1].decode('latin1') for i in range(i32(data, ti))]
tags = [(i16(data, tb + 8 * i), u32(data, tb + 8 * i + 4)) for i in range(tag_count)]
name_tbl = hdr_ptr(u32(data, 0x24))
name_idx = hdr_ptr(u32(data, 0x2C))


def cstr(off): return data[off: data.index(b'\0', off)].decode('latin1')


def tag_name(i):
    off = i32(data, name_idx + 4 * i)
    return None if off < 0 else cstr(name_tbl + off)


def block(meta, off):
    cnt = i32(data, meta + off)
    return cnt, (shifted(u32(data, meta + off + 4)) if cnt else None)


def tagref(meta, off):
    tid = i32(data, meta + off + 12)
    return None if tid == -1 else tid & 0xFFFF


scenario_path = cstr(0xE0)
scnr = shifted(tags[[i for i, (ci, m) in enumerate(tags) if ci >= 0 and classes[ci] == 'scnr' and tag_name(i) == scenario_path][0]][1])


def placements(kind):
    off_pl, off_pal, size = {'scenery': (180, 192, 180), 'crate': (1468, 1480, 176), 'machine': (312, 324, 136)}[kind]
    n, p = block(scnr, off_pl)
    npal, pp = block(scnr, off_pal)
    pal = [tagref(pp + 16 * k, 0) for k in range(npal)]
    out = []
    for i in range(n):
        o = p + size * i
        out.append(dict(palette=i16(data, o), pos=(f32(data, o + 8), f32(data, o + 12), f32(data, o + 16)),
                        rot=(f32(data, o + 20), f32(data, o + 24), f32(data, o + 28)), scale=f32(data, o + 32),
                        tag=tag_name(pal[i16(data, o)]) if 0 <= i16(data, o) < npal and pal[i16(data, o)] is not None else None))
    return out


def node_matrix_from_placement(pl):
    sc = pl['scale'] if pl['scale'] != 0 else 1
    xf = mat_mul(mat_mul(mat_scale(sc), mat_from_quat(euler_to_quat(*pl['rot']))), mat_trans(*pl['pos']))
    return to_gltf(xf)


worst = 0
for kind in ('scenery', 'crate', 'machine'):
    pls = placements(kind)
    if not pls:
        continue
    # HaloExport groups placements by palette index (first-appearance order) and numbers nodes per group
    seq = {}
    by_key = {}
    for idx, pl in enumerate(pls):
        k = pl['palette']
        by_key[(k, seq.get(k, 0))] = idx
        seq[k] = seq.get(k, 0) + 1
    nodes = [n for n in g['nodes'] if n['extras']['halo']['kind'] == kind]
    checked = 0
    shown = 0
    for n in nodes:
        num = int(n['name'].rsplit('-', 1)[1])
        pl = pls[by_key[(n['extras']['halo']['palette_index'], num)]]
        exp = node_matrix_from_placement(pl)
        diff = max(abs(a - b) for a, b in zip(exp, n['matrix']))
        worst = max(worst, diff)
        checked += 1
        rotated = any(abs(r) > 1e-3 for r in pl['rot'])
        if shown < 2 and rotated:
            shown += 1
            print(f'{kind} node {n["name"]} <- raw placement palette {pl["palette"]} tag {pl["tag"]}')
            print(f'   raw pos {pl["pos"]} rot(yaw,pitch,roll rad) {pl["rot"]} scale {pl["scale"]}')
            print(f'   glTF matrix  {[round(v, 5) for v in n["matrix"]]}')
            print(f'   rebuilt      {[round(v, 5) for v in exp]}   max |diff| {diff:.2e}')
    print(f'{kind}: {checked} nodes compared, worst |diff| so far {worst:.2e}')

# bsp instances: match by section + translation, then compare the whole matrix
nb, bsp_tbl = block(scnr, 24)
sbsp = shifted(tags[tagref(bsp_tbl, 0)][1])
ni, ip = block(sbsp, 448)
raw = []
for i in range(ni):
    o = ip + 120 * i
    m = [f32(data, o + 4 + 4 * k) for k in range(12)]
    raw.append(dict(scale=f32(data, o), m=m, section=i16(data, o + 52)))


def instance_matrix(r):
    m = r['m']
    M = [m[0], m[1], m[2], 0, m[3], m[4], m[5], 0, m[6], m[7], m[8], 0, m[9], m[10], m[11], 1]
    # Reclaimer GetFinalTransform: decompose into scale / rotation / translation, then S(uniform) * S(scale) * R * T.
    # For a rotation-plus-translation matrix that equals S(uniform) * M, so compare against that.
    return to_gltf(mat_mul(mat_scale(r['scale']), M))


inst_nodes = [n for n in g['nodes'] if n['extras']['halo']['kind'] == 'bsp_instance']
worst_i = 0
shown = 0
matched = 0
for n in inst_nodes:
    M = n['matrix']
    t_halo = (M[12] / S, -M[14] / S, M[13] / S)
    cands = [r for r in raw if r['section'] == n['extras']['halo']['section'] and max(abs(a - b) for a, b in zip(t_halo, r['m'][9:12])) < 1e-3]
    if len(cands) != 1:
        continue
    r = cands[0]
    exp = instance_matrix(r)
    diff = max(abs(a - b) for a, b in zip(exp, M))
    worst_i = max(worst_i, diff)
    matched += 1
    if shown < 2 and abs(r['scale'] - 1) > 1e-3:
        shown += 1
        print(f'bsp_instance node {n["name"]} section {r["section"]} TransformScale {r["scale"]}')
        print(f'   raw 4x3 rows {[round(v, 5) for v in r["m"]]}')
        print(f'   glTF matrix  {[round(v, 5) for v in M]}')
        print(f'   rebuilt      {[round(v, 5) for v in exp]}   max |diff| {diff:.2e}')
print(f'bsp_instance: {matched} of {len(inst_nodes)} nodes matched by section+translation, worst |diff| {worst_i:.2e}')
