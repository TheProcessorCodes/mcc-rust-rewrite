# Read-only: which trigger volumes (kill volumes marked) hold a point, using the
# box rule of docs/format/zone-sets.md 5. Default points: the Warthog run's start flag and
# a few spots along its first plates.
# Run:  python -I probe_kill_at.py [map] [x,y,z ...]
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h3_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "120_halo")
pts = [tuple(float(v) for v in a.split(",")) for a in sys.argv[2:]] or [
    (-176.5, -351.47, -1.3), (-150.0, -300.0, -10.0), (-83.8, -191.3, -20.5)]
s = m.meta(m.find("scnr"))
vols = []
for o in m.elements(s + 0x258, 68):
    f, u, p, e = (m.f32n(o + k, 3) for k in (0x0C, 0x18, 0x24, 0x30))
    vols.append((m.sid_at(o), f, u, p, e))


def inside(v, q):
    name, f, u, p, e = v
    left = (u[1] * f[2] - u[2] * f[1], u[2] * f[0] - u[0] * f[2], u[0] * f[1] - u[1] * f[0])
    d = [q[k] - p[k] for k in range(3)]
    local = [sum(d[k] * a[k] for k in range(3)) for a in (f, left, u)]
    return all(0 <= local[k] <= e[k] for k in range(3))


for q in pts:
    hits = [v[0] for v in vols if inside(v, q)]
    kills = [h for h in hits if h.lower().startswith("kill") or "_kill" in h.lower()]
    print(q, "kill:", kills, "| all:", hits)
