//! Halo CE's HUD countdown, as the map's scripts drive it
//! (`hud_set_timer_time`, `show_hud_timer`, `pause_hud_timer`,
//! `hud_set_timer_warning_time`; `halo3_script::vm::HudTimer`): The Maw's
//! six minutes to the Longsword. Drawn in the bottom right over the
//! speedometer, where the scripts put it (`bottom_right`), red once it is
//! under the warning time.

use bevy::prelude::*;
use halo3_script::Scripts;

pub struct CountdownPlugin;

impl Plugin for CountdownPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(halo3_core::map::MapStartup, spawn)
            .add_systems(Update, show.run_if(halo3_core::map::ready));
    }
}

#[derive(Component)]
struct Countdown;

const LIT: Color = Color::srgb(0.95, 0.97, 1.0);
const WARN: Color = Color::srgb(1.0, 0.28, 0.22);

fn spawn(mut commands: Commands) {
    commands.spawn((
        Countdown,
        Text::new("0:00"),
        TextFont::from_font_size(44.0),
        TextColor(LIT),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(28.0),
            bottom: Val::Px(92.0),
            ..default()
        },
        Visibility::Hidden,
    ));
}

fn show(scripts: Option<Res<Scripts>>, mut shown: Query<(&mut Text, &mut TextColor, &mut Visibility), With<Countdown>>) {
    let timer = scripts.as_deref().map(|s| s.runtime.hud_timer).filter(|t| t.shown);
    for (mut text, mut colour, mut visibility) in &mut shown {
        let Some((ticks, warns)) = timer.and_then(|t| Some((t.ticks?, t.warns()))) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let s = crate::finish::countdown(ticks);
        if text.0 != s {
            text.0 = s;
        }
        colour.set_if_neq(TextColor(if warns { WARN } else { LIT }));
    }
}
