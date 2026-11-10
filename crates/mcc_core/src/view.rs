//! The player's view settings, chosen on the main menu's settings screen and
//! saved with the other settings (`game_paths::key`): how fast the mouse and
//! the right stick turn the camera, whether looking up and down is turned
//! round, how wide every camera sees, and the window's display mode and
//! vsync.
//!
//! [`LookSettings`] is a resource (`FlyCameraPlugin` puts it in) read by the
//! flying camera, on foot, on the bike and on the board. Sensitivity scales
//! each mode's own turning speed; the field of view scales each camera's
//! own (flying 75 degrees, on foot 55 / 60, the bike's chase camera 70,
//! the board's from Skate 3's camera). Invert look turns round the mouse's
//! and the stick's up and down where they look (flying, on foot); the
//! bike's chase camera has no look, and on the board the right stick does
//! Skate 3's tricks, so neither is turned round.

use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, WindowMode, WindowPosition};

/// The sensitivity steps, in percent.
pub const SENSITIVITY_STEPS: [u32; 12] = [25, 35, 50, 65, 80, 100, 120, 150, 180, 220, 260, 300];
/// The field of view steps, in percent of each camera's own.
pub const FOV_STEPS: [u32; 11] = [80, 85, 90, 95, 100, 105, 110, 115, 120, 125, 130];

/// The flying camera's field of view, in degrees, before the setting.
pub const FLY_FOV: f32 = 75.0;

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct LookSettings {
    /// The mouse's turning speed, in percent of the stock.
    pub mouse: u32,
    /// The right stick's turning speed, in percent.
    pub stick: u32,
    /// Up and down turned round.
    pub invert: bool,
    /// Every camera's field of view, in percent of its own.
    pub fov: u32,
}

impl Default for LookSettings {
    fn default() -> Self {
        LookSettings { mouse: 100, stick: 100, invert: false, fov: 100 }
    }
}

/// A saved percentage, put on the nearest of `steps`; `default` when there
/// is none or it is not a number.
fn percent(settings: &game_paths::Settings, key: &str, steps: &[u32], default: u32) -> u32 {
    let Some(v) = settings.get(key).and_then(|v| v.trim().trim_end_matches('%').trim().parse::<f32>().ok()) else {
        return default;
    };
    steps.iter().copied().min_by_key(|s| (*s as f32 - v).abs() as u32).unwrap_or(default)
}

fn on(settings: &game_paths::Settings, key: &str, default: bool) -> bool {
    match settings.get(key).map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) if v == "on" => true,
        Some(v) if v == "off" => false,
        _ => default,
    }
}

impl LookSettings {
    /// The saved settings, or the defaults.
    pub fn load() -> LookSettings {
        Self::from_settings(&game_paths::settings())
    }

    fn from_settings(s: &game_paths::Settings) -> LookSettings {
        use game_paths::key;
        LookSettings {
            mouse: percent(s, key::MOUSE_SENSITIVITY, &SENSITIVITY_STEPS, 100),
            stick: percent(s, key::STICK_SENSITIVITY, &SENSITIVITY_STEPS, 100),
            invert: on(s, key::INVERT_LOOK, false),
            fov: percent(s, key::FIELD_OF_VIEW, &FOV_STEPS, 100),
        }
    }

    /// Saves all four.
    pub fn save(&self) -> Result<(), String> {
        use game_paths::key;
        let path = game_paths::settings_path();
        let mut s = game_paths::Settings::read(&path);
        s.set(key::MOUSE_SENSITIVITY, &self.mouse.to_string());
        s.set(key::STICK_SENSITIVITY, &self.stick.to_string());
        s.set(key::INVERT_LOOK, if self.invert { "on" } else { "off" });
        s.set(key::FIELD_OF_VIEW, &self.fov.to_string());
        s.write(&path)
    }

    /// The mouse's scale on a mode's own turning speed.
    pub fn mouse_scale(&self) -> f32 {
        self.mouse as f32 / 100.0
    }

    /// The stick's scale on a mode's own turning speed.
    pub fn stick_scale(&self) -> f32 {
        self.stick as f32 / 100.0
    }

    /// A turn (yaw, pitch) with up and down turned round when chosen.
    pub fn turn(&self, turn: Vec2) -> Vec2 {
        if self.invert { Vec2::new(turn.x, -turn.y) } else { turn }
    }

    /// A camera's field of view of `degrees` as the setting widens or
    /// narrows it, in radians (kept under 150 degrees).
    pub fn fov_radians(&self, degrees: f32) -> f32 {
        (degrees * self.fov as f32 / 100.0).min(150.0).to_radians()
    }

    /// The same for a field of view already in radians (Skate 3's camera).
    pub fn scale_fov(&self, radians: f32) -> f32 {
        self.fov_radians(radians.to_degrees())
    }
}

/// The step after (`by` 1) or before (`by` -1) `now` in `steps`, stopping
/// at the ends; `by` 0 goes to the next and round from the last to the
/// first (the CHANGE button).
pub fn step(steps: &[u32], now: u32, by: i32) -> u32 {
    let at = steps.iter().position(|s| *s >= now).unwrap_or(steps.len() - 1);
    match by {
        0 => steps[(at + 1) % steps.len()],
        b if b < 0 => steps[at.saturating_sub(1)],
        _ => steps[(at + 1).min(steps.len() - 1)],
    }
}

/// The window's display mode and vsync.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Display {
    /// A window instead of borderless fullscreen.
    pub windowed: bool,
    pub vsync: bool,
}

impl Default for Display {
    fn default() -> Self {
        Display { windowed: false, vsync: true }
    }
}

impl Display {
    /// The saved choice, or borderless fullscreen with vsync.
    pub fn load() -> Display {
        let s = game_paths::settings();
        Display {
            windowed: s.get(game_paths::key::DISPLAY_MODE).is_some_and(|v| v.trim().eq_ignore_ascii_case("windowed")),
            vsync: on(&s, game_paths::key::VSYNC, true),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = game_paths::settings_path();
        let mut s = game_paths::Settings::read(&path);
        s.set(game_paths::key::DISPLAY_MODE, if self.windowed { "windowed" } else { "borderless" });
        s.set(game_paths::key::VSYNC, if self.vsync { "on" } else { "off" });
        s.write(&path)
    }

    pub fn mode(&self) -> WindowMode {
        if self.windowed {
            WindowMode::Windowed
        } else {
            // On the monitor the window is on.
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        }
    }

    pub fn present_mode(&self) -> PresentMode {
        if self.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync }
    }

    /// Sets up a window that is about to open.
    pub fn configure(&self, window: &mut Window) {
        window.mode = self.mode();
        window.present_mode = self.present_mode();
        if self.windowed {
            // A new window has no monitor yet to be centred on.
            window.position = WindowPosition::Centered(MonitorSelection::Primary);
        }
    }

    /// Changes an open window to this: going windowed it gets the windowed
    /// size, centred.
    pub fn apply(&self, window: &mut Window) {
        let was_windowed = window.mode == WindowMode::Windowed;
        if window.mode != self.mode() {
            window.mode = self.mode();
        }
        if self.windowed && !was_windowed {
            window.resolution.set(WINDOWED.x as f32, WINDOWED.y as f32);
            window.position = WindowPosition::Centered(MonitorSelection::Current);
        }
        if window.present_mode != self.present_mode() {
            window.present_mode = self.present_mode();
        }
    }
}

/// The size a window gets on going windowed, in logical pixels (the same
/// as a new window's).
const WINDOWED: UVec2 = UVec2::new(1280, 720);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stop_at_the_ends_and_change_goes_round() {
        assert_eq!(step(&FOV_STEPS, 100, 1), 105);
        assert_eq!(step(&FOV_STEPS, 100, -1), 95);
        assert_eq!(step(&FOV_STEPS, 130, 1), 130);
        assert_eq!(step(&FOV_STEPS, 80, -1), 80);
        assert_eq!(step(&FOV_STEPS, 130, 0), 80);
        assert_eq!(step(&SENSITIVITY_STEPS, 100, 0), 120);
    }

    #[test]
    fn saved_values_land_on_a_step() {
        let mut s = game_paths::Settings::default();
        assert_eq!(LookSettings::from_settings(&s), LookSettings::default());
        s.set(game_paths::key::MOUSE_SENSITIVITY, "155");
        s.set(game_paths::key::FIELD_OF_VIEW, "nonsense");
        s.set(game_paths::key::INVERT_LOOK, "ON");
        let look = LookSettings::from_settings(&s);
        assert_eq!(look.mouse, 150);
        assert_eq!(look.fov, 100);
        assert!(look.invert);
        assert_eq!(look.turn(Vec2::new(1.0, 2.0)), Vec2::new(1.0, -2.0));
        assert!((look.fov_radians(60.0) - 60f32.to_radians()).abs() < 1e-6);
    }
}
