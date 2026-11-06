//! The session marker, the way Skate 3 does it on the board (skate-host's
//! `session_marker`, from the game's `PlayerUI::UpdateSessionMarker`):
//!
//! - LB held + d-pad down pressed sets the marker where the bike is, if it
//!   is upright on both wheels and not crashed.
//! - LB held + d-pad up held takes the bike back to it, at rest, once the
//!   hold has lasted 0.2 s (marker within 100 m) up to 1 s (1000 m and
//!   more). Within half a metre nothing happens; one hold returns once.
//! - Getting on the bike ignores LB until it is let go, as the skate does
//!   after a suspend.

use bevy::prelude::Vec3;

/// The pad buttons the marker reads this frame.
#[derive(Clone, Copy, Default, Debug)]
pub struct MarkerButtons {
    /// LB held.
    pub modifier: bool,
    /// D-pad down pressed this frame.
    pub set: bool,
    /// D-pad up held.
    pub back: bool,
}

/// Where the marker is: the bike's position and heading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub position: Vec3,
    pub heading: f32,
}

/// What `SessionMarker::update` asks the bike to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    None,
    Placed,
    Refused,
    Return(Marker),
}

#[derive(Default, Debug)]
pub struct SessionMarker {
    pub marker: Option<Marker>,
    hold: Hold,
    blocked_until_release: bool,
}

/// The bike as the marker sees it.
#[derive(Clone, Copy, Debug)]
pub struct BikeState {
    pub position: Vec3,
    pub heading: f32,
    /// Both wheels down, upright (up.y above 0.71, as the skate's deck test)
    /// and not crashed.
    pub can_place: bool,
    /// Not lying in the dirt.
    pub can_return: bool,
}

impl SessionMarker {
    /// Getting on: LB does nothing until it is let go.
    pub fn suspend(&mut self) {
        self.hold = Hold::default();
        self.blocked_until_release = true;
    }

    pub fn update(&mut self, buttons: MarkerButtons, bike: BikeState, dt: f32) -> Action {
        if self.blocked_until_release {
            if !buttons.modifier {
                self.blocked_until_release = false;
            }
            return Action::None;
        }
        let mut action = Action::None;
        if buttons.modifier && buttons.set {
            if bike.can_place {
                self.marker = Some(Marker {
                    position: bike.position,
                    heading: bike.heading,
                });
                action = Action::Placed;
            } else {
                action = Action::Refused;
            }
        }
        let distance = self.marker.map_or(0.0, |m| bike.position.distance(m.position));
        let held = buttons.modifier && buttons.back;
        if self.hold.update(held, self.marker.is_some() && bike.can_return, distance, dt)
            && let Some(m) = self.marker
        {
            action = Action::Return(m);
        }
        action
    }
}

#[derive(Default, Debug)]
struct Hold {
    elapsed: f32,
    fired: bool,
}

impl Hold {
    /// True on the frame the hold completes.
    fn update(&mut self, held: bool, usable: bool, distance: f32, dt: f32) -> bool {
        if !held {
            *self = Hold::default();
            return false;
        }
        if self.fired || !usable {
            return false;
        }
        self.elapsed += dt;
        if distance > 0.5 && distance.is_finite() && self.elapsed > duration(distance) {
            self.fired = true;
            return true;
        }
        false
    }
}

/// How long LB + d-pad up is held before the return, seconds: 0.2 s up to
/// 100 m, rising in a line to 1 s at 1000 m.
fn duration(distance: f32) -> f32 {
    if distance <= 100.0 {
        0.2
    } else if distance >= 1000.0 {
        1.0
    } else {
        distance.mul_add(f32::from_bits(0x3a69_0453), f32::from_bits(0x3de3_8e39))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn bike(x: f32) -> BikeState {
        BikeState {
            position: Vec3::new(x, 0.0, 0.0),
            heading: 0.0,
            can_place: true,
            can_return: true,
        }
    }

    const SET: MarkerButtons = MarkerButtons { modifier: true, set: true, back: false };
    const BACK: MarkerButtons = MarkerButtons { modifier: true, set: false, back: true };

    #[test]
    fn set_needs_lb_and_an_upright_bike() {
        let mut m = SessionMarker::default();
        let no_lb = MarkerButtons { modifier: false, set: true, back: false };
        assert_eq!(m.update(no_lb, bike(0.0), DT), Action::None);
        let fallen = BikeState { can_place: false, ..bike(0.0) };
        assert_eq!(m.update(SET, fallen, DT), Action::Refused);
        assert_eq!(m.marker, None);
        assert_eq!(m.update(SET, bike(3.0), DT), Action::Placed);
        assert_eq!(m.marker.unwrap().position.x, 3.0);
    }

    #[test]
    fn return_after_the_hold_and_only_once() {
        let mut m = SessionMarker::default();
        m.update(SET, bike(0.0), DT);
        let returns = (0..120)
            .filter(|_| matches!(m.update(BACK, bike(20.0), DT), Action::Return(_)))
            .count();
        assert_eq!(returns, 1);
        // Released and held again: returns again.
        m.update(MarkerButtons::default(), bike(20.0), DT);
        let tick = (1..=120).find(|_| matches!(m.update(BACK, bike(20.0), DT), Action::Return(_)));
        assert_eq!(tick, Some(13));
    }

    #[test]
    fn nothing_nearby_unset_or_crashed() {
        let mut m = SessionMarker::default();
        for _ in 0..100 {
            assert_eq!(m.update(BACK, bike(10.0), DT), Action::None);
        }
        m.update(SET, bike(0.0), DT);
        for _ in 0..100 {
            assert_eq!(m.update(BACK, bike(0.4), DT), Action::None);
        }
        m.update(MarkerButtons::default(), bike(0.0), DT);
        let crashed = BikeState { can_return: false, ..bike(10.0) };
        for _ in 0..100 {
            assert_eq!(m.update(BACK, crashed, DT), Action::None);
        }
    }

    #[test]
    fn getting_on_waits_for_lb_to_be_let_go() {
        let mut m = SessionMarker::default();
        m.suspend();
        assert_eq!(m.update(SET, bike(0.0), DT), Action::None);
        m.update(MarkerButtons::default(), bike(0.0), DT);
        assert_eq!(m.update(SET, bike(0.0), DT), Action::Placed);
    }

    #[test]
    fn distance_ramp() {
        assert_eq!(duration(100.0), 0.2);
        assert!((duration(550.0) - 0.6).abs() < 1e-6);
        assert_eq!(duration(2000.0), 1.0);
    }
}
