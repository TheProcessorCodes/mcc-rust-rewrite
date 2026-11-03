//! The speedometer in the bottom right while riding the dirt bike or the
//! skateboard, in the unit chosen in the settings (`halo3_core::units`).

use bevy::prelude::*;
use halo3_core::bike::BikeMotion;
use halo3_core::skate::SkateMotion;
use halo3_core::units::SpeedUnit;

pub struct SpeedoPlugin;

impl Plugin for SpeedoPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SpeedUnit::load())
            // Per map: unloading the map (a restart, back to the menu) despawns it.
            .add_systems(halo3_core::map::MapStartup, spawn)
            .add_systems(Update, show.run_if(halo3_core::map::ready));
    }
}

#[derive(Component)]
struct Speedo;
#[derive(Component)]
struct SpeedoNumber;
#[derive(Component)]
struct SpeedoUnitLabel;

const LIT: Color = Color::srgb(0.95, 0.97, 1.0);
const DIM: Color = Color::srgb(0.65, 0.72, 0.82);

fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Speedo,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(28.0),
                bottom: Val::Px(24.0),
                align_items: AlignItems::FlexEnd,
                column_gap: Val::Px(8.0),
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((SpeedoNumber, Text::new("0"), TextFont::from_font_size(54.0), TextColor(LIT)));
            p.spawn((
                SpeedoUnitLabel,
                Text::new("MPH"),
                TextFont::from_font_size(22.0),
                TextColor(DIM),
                Node { margin: UiRect::bottom(Val::Px(8.0)), ..default() },
            ));
        });
}

#[allow(clippy::type_complexity)]
fn show(
    unit: Res<SpeedUnit>,
    // Absent where the bike or the skating is not set up (`--shot`).
    bike: Option<Res<BikeMotion>>,
    skate: Option<Res<SkateMotion>>,
    mut root: Query<&mut Visibility, With<Speedo>>,
    mut number: Query<&mut Text, (With<SpeedoNumber>, Without<SpeedoUnitLabel>)>,
    mut label: Query<&mut Text, (With<SpeedoUnitLabel>, Without<SpeedoNumber>)>,
) {
    // The bike's motion is empty off the bike; the skate's is live only while skating.
    let mps = match (bike.as_deref(), skate.as_deref()) {
        (Some(b), _) if b.riding => Some(b.speed),
        (_, Some(s)) if s.riding => Some(s.speed),
        _ => None,
    };
    for mut v in &mut root {
        v.set_if_neq(if mps.is_some() { Visibility::Inherited } else { Visibility::Hidden });
    }
    let Some(mps) = mps else {
        return;
    };
    let shown = format!("{:.0}", unit.from_mps(mps));
    for mut t in &mut number {
        if t.0 != shown {
            t.0.clone_from(&shown);
        }
    }
    for mut t in &mut label {
        if t.0 != unit.label() {
            t.0 = unit.label().to_string();
        }
    }
}
