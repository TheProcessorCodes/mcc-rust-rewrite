//! Hurting and dying: damage messages taken off shields, then bodies;
//! actors fall over and vanish; the player's shields come back after a
//! pause and his health after them; when he dies he comes back at the last
//! quiet place he stood (a checkpoint taken while no enemy had sight of
//! him). All of that on foot, on the dirt bike and on the board alike: on
//! the bike he comes back on the bike at the checkpoint (the bike rolls on
//! riderless while he is dead), on the board through the skate engine's
//! respawn. A hit does not knock him off the bike (Halo 3's vehicles do not
//! throw their drivers either); his health decides. Also the shield and
//! health bars and the red flash.
//!
//! Halo's rules kept: shields soak a hit before the body; a shot to the
//! head of an unshielded actor does double (our own number; the damage
//! effects' head-shot flags are not read), except on the Flood and
//! Sentinels, which have no heads to speak of. Rounds of one actor do not
//! hurt its allies.

use bevy::prelude::*;
use halo3_core::map::MapAppExt;

use halo3_core::ai::{
    Allegiances, Damage, DamageSource, DamageTarget, PlayerBody, PlayerMode, PlayerRespawn, PlayerVitality, team,
};
use halo3_core::coords::to_halo;
use halo3_foot::OnFoot;
use halo3_foot::physics::Spartan;

use crate::brain::Foe;
use crate::place::{Actor, Ai, root_transform, yaw_of_heading};
use crate::AiSet;

/// Damage taken, deaths, the player's checkpoint and respawn.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct HurtSet;

pub(crate) struct VitalityPlugin;

impl Plugin for VitalityPlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<Checkpoint>()
            .add_systems(halo3_core::map::MapStartup, spawn_hud)
            .add_systems(
                Update,
                (hurt, fall, recharge, checkpoint, respawn)
                    .chain()
                    .in_set(HurtSet)
                    .in_set(AiSet::Hurt)
                    .run_if(halo3_core::pause::running)
                    .run_if(halo3_core::map::ready),
            )
            .add_systems(Update, hud.after(AiSet::Hurt).run_if(halo3_core::map::ready));
    }
}

/// Seconds after a hit before shields come back, and how long they take
/// to fill: the Chief's model damage info stores 5 and 2 (inferred names,
/// `docs/format/ai.md`); the body follows the shields back over this long.
const SHIELD_DELAY: f32 = 5.0;
const SHIELD_FILL: f32 = 2.0;
const BODY_FILL: f32 = 4.0;
/// `HALO3_AI_TRACE=1`: log every hit on the player.
fn trace() -> bool {
    static ON: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var("HALO3_AI_TRACE").is_ok_and(|v| v == "1"));
    *ON
}

/// Actors hurting actors deal this share of the damage effect's number.
/// Ours, where the game leaves it to engine behaviour we cannot read (the
/// targets' material responses): at the full number 120_halo's sentinel
/// beams (4 a hit, 30 a second) cut down the run's scripted Flood fights
/// (`trench_combat_forms01..03` placed among `trench_sentinel_100_4`) in
/// under a second, long before the rider gets there; the player's damage
/// to actors and theirs to him are not scaled by this.
const AI_VS_AI: f32 = 0.2;

/// Seconds a dead player waits before coming back.
const RESPAWN_AFTER: f32 = 3.0;

#[allow(clippy::too_many_arguments)]
fn hurt(
    ai: Option<Res<Ai>>,
    mut reader: MessageReader<Damage>,
    mut actors: Query<&mut Actor>,
    mut vitality: ResMut<PlayerVitality>,
    allegiances: Res<Allegiances>,
    player: Res<PlayerBody>,
    difficulty: Res<halo3_core::ai::Difficulty>,
) {
    let playing = player.0.is_some();
    // The globals' scale on what enemies deal him (normal 0.775 on 120_halo).
    let enemy_scale = ai
        .as_deref()
        .and_then(|ai| ai.assets.enemy_damage)
        .map_or(1.0, |s| s[difficulty.0.min(3)]);
    for d in reader.read() {
        let source_kind = match d.source {
            DamageSource::Actor(e) => actors.get(e).ok().map(|a| a.kind),
            _ => None,
        };
        let source_team = match d.source {
            DamageSource::Actor(e) => actors.get(e).ok().map(|a| a.team),
            DamageSource::Player => Some(team::PLAYER),
            DamageSource::World => None,
        };
        match d.target {
            DamageTarget::Player => {
                if !playing || vitality.god || vitality.dead.is_some() {
                    continue;
                }
                if source_team.is_some_and(|t| t != team::PLAYER && allegiances.allied(t, team::PLAYER)) {
                    continue;
                }
                if trace() {
                    let who = match d.source {
                        DamageSource::Actor(e) => actors
                            .get(e)
                            .ok()
                            .and_then(|a| ai.as_deref()?.assets.kinds.get(a.kind)?.as_ref())
                            .map_or("an actor", |k| k.character.tag_path.rsplit('\\').next().unwrap_or("")),
                        DamageSource::Player => "himself",
                        DamageSource::World => "the world",
                    };
                    info!(
                        "ai: the player takes {:.1}{} from {who} (shield {:.0}, body {:.0})",
                        d.amount,
                        if d.melee { " (melee)" } else { "" },
                        vitality.shield,
                        vitality.body
                    );
                }
                let mut amount = d.amount;
                if matches!(d.source, DamageSource::Actor(_)) {
                    amount *= enemy_scale;
                }
                let soak = vitality.shield.min(amount);
                vitality.shield -= soak;
                amount -= soak;
                if amount > 0.0 {
                    vitality.body -= amount;
                    vitality.flash = 0.4;
                }
                vitality.since_hit = 0.0;
                if vitality.body <= 0.0 {
                    vitality.body = 0.0;
                    vitality.dead = Some(0.0);
                    info!("ai: the player died ({:?})", player.0.map(|b| b.mode));
                }
            }
            DamageTarget::Actor(e) => {
                let Ok(mut a) = actors.get_mut(e) else {
                    continue;
                };
                if !a.alive() {
                    continue;
                }
                if let (DamageSource::Actor(_), Some(t)) = (d.source, source_team)
                    && allegiances.allied(t, a.team)
                {
                    continue;
                }
                let kind = ai.as_deref().and_then(|ai| ai.assets.kinds.get(a.kind)).and_then(Option::as_ref);
                let scale = if matches!(d.source, DamageSource::Actor(_)) { AI_VS_AI } else { 1.0 };
                let headless = kind.is_none_or(|k| matches!(k.unit.team, team::FLOOD | team::SENTINEL | team::GUILTY));
                let mut amount = d.amount * scale;
                if a.shield > 0.0 {
                    let soak = a.shield.min(amount);
                    a.shield -= soak;
                    amount -= soak;
                } else if d.head && !headless && !d.melee {
                    amount *= 2.0;
                }
                a.health -= amount;
                a.since_hit = 0.0;
                a.brain.provoked = Some(d.source);
                if a.health <= 0.0 {
                    a.health = 0.0;
                    a.dead = Some(0.0);
                    if trace() {
                        let name = |k: usize| {
                            ai.as_deref()
                                .and_then(|ai| ai.assets.kinds.get(k)?.as_ref())
                                .map_or(String::from("?"), |k| k.character.tag_path.rsplit('\\').next().unwrap_or("").to_string())
                        };
                        let by = match d.source {
                            DamageSource::Actor(_) => source_kind.map_or(String::from("an actor"), name),
                            DamageSource::Player => String::from("the player"),
                            DamageSource::World => String::from("the world"),
                        };
                        info!("ai: {} killed by {by}{}", name(a.kind), if d.melee { " (melee)" } else { "" });
                    }
                    // Its body flies the way the blow went, a little.
                    a.velocity = d.direction * 1.5;
                }
            }
        }
    }
}

/// The dead fall over, sink and go; infection forms pop at once.
fn fall(
    mut commands: Commands,
    ai: Option<Res<Ai>>,
    mut actors: Query<(
        Entity,
        &mut Actor,
        &mut Transform,
        &mut halo3_core::ai::ActorActivity,
        Option<&crate::animate::Animated>,
    )>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (e, mut a, mut t, mut activity, animated) in &mut actors {
        let Some(since) = a.dead else {
            continue;
        };
        let since = since + dt;
        a.dead = Some(since);
        let pops = ai
            .as_deref()
            .and_then(|ai| ai.assets.kinds.get(a.kind))
            .and_then(Option::as_ref)
            .is_some_and(|k| k.character.actor_type == 10 || k.bursts);
        if pops && since > 0.15 {
            commands.entity(e).despawn();
            continue;
        }
        activity.state = if since < 0.6 { halo3_core::ai::ActorState::Dying } else { halo3_core::ai::ActorState::Dead };
        activity.time_in_state = since;
        if since > 6.0 {
            commands.entity(e).despawn();
            continue;
        }
        // Flyers drop; walkers tip over; both sink at the end.
        let v = a.velocity;
        a.position += Vec3::new(v.x, 0.0, v.z) * dt * (1.0 - since).max(0.0);
        if a.flying && since < 1.5 {
            a.position.y -= 4.0 * dt * since.min(1.0);
        }
        if since > 4.0 {
            a.position.y -= 0.3 * dt;
        }
        // A death animation lays the body down itself.
        let tilt = (since / 0.6).min(1.0) * std::f32::consts::FRAC_PI_2;
        let tips = !a.flying && !animated.is_some_and(|x| x.has_death);
        *t = root_transform(a.position, a.heading, if tips { tilt } else { 0.0 });
    }
}

/// Shields come back after a pause, then health (the player's and the
/// actors' with shields: the Arbiter).
fn recharge(mut vitality: ResMut<PlayerVitality>, mut actors: Query<&mut Actor>, time: Res<Time>) {
    let dt = time.delta_secs();
    let v = &mut *vitality;
    v.flash = (v.flash - dt).max(0.0);
    if v.dead.is_none() {
        v.since_hit += dt;
        if v.since_hit > SHIELD_DELAY {
            if v.shield < v.max_shield {
                v.shield = (v.shield + v.max_shield * dt / SHIELD_FILL).min(v.max_shield);
            } else {
                v.body = (v.body + v.max_body * dt / BODY_FILL).min(v.max_body);
            }
        }
    }
    for mut a in &mut actors {
        if !a.alive() {
            continue;
        }
        a.since_hit += dt;
        if a.max_shield > 0.0 && a.since_hit > SHIELD_DELAY {
            a.shield = (a.shield + a.max_shield * dt / SHIELD_FILL).min(a.max_shield);
        }
    }
}

/// Where the player comes back: renderer frame feet and heading.
#[derive(Resource, Default)]
struct Checkpoint {
    at: Option<(Vec3, f32)>,
    /// Seconds without an enemy seeing him.
    quiet: f32,
}

fn checkpoint(
    mut point: ResMut<Checkpoint>,
    player: Res<PlayerBody>,
    vitality: Res<PlayerVitality>,
    actors: Query<&Actor>,
    time: Res<Time>,
) {
    let Some(b) = player.0 else {
        point.quiet = 0.0;
        return;
    };
    if vitality.dead.is_some() {
        return;
    }
    let seen = actors.iter().any(|a| a.alive() && a.brain.sees && a.brain.foe == Some(Foe::Player));
    point.quiet = if seen { 0.0 } else { point.quiet + time.delta_secs() };
    if point.at.is_none() || (point.quiet > 3.0 && b.steady) {
        point.at = Some((b.feet, b.heading));
    }
}

#[allow(clippy::too_many_arguments)]
fn respawn(
    mut vitality: ResMut<PlayerVitality>,
    mut foot: Option<ResMut<OnFoot>>,
    player: Res<PlayerBody>,
    mut ride: ResMut<PlayerRespawn>,
    skate: Option<ResMut<halo3_core::skate::Respawn>>,
    point: Res<Checkpoint>,
    time: Res<Time>,
) {
    ride.dead = vitality.dead.is_some();
    let Some(since) = vitality.dead else {
        return;
    };
    let since = since + time.delta_secs();
    vitality.dead = Some(since);
    if since < RESPAWN_AFTER {
        return;
    }
    let mode = player.0.map(|b| b.mode);
    let fallback = player.0.map(|b| (b.feet, b.heading));
    match mode {
        Some(PlayerMode::Bike) => {
            if let Some(at) = point.at.or(fallback) {
                ride.bike = Some(at);
            }
        }
        Some(PlayerMode::Board) => {
            if let (Some(mut skate), Some((at, heading))) = (skate, point.at.or(fallback)) {
                skate.0 = Some((to_halo(at), yaw_of_heading(heading)));
            }
        }
        Some(PlayerMode::Foot) | None => {
            if let Some(foot) = foot.as_deref_mut()
                && let Some(s) = foot.spartan.as_mut()
            {
                let (at, heading) = point.at.or(s.safe).unwrap_or((s.position, s.heading));
                let tuning = s.tuning;
                *s = Spartan::new(at + Vec3::Y * 0.3, heading);
                s.tuning = tuning;
                foot.yaw = heading - std::f32::consts::PI;
                foot.pitch = 0.0;
            }
        }
    }
    ride.dead = false;
    let v = &mut *vitality;
    v.dead = None;
    v.body = v.max_body;
    v.shield = v.max_shield;
    v.since_hit = 100.0;
    v.flash = 0.0;
    info!("ai: the player is back at his checkpoint");
}

#[derive(Component)]
struct Bars;

#[derive(Component)]
struct ShieldBar;

#[derive(Component)]
struct HealthBar;

#[derive(Component)]
struct Flash;

#[derive(Component)]
struct DeadText;

const BAR_WIDTH: f32 = 260.0;

fn spawn_hud(mut commands: Commands) {
    // Shields and health, top middle, Halo blue and red.
    commands
        .spawn((
            Bars,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(18.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-BAR_WIDTH * 0.5)),
                width: Val::Px(BAR_WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(11),
        ))
        .with_children(|p| {
            for (shield, colour, height) in [(true, Color::srgba(0.45, 0.80, 1.0, 0.9), 10.0), (false, Color::srgba(1.0, 0.3, 0.25, 0.85), 4.0)] {
                p.spawn((
                    Node {
                        width: Val::Px(BAR_WIDTH),
                        height: Val::Px(height),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgba(0.45, 0.80, 1.0, 0.5)),
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                ))
                .with_children(|bar| {
                    let mut fill = bar.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(colour),
                    ));
                    if shield {
                        fill.insert(ShieldBar);
                    } else {
                        fill.insert(HealthBar);
                    }
                });
            }
        });
    commands.spawn((
        Flash,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.8, 0.0, 0.0, 0.0)),
        Visibility::Hidden,
        GlobalZIndex(9),
    ));
    commands.spawn((
        DeadText,
        Text::new("You died"),
        TextFont::from_font_size(42.0),
        TextColor(Color::srgba(1.0, 0.85, 0.8, 0.95)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(40.0),
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-80.0)),
            ..default()
        },
        Visibility::Hidden,
        GlobalZIndex(12),
    ));
}

#[allow(clippy::type_complexity)]
fn hud(
    vitality: Res<PlayerVitality>,
    player: Res<PlayerBody>,
    ai: Option<Res<Ai>>,
    mut bars: Query<&mut Visibility, (With<Bars>, Without<Flash>, Without<DeadText>)>,
    mut shield: Query<&mut Node, (With<ShieldBar>, Without<HealthBar>)>,
    mut health: Query<&mut Node, (With<HealthBar>, Without<ShieldBar>)>,
    mut flash: Query<(&mut BackgroundColor, &mut Visibility), (With<Flash>, Without<Bars>, Without<DeadText>)>,
    mut dead: Query<&mut Visibility, (With<DeadText>, Without<Bars>, Without<Flash>)>,
) {
    let on = ai.is_some() && player.0.is_some();
    let show = |on: bool| if on { Visibility::Visible } else { Visibility::Hidden };
    for mut v in &mut bars {
        v.set_if_neq(show(on));
    }
    let share = |a: f32, b: f32| if b > 0.0 { (a / b).clamp(0.0, 1.0) * 100.0 } else { 0.0 };
    for mut n in &mut shield {
        n.width = Val::Percent(share(vitality.shield, vitality.max_shield));
    }
    for mut n in &mut health {
        n.width = Val::Percent(share(vitality.body, vitality.max_body));
    }
    for (mut c, mut v) in &mut flash {
        let a = if vitality.dead.is_some() { 0.45 } else { vitality.flash * 0.8 };
        v.set_if_neq(show(on && a > 0.0));
        c.0 = Color::srgba(0.7, 0.0, 0.0, a);
    }
    for mut v in &mut dead {
        v.set_if_neq(show(on && vitality.dead.is_some()));
    }
}

