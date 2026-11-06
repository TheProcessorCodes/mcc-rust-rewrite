//! Halo CE's placed objects for the scripts: the object table built from
//! the scene `blam_h1` decoded (no Halo 3 cache file behind it), with the
//! machines' own numbers (`blam_h1::objects::Device`, handed over as
//! [`H1Device`]): initially open or off, how long a move takes, and the
//! doors that open by themselves.
//!
//! Halo CE's device animations (`antr`) are not posed here: a door's
//! collision and drawing go when it is more than half open
//! ([`DeviceSpec::opens_away`]), and a platform keeps its rest pose.

use blam_geometry::{Kind, Scene};
use blam_script::ScriptData;

use crate::objects::{AutoDoor, DeviceSpec, ObjectInfo, Objects};

/// What the game side read about one device (`blam_h1::objects::Device`).
#[derive(Clone, Debug, Default)]
pub struct H1Device {
    /// A door machine (`mach +0x290` 0); `false` for platforms, gears and
    /// controls.
    pub door: bool,
    /// The door opens by itself (no "does not operate automatically").
    pub automatic: bool,
    pub position_seconds: f32,
    pub door_open_seconds: f32,
    pub initially_open: bool,
    pub initially_off: bool,
    /// The object's bounding sphere, object frame.
    pub radius: f32,
    pub centre: glam::Vec3,
}

/// How far past its bounding sphere (wu) the player opens an automatic
/// door: about 2 m. Ours, not read from the game.
pub const DOOR_REACH: f32 = 0.6;

/// The object table: the scene's objects first (by `Scene::objects`
/// index), then the named objects nothing draws. `devices` is by scene
/// object (shorter or empty: no devices).
pub fn objects(scene: &Scene, data: &ScriptData, devices: &[Option<H1Device>]) -> Objects {
    let mut info = Vec::with_capacity(scene.objects.len());
    let mut by_name: Vec<Option<u32>> = vec![None; data.object_names.len()];
    for (i, o) in scene.objects.iter().enumerate() {
        if let Some(n) = o.name_index
            && let Some(slot) = by_name.get_mut(n as usize)
        {
            *slot = Some(i as u32);
        }
        let device = devices.get(i).cloned().flatten().map(|d| DeviceSpec {
            position_seconds: d.position_seconds,
            opens_away: d.door,
            door: (d.door && d.automatic).then(|| AutoDoor {
                centre: o.transform.transform_point3(d.centre),
                radius: d.radius + DOOR_REACH,
                hold_seconds: d.door_open_seconds,
            }),
        });
        info.push(ObjectInfo {
            name: o.name.clone(),
            scene: Some(i as u32),
            object_type: o
                .name_index
                .and_then(|n| data.object_names.get(n as usize))
                .map_or(kind_type(o.kind), |n| n.object_type),
            base: o.transform,
            folder: None,
            starts_absent: o.starts_absent(),
            sections: Vec::new(),
            tag_path: o.tag_path.clone(),
            tag: Some(o.tag),
            contacts: Vec::new(),
            physics: None,
            device,
        });
    }
    // Named objects nothing draws (vehicles, bipeds, sound scenery).
    for (n, name) in data.object_names.iter().enumerate() {
        if by_name[n].is_none() {
            by_name[n] = Some(info.len() as u32);
            info.push(ObjectInfo {
                name: Some(name.name.clone()),
                scene: None,
                object_type: name.object_type,
                base: glam::Mat4::IDENTITY,
                folder: None,
                starts_absent: false,
                sections: Vec::new(),
                tag_path: String::new(),
                tag: None,
                contacts: Vec::new(),
                physics: None,
                device: None,
            });
        }
    }
    let mut objects = Objects::new(info, by_name);
    for (i, d) in devices.iter().enumerate().take(scene.objects.len()) {
        let Some(d) = d else { continue };
        let s = &mut objects.state[i];
        if d.initially_open {
            s.device.animate(1.0, 0.0);
            if d.door {
                s.solid = false;
                s.hidden = true;
            }
        }
        if d.initially_off {
            s.device.power = 0.0;
        }
    }
    objects
}

/// Marks what the world must draw on its own: what the scripts name or
/// create (`setup::mark_dynamic`), and the doors, which open. Returns how
/// many.
pub fn mark_dynamic(scene: &mut Scene, data: &ScriptData, devices: &[Option<H1Device>]) -> usize {
    crate::setup::mark_dynamic(scene, data);
    let mut n = 0;
    for (i, o) in scene.objects.iter_mut().enumerate() {
        if devices.get(i).is_some_and(|d| d.as_ref().is_some_and(|d| d.door)) {
            o.dynamic = true;
        }
        n += o.dynamic as usize;
    }
    n
}

fn kind_type(kind: Kind) -> i16 {
    match kind {
        Kind::Vehicle => 1,
        Kind::Machine => 7,
        Kind::Control => 8,
        Kind::Crate => 10,
        _ => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blam_geometry::SceneObject;
    use glam::{Mat4, Vec3};

    fn door_scene() -> (Scene, Vec<Option<H1Device>>) {
        let scene = Scene {
            objects: vec![SceneObject {
                kind: Kind::Machine,
                name_index: None,
                name: None,
                placement: 0,
                tag: 1,
                tag_path: "door small".into(),
                transform: Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)),
                bsps: 0,
                folder: None,
                flags: 0,
                placements: 0..0,
                dynamic: false,
            }],
            ..Default::default()
        };
        let door = H1Device {
            door: true,
            automatic: true,
            position_seconds: 0.8,
            door_open_seconds: 1.5,
            radius: 1.0,
            centre: Vec3::new(0.0, 0.0, 0.5),
            ..Default::default()
        };
        (scene, vec![Some(door)])
    }

    /// Steps the doors and the objects as the runtime does, `seconds` long.
    fn run(o: &mut Objects, player: Vec3, seconds: f32) {
        for _ in 0..(seconds * 30.0) as usize {
            o.auto_doors(Some(player), 1.0 / 30.0);
            o.step(1.0 / 30.0, None);
        }
    }

    #[test]
    fn automatic_door_opens_and_closes() {
        let (scene, devices) = door_scene();
        let mut o = objects(&scene, &ScriptData::default(), &devices);
        let far = Vec3::new(0.0, 0.0, 0.0);
        let near = Vec3::new(10.5, 0.5, 0.0);
        run(&mut o, far, 1.0);
        assert!(o.state[0].solid && o.state[0].device.position == 0.0);
        // Open within its 0.8 s, out of the way.
        run(&mut o, near, 0.9);
        assert!((o.state[0].device.position - 1.0).abs() < 1e-4, "{}", o.state[0].device.position);
        assert!(!o.state[0].solid && o.state[0].hidden);
        // Held open 1.5 s after the player left, then shut.
        run(&mut o, far, 1.2);
        assert!(!o.state[0].solid);
        run(&mut o, far, 1.5);
        assert!(o.state[0].solid && o.state[0].device.position < 1e-4);
    }

    #[test]
    fn locked_door_stays_shut() {
        let (scene, mut devices) = door_scene();
        devices[0].as_mut().unwrap().initially_off = true;
        let mut o = objects(&scene, &ScriptData::default(), &devices);
        run(&mut o, Vec3::new(10.0, 0.0, 0.0), 2.0);
        assert!(o.state[0].solid && o.state[0].device.position == 0.0);
    }
}
