//! The speedometer's unit, chosen on the main menu's settings screen and
//! saved with the other settings (`game_paths::key::SPEED_UNIT`).

use bevy::prelude::*;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpeedUnit {
    #[default]
    Mph,
    Kph,
}

impl SpeedUnit {
    /// The saved unit, or miles per hour.
    pub fn load() -> SpeedUnit {
        match game_paths::settings().get(game_paths::key::SPEED_UNIT) {
            Some(v) if v.eq_ignore_ascii_case("kph") => SpeedUnit::Kph,
            _ => SpeedUnit::Mph,
        }
    }

    pub fn save(self) -> Result<(), String> {
        let mut settings = game_paths::settings();
        settings.set(game_paths::key::SPEED_UNIT, self.label().to_lowercase().as_str());
        settings.write(&game_paths::settings_path())
    }

    pub fn toggled(self) -> SpeedUnit {
        match self {
            SpeedUnit::Mph => SpeedUnit::Kph,
            SpeedUnit::Kph => SpeedUnit::Mph,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SpeedUnit::Mph => "MPH",
            SpeedUnit::Kph => "KPH",
        }
    }

    /// A speed in m/s, in this unit.
    pub fn from_mps(self, mps: f32) -> f32 {
        match self {
            SpeedUnit::Mph => mps * 2.236_936,
            SpeedUnit::Kph => mps * 3.6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts() {
        assert!((SpeedUnit::Kph.from_mps(10.0) - 36.0).abs() < 1e-4);
        assert!((SpeedUnit::Mph.from_mps(10.0) - 22.369).abs() < 1e-2);
        assert_eq!(SpeedUnit::Mph.toggled(), SpeedUnit::Kph);
    }
}
