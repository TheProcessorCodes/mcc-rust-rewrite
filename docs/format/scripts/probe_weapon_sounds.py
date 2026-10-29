# Read-only probe of the sounds a Halo 3 weapon makes (docs/format/weapon-sounds.md):
# firing and empty-click effects of its barrels, the reload and ready sounds of its
# first-person animation graph, the charging loop it carries, and what its projectile
# sounds like when it lands (detonation and impact effects, material effects); plus
# the sound classes' default distances and voice limits.
# Run: python -I probe_weapon_sounds.py [map name or path] [--all] [--classes]
# Never writes anything.
import os
import sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map

args = [a for a in sys.argv[1:] if not a.startswith("--")]
m = Map(args[0] if args else "riverworld")
SHOW_ALL = "--all" in sys.argv

UGH = m.meta(m.find("ugh!"))
PLAYBACKS = m.block(UGH + 0x10)[1]
SNCL = m.find("sncl")
CLASSES = m.elements(m.meta(SNCL), 0x98) if SNCL is not None else []

# The surface an impact is heard on (the messages carry no material; Valhalla is
# mostly grass), then its parents by name, then the default.
MATERIAL_CHAIN = ["tough_terrain_grass", "tough_terrain", "tough", "default_material"]
IMPACT_TYPE = 15  # material effects element index for a projectile landing (inferred)


def short(n): return n.rsplit("\\", 1)[-1]


def snd(t):
    """snd! -> (class, flags, four distances, gain dB, longest ms)."""
    b = m.meta(t)
    p = PLAYBACKS + 68 * m.i16(b + 0x0C)
    return dict(cls=m.u8(b + 2), flags=m.u32(p), dist=tuple(round(x, 2) for x in m.f32n(p + 4, 4)),
                gain=round(m.f32(p + 0x1C), 2), ms=m.i32(b + 0x14))


def fmt(t):
    if t is None:
        return "-"
    if m.cls(t) != "snd!":
        return f"{m.cls(t)} {m.names[t]}"
    s = snd(t)
    return f"{m.names[t]}  [class {s['cls']} flags {s['flags']:#x} dist {s['dist']} gain {s['gain']} {s['ms']} ms]"


def effect_sounds(t):
    """effe +0x2C events (68 bytes), event +0x20 parts (96 bytes), part +0x10 tag ref."""
    out = []
    if t is None:
        return out
    if m.cls(t) in ("snd!", "lsnd"):
        return [t]
    if m.cls(t) != "effe":
        return out
    for ev in m.elements(m.meta(t) + 0x2C, 0x44):
        for p in m.elements(ev + 0x20, 0x60):
            r = m.ref(p + 0x10)
            if r is not None and m.cls(r) == "snd!":
                out.append(r)
    return out


def anim_sounds(jmad, names):
    """jmad +0x20 sound references (20 bytes, ref at +0), +0x50 animations (136 bytes:
    +0x00 name, +0x38 sound events of 8 bytes: i16 sound, i16 frame)."""
    refs = [m.ref(e) for e in m.elements(m.meta(jmad) + 0x20, 20)]
    out = []
    for want in names:
        for a in m.elements(m.meta(jmad) + 0x50, 136):
            if m.sid_at(a) != want:
                continue
            for ev in m.elements(a + 0x38, 8):
                i, frame = m.i16(ev), m.i16(ev + 2)
                if 0 <= i < len(refs) and refs[i] is not None:
                    out.append((want, frame, refs[i]))
        if out:
            break
    return out


def material_sounds(foot):
    """foot +0x00 effect types (36 bytes: blocks at +0x00, +0x0C sounds, +0x18 effects);
    sound / effect entries 40 bytes: ref +0x00, ref +0x10, material name +0x20."""
    if foot is None:
        return []
    types = m.elements(m.meta(foot), 36)
    if len(types) <= IMPACT_TYPE:
        return []
    e = types[IMPACT_TYPE]
    hits = []
    # Sounds and effects are looked up separately, each walking up the material's
    # name (tough_terrain_grass, tough_terrain, tough, then default_material).
    for label, block, get in (("sound", 0x0C, lambda s: [m.ref(s), m.ref(s + 16)]),
                              ("effect", 0x18, lambda s: effect_sounds(m.ref(s)))):
        entries = {m.sid_at(s + 0x20): s for s in m.elements(e + block, 40)}
        for mat in MATERIAL_CHAIN:
            if mat in entries:
                hits += [(f"{label} {mat}", r) for r in get(entries[mat]) if r is not None]
                break
    return hits


PLACED = {}
S = m.meta(m.find("scnr"))
pal = [m.ref(o) for o in m.elements(S + 0x120, 16)]
for e in m.elements(S + 0x114, 168):
    pi = m.i16(e)
    if 0 <= pi < len(pal) and pal[pi] is not None:
        PLACED[pal[pi]] = PLACED.get(pal[pi], 0) + 1
ar = m.find("weap", r"objects\weapons\rifle\assault_rifle\assault_rifle")
if ar is not None:
    PLACED.setdefault(ar, 0)

for w in m.all("weap"):
    if not SHOW_ALL and w not in PLACED:
        continue
    b = m.meta(w)
    print(f"\n#{w} {m.names[w]}  placed {PLACED.get(w, 0)}")
    print(f"   ammo pickup (+0x3A0): {fmt(m.ref(b + 0x3A0))}")
    print(f"   zoom in / out (+0x3B0 / +0x3C0): {short(m.refname(b + 0x3B0))} / {short(m.refname(b + 0x3C0))}")
    for k, e in enumerate(m.elements(b + 0x43C, 308)):
        for q, fe in enumerate(m.elements(e + 0x128, 0x80)):
            print(f"   barrel {k} firing effect {q}: shots {m.i16(fe)}..{m.i16(fe + 2)}")
            fire, empty = m.ref(fe + 0x04), m.ref(fe + 0x24)
            print(f"      fire  {m.refname(fe + 0x04)}")
            for s in effect_sounds(fire):
                print(f"         {fmt(s)}")
            print(f"      empty {m.refname(fe + 0x24)}")
            for s in effect_sounds(empty):
                print(f"         {fmt(s)}")
        p = m.ref(e + 0x98)
        if p is None or m.cls(p) != "proj":
            continue
        P = m.meta(p)
        print(f"      projectile {short(m.names[p])}: flyby {short(m.refname(P + 0x1C0))}")
        for label, off in (("airborne detonation", 0x138), ("detonation", 0x148), ("impact", 0x1D0)):
            t = m.ref(P + off)
            if t is not None:
                print(f"         {label} (+{off:#x}) {m.names[t]}")
                for s in effect_sounds(t):
                    print(f"            {fmt(s)}")
        foot = m.ref(P + 0x80)
        print(f"         material effects {m.refname(P + 0x80)}")
        for label, s in material_sounds(foot):
            print(f"            {label}: {fmt(s)}")
    fps = m.elements(b + 0x3FC, 32)
    if fps:
        j = m.ref(fps[0] + 16)
        if j is not None:
            print(f"   first-person animations {m.names[j]}")
            for label, names in (("ready", ["first_person:ready"]),
                                 ("reload", ["first_person:reload_full", "first_person:reload_empty",
                                             "first_person:reload_enter", "first_person:reload_continue_full"])):
                for anim, frame, s in anim_sounds(j, names):
                    print(f"      {label} ({anim} frame {frame}): {fmt(s)}")
    for e in m.elements(b + 0xBC, 32):
        r = m.ref(e)
        if r is not None and m.cls(r) == "lsnd":
            print(f"   attached looping sound (charge) {m.names[r]}")
            for tr in m.elements(m.meta(r) + 0x28, 144):
                for label, o in (("in", 0x14), ("loop", 0x24), ("out", 0x34)):
                    t = m.ref(tr + o)
                    if t is not None:
                        print(f"      {label}: {fmt(t)}")

if "--classes" in sys.argv:
    print("\nsound classes (sncl element 0x98 bytes): index, max per tag, max per object, preemption ms,"
          " distances +0x34..+0x40")
    for k, e in enumerate(CLASSES):
        print(f"   {k:2d} {m.i16(e):3d} {m.i16(e + 2):3d} {m.i16(e + 4):4d}"
              f"  {tuple(round(x, 2) for x in m.f32n(e + 0x34, 4))}")
