//! Which structure BSPs are loaded: zone sets.
//!
//! Every BSP of the map is decoded at start; only the loaded ones are on
//! the GPU (`stream.rs` uploads and drops them per BSP). Each world chunk
//! carries a [`BspMember`] saying which BSPs it belongs to, and
//! [`LoadedBsps`] says which are loaded. [`load_zone_set`] and [`load_bsps`]
//! are the whole API: change the resource and the streaming and culling
//! follow (`stream.rs`, `culling.rs`), the skate world rebuilds from the
//! loaded BSPs' collision (`skate.rs`), and the sky and haze follow the
//! camera's cluster ([`follow_cluster`]).
//!
//! Keys: PageDown / PageUp step to the next / previous zone set, Home loads
//! every BSP at once. `docs/format/zone-sets.md` has the tag layout.

use bevy::prelude::*;

use blam_geometry::{Kind, Scene};

use super::sky::CameraAir;
use crate::fly_camera::FlyCamera;

/// Which BSPs (bit `n`: BSP `n`) a batch belongs to, and for a sky batch
/// which scenario sky it is. `bsps == 0`: shown whatever is loaded.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct BspMember {
    pub bsps: u32,
    pub sky: Option<u32>,
}

/// The map's zone sets, BSPs and clusters, kept for switching.
#[derive(Resource, Default)]
pub struct Zones {
    /// Only `bsps`, `zone_sets` and `starts` are filled.
    pub layout: Scene,
    /// Placed skies: (scenario sky index, active-BSP mask).
    pub skies: Vec<(u32, u32)>,
}

impl Zones {
    pub fn of(scene: &Scene) -> Zones {
        let mut skies: Vec<(u32, u32)> = scene
            .placements
            .iter()
            .filter(|p| p.kind == Kind::Sky)
            .filter_map(|p| p.sky.map(|s| (s, p.bsps)))
            .collect();
        skies.sort_unstable();
        skies.dedup();
        Zones {
            layout: Scene {
                name: scene.name.clone(),
                bsps: scene.bsps.clone(),
                zone_sets: scene.zone_sets.clone(),
                starts: scene.starts.clone(),
                ..Default::default()
            },
            skies,
        }
    }

    pub fn all(&self) -> u32 {
        self.layout.all_bsps()
    }

    /// A readable list of the BSPs in `mask`.
    pub fn describe(&self, mask: u32) -> String {
        let names: Vec<String> = self
            .layout
            .bsps
            .iter()
            .filter(|b| mask & (1 << b.index) != 0)
            .map(|b| format!("{} {}", b.index, b.name.rsplit('\\').next().unwrap_or(&b.name)))
            .collect();
        names.join(", ")
    }
}

/// The BSPs currently loaded, the zone set they came from, and the sky
/// drawn.
#[derive(Resource, Clone, Debug, Default)]
pub struct LoadedBsps {
    /// The BSPs wanted: switching zone sets changes this.
    pub mask: u32,
    /// The BSPs on screen: follows `mask` once their units are uploaded
    /// (`stream.rs`), so the old zone set stays drawn until then.
    pub shown: u32,
    pub zone_set: Option<usize>,
    pub sky: Option<u32>,
}

impl LoadedBsps {
    pub fn shows(&self, m: &BspMember) -> bool {
        // `HALO3_SKY=0` hides the sky, for telling sky pieces from the world.
        static SKY_OFF: std::sync::LazyLock<bool> =
            std::sync::LazyLock::new(|| std::env::var("HALO3_SKY").is_ok_and(|v| v == "0"));
        (m.bsps == 0 || m.bsps & self.shown != 0)
            && (m.sky.is_none() || (m.sky == self.sky && !*SKY_OFF))
    }
}

/// Loads zone set `n`: its BSPs are streamed in and the others out
/// (`stream.rs`).
pub fn load_zone_set(loaded: &mut LoadedBsps, zones: &Zones, n: usize) {
    let Some(z) = zones.layout.zone_sets.get(n) else {
        return;
    };
    loaded.mask = z.bsps & zones.all();
    loaded.zone_set = Some(n);
    loaded.sky = zones.layout.main_sky(loaded.mask, &zones.skies);
    info!(
        "zone set {n} {}: BSPs {}",
        z.name,
        zones.describe(loaded.mask)
    );
}

/// Loads exactly the BSPs in `mask`, outside any zone set.
pub fn load_bsps(loaded: &mut LoadedBsps, zones: &Zones, mask: u32) {
    loaded.mask = mask & zones.all();
    loaded.zone_set = zones
        .layout
        .zone_sets
        .iter()
        .position(|z| z.bsps & zones.all() == loaded.mask);
    loaded.sky = zones.layout.main_sky(loaded.mask, &zones.skies);
    info!("BSPs {}", zones.describe(loaded.mask));
}

/// What to load at start: `--zone N|NAME`, `--bsps 0,3,5`, `--all-bsps`,
/// else the zone set the start lies in (the first on campaign maps), else
/// everything.
pub fn initial(args: &[String], zones: &Zones) -> anyhow::Result<LoadedBsps> {
    let mut loaded = LoadedBsps::default();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
    if args.iter().any(|a| a == "--all-bsps") {
        load_bsps(&mut loaded, zones, u32::MAX);
    } else if let Some(list) = value("--bsps") {
        let mut mask = 0u32;
        for b in list.split(',') {
            let b: u32 = b.trim().parse()?;
            anyhow::ensure!(b < 32, "--bsps: {b} is not a BSP index");
            mask |= 1 << b;
        }
        load_bsps(&mut loaded, zones, mask);
    } else if let Some(z) = value("--zone") {
        let sets = &zones.layout.zone_sets;
        let n = match z.parse::<usize>() {
            Ok(n) => n,
            Err(_) => sets
                .iter()
                .position(|s| s.name == *z)
                .ok_or_else(|| anyhow::anyhow!("--zone: no zone set named {z}"))?,
        };
        anyhow::ensure!(n < sets.len(), "--zone: the map has {} zone sets", sets.len());
        load_zone_set(&mut loaded, zones, n);
    } else if let Some(n) = zones.layout.start_zone_set() {
        load_zone_set(&mut loaded, zones, n);
    } else {
        load_bsps(&mut loaded, zones, u32::MAX);
    }
    if loaded.mask == 0 {
        // A zone set naming no decoded BSP would show nothing at all.
        load_bsps(&mut loaded, zones, u32::MAX);
    }
    Ok(loaded)
}

/// Whether the collision shell is shown (V).
#[derive(Resource, Default)]
pub struct ShowCollision(pub bool);

pub struct ZonesPlugin;

impl Plugin for ZonesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Zones>()
            .init_resource::<LoadedBsps>()
            .init_resource::<ShowCollision>()
            .add_systems(Update, (zone_keys, follow_cluster).chain().in_set(ZoneKeys));
    }
}

/// The zone keys and the camera's cluster, before streaming.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ZoneKeys;

fn zone_keys(keys: Res<ButtonInput<KeyCode>>, zones: Res<Zones>, mut loaded: ResMut<LoadedBsps>) {
    let count = zones.layout.zone_sets.len();
    if keys.just_pressed(KeyCode::Home) {
        load_bsps(&mut loaded, &zones, u32::MAX);
    } else if count > 0 && keys.just_pressed(KeyCode::PageDown) {
        let n = loaded.zone_set.map_or(0, |n| (n + 1) % count);
        load_zone_set(&mut loaded, &zones, n);
    } else if count > 0 && keys.just_pressed(KeyCode::PageUp) {
        let n = loaded.zone_set.map_or(count - 1, |n| (n + count - 1) % count);
        load_zone_set(&mut loaded, &zones, n);
    }
}

/// Halo picks the sky and the air by the cluster the camera is in. Indoors
/// (no sky) the last sky stays. Outside every cluster box, or in a cluster
/// naming no air, the air most loaded clusters name.
fn follow_cluster(
    zones: Res<Zones>,
    mut loaded: ResMut<LoadedBsps>,
    mut air: ResMut<CameraAir>,
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
