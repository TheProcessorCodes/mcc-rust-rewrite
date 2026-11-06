//! Crash ("wreck") decision, once per frame after the substeps: the game's WreckChecker (`006373e0`) and the
//! rider controller's recovery window (`0054e8f0` / `0054ea30`), decoded from the asm (local notes
//! `local\notes\crash.md`). There are no lean or pitch angle limits: a crash is a chassis crash-point contact,
//! or a pitch / yaw rate or tyre load past hard-coded limits, all scaled by the vertical speed. Between the soft
//! and the hard limit a 0.75..1 s recovery window opens that a stick flick (> 0.25 in the right direction) saves.

/// Wreck reasons (`veh+0x280`).
pub mod reason {
    pub const NOSE_DOWN_RATE: u8 = 0x0b;
    pub const NOSE_UP_RATE: u8 = 0x0c;
    pub const YAW_NEG: u8 = 0x0f;
    pub const YAW_POS: u8 = 0x10;
    pub const LOAD_A: u8 = 0x11;
    pub const LOAD_B: u8 = 0x12;
    pub const FRONT_LOAD: u8 = 0x13;
    pub const REAR_LOAD: u8 = 0x14;
    pub const CONTACT: u8 = 0x17;
}

/// The way the rider stick must be flicked (> 0.25 from where it was when the window opened) to save an open
/// recovery window. Reflex shows it on screen as a prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Save {
    Forward,
    Back,
    Left,
    Right,
}

/// Bike limit table (0x009c4c00): vertical-speed scale, then (soft, hard) pairs.
struct Table {
    vy_range: (f64, f64),
    scale: (f64, f64),
    cut: (f64, f64),
    pitch_up: (f64, f64),
    pitch_down: (f64, f64),
    yaw_neg: (f64, f64),
    yaw_pos: (f64, f64),
    mean_load: (f64, f64),
    front_load: (f64, f64),
    rear_load: (f64, f64),
}

const BIKE: Table = Table {
    vy_range: (-10.0, -65.0),
    scale: (1.5, 0.78),
    cut: (0.95, 0.90),
    pitch_up: (-8.0, -15.0),
    pitch_down: (6.5, 14.0),
    yaw_neg: (-3.0, -8.5),
    yaw_pos: (3.0, 8.5),
    mean_load: (1300.0, 2500.0),
    front_load: (1500.0, 3500.0),
    rear_load: (1500.0, 3000.0),
};

/// Seconds wrecked before the automatic respawn (`WreckDuration`), and before a manual one is allowed.
pub const WRECK_DURATION: f64 = 1.0;
pub const MIN_WRECK_BEFORE_RESET: f64 = 0.5;

/// What the checker reads each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct WreckInputs {
    /// Frame OR of slot 4 (touch) and slot 5 (crash-point hit).
    pub touch: bool,
    pub hit: bool,
    /// Chassis vertical velocity, ft/s.
    pub vy: f64,
    /// Chassis body-frame angular velocity plus the roll rate the lean constraint removed: x pitch, y yaw.
    pub rate_pitch: f64,
    pub rate_yaw: f64,
    /// Filtered tyre loads, lbf, and the contact alignment factors E (1 without contact).
    pub n_front: f64,
    pub n_rear: f64,
    pub e_front: f64,
    pub e_rear: f64,
    pub airborne: bool,
    /// Rear doughnut active (|factor| > 0.6): yaw-rate checks are skipped.
    pub doughnut: bool,
    /// Stick used for the save (rider / right stick).
    pub stick: (f64, f64),
}

#[derive(Clone, Debug, Default)]
pub struct WreckChecker {
    cool: f64,
    touch_t: f64,
    pending: u8,
    t14: f64,
    lcg: u32,
    /// Recovery window: time left, kind k, reason, save accumulator, previous stick.
    rec_timer: f64,
    rec_k: u8,
    rec_reason: u8,
    rec_acc: f64,
    rec_prev: (f64, f64),
    /// Last wreck reason, 0 = riding.
    pub reason: u8,
    /// Time left until the automatic respawn while wrecked.
    pub wreck_timer: f64,
}

fn reason_type(r: u8) -> u8 {
    match r {
        0x0b | 0x13 => 7,
        0x0c | 0x14 => 4,
        0x0d | 0x0f | 0x11 => 5,
        0x0e | 0x10 | 0x12 => 6,
        _ => 1,
    }
}

fn type_kt(t: u8) -> (i32, f64) {
    match t {
        4 => (3, 1.0),
        5 => (9, 1.0),
        6 => (12, 1.0),
        7 => (6, 1.0),
        _ => (9, 1e-5),
    }
}

/// Severity: 1e-5 past the hard limit (instant), else 0.75 (at hard) .. 1.0 (at soft).
fn sev(hard: f64, soft: f64, val: f64) -> f64 {
    if val.abs() > hard.abs() {
        1e-5
    } else {
        0.75 + 0.25 * (hard - val) / (hard - soft)
    }
}

impl WreckChecker {
    /// Contact alignment factor E of a tyre (`00466bd0`): contact normal n against the chassis up axis u.
    pub fn alignment(contact: bool, n: crate::Vec3, u: crate::Vec3) -> f64 {
        if !contact {
            return 1.0;
        }
        let a = n.dot(u).max(0.0);
        let cc = (1.1 * a - 0.1).clamp(0.0, 1.0);
        let m = cc.max(0.1);
        let base = if n.y <= 0.0 {
            m
        } else if n.y >= 1.0 {
            cc
        } else {
            m + (cc - m) * n.y
        };
        base.sqrt()
    }

    /// One frame. Returns true when the bike wrecks this frame.
    pub fn frame(&mut self, w: &WreckInputs, dt: f64) -> bool {
        if self.reason != 0 {
            return false;
        }
        // touch timer and contact path
        self.cool = (self.cool - dt).max(0.0);
        if w.touch && self.cool <= 0.0 {
            self.touch_t += dt;
        } else {
            self.touch_t = (self.touch_t - 0.75 * dt).max(0.0);
        }
        if w.touch && self.touch_t > 0.25 {
            if self.pending == 0 {
                self.pending = reason::CONTACT;
            }
            self.cool = 4.0;
            self.touch_t = 0.0;
        }
        if w.hit && self.pending == 0 {
            self.pending = reason::CONTACT;
        }

        let t = &BIKE;
        let s = (t.scale.0 + (t.scale.1 - t.scale.0) * (w.vy - t.vy_range.0) / (t.vy_range.1 - t.vy_range.0))
            .clamp(t.scale.1, t.scale.0);
        let l = |p: (f64, f64), e: f64| (p.0 * s * e, p.1 * s * e);
        let (pu, pd, yn, yp) = (l(t.pitch_up, 1.0), l(t.pitch_down, 1.0), l(t.yaw_neg, 1.0), l(t.yaw_pos, 1.0));
        let (ml, fl, rl) = (l(t.mean_load, 1.0), l(t.front_load, w.e_front), l(t.rear_load, w.e_rear));
        let grounded_slow = !w.airborne && w.vy < 5.0;
        let mean = (w.n_front + w.n_rear) * 0.5;

        let (mut why, mut x) = (0u8, 1.0);
        if self.pending != 0 {
            why = self.pending;
            self.pending = 0;
            x = 0.01;
        } else if w.rate_pitch > pd.0 {
            why = reason::NOSE_DOWN_RATE;
            x = sev(pd.1, pd.0, w.rate_pitch);
        } else if w.rate_pitch < pu.0 {
            why = reason::NOSE_UP_RATE;
            x = sev(pu.1, pu.0, w.rate_pitch);
        } else if w.rate_yaw < yn.0 {
            if !w.doughnut {
                why = reason::YAW_NEG;
                x = sev(yn.1, yn.0, w.rate_yaw);
            }
        } else if w.rate_yaw > yp.0 {
            if !w.doughnut {
                why = reason::YAW_POS;
                x = sev(yp.1, yp.0, w.rate_yaw);
            }
        } else if mean > ml.0 && grounded_slow {
            self.lcg = self.lcg.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
            why = [0, reason::LOAD_A, reason::LOAD_B, reason::FRONT_LOAD][((self.lcg >> 16) & 3) as usize];
            x = sev(ml.1, ml.0, mean);
        } else if w.n_front > fl.0 && grounded_slow {
            why = reason::FRONT_LOAD;
            x = sev(fl.1, fl.0, w.n_front);
        } else if w.n_front > rl.0 && grounded_slow {
            // the original reads the front load here too (00466b70)
            why = reason::REAR_LOAD;
            x = sev(rl.1, rl.0, w.n_front);
        }
        if fl.0 == 0.0 {
            why = reason::FRONT_LOAD;
            x = sev(fl.1, fl.0, w.n_front);
        }

        let wrecked = if why == 0 {
            false
        } else {
            let (mut k, tt) = type_kt(reason_type(why));
            let tt = tt * x;
            if x > t.cut.0 {
                k -= 2;
            } else if x > t.cut.1 {
                k -= 1;
            }
            if tt <= 1e-5 {
                true
            } else if self.rec_timer <= 0.0 {
                self.begin(k, tt, why, w.stick);
                false
            } else {
                self.t14 += dt;
                if why != self.rec_reason && self.t14 > 0.15 {
                    self.t14 = 0.0;
                    self.rec_timer -= tt;
                    self.rec_timer < 0.0
                } else {
                    false
                }
            }
        };
        let wrecked = wrecked || self.recovery(w.stick, dt);
        if wrecked {
            self.reason = if why != 0 { why } else { self.rec_reason };
            self.wreck_timer = WRECK_DURATION;
            self.rec_timer = 0.0;
        }
        wrecked
    }

    fn begin(&mut self, k: i32, t: f64, why: u8, stick: (f64, f64)) {
        self.rec_k = k.max(0) as u8;
        self.rec_timer = t;
        self.rec_reason = why;
        self.rec_acc = 0.0;
        self.rec_prev = stick;
    }

    /// Recovery window step; true when it runs out without a save (and the kind is not self-ending).
    fn recovery(&mut self, stick: (f64, f64), dt: f64) -> bool {
        if self.rec_timer <= 0.0 {
            return false;
        }
        let (sx, sy) = stick;
        let (px, py) = self.rec_prev;
        match self.rec_k {
            2 | 3 => {
                if sy > 0.0 && sy > py {
                    self.rec_acc += sy - py
                } else if sy < py {
                    self.rec_acc = 0.0
                }
            }
            5 | 6 => {
                if sy < 0.0 && sy < py {
                    self.rec_acc += py - sy
                } else if sy > py {
                    self.rec_acc = 0.0
                }
            }
            8 | 9 => {
                if sx < 0.0 && sx < px {
                    self.rec_acc += px - sx
                } else if sx > px {
                    self.rec_acc = 0.0
                }
            }
            11 | 12 => {
                if sx > 0.0 && sx > px {
                    self.rec_acc += sx - px
                } else if sx < px {
                    self.rec_acc = 0.0
                }
            }
            _ => {}
        }
        self.rec_prev = stick;
        if self.rec_acc > 0.25 {
            self.rec_timer = 0.0;
            return false;
        }
        self.rec_timer -= dt;
        if self.rec_timer <= 0.0 {
            let auto = matches!(self.rec_k, 1 | 4 | 7 | 10);
            return !auto;
        }
        false
    }

    /// The open recovery window that wrecks unless saved: the flick it wants and the seconds left. `None` when
    /// no window is open, or the open one ends by itself (the mildest kinds).
    pub fn save_prompt(&self) -> Option<(Save, f64)> {
        if self.reason != 0 || self.rec_timer <= 0.0 {
            return None;
        }
        let save = match self.rec_k {
            2 | 3 => Save::Forward,
            5 | 6 => Save::Back,
            8 | 9 => Save::Left,
            11 | 12 => Save::Right,
            _ => return None,
        };
        Some((save, self.rec_timer))
    }

    /// Clears the wreck after a respawn.
    pub fn reset(&mut self) {
        *self = WreckChecker { lcg: self.lcg, ..Default::default() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_limits_and_contacts_wreck_instantly() {
        let mut c = WreckChecker::default();
        let w = WreckInputs { rate_pitch: 30.0, e_front: 1.0, e_rear: 1.0, ..Default::default() };
        assert!(c.frame(&w, 1.0 / 60.0));
        assert_eq!(c.reason, reason::NOSE_DOWN_RATE);
        let mut c = WreckChecker::default();
        let w = WreckInputs { hit: true, e_front: 1.0, e_rear: 1.0, ..Default::default() };
        assert!(c.frame(&w, 1.0 / 60.0));
        assert_eq!(c.reason, reason::CONTACT);
    }

    #[test]
    fn soft_limit_opens_a_window_a_flick_saves() {
        // nose-up rate just past soft (-8 x 1.5 = -12 at vy 0): window, then a forward flick saves it
        let mut c = WreckChecker::default();
        let mut w = WreckInputs { rate_pitch: -13.0, e_front: 1.0, e_rear: 1.0, ..Default::default() };
        assert!(!c.frame(&w, 1.0 / 60.0));
        // so mild a window ends by itself: no prompt
        assert_eq!(c.save_prompt(), None);
        w.rate_pitch = 0.0;
        w.stick = (0.0, 0.6);
        assert!(!c.frame(&w, 1.0 / 60.0));
        for _ in 0..120 {
            assert!(!c.frame(&w, 1.0 / 60.0));
        }
        assert_eq!(c.save_prompt(), None);
        // no save: the window runs out into a wreck
        let mut c = WreckChecker::default();
        let mut w = WreckInputs { rate_pitch: -20.0, e_front: 1.0, e_rear: 1.0, ..Default::default() };
        assert!(!c.frame(&w, 1.0 / 60.0));
        assert_eq!(c.save_prompt().map(|p| p.0), Some(Save::Forward));
        w.rate_pitch = 0.0;
        let wrecked = (0..120).any(|_| c.frame(&w, 1.0 / 60.0));
        assert!(wrecked);
    }
}
