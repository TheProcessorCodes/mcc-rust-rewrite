//! Halo CE's devices handed to the scripts: what `blam_h1` reads of each
//! machine and control (`blam_h1::objects::Device`) in the shape the
//! script runtime takes (`mcc_script::h1_objects::H1Device`).

use blam_geometry::Scene;
use mcc_script::h1_objects::H1Device;

/// Each scene object's device, by `Scene::objects` index.
pub fn devices(map: &blam_h1::Map, scene: &Scene) -> Vec<Option<H1Device>> {
    let out: Vec<Option<H1Device>> = blam_h1::objects::devices(map, scene)
        .into_iter()
        .map(|d| {
            d.map(|d| H1Device {
                door: d.machine == Some(blam_h1::objects::MachineType::Door),
                automatic: d.automatic_door(),
                position_seconds: d.position_seconds,
                door_open_seconds: d.door_open_seconds,
                initially_open: d.initially_open(),
                initially_off: d.initially_off(),
                radius: d.radius,
                centre: d.centre,
            })
        })
        .collect();
    let n = out.iter().flatten().count();
    if n > 0 {
        let doors = out.iter().flatten().filter(|d| d.door).count();
        eprintln!("halo ce devices: {n} ({doors} doors, {} open by themselves)", out.iter().flatten().filter(|d| d.automatic).count());
    }
    out
}
