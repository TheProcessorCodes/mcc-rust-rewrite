//! The on-foot HUD: Halo's ring crosshair in the middle, the weapons and
//! their ammo top left, prompts (pick up, reloading, charging) low in the
//! middle. Shown only on foot.

use bevy::prelude::*;

use super::{Armory, Inventory};
use halo3_foot::OnFoot;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn).add_systems(Update, update.after(super::place_held));
    }
}

#[derive(Component)]
struct Hud;

#[derive(Component)]
struct Crosshair;

#[derive(Component)]
struct ActiveText;

#[derive(Component)]
struct OtherText;

#[derive(Component)]
struct PromptText;

/// Halo's HUD blue.
const BLUE: Color = Color::srgba(0.45, 0.80, 1.0, 0.9);
const DIM: Color = Color::srgba(0.45, 0.80, 1.0, 0.55);

fn spawn(mut commands: Commands) {
    let ring = 26.0;
    commands
        .spawn((
            Hud,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(10),
        ))
        .with_children(|root| {
            // Crosshair: a ring and a dot, centred.
            root.spawn((
                Crosshair,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(50.0),
                    top: Val::Percent(50.0),
                    width: Val::Px(ring),
                    height: Val::Px(ring),
                    margin: UiRect {
                        left: Val::Px(-ring / 2.0),
                        top: Val::Px(-ring / 2.0),
                        ..default()
                    },
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::MAX,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BorderColor::all(BLUE),
            ))
            .with_child((
                Node {
                    width: Val::Px(3.0),
                    height: Val::Px(3.0),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(BLUE),
            ));
            // Weapons, top left.
            root.spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(28.0),
                top: Val::Px(22.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|p| {
                p.spawn((ActiveText, Text::new(""), TextFont::from_font_size(26.0), TextColor(BLUE)));
                p.spawn((OtherText, Text::new(""), TextFont::from_font_size(16.0), TextColor(DIM)));
            });
            // Prompts, low in the middle.
            root.spawn(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Percent(22.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_child((PromptText, Text::new(""), TextFont::from_font_size(20.0), TextColor(Color::WHITE)));
        });
}

#[allow(clippy::type_complexity)]
fn update(
    inv: Res<Inventory>,
    armory: Option<Res<Armory>>,
    foot: Res<OnFoot>,
    mut hud: Query<&mut Visibility, With<Hud>>,
    mut texts: ParamSet<(
        Query<&mut Text, With<ActiveText>>,
        Query<&mut Text, With<OtherText>>,
        Query<&mut Text, With<PromptText>>,
    )>,
    mut crosshair: Query<&mut BorderColor, With<Crosshair>>,
) {
    let on = foot.spartan.is_some();
    for mut v in &mut hud {
        let want = if on { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    let Some(armory) = armory else {
        return;
    };
    if !on {
        return;
    }
    let line = |slot: Option<&super::Held>| -> String {
        let Some(h) = slot else {
            return String::new();
        };
        let kind = &armory.kinds[h.kind];
        let name = kind.name();
        if kind.weapon.magazines.is_empty() {
            format!("{name}   {:.0}%", h.ammo.energy * 100.0)
        } else {
            format!("{name}   {} / {}", h.ammo.loaded, h.ammo.reserve)
        }
    };
    let active = line(inv.current());
    let other = line(inv.slots[1 - inv.active].as_ref());
    set(&mut texts.p0(), &active);
    set(&mut texts.p1(), &other);
    let prompt = match inv.current() {
        Some(h) if h.reload > 0.0 => "Reloading".to_string(),
        Some(h) if h.charge > 0.0 => "Charging".to_string(),
        Some(h) if !armory.kinds[h.kind].weapon.magazines.is_empty() && h.ammo.loaded == 0 && h.ammo.reserve == 0 => {
            "No ammo".to_string()
        }
        _ => inv.prompt.clone().unwrap_or_default(),
    };
    set(&mut texts.p2(), &prompt);
    // The ring turns red when the weapon is empty.
    let empty = inv.current().is_some_and(|h| {
        let kind = &armory.kinds[h.kind];
        if kind.weapon.magazines.is_empty() { h.ammo.energy <= 0.0 } else { h.ammo.loaded == 0 }
    });
    for mut c in &mut crosshair {
        let want = BorderColor::all(if empty { Color::srgba(1.0, 0.3, 0.25, 0.9) } else { BLUE });
        if *c != want {
            *c = want;
        }
    }
}

fn set(q: &mut Query<&mut Text, impl bevy::ecs::query::QueryFilter>, s: &str) {
    for mut t in q.iter_mut() {
        if t.0 != s {
            t.0 = s.to_string();
        }
    }
}
