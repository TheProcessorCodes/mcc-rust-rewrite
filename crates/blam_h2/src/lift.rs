//! Gravity lifts and other pushing volumes (`docs/format/halo2.md` 11): a
//! placed object whose model names a physics model (`phmo`) with phantom
//! types. Each shape of that physics model that uses a phantom type becomes
//! a [`LiftVolume`], aimed by the render model marker the phantom type
//! names. Lockout's lift is the crate `lockout_lift_tube`.

use blam_geometry::{LiftPush, LiftShape, LiftVolume};
use glam::{Mat4, Quat, Vec3};

use crate::{Map, Reader, Result};

/// Scenario object lists that can carry a lift: (placements, element
/// size, palette), as in `decode_objects`.
const LISTS: [(usize, usize, usize); 4] = [
    (0x50, 0x5C, 0x58),   // scenery
    (0xA8, 0x48, 0xB0),   // machines
    (0xB8, 0x44, 0xC0),   // controls
    (0x328, 0x4C, 0x330), // crates
];

/// Phantom type flags: gravity off inside, players left alone, vehicles
/// left alone.
const NEGATES_GRAVITY: u32 = 1 << 2;
const IGNORES_PLAYERS: u32 = 1 << 3;
const IGNORES_VEHICLES: u32 = 1 << 6;

struct PhantomType {
    flags: u32,
    marker: String,
    push: LiftPush,
}

/// Every pushing volume the scenario places.
pub(crate) fn decode(map: &Map, scnr: Reader<'_>) -> Result<Vec<LiftVolume>> {
    let mut out = Vec::new();
    for (at, size, palette_at) in LISTS {
        let palette: Vec<u32> = scnr.block(palette_at, 0x28)?.into_iter().map(|p| p.tag_ref(0)).collect::<Result<_>>()?;
        for (k, e) in scnr.block(at, size)?.into_iter().enumerate() {
            let Some(&object) = usize::try_from(e.i16(0)?).ok().and_then(|p| palette.get(p)) else { continue };
            let scale = e.f32(0x20)?;
            let transform = crate::object_transform(e.vec3(0x8)?, e.vec3(0x14)?, if scale > 0.0 { scale } else { 1.0 });
            let name = map.tag_name(object).unwrap_or("?");
            let name = format!("{} {k}", name.rsplit('\\').next().unwrap_or(name));
            object_lifts(map, object, transform, &name, &mut out)?;
        }
    }
    Ok(out)
}

/// The volumes of one placed object.
fn object_lifts(map: &Map, object: u32, transform: Mat4, name: &str, out: &mut Vec<LiftVolume>) -> Result<()> {
    // object +0x34 the model (hlmt); hlmt +0x00 the render model, +0x20
    // the physics model.
    let Some(o) = map.tag_data(object) else { return Ok(()) };
    let Some(hlmt) = map.tag_data(o.tag_ref(0x34)?) else { return Ok(()) };
    let Some(phmo) = map.tag_data(hlmt.tag_ref(0x20)?) else { return Ok(()) };
    let types: Vec<PhantomType> = phmo
        .block(0x28, 0x68)?
        .into_iter()
        .map(|t| -> Result<PhantomType> {
            let flags = t.u32(0)?;
            let pair = |at: usize| -> Result<(f32, f32)> { Ok((t.f32(at)?, t.f32(at + 4)?)) };
            Ok(PhantomType {
                flags,
                marker: t.map.string(t.u32(0x8)?).to_string(),
                push: LiftPush {
                    dead_radius: t.f32(0x1C)?,
                    center: pair(0x20)?,
                    axis: pair(0x28)?,
                    direction: pair(0x30)?,
                    negates_gravity: flags & NEGATES_GRAVITY != 0,
                    ignores_vehicles: flags & IGNORES_VEHICLES != 0,
                },
            })
        })
        .collect::<Result<_>>()?;
    if types.is_empty() {
        return Ok(());
    }
    let markers = markers(map, hlmt.tag_ref(0)?)?;
    let (_, rotation, _) = transform.to_scale_rotation_translation();

    // Shapes, in the physics model's node space (node 0 on every lift
    // seen, at the object's origin). Phantom type index i8 at +0x1E.
    let mut shapes: Vec<(String, i8, LiftShape)> = Vec::new();
    let point = |p: Vec3| transform.transform_point3(p);
    // Spheres (0xA0): radius +0x38, translation +0x90.
    for s in phmo.block(0x48, 0xA0)? {
        let c = point(s.vec3(0x90)?);
        let r = s.f32(0x38)? * scale_of(transform);
        shapes.push((s.map.string(s.u32(0)?).to_string(), s.u8(0x1E)? as i8, LiftShape::Capsule { a: c, b: c, radius: r }));
    }
    // Pills (0x60): radius +0x38, the segment's ends +0x40 and +0x50.
    for s in phmo.block(0x58, 0x60)? {
        let shape = LiftShape::Capsule {
            a: point(s.vec3(0x40)?),
            b: point(s.vec3(0x50)?),
            radius: s.f32(0x38)? * scale_of(transform),
        };
        shapes.push((s.map.string(s.u32(0)?).to_string(), s.u8(0x1E)? as i8, shape));
    }
    // Boxes (0xB0): half sizes +0x40, then the transform they sit by: three
    // rotation rows +0x70, +0x80, +0x90 (16 bytes apart) and the
    // translation +0xA0.
    for s in phmo.block(0x60, 0xB0)? {
        let rows = [s.vec3(0x70)?, s.vec3(0x80)?, s.vec3(0x90)?];
        let local_axes = [
            Vec3::new(rows[0].x, rows[1].x, rows[2].x),
            Vec3::new(rows[0].y, rows[1].y, rows[2].y),
            Vec3::new(rows[0].z, rows[1].z, rows[2].z),
        ];
        let shape = LiftShape::Box {
            centre: point(s.vec3(0xA0)?),
            axes: local_axes.map(|a| (rotation * a).normalize_or(Vec3::Z)),
            half: s.vec3(0x40)? * scale_of(transform),
        };
        shapes.push((s.map.string(s.u32(0)?).to_string(), s.u8(0x1E)? as i8, shape));
    }
    // Polyhedra (0x120): their bounding box, half sizes +0x40 about the
    // centre +0x50. Every lift polyhedron seen is a box or close to one.
    for s in phmo.block(0x70, 0x120)? {
        let shape = LiftShape::Box {
            centre: point(s.vec3(0x50)?),
            axes: [Vec3::X, Vec3::Y, Vec3::Z].map(|a| rotation * a),
            half: s.vec3(0x40)? * scale_of(transform),
        };
        shapes.push((s.map.string(s.u32(0)?).to_string(), s.u8(0x1E)? as i8, shape));
    }

    for (shape_name, phantom, shape) in shapes {
        let Some(t) = usize::try_from(phantom).ok().and_then(|p| types.get(p)) else { continue };
        if t.flags & IGNORES_PLAYERS != 0 {
            continue;
        }
        // The marker it is aimed by; none (or an empty name): the object's
        // origin and its up.
        let (origin, direction) = match markers.iter().find(|(n, _, _)| !t.marker.is_empty() && *n == t.marker) {
            Some((_, at, forward)) => (point(*at), rotation * *forward),
            None => (transform.w_axis.truncate(), rotation * Vec3::Z),
        };
        out.push(LiftVolume {
            name: format!("{name} {shape_name}"),
            shape,
            origin,
            direction: direction.normalize_or(Vec3::Z),
            push: t.push,
        });
    }
    Ok(())
}

fn scale_of(m: Mat4) -> f32 {
    m.x_axis.truncate().length()
}

/// A render model's markers: name, position, and forward (the marker's +X),
/// node space. Marker groups at `mode` +0x58 (12 bytes: name string id,
/// markers block +0x04 of 36 bytes: node i8 +0x02, translation +0x04,
/// rotation i, j, k, w +0x10).
fn markers(map: &Map, mode: u32) -> Result<Vec<(String, Vec3, Vec3)>> {
    let Some(r) = map.tag_data(mode) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for g in r.block(0x58, 12)? {
        let name = g.map.string(g.u32(0)?).to_string();
        for m in g.block(4, 36)? {
            let q = Quat::from_xyzw(m.f32(0x10)?, m.f32(0x14)?, m.f32(0x18)?, m.f32(0x1C)?).normalize();
            out.push((name.clone(), m.vec3(0x4)?, q * Vec3::X));
        }
    }
    Ok(out)
}
