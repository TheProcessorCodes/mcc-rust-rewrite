"""Fill the <!-- GEN:... --> markers in docs/format/reference-numbers.md from the HaloExport sidecars,
the glTF files and the raw .map bytes (matrix test vectors).

Usage:  python -I gen_reference_tables.py <reference-numbers.md> <maps dir> <halo-exports dir>

Reads <maps dir>\riverworld.map and cyberdyne.map read-only; writes only the markdown file given.
"""
import json
import math
import struct
import sys

S = 3.048
MD, MAPS, EXPORTS = sys.argv[1], sys.argv[2], sys.argv[3]


def u32(b, o): return struct.unpack_from('<I', b, o)[0]
def i32(b, o): return struct.unpack_from('<i', b, o)[0]
def i16(b, o): return struct.unpack_from('<h', b, o)[0]
def i64(b, o): return struct.unpack_from('<q', b, o)[0]
def f32(b, o): return struct.unpack_from('<f', b, o)[0]


def esc(s): return str(s).replace('|', '\\|')


def materials_table(side):
    rows = ['| name | class | category | technique_set | triangles | tag | base colour |', '|---|---|---|---|---|---|---|']
    for m in side['materials']:
        rows.append(f"| `{esc(m['name'])}` | {m['class']} | {m['category']} | `{m['technique_set']}` | {m['triangles']:,} | `{esc(m['tag'])}` | {'yes' if m['base_color'] else 'no'} |")
    return '\n'.join(rows)


def world_prims(name, g):
    world = [n for n in g['nodes'] if n.get('name') == 'World'][0]
    rows = [f'**{name}** (mesh {world["mesh"]}, {len(g["meshes"][world["mesh"]]["primitives"])} primitives):', '',
            '| # | material | class | category | triangles | vertices |', '|---|---|---|---|---|---|']
    for k, p in enumerate(g['meshes'][world['mesh']]['primitives']):
        mat = g['materials'][p['material']]
        h = mat['extras']['halo']
        rows.append(f"| {k} | `{esc(mat['name'])}` | {h['class']} | {h['category']} | {g['accessors'][p['indices']]['count'] // 3:,} | {g['accessors'][p['attributes']['POSITION']]['count']:,} |")
    return '\n'.join(rows)


class Map:
    def __init__(self, path):
        d = self.d = open(path, 'rb').read()
        assert d[:4] == b'daeh' and i32(d, 4) == 13
        self.sec_off = [u32(d, 0x4CC + 4 * i) for i in range(4)]
        sec = [(u32(d, 0x4DC + 8 * i), u32(d, 0x4E0 + 8 * i)) for i in range(4)]
        self.vbase = i64(d, 0x2E0)
        self.tbase = (sec[2][0] + self.sec_off[2]) & 0xFFFFFFFF
        ti = self.virt(i64(d, 0x2E8))
        cb = self.virt(i64(d, ti + 8))
        tb = self.virt(i64(d, ti + 24))
        n = i32(d, 0x20)
        self.classes = [d[cb + 16 * i: cb + 16 * i + 4][::-1].decode('latin1') for i in range(i32(d, ti))]
        self.tags = [(i16(d, tb + 8 * i), u32(d, tb + 8 * i + 4)) for i in range(n)]
        self.name_tbl = (u32(d, 0x24) + self.sec_off[0]) & 0xFFFFFFFF
        self.name_idx = (u32(d, 0x2C) + self.sec_off[0]) & 0xFFFFFFFF
        scen = self.cstr(0xE0)
        self.scnr = self.meta([i for i, (ci, m) in enumerate(self.tags) if ci >= 0 and self.classes[ci] == 'scnr' and self.tag_name(i) == scen][0])

    def virt(self, v): return v - self.vbase + self.tbase
    def shifted(self, p): return self.virt((p & 0xFFFFFFFF) << 2)
    def cstr(self, off): return self.d[off: self.d.index(b'\0', off)].decode('latin1')

    def tag_name(self, i):
        off = i32(self.d, self.name_idx + 4 * i)
        return None if off < 0 else self.cstr(self.name_tbl + off)

    def meta(self, i): return self.shifted(self.tags[i][1])

    def block(self, meta, off):
        cnt = i32(self.d, meta + off)
        return cnt, (self.shifted(u32(self.d, meta + off + 4)) if cnt else None)

    def tagref(self, meta, off):
        tid = i32(self.d, meta + off + 12)
        return None if tid == -1 else tid & 0xFFFF

    def placements(self, kind):
        off_pl, off_pal, size = {'scenery': (180, 192, 180), 'crate': (1468, 1480, 176), 'machine': (312, 324, 136)}[kind]
        n, p = self.block(self.scnr, off_pl)
        out = []
        for i in range(n):
            o = p + size * i
            out.append(dict(index=i, palette=i16(self.d, o), pos=tuple(f32(self.d, o + 8 + 4 * k) for k in range(3)),
                            rot=tuple(f32(self.d, o + 20 + 4 * k) for k in range(3)), scale=f32(self.d, o + 32)))
        return out

    def instances(self):
        nb, bsp_tbl = self.block(self.scnr, 24)
        sbsp = self.meta(self.tagref(bsp_tbl, 0))
        ni, ip = self.block(sbsp, 448)
        out = []
        for i in range(ni):
            o = ip + 120 * i
            out.append(dict(index=i, scale=f32(self.d, o), m=[f32(self.d, o + 4 + 4 * k) for k in range(12)], section=i16(self.d, o + 52)))
        return out


def fmt(v): return repr(float(f'{v:.9g}'))


def test_vectors(name, mp, g, wanted):
    out = []
    by_kind = {}
    for kind in ('scenery', 'crate', 'machine'):
        pls = mp.placements(kind)
        seq, by_key = {}, {}
        for pl in pls:
            k = pl['palette']
            by_key[(k, seq.get(k, 0))] = pl
            seq[k] = seq.get(k, 0) + 1
        by_kind[kind] = by_key
    inst = mp.instances()
    for node_name in wanted:
        n = [x for x in g['nodes'] if x['name'] == node_name][0]
        kind = n['extras']['halo']['kind']
        M = n['matrix']
        out.append(f'**{name} `{node_name}`** (kind {kind}, mesh {n["mesh"]})')
        out.append('')
        if kind == 'bsp_instance':
            t = (M[12] / S, -M[14] / S, M[13] / S)
            r = [r for r in inst if r['section'] == n['extras']['halo']['section'] and max(abs(a - b) for a, b in zip(t, r['m'][9:12])) < 1e-3][0]
            out.append(f'* raw GeometryInstances[{r["index"]}]: TransformScale `{fmt(r["scale"])}`, SectionIndex {r["section"]}, '
                       f'instance_name `{n["extras"]["halo"]["instance_name"]}`')
            out.append(f'* raw 4x3 rows: `{[float(fmt(v)) for v in r["m"][0:3]]}`, `{[float(fmt(v)) for v in r["m"][3:6]]}`, '
                       f'`{[float(fmt(v)) for v in r["m"][6:9]]}`, translation `{[float(fmt(v)) for v in r["m"][9:12]]}`')
        else:
            pl = by_kind[kind][(n['extras']['halo']['palette_index'], int(node_name.rsplit('-', 1)[1]))]
            out.append(f'* raw placement [{pl["index"]}] (palette {pl["palette"]}, tag `{esc(n["extras"]["halo"]["tag"])}`): '
                       f'Position `{[float(fmt(v)) for v in pl["pos"]]}`, Rotation yaw/pitch/roll `{[float(fmt(v)) for v in pl["rot"]]}`, Scale `{fmt(pl["scale"])}`')
        out.append(f'* glTF `matrix`: `{[float(fmt(v)) for v in M]}`')
        out.append('')
    return '\n'.join(out)


text = open(MD, encoding='utf-8').read()
side = {m: json.load(open(f'{EXPORTS}/{m}/halo_export.json')) for m in ('riverworld', 'cyberdyne')}
gltf = {m: json.load(open(f'{EXPORTS}/{m}/scene.gltf')) for m in ('riverworld', 'cyberdyne')}
maps = {m: Map(f'{MAPS}/{m}.map') for m in ('riverworld', 'cyberdyne')}

text = text.replace('<!-- GEN:materials_riverworld -->', materials_table(side['riverworld']))
text = text.replace('<!-- GEN:materials_cyberdyne -->', materials_table(side['cyberdyne']))
text = text.replace('<!-- GEN:world_prims -->', world_prims('riverworld', gltf['riverworld']) + '\n\n' + world_prims('cyberdyne', gltf['cyberdyne']))
tv = test_vectors('riverworld', maps['riverworld'], gltf['riverworld'],
                  ['respawn_point#scenery-0000', 'man_cannon_river#crate-0001', 'koth_hill_static#crate-0001',
                   'backet_bottom#bsp0-00000', 'rock#bsp0-00092', 'rock#bsp0-00093'])
tv += '\n' + test_vectors('cyberdyne', maps['cyberdyne'], gltf['cyberdyne'],
                          ['jump_fan#machine-0000', 'respawn_point#scenery-0000', 'lightfix_i_blue#bsp0-00461'])
text = text.replace('<!-- GEN:test_vectors -->', tv)
assert '<!-- GEN:' not in text, 'marker left unfilled'
open(MD, 'w', encoding='utf-8', newline='\n').write(text)
print('filled', MD, len(text), 'chars')
