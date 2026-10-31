# Read-only probe of the on-foot movement numbers in MCC Halo 3 (U13) maps:
# the globals tag's player control / player information / falling damage
# blocks and the player biped's unit, biped and character physics fields.
# Companion to docs/format/movement.md; every offset used there is printed here.
# Run: python -I probe_movement.py [map name or path ...]   (default riverworld)
import math, struct, sys, os

MAPS = os.environ.get("HALO3_MAPS", "")


def probe(name):
    path = name if name.lower().endswith(".map") else os.path.join(MAPS, name + ".map")
    d = open(path, "rb").read()

    def i32(o): return struct.unpack_from("<i", d, o)[0]
    def u32(o): return struct.unpack_from("<I", d, o)[0]
    def i16(o): return struct.unpack_from("<h", d, o)[0]
    def i64(o): return struct.unpack_from("<q", d, o)[0]
    def f32(o): return struct.unpack_from("<f", d, o)[0]
    def cstr(o):
        return d[o:d.index(b"\0", o)].decode("latin1")

    # Header, sections and tag index: docs/format/container.md.
    secoff = [u32(0x4CC + 4 * i) for i in range(4)]
    secs = [(u32(0x4DC + 8 * i), u32(0x4E0 + 8 * i)) for i in range(4)]
    vbase = i64(0x2E0); idxptr = i64(0x2E8)
    magic2 = (secs[2][0] + secoff[2]) & 0xFFFFFFFF
    def tagfile(v): return v - vbase + magic2
    def tp(p): return tagfile((p << 2) & 0xFFFFFFFFFFFFFFFF) if p else -1
    ti = tagfile(idxptr)
    classcount = i32(ti); cf = tagfile(i64(ti + 8)); tf = tagfile(i64(ti + 24))
    classes = [d[cf + 16 * i:cf + 16 * i + 4][::-1].decode("latin1") for i in range(classcount)]
    nameidx = (i32(0x2C) + secoff[0]) & 0xFFFFFFFF
    names = (i32(0x24) + secoff[0]) & 0xFFFFFFFF
    tags = []
    for i in range(i32(0x20)):
        ci = i16(tf + 8 * i)
        no = i32(nameidx + 4 * i)
        tags.append((classes[ci] if ci >= 0 else None, cstr(names + no) if no >= 0 else None,
                     tp(u32(tf + 8 * i + 4)) if ci >= 0 else -1))

    def block(o):
        c = i32(o); return c, (tp(u32(o + 4)) if c > 0 else -1)
    def ref(o):
        tid = i32(o + 12)
        return None if tid == -1 else tid & 0xFFFF

    print("=====", os.path.basename(path))
    matg = [t for t in tags if t[0] == "matg"][0][2]

    # matg +0x404: player control (one element).
    n, pc = block(matg + 0x404)
    print("player control n=%d" % n)
    for off, label in [(0x00, "magnetism friction"), (0x04, "magnetism adhesion"),
                       (0x08, "inconsequential target scale"), (0x0C, "crosshair x"),
                       (0x10, "crosshair y"), (0x14, "seconds to start"),
                       (0x18, "seconds to full speed"), (0x1C, "decay rate"),
                       (0x20, "full speed multiplier"), (0x24, "pegged magnitude"),
                       (0x28, "pegged angular threshold (deg)"), (0x2C, "look default pitch rate (deg/s)"),
                       (0x30, "look default yaw rate (deg/s)"), (0x34, "look peg threshold"),
                       (0x38, "look yaw acceleration time"), (0x3C, "look yaw acceleration scale"),
                       (0x40, "look pitch acceleration time"), (0x44, "look pitch acceleration scale"),
                       (0x48, "look autoleveling scale"), (0x54, "min angle for vehicle flipping (rad)")]:
        print("  +0x%02X %-36s %g" % (off, label, f32(pc + off)))

    # matg +0x434: player information (one element, 0xC0 bytes).
    n, pi = block(matg + 0x434)
    print("player information n=%d" % n)
    for off, label in [(0x00, "walking speed"), (0x04, "run forward"), (0x08, "run backward"),
                       (0x0C, "run sideways"), (0x10, "run acceleration"), (0x14, "sneak forward"),
                       (0x18, "sneak backward"), (0x1C, "sneak sideways"), (0x20, "sneak acceleration"),
                       (0x24, "airborne acceleration")]:
        print("  +0x%02X %-36s %g" % (off, label, f32(pi + off)))

    # matg +0x440: player representation; +0x28 in each 0x5C element is the third-person unit.
    n, pr = block(matg + 0x440)
    bipeds = [ref(pr + 0x5C * i + 0x28) for i in range(n)]
    print("player representation n=%d, bipeds %s" % (n, [tags[b][1] if b is not None else None for b in bipeds]))

    # matg +0x458: falling damage (one element).
    n, fd = block(matg + 0x458)
    h0, h1, hmax = f32(fd), f32(fd + 4), f32(fd + 0x58)
    vmax, v0, v1 = f32(fd + 0x6C), f32(fd + 0x70), f32(fd + 0x74)
    print("falling damage n=%d: harmful distance %g..%g, max distance %g" % (n, h0, h1, hmax))
    print("  runtime velocities %g %g %g" % (vmax, v0, v1))
    gs = [v * v / (2 * h) for v, h in ((v0, h0), (v1, h1), (vmax, hmax))]
    print("  implied gravity v^2/2h: %s wu/s^2" % ", ".join("%.5f" % g for g in gs))
    g = gs[2]

    b = next(x for x in bipeds if x is not None)
    bp = tags[b][2]
    print("biped", tags[b][1])
    print("  unit +0x10C camera field of view %g rad = %g deg" % (f32(bp + 0x10C), math.degrees(f32(bp + 0x10C))))
    for off, label in [(0x2BC, "moving turning speed (rad/s)"), (0x2C4, "stationary turning threshold (rad)"),
                       (0x2C8, "jump velocity"), (0x2CC, "maximum soft landing time"),
                       (0x2D0, "maximum hard landing time"), (0x2D4, "minimum soft landing velocity"),
                       (0x2D8, "minimum hard landing velocity"), (0x2DC, "maximum hard landing velocity"),
                       (0x2E0, "death hard landing velocity"), (0x2E4, "stun duration"),
                       (0x2E8, "standing camera height"), (0x2EC, "crouching camera height"),
                       (0x2F0, "crouch transition time"), (0x2F4, "camera interpolation start (rad)"),
                       (0x2F8, "camera interpolation end (rad)"), (0x308, "camera exclusion distance"),
                       (0x30C, "autoaim width"), (0x314, "lock-on distance"),
                       (0x340, "physics height standing"), (0x344, "physics height crouching"),
                       (0x348, "physics radius"), (0x34C, "physics mass"),
                       (0x380, "maximum slope angle (rad)"), (0x384, "downhill falloff angle"),
                       (0x388, "downhill cutoff angle"), (0x38C, "uphill falloff angle"),
                       (0x390, "uphill cutoff angle"), (0x394, "downhill velocity scale"),
                       (0x398, "uphill velocity scale"), (0x39C, "runtime cos(max slope)"),
                       (0x3B0, "climb inflection angle (rad)")]:
        print("  +0x%03X %-36s %g" % (off, label, f32(bp + off)))
    print("  checks: cos(max slope)=%.6f (stored %.6f), sin(downhill cutoff)=%.6f (stored -%.6f at +0x3A4)" % (
        math.cos(f32(bp + 0x380)), f32(bp + 0x39C), math.sin(f32(bp + 0x388)), -f32(bp + 0x3A4)))
    n, pills = block(bp + 0x368)
    for i in range(n):
        e = pills + 0x70 * i
        r = f32(e + 0x40); top = f32(e + 0x58); bot = f32(e + 0x68)
        print("  pill %d: radius %g, centres z %g..%g -> spans z %g..%g" % (i, r, bot, top, bot - r, top + r))
    v = f32(bp + 0x2C8)
    print("  jump apex v^2/2g = %.4f wu (%.3f m) with g = %.5f" % (v * v / (2 * g), v * v / (2 * g) * 3.048, g))


for name in (sys.argv[1:] or ["riverworld"]):
    probe(name)
