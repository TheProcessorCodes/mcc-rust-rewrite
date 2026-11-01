//! A Call of Duty style kill feed, bottom left: "<username> [weapon]
//! Enemy" for each kill (the name is the Settings one, `Player1` by default), newest at the bottom, fading after a few seconds.

use bevy::prelude::*;

use super::Armory;
use super::assets::display_name;
use super::enemy::EnemyKilled;

pub struct KillFeedPlugin;

impl Plugin for KillFeedPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(halo3_core::map::MapStartup, spawn).add_systems(Update, (add, fade).chain().run_if(halo3_core::map::ready));
    }
}

/// Rows kept at most, seconds a row shows, and seconds of it fading out.
const ROWS: usize = 5;
const SHOWN: f32 = 6.0;
const FADE: f32 = 1.0;

const FRIEND: Color = Color::srgb(0.45, 0.80, 1.0);
const WEAPON: Color = Color::srgb(0.92, 0.92, 0.92);
const FOE: Color = Color::srgb(1.0, 0.32, 0.25);
const BACK: Color = Color::srgba(0.0, 0.0, 0.0, 0.45);

#[derive(Component)]
struct Feed;

/// A row and its age in seconds.
#[derive(Component)]
struct Row(f32);

fn spawn(mut commands: Commands) {
    commands.spawn((
        Feed,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(24.0),
            bottom: Val::Px(150.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexStart,
            row_gap: Val::Px(4.0),
            ..default()
        },
    ));
}

fn add(
    mut commands: Commands,
    mut kills: MessageReader<EnemyKilled>,
    armory: Option<Res<Armory>>,
    feed: Query<Entity, With<Feed>>,
    rows: Query<(Entity, &Row)>,
) {
    let Ok(feed) = feed.single() else {
        kills.clear();
        return;
    };
    let mut count = rows.iter().count();
    for kill in kills.read() {
        let weapon = armory
            .as_ref()
            .and_then(|a| a.kinds.iter().find(|k| k.weapon.tag_index == kill.weapon))
            .map_or_else(|| "Weapon".to_string(), |k| display_name(&k.weapon.tag_path));
        let weapon = if kill.headshot { format!("[ {weapon} | Headshot ]") } else { format!("[ {weapon} ]") };
        let cell = |s: String, colour: Color| (Text::new(s), TextFont::from_font_size(20.0), TextColor(colour));
        commands.entity(feed).with_children(|f| {
            f.spawn((
                Row(0.0),
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                    column_gap: Val::Px(12.0),
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(BACK),
            ))
            .with_children(|r| {
                r.spawn(cell(game_paths::username(), FRIEND));
                r.spawn(cell(weapon, WEAPON));
                r.spawn(cell("Enemy".to_string(), FOE));
            });
        });
        count += 1;
    }
    // Too many: the oldest go first.
    if count > ROWS {
        let mut old: Vec<_> = rows.iter().collect();
        old.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));
        for (e, _) in old.into_iter().take(count - ROWS) {
            commands.entity(e).try_despawn();
        }
    }
}

fn fade(
    mut commands: Commands,
    time: Res<Time>,
    mut rows: Query<(Entity, &mut Row, &mut BackgroundColor, &Children)>,
    mut texts: Query<&mut TextColor>,
) {
    for (e, mut row, mut back, children) in &mut rows {
        row.0 += time.delta_secs();
        if row.0 > SHOWN {
            commands.entity(e).try_despawn();
            continue;
        }
        let alpha = ((SHOWN - row.0) / FADE).clamp(0.0, 1.0);
        back.0 = BACK.with_alpha(BACK.alpha() * alpha);
        for c in children.iter() {
            if let Ok(mut t) = texts.get_mut(c) {
                t.0 = t.0.with_alpha(alpha);
            }
        }
    }
}
