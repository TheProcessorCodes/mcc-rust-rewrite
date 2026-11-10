//! The zone-set keys and the cluster the camera is in. The zone-set state
//! itself ([`LoadedBsps`], [`Zones`]) lives in `mcc_core::zones`.

use bevy::prelude::*;
use mcc_core::map::MapAppExt;

pub use mcc_core::zones::*;

use mcc_core::fly_camera::FlyCamera;

pub struct ZonesPlugin;

impl Plugin for ZonesPlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<Zones>()
            .init_map_resource::<CameraOutside>()
            .init_map_resource::<LoadedBsps>()
            .init_map_resource::<ShowCollision>()
            .add_systems(
                Update,
                (zone_keys.run_if(mcc_core::pause::running), follow_cluster)
                    .chain()
                    .in_set(ZoneKeys).run_if(mcc_core::map::ready),
            );
    }
}

fn zone_keys(keys: Res<ButtonInput<KeyCode>>, zones: Res<Zones>, mut loaded: ResMut<LoadedBsps>) {
    let count = zones.layout.zone_sets.len();
    if keys.just_pressed(KeyCode::Home) {
        load_bsps(&mut loaded, &zones, u32::MAX);
    } else if count > 0 && keys.just_pressed(KeyCode::PageDown) {
        // The next zone set this map (or mission) allows.
        let from = loaded.zone_set.map_or(count - 1, |n| n);
        if let Some(n) = (1..=count).map(|k| (from + k) % count).find(|&n| zones.allows(n)) {
            load_zone_set(&mut loaded, &zones, n);
        }
    } else if count > 0 && keys.just_pressed(KeyCode::PageUp) {
        let from = loaded.zone_set.unwrap_or(0);
        if let Some(n) = (1..=count).map(|k| (from + count - k) % count).find(|&n| zones.allows(n)) {
            load_zone_set(&mut loaded, &zones, n);
        }
    }
}

/// Halo picks the sky and the air by the cluster the camera is in. Indoors
/// (no sky) the last sky stays. Outside every cluster box, or in a cluster
/// naming no air, the air most loaded clusters name.
fn follow_cluster(
    zones: Res<Zones>,
    mut loaded: ResMut<LoadedBsps>,
    mut air: ResMut<CameraAir>,
    mut outside: ResMut<CameraOutside>,
    cameras: Query<&GlobalTransform, With<FlyCamera>>,
    mut main_air: Local<Option<(u32, Option<u32>)>>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    let main = match *main_air {
        Some((mask, a)) if mask == loaded.mask => a,
        _ => {
            let a = zones.layout.main_atmosphere(loaded.mask);
            *main_air = Some((loaded.mask, a));
            a
        }
    };
    let at = super::to_halo(camera.translation());
    let cluster = zones.layout.cluster_at(at, loaded.mask);
    let want = cluster.and_then(|c| c.atmosphere).or(main);
    if air.0 != want {
        air.0 = want;
    }
    let sky = cluster.is_none_or(|c| c.sky.is_some());
    if outside.0 != sky {
        outside.0 = sky;
    }
    let Some(cluster) = cluster else {
        return;
    };
    if let Some(sky) = cluster.sky
        && loaded.sky != Some(sky)
        && zones.skies.len() > 1
        && zones.skies.iter().any(|(s, _)| *s == sky)
    {
        loaded.sky = Some(sky);
        info!("sky {sky}");
    }
}
