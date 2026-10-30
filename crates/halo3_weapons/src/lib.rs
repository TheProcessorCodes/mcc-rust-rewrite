//! Weapons: the map's guns lying where the scenario puts them, the one in
//! the Spartan's hands, and shooting it (`assets.rs` for the models,
//! `fire.rs` for rounds in flight and their marks, `hud.rs` for the
//! crosshair and ammo count).
//!
//! On foot (`halo3_foot`) the Spartan starts with the first of
//! `halo3_foot::STARTING_WEAPONS` the map has: the sniper rifle. Walking
//! over a weapon he does not carry takes it into a free hand; over one he
//! carries, takes its ammo. E (X or RB on a pad) swaps the weapon in his
//! hands for the one at his feet, dropping his own.
//!
//! | controller | keyboard and mouse | |
//! |---|---|---|
//! | RT | left button | fire |
//! | right stick click | right button | zoom (weapons with a scope) |
//! | X / RB | R | reload |
//! | X / RB, by a weapon | E | pick it up (swap) |
//! | Y | Tab, wheel, 1 / 2 | switch weapons |
//!
//! The numbers are the tags': magazine sizes and spare rounds, rounds per
//! second, bursts, spread, projectile speeds and ranges, zoom.
//! Not from the tags (`docs/format/weapons.md` says why): reload times,
//! the shotgun's pellet count, which rounds fall (grenades), battery use,
//! the charge times. Rounds damage actors and, fired by actors, the
//! Spartan (`fire.rs`, `halo3_ai`).

pub mod assets;
mod fire;
mod hud;

use bevy::input::gamepad::GamepadButton as B;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;

pub use halo3_core::weapons::{WeaponAction, WeaponEvent, WeaponImpact};
pub use fire::Shots;

use halo3_core::coords::{to_halo, to_render};
use halo3_core::skate::RiderPose;
use halo3_foot::OnFoot;
use assets::{Arsenal, Kind, Look};

/// The map's weapons, handed over by `main`.
#[derive(Resource)]
pub struct PendingArsenal(pub Option<Arsenal>);

pub struct WeaponsPlugin;

impl Plugin for WeaponsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<WeaponEvent>()
            .add_message::<WeaponImpact>()
            .init_resource::<Inventory>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    (arm, pick_up, handle, trigger).chain().run_if(halo3_core::pause::running),
                    place_held,
                    light_items,
                    respawn_items.run_if(halo3_core::pause::running),
                )
                    .chain()
                    .after(halo3_foot::walk),
            )
            .add_plugins((fire::FirePlugin, hud::HudPlugin));
    }
}

/// World units to metres.
const WU: f32 = blam_geometry::WORLD_UNIT_METRES;

/// Halo world frame (wu, Z up) to the renderer's (metres, Y up).
fn halo_to_render() -> Mat4 {
    Mat4::from_cols(Vec4::X * WU, Vec4::NEG_Z * WU, Vec4::Y * WU, Vec4::W)
}

/// The weapons' models on the GPU and their numbers.
#[derive(Resource)]
pub struct Armory {
    pub kinds: Vec<Kind>,
    looks: Vec<Look>,
    /// The kinds a Spartan starts with.
    loadout: Vec<usize>,
}

impl Armory {
    /// The kind made from `weap` tag `tag`, if the map's weapons include it
    /// (`assets::load`'s `extra` adds the AI's).
    pub fn kind_of_tag(&self, tag: u32) -> Option<usize> {
        self.kinds.iter().position(|k| k.weapon.tag_index == tag)
    }

    /// A copy of weapon kind `kind` as a child of `parent`, placed in the
    /// parent's frame by `grip_to_parent` (grip space, world units, to the
    /// parent's frame). Its materials come back for lighting
    /// ([`assets::LookMaterial::lit`]).
    pub fn spawn_child(
        &self,
        commands: &mut Commands,
        materials: &mut Assets<StandardMaterial>,
        kind: usize,
        parent: Entity,
        grip_to_parent: Mat4,
    ) -> (Entity, Vec<(Handle<StandardMaterial>, assets::LookMaterial)>) {
        let (e, mats) = spawn_copy(commands, &self.looks[kind], materials, grip_to_parent, Vec3::ONE);
        commands.entity(e).insert(ChildOf(parent));
        (e, mats)
    }
}

/// A weapon lying in the world.
struct Item {
    kind: usize,
    entity: Entity,
    /// Halo world frame of the model (placement), and where it is.
    at: Vec3,
    /// Its ammo, if dropped by the player (placed weapons come full).
    ammo: Option<Ammo>,
    /// The scenario placement it came from, and when it comes back once
    /// taken (seconds of game time; 0: never).
    placement: Option<usize>,
    lit: bool,
    materials: Vec<(Handle<StandardMaterial>, assets::LookMaterial)>,
}

/// Rounds in a weapon, or its battery.
#[derive(Clone, Copy, Debug)]
pub struct Ammo {
    pub loaded: i32,
    pub reserve: i32,
    /// 0..1 for energy weapons (no magazine).
    pub energy: f32,
}

/// A weapon in a hand.
#[derive(Clone, Debug)]
pub struct Held {
    pub kind: usize,
    pub ammo: Ammo,
    /// Seconds until the next round may leave.
    cooldown: f32,
    /// Rounds left in the current burst.
    burst: i32,
    /// Seconds left of a reload.
    pub reload: f32,
    /// Seconds the trigger has been held charging.
    pub charge: f32,
    /// Zoom level (0: none).
    pub zoom: usize,
    /// Extra spread from firing, 0..1 (towards the barrel's widest).
    bloom: f32,
}

/// What the Spartan carries.
#[derive(Resource, Default)]
pub struct Inventory {
    pub slots: [Option<Held>; 2],
    pub active: usize,
    /// He has been given the loadout.
    armed: bool,
    /// The weapon entity in his hands: kind and its materials.
    shown: Option<(Entity, usize, Vec<(Handle<StandardMaterial>, assets::LookMaterial)>)>,
    /// Light at the held weapon, and where it was sampled.
    lit_at: Option<Vec3>,
    /// What the HUD says at the bottom: a pickup prompt.
    pub prompt: Option<String>,
    /// Seconds of game time, for respawns.
    clock: f64,
}

impl Inventory {
    pub fn current(&self) -> Option<&Held> {
        self.slots[self.active].as_ref()
    }
}

#[derive(Resource, Default)]
struct Items(Vec<Item>);

fn setup(
    mut commands: Commands,
    pending: Option<ResMut<PendingArsenal>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(arsenal) = pending.and_then(|mut p| p.0.take()) else {
        return;
    };
    let looks = assets::upload(&arsenal, &mut meshes, &mut images);
    let mut items = Items::default();
    for (n, (kind, placement)) in arsenal.placed.iter().enumerate() {
        let model = halo_to_render() * placement.transform() * arsenal.kinds[*kind].to_grip.inverse();
        let (entity, mats) = spawn_copy(&mut commands, &looks[*kind], &mut materials, model, Vec3::ONE);
        items.0.push(Item {
            kind: *kind,
            entity,
            at: placement.position,
            ammo: None,
            placement: Some(n),
            lit: false,
            materials: mats,
        });
    }
    commands.insert_resource(items);
    commands.insert_resource(Placements(arsenal.placed.iter().map(|(k, p)| (*k, p.clone(), 0.0)).collect()));
    commands.insert_resource(Armory {
        kinds: arsenal.kinds,
        looks,
        loadout: arsenal.loadout,
    });
}

/// The scenario's placements: kind, placement, and when a taken one comes
/// back (game seconds; 0: it is there or never comes back).
#[derive(Resource)]
struct Placements(Vec<(usize, blam_geometry::weapons::ItemPlacement, f64)>);

/// One copy of a weapon kind: a parent at `model` (grip space to the
/// renderer frame) with a child per mesh piece, each with its own
/// material lit by `light`.
fn spawn_copy(
    commands: &mut Commands,
    look: &Look,
    materials: &mut Assets<StandardMaterial>,
    model: Mat4,
    light: Vec3,
) -> (Entity, Vec<(Handle<StandardMaterial>, assets::LookMaterial)>) {
    let mut mats = Vec::new();
    let parent = commands
        .spawn((Transform::from_matrix(model), Visibility::Visible, Name::new("weapon")))
        .with_children(|p| {
            for (mesh, look) in &look.pieces {
                let m = materials.add(look.lit(light));
                mats.push((m.clone(), look.clone()));
                p.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(m), Transform::IDENTITY));
            }
        })
        .id();
    (parent, mats)
}

fn new_held(kind: &Kind, k: usize) -> Held {
    let mag = kind.weapon.magazines.first();
    let ammo = match mag {
        Some(m) => {
            let loaded = (m.rounds_loaded_maximum as i32).min(m.rounds_total_initial as i32).max(0);
            Ammo {
                loaded,
                reserve: (m.rounds_total_initial as i32 - loaded).max(0),
                energy: 1.0,
            }
        }
        None => Ammo {
            loaded: 0,
            reserve: 0,
            energy: 1.0,
        },
    };
    Held {
        kind: k,
        ammo,
        cooldown: 0.0,
        burst: 0,
        reload: 0.0,
        charge: 0.0,
        zoom: 0,
        bloom: 0.0,
    }
}

/// Gives the loadout on first stepping out; clears the zoom off foot.
fn arm(mut inv: ResMut<Inventory>, armory: Option<Res<Armory>>, mut foot: ResMut<OnFoot>, mut events: MessageWriter<WeaponEvent>) {
    let Some(armory) = armory else {
        return;
    };
    let Some(spartan) = foot.spartan.as_ref() else {
        foot.zoom = None;
        return;
    };
    if !inv.armed {
        inv.armed = true;
        for (slot, &k) in armory.loadout.iter().take(1).enumerate() {
            inv.slots[slot] = Some(new_held(&armory.kinds[k], k));
        }
        inv.active = 0;
        if let Some(h) = inv.current() {
            events.write(WeaponEvent {
                weapon: armory.kinds[h.kind].weapon.tag_index,
                action: WeaponAction::Ready,
                at: spartan.eye(),
                own: true,
            });
        }
    }
}

/// How close (metres, from the feet) a weapon must lie to be picked up.
const REACH: f32 = 1.3;

/// Walking over weapons: a free hand takes a new kind, a carried kind
/// gives its ammo; E swaps for the nearest.
#[allow(clippy::too_many_arguments)]
fn pick_up(
    mut commands: Commands,
    mut inv: ResMut<Inventory>,
    armory: Option<Res<Armory>>,
    items: Option<ResMut<Items>>,
    mut placements: Option<ResMut<Placements>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    foot: Res<OnFoot>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    mut events: MessageWriter<WeaponEvent>,
) {
    inv.clock += time.delta_secs_f64();
    inv.prompt = None;
    let (Some(armory), Some(mut items), Some(spartan)) = (armory, items, foot.spartan.as_ref()) else {
        return;
    };
    let feet = spartan.position;
    let near = |item: &Item| to_render(item.at).distance(feet + Vec3::Y * 0.3) < REACH;
    // Nearest weapon within reach.
    let Some(i) = items
        .0
        .iter()
        .enumerate()
        .filter(|(_, it)| near(it))
        .min_by(|a, b| {
            let d = |it: &Item| to_render(it.at).distance(feet);
            d(a.1).total_cmp(&d(b.1))
        })
        .map(|(i, _)| i)
    else {
        return;
    };
    let kind = items.0[i].kind;
    let full = |k: usize, ammo: &Ammo| {
        let m = armory.kinds[k].weapon.magazines.first();
        m.is_none_or(|m| ammo.loaded + ammo.reserve >= m.rounds_total_maximum as i32)
    };
    let carried = inv.slots.iter().position(|s| s.as_ref().is_some_and(|h| h.kind == kind));
    let ammo_in = |item: &Item| item.ammo.unwrap_or_else(|| new_held(&armory.kinds[kind], kind).ammo);
    let take = |items: &mut Items, placements: &mut Option<ResMut<Placements>>, i: usize, commands: &mut Commands, clock: f64| {
        let item = items.0.swap_remove(i);
        commands.entity(item.entity).despawn();
        if let (Some(n), Some(p)) = (item.placement, placements.as_mut()) {
            let respawn = p.0[n].1.respawn_seconds;
            p.0[n].2 = if respawn > 0 { clock + respawn as f64 } else { 0.0 };
        }
        item
    };
    let pad = gamepads.iter().next();
    let swap = keys.just_pressed(KeyCode::KeyE) || pad.is_some_and(|p| p.just_pressed(B::West) || p.just_pressed(B::RightTrigger));
    let at = spartan.eye();
    let tag = armory.kinds[kind].weapon.tag_index;
    let clock = inv.clock;
    match carried {
        // Ammo from a weapon he carries.
        Some(slot) => {
            let held = inv.slots[slot].as_mut().expect("carried");
            if full(kind, &held.ammo) || armory.kinds[kind].weapon.magazines.is_empty() {
                return;
            }
            let give = ammo_in(&items.0[i]);
            let max = armory.kinds[kind].weapon.magazines[0].rounds_total_maximum as i32;
            let room = (max - held.ammo.loaded - held.ammo.reserve).max(0);
            held.ammo.reserve += (give.loaded + give.reserve).min(room);
            take(&mut items, &mut placements, i, &mut commands, clock);
            info!("weapons: ammo from a {}", armory.kinds[kind].name());
        }
        None => {
            let free = inv.slots.iter().position(Option::is_none);
            match free {
                Some(slot) => {
                    let item = take(&mut items, &mut placements, i, &mut commands, clock);
                    let mut held = new_held(&armory.kinds[kind], kind);
                    if let Some(a) = item.ammo {
                        held.ammo = a;
                    }
                    inv.slots[slot] = Some(held);
                    info!("weapons: picked up a {}", armory.kinds[kind].name());
                    events.write(WeaponEvent { weapon: tag, action: WeaponAction::Ready, at, own: true });
                }
                None if swap => {
                    let item = take(&mut items, &mut placements, i, &mut commands, clock);
                    // The same press does not also reload (`handle`).
                    inv.prompt = Some(String::new());
                    let active = inv.active;
                    let dropped = inv.slots[active].take();
                    let mut held = new_held(&armory.kinds[kind], kind);
                    if let Some(a) = item.ammo {
                        held.ammo = a;
                    }
                    inv.slots[active] = Some(held);
                    info!("weapons: swapped for a {}", armory.kinds[kind].name());
                    events.write(WeaponEvent { weapon: tag, action: WeaponAction::Ready, at, own: true });
                    // His old weapon falls at his feet.
                    if let Some(old) = dropped {
                        let yaw = spartan.heading;
                        let halo_at = to_halo(feet + Vec3::Y * 0.05);
                        let lying = Mat4::from_translation(halo_at)
                            * Mat4::from_rotation_z(-yaw)
                            * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
                        let model = halo_to_render() * lying;
                        let (entity, mats) =
                            spawn_copy(&mut commands, &armory.looks[old.kind], &mut materials, model, Vec3::ONE);
                        items.0.push(Item {
                            kind: old.kind,
                            entity,
                            at: halo_at,
                            ammo: Some(old.ammo),
                            placement: None,
                            lit: false,
                            materials: mats,
                        });
                    }
                }
                None => {
                    inv.prompt = Some(format!("E / X: pick up the {}", armory.kinds[kind].name()));
                }
            }
        }
    }
}

/// Switching weapons, reloading, zooming.
fn handle(
    mut inv: ResMut<Inventory>,
    armory: Option<Res<Armory>>,
    mut foot: ResMut<OnFoot>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    mut events: MessageWriter<WeaponEvent>,
) {
    let Some(armory) = armory else {
        return;
    };
    let Some(at) = foot.spartan.as_ref().map(|s| s.eye()) else {
        return;
    };
    let inv = &mut *inv;
    let pad = gamepads.iter().next();
    let dt = time.delta_secs();
    let switch = keys.just_pressed(KeyCode::Tab)
        || scroll.delta.y.abs() > 0.0
        || pad.is_some_and(|p| p.just_pressed(B::North))
        || (keys.just_pressed(KeyCode::Digit1) && inv.active != 0)
        || (keys.just_pressed(KeyCode::Digit2) && inv.active != 1);
    if switch && inv.slots[1 - inv.active].is_some() {
        if let Some(h) = inv.slots[inv.active].as_mut() {
            h.reload = 0.0;
            h.charge = 0.0;
            h.zoom = 0;
            h.burst = 0;
        }
        inv.active = 1 - inv.active;
        if let Some(h) = inv.slots[inv.active].as_mut() {
            h.cooldown = h.cooldown.max(0.4);
            events.write(WeaponEvent {
                weapon: armory.kinds[h.kind].weapon.tag_index,
                action: WeaponAction::Ready,
                at,
                own: true,
            });
        }
    }
    let prompt_up = inv.prompt.is_some();
    let active = inv.active;
    let Some(held) = inv.slots[active].as_mut() else {
        foot.holding = None;
        foot.zoom = None;
        return;
    };
    let kind = &armory.kinds[held.kind];
    foot.holding = Some(kind.left_hand);

    // Reload: by hand, or on its own when the magazine runs dry.
    let wants = keys.just_pressed(KeyCode::KeyR)
        || (!prompt_up && pad.is_some_and(|p| p.just_pressed(B::West) || p.just_pressed(B::RightTrigger)));
    let mag = kind.weapon.magazines.first();
    if let Some(m) = mag {
        let can = held.reload <= 0.0 && held.ammo.reserve > 0 && held.ammo.loaded < m.rounds_loaded_maximum as i32;
        if can && (wants || held.ammo.loaded == 0) {
            held.reload = reload_time(kind);
            held.zoom = 0;
            held.burst = 0;
            events.write(WeaponEvent {
                weapon: kind.weapon.tag_index,
                action: WeaponAction::Reload,
                at,
                own: true,
            });
        }
        if held.reload > 0.0 {
            held.reload -= dt;
            if held.reload <= 0.0 {
                let per = if m.rounds_reloaded > 0 { m.rounds_reloaded as i32 } else { m.rounds_loaded_maximum as i32 };
                let put = per.min(m.rounds_loaded_maximum as i32 - held.ammo.loaded).min(held.ammo.reserve);
                held.ammo.loaded += put;
                held.ammo.reserve -= put;
                // Shells one at a time: carry on while there is room.
                held.reload = if per < m.rounds_loaded_maximum as i32
                    && held.ammo.loaded < m.rounds_loaded_maximum as i32
                    && held.ammo.reserve > 0
                {
                    0.5
                } else {
                    0.0
                };
            }
        }
    }

    // Zoom through the levels and back out.
    let zoom_pressed = mouse.just_pressed(MouseButton::Right) || pad.is_some_and(|p| p.just_pressed(B::RightThumb));
    let levels = kind.weapon.zoom_levels.max(0) as usize;
    if zoom_pressed && levels > 0 && held.reload <= 0.0 {
        held.zoom = (held.zoom + 1) % (levels + 1);
    }
    foot.zoom = (held.zoom > 0).then(|| zoom_power(kind, held.zoom));
}

/// Seconds to reload (whole magazine; per shell for a shotgun). Halo keeps
/// these in the weapon's animations, which are not read: a common figure.
fn reload_time(kind: &Kind) -> f32 {
    let m = kind.weapon.magazines.first();
    match m {
        Some(m) if m.rounds_reloaded > 0 && m.rounds_reloaded < m.rounds_loaded_maximum => 0.6,
        _ if kind.weapon.tag_path.contains("sniper") => 2.6,
        _ if kind.weapon.tag_path.contains("missile") || kind.weapon.tag_path.contains("rocket") => 3.0,
        _ => 2.1,
    }
}

/// Magnification at zoom level `level` (1-based): the tag's range from
/// its first level to its last.
fn zoom_power(kind: &Kind, level: usize) -> f32 {
    let levels = kind.weapon.zoom_levels.max(1) as usize;
    let [lo, hi] = kind.weapon.zoom_range;
    let t = if levels <= 1 { 0.0 } else { (level - 1) as f32 / (levels - 1) as f32 };
    (lo + (hi - lo) * t).max(1.25)
}

/// The trigger: when rounds leave (`fire::shoot`).
#[allow(clippy::too_many_arguments)]
fn trigger(
    mut inv: ResMut<Inventory>,
    armory: Option<Res<Armory>>,
    foot: Res<OnFoot>,
    rider: Res<RiderPose>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    windows: Query<&bevy::window::CursorOptions, With<bevy::window::PrimaryWindow>>,
    time: Res<Time>,
    mut shots: ResMut<fire::Shots>,
    mut events: MessageWriter<WeaponEvent>,
    mut auto_fire: Local<Option<bool>>,
) {
    let Some(armory) = armory else {
        return;
    };
    if foot.spartan.is_none() {
        return;
    }
    let dt = time.delta_secs();
    let pad = gamepads.iter().next();
    // The mouse fires only while the game has it (a click to take it back
    // does not shoot).
    let grabbed = windows.single().is_ok_and(|c| c.grab_mode != bevy::window::CursorGrabMode::None);
    // `HALO3_FOOT_FIRE=1`: hold the trigger (screenshot mode).
    let held_down = *auto_fire.get_or_insert_with(|| std::env::var("HALO3_FOOT_FIRE").is_ok_and(|v| v == "1"));
    let down = (grabbed && mouse.pressed(MouseButton::Left))
        || pad.is_some_and(|p| p.get(B::RightTrigger2).unwrap_or(0.0) > 0.4)
        || held_down;
    let pressed = (grabbed && mouse.just_pressed(MouseButton::Left))
        || shots.auto_pull(pad.is_some_and(|p| p.get(B::RightTrigger2).unwrap_or(0.0) > 0.4));
    let active = inv.active;
    let Some(held) = inv.slots[active].as_mut() else {
        return;
    };
    let kind = &armory.kinds[held.kind];
    held.cooldown -= dt;
    held.bloom = (held.bloom - dt * 1.5).max(0.0);
    if held_down && shots.launched > 0 && shots.launched % 10 == 0 {
        eprintln!(
            "weapons: {} rounds launched, {} hits; {} loaded, {} spare",
            shots.launched, shots.hits, held.ammo.loaded, held.ammo.reserve
        );
    }
    let Some(trigger) = kind.weapon.triggers.first() else {
        return;
    };
    let barrel_of = |i: Option<u16>| i.and_then(|i| kind.weapon.barrels.get(i as usize));
    let Some(barrel) = barrel_of(trigger.primary_barrel).or(kind.weapon.barrels.first()) else {
        return;
    };
    let (Some(weapon_world), Some((view, _))) = (foot.weapon_world, rider.camera) else {
        return;
    };
    let rate = barrel.rounds_per_second[1].max(barrel.rounds_per_second[0]).max(0.5);
    let interval = 1.0 / rate;
    let energy = kind.weapon.magazines.is_empty() || barrel.magazine.is_none();
    let has_round = |h: &Held| if energy { h.ammo.energy > 0.0 } else { h.ammo.loaded > 0 };
    let tag = kind.weapon.tag_index;
    let muzzle_at = (weapon_world * kind.muzzle).w_axis.truncate();
    let mut fire_now = |h: &mut Held, barrel: &blam_geometry::weapons::Barrel, power: f32, events: &mut MessageWriter<WeaponEvent>| {
        if !has_round(h) {
            events.write(WeaponEvent { weapon: tag, action: WeaponAction::Empty, at: muzzle_at, own: true });
            h.cooldown = 0.3;
            return;
        }
        if energy {
            h.ammo.energy = (h.ammo.energy - battery_use(kind, power)).max(0.0);
        } else {
            h.ammo.loaded = (h.ammo.loaded - barrel.rounds_per_shot.max(1) as i32).max(0);
        }
        let spread = barrel.error_angle[0] + (barrel.error_angle[1] - barrel.error_angle[0]) * h.bloom;
        shots.shoot(kind, barrel, weapon_world, view, spread.max(barrel.minimum_error), power);
        h.bloom = (h.bloom + 0.15).min(1.0);
        events.write(WeaponEvent { weapon: tag, action: WeaponAction::Fire, at: muzzle_at, own: true });
    };
    if held.reload > 0.0 {
        held.charge = 0.0;
        return;
    }
    match trigger.behavior {
        // Charge: the Spartan laser fires once charged; the plasma pistol
        // fires a quick shot on a tap and an overcharge on release.
        2 | 6 => {
            let laser = trigger.behavior == 6;
            let full = if laser { 1.4 } else { 0.9 };
            if down && held.cooldown <= 0.0 && has_round(held) {
                if held.charge == 0.0 {
                    events.write(WeaponEvent { weapon: tag, action: WeaponAction::Charge, at: muzzle_at, own: true });
                }
                held.charge += dt;
                if laser && held.charge >= full {
                    held.charge = 0.0;
                    fire_now(held, barrel, 1.0, &mut events);
                    held.cooldown = barrel.fire_recovery_time.max(1.0);
                }
            } else if !down && held.charge > 0.0 {
                let charged = held.charge >= full;
                held.charge = 0.0;
                if !laser {
                    let b = if charged { barrel_of(trigger.secondary_barrel).unwrap_or(barrel) } else { barrel };
                    fire_now(held, b, if charged { 1.0 } else { 0.0 }, &mut events);
                    held.cooldown = if charged { 1.0 } else { interval };
                }
            } else if pressed && !has_round(held) {
                fire_now(held, barrel, 0.0, &mut events);
            }
        }
        // A burst per pull.
        4 => {
            if pressed && held.cooldown <= 0.0 && held.burst == 0 {
                held.burst = barrel.shots_per_fire[1].max(barrel.shots_per_fire[0]).max(1) as i32;
            }
            if held.burst > 0 && held.cooldown <= 0.0 {
                held.burst -= 1;
                fire_now(held, barrel, 0.0, &mut events);
                held.cooldown = if held.burst > 0 { interval } else { interval + barrel.fire_recovery_time };
                if !has_round(held) {
                    held.burst = 0;
                }
            }
        }
        // One round per pull.
        1 => {
            if pressed && held.cooldown <= 0.0 {
                fire_now(held, barrel, 0.0, &mut events);
                held.cooldown = interval.max(barrel.fire_recovery_time);
            }
        }
        // Automatic while held.
        _ => {
            if down && held.cooldown <= 0.0 {
                fire_now(held, barrel, 0.0, &mut events);
                held.cooldown += interval;
                held.cooldown = held.cooldown.max(0.0);
            }
            if !down {
                held.cooldown = held.cooldown.max(0.0);
            }
        }
    }
}

/// Share of the battery a shot uses (no tag number read): the laser has
/// five shots, the plasma pistol a hundred quick ones, ten charged.
fn battery_use(kind: &Kind, power: f32) -> f32 {
    if kind.weapon.tag_path.contains("laser") {
        0.2
    } else if power > 0.5 {
        0.1
    } else {
        0.01
    }
}

/// Puts the weapon in his hands (third person) or in front of the camera
/// (his eyes), or hides it.
#[allow(clippy::too_many_arguments)]
fn place_held(
    mut commands: Commands,
    mut inv: ResMut<Inventory>,
    armory: Option<Res<Armory>>,
    foot: Res<OnFoot>,
    rider: Res<RiderPose>,
    lights: Option<Res<halo3_world::lighting::SceneLights>>,
    loaded: Option<Res<halo3_core::zones::LoadedBsps>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut transforms: Query<(&mut Transform, &mut Visibility)>,
) {
    let Some(armory) = armory else {
        return;
    };
    let want = foot.spartan.as_ref().and(inv.current().map(|h| h.kind));
    // A different weapon: a new copy.
    if inv.shown.as_ref().map(|s| s.1) != want {
        if let Some((e, _, _)) = inv.shown.take() {
            commands.entity(e).despawn();
        }
        if let Some(k) = want {
            let (e, mats) = spawn_copy(&mut commands, &armory.looks[k], &mut materials, Mat4::IDENTITY, Vec3::ONE);
            commands.entity(e).insert(Visibility::Hidden);
            inv.shown = Some((e, k, mats));
            inv.lit_at = None;
        }
        return;
    }
    let Some((entity, _, _)) = inv.shown.as_ref() else {
        return;
    };
    let Ok((mut t, mut v)) = transforms.get_mut(*entity) else {
        return;
    };
    let zoomed = foot.zoom.is_some();
    let place = if zoomed {
        None
    } else if rider.first_person {
        // In front of the eyes, low and to the right, barrel ahead.
        rider.camera.map(|(view, _)| {
            let axes = Mat4::from_cols(Vec4::NEG_Z * WU, Vec4::NEG_X * WU, Vec4::Y * WU, Vec4::W);
            view.to_matrix() * Mat4::from_translation(Vec3::new(0.2, -0.24, -0.38)) * axes
        })
    } else {
        foot.weapon_world
    };
    match place {
        Some(m) => {
            *t = Transform::from_matrix(m);
            *v = Visibility::Visible;
            // Lit where he stands.
            let at = to_halo(m.w_axis.truncate());
            if inv.lit_at.is_none_or(|p| p.distance(at) > 0.3) {
                inv.lit_at = Some(at);
                let e = halo3_rider::object_light(lights.as_deref(), at, loaded.as_deref().map_or(0, |b| b.shown));
                if let Some((_, _, mats)) = inv.shown.as_ref() {
                    for (h, look) in mats {
                        if let Some(mut m) = materials.get_mut(h) {
                            *m = look.lit(e);
                        }
                    }
                }
            }
        }
        None => *v = Visibility::Hidden,
    }
}

/// Lights weapons lying in the world once the map's light is known.
fn light_items(
    items: Option<ResMut<Items>>,
    lights: Option<Res<halo3_world::lighting::SceneLights>>,
    loaded: Option<Res<halo3_core::zones::LoadedBsps>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(mut items), Some(lights)) = (items, lights) else {
        return;
    };
    let shown = loaded.as_deref().map_or(0, |b| b.shown);
    for item in items.0.iter_mut().filter(|i| !i.lit) {
        item.lit = true;
        let e = halo3_rider::object_light(Some(&lights), item.at + Vec3::Z * 0.1, shown);
        for (h, look) in &item.materials {
            if let Some(mut m) = materials.get_mut(h) {
                *m = look.lit(e);
            }
        }
    }
}

/// Puts taken weapons back at their placement when their time comes.
fn respawn_items(
    mut commands: Commands,
    inv: Res<Inventory>,
    armory: Option<Res<Armory>>,
    items: Option<ResMut<Items>>,
    placements: Option<ResMut<Placements>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(armory), Some(mut items), Some(mut placements)) = (armory, items, placements) else {
        return;
    };
    for (n, (kind, placement, when)) in placements.0.iter_mut().enumerate() {
        if *when == 0.0 || inv.clock < *when {
            continue;
        }
        *when = 0.0;
        let model = halo_to_render() * placement.transform() * armory.kinds[*kind].to_grip.inverse();
        let (entity, mats) = spawn_copy(&mut commands, &armory.looks[*kind], &mut materials, model, Vec3::ONE);
        items.0.push(Item {
            kind: *kind,
            entity,
            at: placement.position,
            ammo: None,
            placement: Some(n),
            lit: false,
            materials: mats,
        });
    }
}
