//! Halo 3's shield meter and its sounds, on foot, on the bike and on the
//! board.
//!
//! **The meter.** The pictures are the map's own: the Spartan HUD's
//! (`chdt ui\chud\spartan`) `ui\chud\bitmaps\shield_meter`, `shield_border`
//! (its `shield_flash_bleed` glow is left out: drawn plainly it is a
//! hard-edged box), decoded from the user's map. They are chud
//! shader masks, not colour pictures: the meter's alpha is its shape, its
//! green channel the shading top to bottom and its blue channel a ramp left
//! to right (16 at the left end, 239 at the right) that the chud meter
//! shader compares with the shield fraction, so the bar empties from the
//! right; the border's alpha is the outline.
//! What is ours (the `chdt` widget layout and the chud globals' colour
//! functions are not decoded): the place (top middle, at the bitmaps' own
//! pixel size for a 720-line screen), the colours (pale blue, red below a
//! quarter), the red flashing of the border while the shields are down.
//! Halo 3 draws no health meter for the Chief; neither do we (the red flash and "You died" stay).
//!
//! **The sounds.** The HUD globals' shield sounds
//! (`halo3_core::ai::ShieldSounds`): `shield_hit` as the shields take a hit,
//! the `shield_charge` loop while they recharge, `shield_low` while they
//! are under a quarter and `shield_depleted` while they are empty, played
//! through the sound side's script queue (their own sound classes pick the
//! bus and gain). The thresholds are ours (the chud states that latch them
//! are not decoded).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use blam_cache::CacheFile;
use halo3_core::ai::{PlayerBody, PlayerVitality, ShieldSounds};
use halo3_core::script_sound::{ScriptSounds, SoundRequest};

use crate::place::Ai;

/// A decoded HUD bitmap: RGBA8, rows top to bottom.
#[derive(Clone, Debug)]
pub struct Pic {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Pic {
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }
}

/// The shield meter's bitmaps and the shield sounds' tags.
#[derive(Clone, Debug)]
pub struct HudAssets {
    pub meter: Pic,
    pub border: Pic,
    /// [`ShieldSounds::ALL`]'s tag indices, as found.
    pub sounds: [Option<u32>; 4],
}

pub const METER: &str = r"ui\chud\bitmaps\shield_meter";
pub const BORDER: &str = r"ui\chud\bitmaps\shield_border";

fn decode(cache: &CacheFile, name: &str) -> Option<Pic> {
    let tag = cache.tags.find("bitm", name)?;
    let resources = cache.resources().ok()?;
    let data = blam_texture::bitmap::read_tag(cache, tag.index).ok()?;
    let tex = blam_texture::bitmap::decode_from(&resources, &data, 0).ok()?;
    let level = tex.level(0, 0)?;
    let rgba = blam_texture::rgba::decode_level(tex.format, tex.width, tex.height, level).ok()?;
    Some(Pic {
        width: tex.width,
        height: tex.height,
        rgba,
    })
}

/// Reads the meter's bitmaps and the sounds' tags; `None` (logged) without
/// the bitmaps.
pub fn read(cache: &CacheFile) -> Option<HudAssets> {
    let (meter, border) = match (decode(cache, METER), decode(cache, BORDER)) {
        (Some(m), Some(b)) => (m, b),
        _ => {
            eprintln!("hud: the Spartan shield meter's bitmaps are not on this map");
            return None;
        }
    };
    let sounds = ShieldSounds::ALL.map(|(class, name)| cache.tags.find(class, name).map(|t| t.index));
    Some(HudAssets { meter, border, sounds })
}

/// The meter's column where the blue ramp passes `fraction`: the fill
/// shows columns left of it.
pub fn fill_columns(meter: &Pic, fraction: f32) -> u32 {
    if fraction <= 0.0 {
        return 0;
    }
    if fraction >= 1.0 {
        return meter.width;
    }
    let y = meter.height / 2;
    let t = (fraction * 255.0) as u8;
    let mut last = 0;
    for x in 0..meter.width {
        let [_, _, b, a] = meter.at(x, y);
        if a > 0 {
            if b > t {
                return x;
            }
            last = x + 1;
        }
    }
    last
}

/// The masks as white pictures with their alpha (and the meter's shading),
/// for the UI to tint.
fn white(p: &Pic, shade: bool) -> Image {
    let mut out = Vec::with_capacity(p.rgba.len());
    for px in p.rgba.chunks_exact(4) {
        let g = if shade { 0.55 + 0.45 * (1.0 - px[1] as f32 / 255.0) } else { 1.0 };
        let v = (g * 255.0) as u8;
        out.extend_from_slice(&[v, v, v, px[3]]);
    }
    Image::new(
        Extent3d {
            width: p.width,
            height: p.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        out,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

#[derive(Component)]
pub(crate) struct Meter;

#[derive(Component)]
struct Fill;

#[derive(Component)]
struct FillImage;

#[derive(Component)]
struct Border;


/// Tints, ours: Halo's pale blue, red when low or empty.
const SHIELD: Color = Color::srgba(0.55, 0.82, 1.0, 0.95);
const LOW: Color = Color::srgba(1.0, 0.28, 0.18, 0.95);
const OUTLINE: Color = Color::srgba(0.55, 0.82, 1.0, 0.75);
/// Under this share of the shields the meter turns red and beeps.
pub const LOW_SHARE: f32 = 0.25;

pub(crate) struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (spawn, meter, sounds, onto_shot_camera)
                .chain()
                .after(crate::AiSet::Hurt)
                .run_if(halo3_core::map::ready),
        );
    }
}

/// A headless `--shot` draws into an image, not a window: the meter goes
/// on the 3D camera to be in the picture.
fn onto_shot_camera(
    mut commands: Commands,
    windows: Query<(), With<bevy::window::PrimaryWindow>>,
    meters: Query<Entity, (With<Meter>, Without<bevy::ui::UiTargetCamera>)>,
    cameras: Query<Entity, With<Camera3d>>,
) {
    if !windows.is_empty() {
        return;
    }
    let Some(camera) = cameras.iter().next() else { return };
    for m in &meters {
        commands.entity(m).insert(bevy::ui::UiTargetCamera(camera));
    }
}

/// Builds the meter once the map's AI (and with it the bitmaps) is in.
fn spawn(
    mut commands: Commands,
    ai: Option<Res<Ai>>,
    existing: Query<(), With<Meter>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(ai) = ai else { return };
    if !existing.is_empty() {
        return;
    }
    let Some(hud) = ai.assets.hud.as_ref() else {
        // Mark it done anyway: nothing to draw.
        commands.spawn((Meter, Node { display: Display::None, ..default() }));
        return;
    };
    let (w, h) = (hud.meter.width as f32, hud.meter.height as f32);
    let meter = images.add(white(&hud.meter, true));
    let border = images.add(white(&hud.border, false));
    commands
        .spawn((
            Meter,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(22.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-w * 0.5)),
                width: Val::Px(w),
                height: Val::Px(h),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(11),
        ))
        .with_children(|p| {
            p.spawn((
                Fill,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(w),
                    height: Val::Px(h),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|f| {
                f.spawn((
                    FillImage,
                    ImageNode::new(meter).with_color(SHIELD),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        width: Val::Px(w),
                        height: Val::Px(h),
                        ..default()
                    },
                ));
            });
            p.spawn((
                Border,
                ImageNode::new(border).with_color(OUTLINE),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(w),
                    height: Val::Px(h),
                    ..default()
                },
            ));
        });
}

#[allow(clippy::type_complexity)]
fn meter(
    ai: Option<Res<Ai>>,
    vitality: Res<PlayerVitality>,
    player: Res<PlayerBody>,
    time: Res<Time<Real>>,
    mut root: Query<&mut Visibility, With<Meter>>,
    mut fill: Query<&mut Node, (With<Fill>, Without<Meter>)>,
    mut fill_image: Query<&mut ImageNode, (With<FillImage>, Without<Border>)>,
    mut border: Query<&mut ImageNode, (With<Border>, Without<FillImage>)>,
) {
    let Some(hud) = ai.as_deref().and_then(|a| a.assets.hud.as_ref()) else {
        return;
    };
    let on = player.0.is_some();
    for mut v in &mut root {
        v.set_if_neq(if on { Visibility::Visible } else { Visibility::Hidden });
    }
    let share = if vitality.max_shield > 0.0 { (vitality.shield / vitality.max_shield).clamp(0.0, 1.0) } else { 0.0 };
    let cols = fill_columns(&hud.meter, share) as f32;
    for mut n in &mut fill {
        n.width = Val::Px(cols);
    }
    let low = share < LOW_SHARE;
    for mut i in &mut fill_image {
        i.color = if low { LOW } else { SHIELD };
    }
    // Empty: the outline flashes red, four times a second.
    let empty = share <= 0.0 || vitality.dead.is_some();
    let blink = (time.elapsed_secs() * 4.0).fract() < 0.5;
    for mut i in &mut border {
        i.color = if empty {
            if blink { LOW } else { LOW.with_alpha(0.25) }
        } else if low {
            LOW.with_alpha(0.8)
        } else {
            OUTLINE
        };
    }
}

/// Which shield loop should be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loop {
    None,
    Charge,
    Low,
    Depleted,
}

/// The loop for the shields' state (`recharging`: they are filling).
pub fn loop_for(share: f32, recharging: bool, dead: bool) -> Loop {
    if dead {
        Loop::None
    } else if recharging {
        Loop::Charge
    } else if share <= 0.0 {
        Loop::Depleted
    } else if share < LOW_SHARE {
        Loop::Low
    } else {
        Loop::None
    }
}

#[derive(Default)]
pub(crate) struct SoundState {
    playing: Option<Loop>,
    last_shield: Option<f32>,
    hit_wait: f32,
}

fn sounds(
    ai: Option<Res<Ai>>,
    vitality: Res<PlayerVitality>,
    player: Res<PlayerBody>,
    queue: Option<ResMut<ScriptSounds>>,
    time: Res<Time>,
    mut state: Local<SoundState>,
) {
    let (Some(ai), Some(mut queue)) = (ai, queue) else { return };
    let Some(hud) = ai.assets.hud.as_ref() else { return };
    let [hit, charge, low, depleted] = hud.sounds;
    let name = |i: usize| ShieldSounds::ALL[i].1.to_string();
    let share = if vitality.max_shield > 0.0 { (vitality.shield / vitality.max_shield).clamp(0.0, 1.0) } else { 0.0 };
    let recharging = state.last_shield.is_some_and(|s| vitality.shield > s + 1e-4) && vitality.dead.is_none();
    let want = if player.0.is_none() { Loop::None } else { loop_for(share, recharging, vitality.dead.is_some()) };
    state.hit_wait -= time.delta_secs();
    // A hit on the shields.
    if let (Some(was), Some(tag)) = (state.last_shield, hit)
        && vitality.shield < was - 0.01
        && state.hit_wait <= 0.0
    {
        state.hit_wait = 0.25;
        queue.push(SoundRequest::Impulse { tag, name: name(0), at: None, scale: 1.0 });
    }
    state.last_shield = Some(vitality.shield);
    let current = state.playing.unwrap_or(Loop::None);
    if want == current {
        return;
    }
    let tag_of = |l: Loop| match l {
        Loop::Charge => charge.map(|t| (t, 1)),
        Loop::Low => low.map(|t| (t, 2)),
        Loop::Depleted => depleted.map(|t| (t, 3)),
        Loop::None => None,
    };
    if let Some((tag, _)) = tag_of(current) {
        queue.push(SoundRequest::LoopStop { tag, immediately: true });
    }
    if let Some((tag, i)) = tag_of(want) {
        queue.push(SoundRequest::LoopStart { tag, name: name(i), at: None, scale: 1.0 });
    }
    state.playing = Some(want);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loops_follow_the_shields() {
        assert_eq!(loop_for(1.0, false, false), Loop::None);
        assert_eq!(loop_for(0.2, false, false), Loop::Low);
        assert_eq!(loop_for(0.0, false, false), Loop::Depleted);
        assert_eq!(loop_for(0.1, true, false), Loop::Charge);
        assert_eq!(loop_for(0.0, false, true), Loop::None);
    }

    #[test]
    fn the_ramp_picks_columns() {
        // A 10-column meter whose blue ramp climbs 0, 25, ... 225.
        let mut rgba = Vec::new();
        for _y in 0..2 {
            for x in 0..10u8 {
                rgba.extend_from_slice(&[255, 0, x * 25, 255]);
            }
        }
        let m = Pic { width: 10, height: 2, rgba };
        assert_eq!(fill_columns(&m, 0.0), 0);
        assert_eq!(fill_columns(&m, 1.0), 10);
        assert_eq!(fill_columns(&m, 0.5), 6);
    }
}
