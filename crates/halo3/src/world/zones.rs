//! Which structure BSPs are loaded: zone sets.
//!
//! Every BSP of the map is decoded and put on the GPU at start (120_halo's
//! eleven take about two seconds); what changes is which are shown. Each
//! world batch carries a [`BspMember`] saying which BSPs it belongs to, and
//! [`LoadedBsps`] says which are loaded. [`load_zone_set`] and [`load_bsps`]
//! are the whole API: change the resource and [`apply_visibility`] follows,
//! the skate world rebuilds from the loaded BSPs' collision (`skate.rs`),
//! and the sky follows the camera's cluster ([`follow_sky`]).
//!
//! Keys: PageDown / PageUp step to the next / previous zone set, Home loads
//! every BSP at once. `docs/format/zone-sets.md` has the tag layout.

use bevy::prelude::*;

use blam_geometry::{Kind, Scene};

use super::CollisionOverlay;
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
    pub mask: u32,
    pub zone_set: Option<usize>,
    pub sky: Option<u32>,
}

impl LoadedBsps {
    pub fn shows(&self, m: &BspMember) -> bool {
        (m.bsps == 0 || m.bsps & self.mask != 0) && (m.sky.is_none() || m.sky == self.sky)
    }
}

/// Loads zone set `n` (BSPs only; nothing is unloaded from the GPU).
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
            .add_systems(Update, (zone_keys, follow_sky, apply_visibility).chain());
    }
}

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

/// Halo picks the sky by the cluster the camera is in; indoors (no sky)
/// the last one stays.
fn follow_sky(
    zones: Res<Zones>,
    mut loaded: ResMut<LoadedBsps>,
    cameras: Query<&GlobalTransform, With<FlyCamera>>,
) {
    if zones.skies.len() < 2 {
        return;
    }
    let Ok(camera) = cameras.single() else {
        return;
    };
    let at = super::to_halo(camera.translation());
    let Some(sky) = zones.layout.sky_at(at, loaded.mask) else {
        return;
    };
    if loaded.sky != Some(sky) && zones.skies.iter().any(|(s, _)| *s == sky) {
        loaded.sky = Some(sky);
        info!("sky {sky}");
    }
}

fn apply_visibility(
    loaded: Res<LoadedBsps>,
    show: Res<ShowCollision>,
    mut members: Query<(&BspMember, &mut Visibility, Has<CollisionOverlay>)>,
    added: Query<(), Added<BspMember>>,
) {
    if !loaded.is_changed() && !show.is_changed() && added.is_empty() {
        return;
    }
    for (m, mut v, overlay) in &mut members {
        let shown = loaded.shows(m) && (!overlay || show.0);
        let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}
