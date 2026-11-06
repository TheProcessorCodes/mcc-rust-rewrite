//! Squads in and out: the models onto the GPU once, actors spawned at
//! their starting locations (by script command or by the stand-in
//! placement, `AutoPlace`), erased when their BSPs unload, and the census
//! scripts read.

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;

use blam_geometry::ai::{Cell, StartingLocation};

use mcc_core::ai::{ActorActivity, ActorSkeleton, ActorTag, AiBridge, AiCensus, AiCommand, AiRef, Allegiances, AutoPlace};
use mcc_core::coords::{to_halo, to_render};
use mcc_core::fly_camera::FlyCamera;
use mcc_core::zones::LoadedBsps;
use mcc_weapons::Armory;

use crate::brain::Brain;
use crate::model::{self, ModelLook};
use crate::{AiAssets, AiSet, PendingAi, WU};

pub(crate) struct PlacePlugin;

impl Plugin for PlacePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(mcc_core::map::MapStartup, setup).add_systems(
            Update,
            (dev_place, bridge_in, commands_in, auto_place, unload, census, bridge_out)
                .chain()
                .in_set(AiSet::Place)
                .run_if(mcc_core::pause::running)
                .run_if(mcc_core::map::ready),
        );
    }
}

/// The map's AI on the GPU, and which squads were placed.
#[derive(Resource)]
pub(crate) struct Ai {
    pub assets: AiAssets,
    pub looks: Vec<ModelLook>,
    /// Squads the stand-in placement has placed (never twice).
    pub auto_placed: Vec<bool>,
    /// Actors ever placed per squad.
    pub placed: Vec<u32>,
    /// Time of the last automatic check.
    pub next_check: f32,
}

/// A weapon in an actor's hands: the armory kind, and its model entity.
pub(crate) struct Gun {
    pub kind: usize,
    pub model: Option<Entity>,
    pub materials: Vec<(Handle<StandardMaterial>, mcc_weapons::assets::LookMaterial)>,
}

/// One actor. Positions in the renderer frame (metres, Y up).
#[derive(Component)]
pub(crate) struct Actor {
    pub kind: usize,
    pub squad: usize,
    pub team: i16,
    /// Feet (for flyers: the bottom of the body).
    pub position: Vec3,
    /// Facing about +Y, the on-foot heading convention (0 looks down +Z).
    pub heading: f32,
    pub velocity: Vec3,
    /// The ground body for walkers, made once there is collision.
    pub body: Option<mcc_foot::physics::Spartan>,
    pub flying: bool,
    pub health: f32,
    pub shield: f32,
    pub max_shield: f32,
    pub since_hit: f32,
    /// Seconds since death.
    pub dead: Option<f32>,
    pub gun: Option<Gun>,
    pub brain: Brain,
    pub head_joint: Option<Entity>,
    pub materials: Vec<(Handle<StandardMaterial>, usize)>,
    pub lit_at: Option<Vec3>,
    pub model: Option<usize>,
}

impl Actor {
    pub fn alive(&self) -> bool {
        self.dead.is_none()
    }
}

/// On-foot heading for a Halo yaw.
pub(crate) fn heading_of_yaw(yaw: f32) -> f32 {
    let d = mcc_core::coords::to_render_dir(Vec3::new(yaw.cos(), yaw.sin(), 0.0));
    d.x.atan2(d.z)
}

/// Halo yaw for an on-foot heading.
pub(crate) fn yaw_of_heading(heading: f32) -> f32 {
    let d = mcc_core::coords::to_halo_dir(Vec3::new(heading.sin(), 0.0, heading.cos()));
    d.y.atan2(d.x)
}

/// The actor's root transform: Halo model space (wu, +Z up, facing +X) at
/// its feet, turned to its heading, in the renderer frame. `tilt` tips it
/// over (death), radians about its own Y.
pub(crate) fn root_transform(position: Vec3, heading: f32, tilt: f32) -> Transform {
    let axes = Mat4::from_cols(Vec4::X * WU, Vec4::NEG_Z * WU, Vec4::Y * WU, Vec4::W);
    let m = Mat4::from_translation(position)
        * axes
        * Mat4::from_rotation_z(yaw_of_heading(heading))
        * Mat4::from_rotation_y(-tilt);
    Transform::from_matrix(m)
}

fn setup(
    mut commands: Commands,
    pending: Option<ResMut<PendingAi>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut binds: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut vitality: ResMut<mcc_core::ai::PlayerVitality>,
) {
    let Some(assets) = pending.and_then(|mut p| p.0.take()) else {
        return;
    };
    let looks = assets
        .models
        .iter()
        .map(|m| model::upload(m, &mut meshes, &mut images, &mut binds))
        .collect();
    if let Some((body, shield)) = assets.player_vitality {
        vitality.max_body = body;
        vitality.max_shield = shield;
        vitality.body = body;
        vitality.shield = shield;
    }
    vitality.god = std::env::var("MCC_GOD").is_ok_and(|v| v == "1");
    match assets.splatter.as_ref() {
        Some(c) => {
            let s = crate::player::Splatter::from_tags(c);
            eprintln!(
                "ai: bike splatter {:.0} damage ({} x {}), from {:.1} to {:.1} wu/s ({})",
                s.damage, c.damage[1], c.scale, s.speeds[0], s.speeds[1], c.vehicle_tag
            );
            commands.insert_resource(s);
        }
        None => eprintln!("ai: no Mongoose collision damage on this map; the bike's splatter uses our own numbers"),
    }
    let n = assets.data.squads.len();
    commands.insert_resource(Ai {
        assets,
        looks,
        auto_placed: vec![false; n],
        placed: vec![0; n],
        next_check: 0.0,
    });
}

/// The squads an [`AiRef`] names.
fn squads_of(ai: &Ai, who: &AiRef) -> Vec<usize> {
    let data = &ai.assets.data;
    match who {
        AiRef::All => (0..data.squads.len()).collect(),
        AiRef::Squad(i) => (*i < data.squads.len()).then_some(*i).into_iter().collect(),
        AiRef::Named(name) => {
            // A squad's or group's own name (Halo CE's `encounter/squad`
            // and `encounter/platoon`), else the `squad/cell` and
            // `squad.location` forms name the squad.
            if let Some(s) = data.squad(name) {
                return vec![s.index];
            }
            if let Some(g) = data.group(name) {
                return data.squads_in_group(g);
            }
            let base = name.split(['/', '.']).next().unwrap_or(name);
            match data.squad(base) {
                Some(s) => vec![s.index],
                None => data.group(base).map_or_else(Vec::new, |g| data.squads_in_group(g)),
            }
        }
    }
}

/// Places squad `s`: each cell's count of actors at its starting locations
/// (cycling through them), at most `limit` actors in all.
#[allow(clippy::too_many_arguments)]
pub(crate) fn place_squad(
    commands: &mut Commands,
    ai: &mut Ai,
    armory: Option<&Armory>,
    materials: &mut Assets<StandardMaterial>,
    s: usize,
    limit: Option<u32>,
) -> u32 {
    let mut placed = 0;
    let squad = ai.assets.data.squads[s].clone();
    for cell in &squad.cells {
        if cell.locations.is_empty() {
            continue;
        }
        let count = cell.count.max(0) as usize;
        for i in 0..count {
            if limit.is_some_and(|l| placed >= l) {
                break;
            }
            let loc = &cell.locations[i % cell.locations.len()];
            if spawn_actor(commands, ai, armory, materials, s, cell, loc).is_some() {
                placed += 1;
            }
        }
    }
    ai.placed[s] += placed;
    placed
}

/// One actor at `loc`. `None` when its character has no unit or model.
fn spawn_actor(
    commands: &mut Commands,
    ai: &Ai,
    armory: Option<&Armory>,
    materials: &mut Assets<StandardMaterial>,
    s: usize,
    cell: &Cell,
    loc: &StartingLocation,
) -> Option<Entity> {
    let data = &ai.assets.data;
    let char_index = loc.character.or(cell.character)? as usize;
    let kind = ai.assets.kinds.get(char_index)?.as_ref()?;
    let squad = &data.squads[s];
    let team = if squad.team > 0 { squad.team } else { kind.unit.team };
    let position = to_render(loc.position);
    let heading = heading_of_yaw(loc.facing[0]);
    let flying = kind.unit.flies;
    let light = Vec3::splat(0.6);
    let root = commands
        .spawn((
            root_transform(position, heading, 0.0),
            Visibility::Visible,
            Name::new(format!("actor {} ({})", squad.name, kind.character.tag_path.rsplit('\\').next().unwrap_or(""))),
            ActorTag {
                squad: s,
                squad_name: squad.name.clone(),
                character: char_index,
                team,
            },
            ActorActivity {
                flying,
                yaw: loc.facing[0],
                ..default()
            },
        ))
        .id();
    let (joints, mats, names, bind) = match kind.model.and_then(|m| ai.looks.get(m)) {
        Some(look) => {
            let b = model::spawn_body(commands, look, materials, root, light);
            (b.joints, b.materials, look.names.clone(), look.bind.clone())
        }
        None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
    };
    let model_data = kind.model.and_then(|m| ai.assets.models.get(m));
    let head_joint = model_data
        .and_then(|m| m.node("head"))
        .and_then(|i| joints.get(i).copied());
    // The weapon: the location's, else the cell's, else the unit's own.
    let weapon_tag = data
        .weapon_of(cell, Some(loc))
        .or_else(|| kind.unit.weapons.first().copied());
    let gun = weapon_tag.and_then(|tag| {
        let armory = armory?;
        let k = armory.kind_of_tag(tag)?;
        // In the right hand: grip on the `right_hand` marker's frame.
        let hand = model_data.and_then(|m| {
            let marker = m.marker("right_hand")?;
            Some((joints.get(marker.node as usize).copied()?, marker.local()))
        });
        let (model, materials) = match hand {
            Some((joint, local)) => {
                let (e, mats) = armory.spawn_child(commands, materials, k, joint, local);
                (Some(e), mats)
            }
            None => (None, Vec::new()),
        };
        Some(crate::place::Gun { kind: k, model, materials })
    });
    commands.entity(root).insert((
        ActorSkeleton {
            joints,
            names,
            bind,
            animations: kind.unit.animations,
            render_model: kind.unit.render_model,
            posed_externally: false,
        },
        Actor {
            kind: char_index,
            squad: s,
            team,
            position,
            heading,
            velocity: Vec3::ZERO,
            body: None,
            flying,
            health: kind.body.max(1.0),
            shield: kind.shield.max(0.0),
            max_shield: kind.shield.max(0.0),
            since_hit: 100.0,
            dead: None,
            gun,
            brain: Brain::new((s as u32).wrapping_mul(7919).wrapping_add(root.to_bits() as u32)),
            head_joint,
            materials: mats,
            lit_at: None,
            model: kind.model,
        },
    ));
    Some(root)
}

/// `MCC_AI_PLACE=squad,squad,...` places those squads (or groups) as play
/// begins, whatever the scripts do: a developer's hook for headless shots
/// of a fight (`--shot`), not used by the game.
fn dev_place(ai: Option<Res<Ai>>, mut done: Local<bool>, mut out: MessageWriter<AiCommand>) {
    if *done || ai.is_none() {
        return;
    }
    *done = true;
    let Ok(list) = std::env::var("MCC_AI_PLACE") else {
        return;
    };
    for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        info!("ai: MCC_AI_PLACE places {name}");
        out.write(AiCommand::Place {
            who: AiRef::named(name),
            count: None,
        });
    }
}

/// Script commands: place, erase, kill, allegiances.
#[allow(clippy::too_many_arguments)]
fn commands_in(
    mut commands: Commands,
    ai: Option<ResMut<Ai>>,
    armory: Option<Res<Armory>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut reader: MessageReader<AiCommand>,
    mut actors: Query<(Entity, &mut Actor)>,
    mut allegiances: ResMut<Allegiances>,
) {
    let Some(mut ai) = ai else {
        reader.clear();
        return;
    };
    for cmd in reader.read() {
        match cmd {
            AiCommand::Place { who, count } => {
                // `squad/location` (`arbiter_trench/trench`): one actor at
                // that starting location when the squad has one by that name.
                let at = match who {
                    AiRef::Named(n) => n.split_once(['/', '.']).map(|(_, l)| l.to_string()),
                    _ => None,
                };
                for s in squads_of(&ai, who) {
                    if let Some(name) = &at
                        && let Some((c, l)) = ai.assets.data.squads[s].cells.iter().enumerate().find_map(|(c, cell)| {
                            cell.locations.iter().position(|l| l.name == *name).map(|l| (c, l))
                        })
                    {
                        let squad = ai.assets.data.squads[s].clone();
                        let (cell, loc) = (&squad.cells[c], &squad.cells[c].locations[l]);
                        let n = u32::from(spawn_actor(&mut commands, &ai, armory.as_deref(), &mut materials, s, cell, loc).is_some());
                        ai.placed[s] += n;
                        info!("ai: placed {} at {name} ({n} actors)", squad.name);
                        continue;
                    }
                    let n = place_squad(&mut commands, &mut ai, armory.as_deref(), &mut materials, s, *count);
                    info!("ai: placed {} ({n} actors)", ai.assets.data.squads[s].name);
                }
            }
            AiCommand::Erase { who } => {
                let list = squads_of(&ai, who);
                for (e, a) in &actors {
                    if list.contains(&a.squad) {
                        commands.entity(e).despawn();
                    }
                }
            }
            AiCommand::Kill { who } => {
                let list = squads_of(&ai, who);
                for (_, mut a) in &mut actors {
                    if list.contains(&a.squad) && a.alive() {
                        a.health = 0.0;
                        a.dead = Some(0.0);
                    }
                }
            }
            AiCommand::Allegiance { a, b, allied } => allegiances.set(*a, *b, *allied),
        }
    }
}

/// The stand-in placement while scripts do not place squads.
#[allow(clippy::too_many_arguments)]
fn auto_place(
    mut commands: Commands,
    ai: Option<ResMut<Ai>>,
    armory: Option<Res<Armory>>,
    mode: Res<AutoPlace>,
    loaded: Res<LoadedBsps>,
    player: Res<mcc_core::ai::PlayerBody>,
    cameras: Query<&Transform, With<FlyCamera>>,
    actors: Query<&Actor>,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mut ai) = ai else {
        return;
    };
    if *mode == AutoPlace::Off || loaded.shown == 0 {
        return;
    }
    // Twice a second is plenty.
    let now = time.elapsed_secs();
    if now < ai.next_check {
        return;
    }
    ai.next_check = now + 0.5;
    let shown = loaded.shown;
    let viewer = player
        .0
        .map(|b| b.feet)
        .or_else(|| cameras.iter().next().map(|t| t.translation));
    let living = actors.iter().filter(|a| a.alive()).count() as u32;
    let data = &ai.assets.data;
    let mut wanted: Vec<usize> = Vec::new();
    for s in &data.squads {
        if ai.auto_placed[s.index] || !auto_squad(&ai, s.index) {
            continue;
        }
        let bsps = data.squad_bsps(s);
        if bsps == 0 || bsps & !shown != 0 {
            continue;
        }
        match *mode {
            AutoPlace::ZoneSet => wanted.push(s.index),
            AutoPlace::Nearby { radius, max_living } => {
                let Some(v) = viewer else {
                    continue;
                };
                if living + wanted.iter().map(|w| data.squads[*w].actor_count() as u32).sum::<u32>() >= max_living {
                    break;
                }
                let near = s
                    .cells
                    .iter()
                    .flat_map(|c| &c.locations)
                    .any(|l| to_render(l.position).distance(v) < radius);
                if near {
                    wanted.push(s.index);
                }
            }
            AutoPlace::Off => {}
        }
    }
    for s in wanted {
        ai.auto_placed[s] = true;
        let n = place_squad(&mut commands, &mut ai, armory.as_deref(), &mut materials, s, None);
        if n > 0 {
            let at = viewer.map(to_halo).unwrap_or_default();
            info!(
                "ai: placed {} ({n} actors) near [{:.1}, {:.1}, {:.1}]",
                ai.assets.data.squads[s].name, at.x, at.y, at.z
            );
        }
    }
}

/// Whether the stand-in placement takes a squad: it must place at least
/// one drawable actor, and not be a test, an easter egg, a vehicle crew
/// (vehicles are not built yet) or one of the finale's scripted waves
/// (`*_finale_*`, `*_boss*`, `*reinforcements*`: 60-odd squads the
/// mission sends in one after another round the ziggurat).
fn auto_squad(ai: &Ai, s: usize) -> bool {
    let squad = &ai.assets.data.squads[s];
    let name = squad.name.as_str();
    if ["test", "egg", "warthog", "chopper", "finale", "boss", "reinforcements"].iter().any(|w| name.contains(w)) {
        return false;
    }
    squad.cells.iter().any(|c| {
        c.vehicle.is_none()
            && c.count > 0
            && !c.locations.is_empty()
            && c.character
                .and_then(|i| ai.assets.kinds.get(i as usize))
                .is_some_and(|k| k.as_ref().is_some_and(|k| k.model.is_some()))
    })
}

/// Actors of squads whose BSPs are no longer loaded go, as Halo erases AI
/// in unloaded BSPs. The stand-in placement may place them again later.
fn unload(
    mut commands: Commands,
    ai: Option<ResMut<Ai>>,
    loaded: Res<LoadedBsps>,
    actors: Query<(Entity, &Actor)>,
) {
    let Some(mut ai) = ai else {
        return;
    };
    if !loaded.is_changed() || loaded.mask == 0 {
        return;
    }
    let mask = loaded.mask;
    for (e, a) in &actors {
        let bsps = ai.assets.data.squad_bsps(&ai.assets.data.squads[a.squad]);
        if bsps != 0 && bsps & mask == 0 {
            commands.entity(e).despawn();
            ai.auto_placed[a.squad] = false;
        }
    }
}

/// Living actors per squad and group, for scripts.
fn census(ai: Option<Res<Ai>>, actors: Query<&Actor>, mut census: ResMut<AiCensus>) {
    let Some(ai) = ai else {
        return;
    };
    let data = &ai.assets.data;
    census.living.clear();
    census.total_living = 0;
    for a in actors.iter().filter(|a| a.alive()) {
        census.total_living += 1;
        let squad = &data.squads[a.squad];
        *census.living.entry(squad.name.clone()).or_default() += 1;
        let mut g = squad.group;
        let mut guard = 0;
        while let Some(i) = g {
            let Some(group) = data.squad_groups.get(i as usize) else {
                break;
            };
            *census.living.entry(group.name.clone()).or_default() += 1;
            g = group.parent;
            guard += 1;
            if guard > 32 {
                break;
            }
        }
    }
    if ai.is_changed() {
        census.placed.clear();
        for (s, n) in ai.placed.iter().enumerate().filter(|(_, n)| **n > 0) {
            let squad = &data.squads[s];
            *census.placed.entry(squad.name.clone()).or_default() += n;
            let mut g = squad.group;
            let mut guard = 0;
            while let Some(i) = g {
                let Some(group) = data.squad_groups.get(i as usize) else {
                    break;
                };
                *census.placed.entry(group.name.clone()).or_default() += n;
                g = group.parent;
                guard += 1;
                if guard > 32 {
                    break;
                }
            }
        }
    }
}

/// Commands queued on the script bridge become messages.
fn bridge_in(bridge: Option<Res<AiBridge>>, mut out: MessageWriter<AiCommand>) {
    let Some(bridge) = bridge else {
        return;
    };
    let queued = bridge.0.lock().map(|mut s| std::mem::take(&mut s.queue)).unwrap_or_default();
    for c in queued {
        out.write(c);
    }
}

/// The census copied to the script bridge.
fn bridge_out(bridge: Option<Res<AiBridge>>, census: Res<AiCensus>) {
    let Some(bridge) = bridge else {
        return;
    };
    if let Ok(mut s) = bridge.0.lock() {
        s.living.clone_from(&census.living);
        s.placed.clone_from(&census.placed);
        s.total_living = census.total_living;
    }
}
