//! Crash ("wreck") decision, once per frame: the game's WreckChecker (OpenReflex world_wreck_checker.c
//! `006373e0`, called through `00637320` as a physics-world pre-listener, so it sees the flags and state the
//! previous frame's substeps left). There are no lean or pitch angle limits: a crash is a chassis crash-point
//! contact, or a pitch / yaw rate or tyre load past the bike's limit table (`0x009c4c00`), all scaled by the
//! vertical speed.
//!
//! Deliberate deviation: the original opens a stick-flick recovery window between the soft and the hard limit
//! (`0054e8f0` starts it, `0054ea30` runs it). That is left out here, so excursions between soft and hard never
//! wreck; only past-hard values and crash contacts do. What the window does in the original:
//! - Every frame a rate or load is past its soft limit, the checker computes a weight
//!   `0.75 + 0.25 (hard - value) / (hard - soft)` (1 at soft, 0.75 at hard; `00638150`). Past hard the weight is
//!   1e-5 and the rider falls at once (`0054ed10`).
//! - Between soft and hard, if no window is open (rider `+0xb38` <= 0) one opens (`0054e8f0`): it stores the wreck
//!   type, sets the window clock `+0xb38` to the weight (0.75 .. 1.0 s), picks a recovery direction from the type's
//!   class (`00637ee0` / `00637ff0` / table `0x54e9f8`: stick y up, y down, x left, x right, or none) and starts a
//!   wobble animation.
//! - While open, each frame (`0054ea30`) the recovery stick (the pad's wreck steer / lean maps) must move in that
//!   direction past its previous value; the moves accumulate, and more than 0.25 of stick travel in a row saves
//!   the rider (`0054ec80`: the window closes, no wreck). A move the wrong way zeroes the sum. Otherwise the clock
//!   runs down by dt and at 0 the rider falls (`0054ed10`); types whose class has no direction are cancelled
//!   (`0054ec40`) instead of falling.
//! - While the window is open, every frame past a soft limit adds dt to a timer (checker `+0x14`); on a frame
//!   whose type differs from the window's and with the timer past 0.15 s, the timer restarts and the clock loses
//!   that frame's weight; below 0 the rider falls.

/// Wreck types (`veh+0x280`, the checker's `type`).
pub mod reason {
    /// Pitch rate past the nose-down (positive) limit.
    pub const NOSE_DOWN_RATE: u8 = 0x0b;
    /// Pitch rate past the nose-up (negative) limit.
    pub const NOSE_UP_RATE: u8 = 0x0c;
    /// Yaw rate (body y), negative / positive.
    pub const YAW_NEG: u8 = 0x0f;
    pub const YAW_POS: u8 = 0x10;
    /// Mean tyre load, the type picked at random from 0x11 / 0x12 / 0x13 (or none).
    pub const LOAD_A: u8 = 0x11;
    pub const LOAD_B: u8 = 0x12;
    /// Front load; also the "stall" type when the front limit scales to zero.
    pub const FRONT_LOAD: u8 = 0x13;
    pub const REAR_LOAD: u8 = 0x14;
    /// Crash contact (the checker's pending "stuck / impact" type): touch held, crash-point hit, bail combo.
    pub const CONTACT: u8 = 0x17;
}

/// Bike limit table (`0x009c4c00`, read from the original's .data): vertical-speed range and the scale it maps
/// to, then (soft, hard) pairs. `0x009c4c60` is the ATV's.
struct Table {
    vy_range: (f64, f64),
    scale: (f64, f64),
    pitch_up: (f64, f64),
    pitch_down: (f64, f64),
    yaw_neg: (f64, f64),
    yaw_pos: (f64, f64),
    mean_load: (f64, f64),
    front_load: (f64, f64),
    rear_load: (f64, f64),
}

// Layout: +0 / +4 vy range, +8 out_hi, +0xc out_lo, +0x10 / +0x14 impact levels (0.95 / 0.9, used only by the
// recovery window's animation pick), then 18 limits. Limits 8..11 (-6 / -7, 6 / 7: wreck types 0xd / 0xe on
// the third rate lane) are checked only for the ATV class (`rider +0x5bab8 == 1`) and are not used here.
const BIKE: Table = Table {
    vy_range: (-10.0, -65.0),
    scale: (1.5, 0.78),
    pitch_up: (-8.0, -15.0),
    pitch_down: (6.5, 14.0),
    yaw_neg: (-3.0, -8.5),
    yaw_pos: (3.0, 8.5),
    mean_load: (1300.0, 2500.0),
    front_load: (1500.0, 3500.0),
    rear_load: (1500.0, 3000.0),
};

/// Seconds wrecked before the automatic respawn (`0x009c5430`, the crash timer `player +0x1168` set by
/// `0054ed10`), and the part of it that must run before a manual reset is allowed (`0x009c5434`; `0054e470`
/// allows it while `0 < timer < WRECK_DURATION - MIN_WRECK_BEFORE_RESET`).
pub const WRECK_DURATION: f64 = 1.0;
pub const MIN_WRECK_BEFORE_RESET: f64 = 0.5;

/// What the checker reads each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct WreckInputs {
    /// Frame OR over the substeps of slot 4 (`S+0x14`: any crash point touching, or stuck) and slot 5
    /// (`S+0x18`: a crash-point hit).
    pub touch: bool,
    pub hit: bool,
    /// Chassis vertical velocity, ft/s (`S+0x100` y).
    pub vy: f64,
    /// Chassis body-frame angular velocity plus the roll rate the lean constraint removed (`S+0x740 + S+0x8b60`):
    /// x pitch, y yaw.
    pub rate_pitch: f64,
    pub rate_yaw: f64,
    /// Filtered tyre loads, lbf (`S+0x89c0` front, `S+0x8870` rear), and the contact alignment factors E
    /// (`00466bd0`, 1 without contact).
    pub n_front: f64,
    pub n_rear: f64,
    pub e_front: f64,
    pub e_rear: f64,
    /// Both wheels off the ground (`S+0x8d98`).
    pub airborne: bool,
    /// Rear doughnut active (`S+0x8da0`, |factor| > 0.6): a yaw rate past its soft limit ends the checks for
    /// the frame.
    pub doughnut: bool,
}

#[derive(Clone, Debug, Default)]
pub struct WreckChecker {
    /// Impact timer (checker `+0x28`): touches don't build up while it runs.
    cool: f64,
    /// Impact force (checker `+0x24`): time spent touching.
    touch_t: f64,
    /// Pending wreck type (checker `+0x1c`).
    pending: u8,
    /// The bail combo's edge latch (checker `+0x18`).
    hop: bool,
    lcg: u32,
    /// The four-button bail combination held this frame (`00548b70`). The host sets it; a fresh press while
    /// riding wrecks the rider (type 0x17).
    pub bail_combo: bool,
    /// Last wreck reason, 0 = riding.
    pub reason: u8,
    /// Time left until the automatic respawn while wrecked.
    pub wreck_timer: f64,
}

/// `00638150`: 1e-5 past the hard limit (a wreck), else 0.75 (at hard) .. 1.0 (at soft).
fn sev(hard: f64, soft: f64, val: f64) -> f64 {
    if val.abs() > hard.abs() {
        1e-5
    } else {
        (hard - val) / (hard - soft) * 0.25 + 0.75
    }
}

/// `00424370`: maps `v` from [in_lo, in_hi] onto [out_lo, out_hi], clamped; out_lo for an empty input range.
fn remap_clamped(in_lo: f64, in_hi: f64, v: f64, out_lo: f64, out_hi: f64) -> f64 {
    if in_lo == in_hi {
        return out_lo;
    }
    let t = (v - in_lo) / (in_hi - in_lo);
    ((out_hi - out_lo) * t + out_lo).clamp(out_lo.min(out_hi), out_lo.max(out_hi))
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

    /// Asks for a wreck of type `why` at the next check (the original's writes of checker `+0x1c` from outside,
    /// e.g. landing on an unfinished trick, fx_stunt.c). Contact-class types wreck outright.
    pub fn request(&mut self, why: u8) {
        self.pending = why;
    }

    /// One frame. Returns true when the bike wrecks this frame.
    pub fn frame(&mut self, w: &WreckInputs, dt: f64) -> bool {
        // 00637320: nothing while the rider is down
        if self.reason != 0 {
            return false;
        }
        // impact timer and force: touching while the timer is out builds force, else it decays at 0.75 / s
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
        // a crash-point hit: pending contact, the timers untouched
        if w.hit && self.pending == 0 {
            self.pending = reason::CONTACT;
        }
        // the bail combo: acts on a fresh press (the latch toggles while it stays held, as the original's)
        if self.bail_combo {
            if self.hop {
                self.hop = false;
            } else {
                self.hop = true;
                if self.pending == 0 {
                    self.pending = reason::CONTACT;
                }
            }
        } else {
            self.hop = false;
        }

        let t = &BIKE;
        let s = remap_clamped(t.vy_range.0, t.vy_range.1, w.vy, t.scale.0, t.scale.1);
        let l = |p: (f64, f64), e: f64| (p.0 * s * e, p.1 * s * e);
        let (pu, pd, yn, yp) = (l(t.pitch_up, 1.0), l(t.pitch_down, 1.0), l(t.yaw_neg, 1.0), l(t.yaw_pos, 1.0));
        let (ml, fl, rl) = (l(t.mean_load, 1.0), l(t.front_load, w.e_front), l(t.rear_load, w.e_rear));
        // the load checks need the wheels down (vfunc 0xac = S+8d98 clear) and vy < 5 (all lanes of S+0x100.y < 5)
        let grounded_slow = !w.airborne && w.vy < 5.0;
        // 00466b70 with 2: the mean of the filtered loads
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
            // CRT rand(): (r & 3) - 1 picks 0x11 / 0x12 / 0x13, or no type at all (r & 3 == 0)
            self.lcg = self.lcg.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
            why = [0, reason::LOAD_A, reason::LOAD_B, reason::FRONT_LOAD][((self.lcg >> 16) & 3) as usize];
            x = sev(ml.1, ml.0, mean);
        } else if w.n_front > fl.0 && grounded_slow {
            why = reason::FRONT_LOAD;
            x = sev(fl.1, fl.0, w.n_front);
        } else if w.n_front > rl.0 && grounded_slow {
            // the original reads the front load here too: 00466b70 returns S+0x89c0 for both 0 and 1
            why = reason::REAR_LOAD;
            x = sev(rl.1, rl.0, w.n_front);
        }
        // a front limit scaled to zero (the stall check) overrides whatever was chosen
        if fl.0 == 0.0 {
            why = reason::FRONT_LOAD;
            x = sev(fl.1, fl.0, w.n_front);
        }

        // Contact types (class 1, animation weight 1e-5) wreck outright; rate and load types only past the hard
        // limit (weight = x). Soft-limit excursions would open the recovery window here; see the module docs.
        let wrecked = why == reason::CONTACT || (why != 0 && x <= 1e-5);
        if wrecked {
            self.reason = why;
            self.wreck_timer = WRECK_DURATION;
        }
        wrecked
    }

    /// Clears the wreck after a respawn. The respawn fade (`0054b780` / `0054b720`) clears only the pending type
    /// and the impact force; the impact timer, the bail latch and the random state carry on.
    pub fn reset(&mut self) {
        *self = WreckChecker { cool: self.cool, hop: self.hop, lcg: self.lcg, bail_combo: self.bail_combo, ..Default::default() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 1.0 / 60.0;

    fn calm() -> WreckInputs {
        WreckInputs { e_front: 1.0, e_rear: 1.0, ..Default::default() }
    }

    #[test]
    fn hard_limits_and_contacts_wreck_instantly() {
        let mut c = WreckChecker::default();
        let w = WreckInputs { rate_pitch: 30.0, ..calm() };
        assert!(c.frame(&w, DT));
        assert_eq!(c.reason, reason::NOSE_DOWN_RATE);
        let mut c = WreckChecker::default();
        let w = WreckInputs { hit: true, ..calm() };
        assert!(c.frame(&w, DT));
        assert_eq!(c.reason, reason::CONTACT);
    }

    #[test]
    fn soft_limit_excursions_do_not_wreck() {
        // nose-up rate past soft (-8 x 1.5 = -12 at vy 0) but under hard (-15 x 1.5 = -22.5): no wreck, ever
        let mut c = WreckChecker::default();
        let mut w = WreckInputs { rate_pitch: -20.0, ..calm() };
        assert!(!c.frame(&w, DT));
        w.rate_pitch = 0.0;
        assert!(!(0..120).any(|_| c.frame(&w, DT)));
        assert_eq!(c.reason, 0);
    }

    #[test]
    fn speed_scale_and_excess() {
        // 00424370: vy -10 -> 1.5, -65 -> 0.78, clamped outside
        assert_eq!(remap_clamped(-10.0, -65.0, 0.0, 1.5, 0.78), 1.5);
        assert_eq!(remap_clamped(-10.0, -65.0, -100.0, 1.5, 0.78), 0.78);
        assert!((remap_clamped(-10.0, -65.0, -37.5, 1.5, 0.78) - 1.14).abs() < 1e-12);
        // 00638150: 1 at soft, 0.75 at hard, 1e-5 past hard
        assert_eq!(sev(14.0, 6.5, 6.5), 1.0);
        assert_eq!(sev(14.0, 6.5, 14.0), 0.75);
        assert_eq!(sev(14.0, 6.5, 14.1), 1e-5);
    }

    #[test]
    fn falling_fast_lowers_the_limits() {
        // pitch rate 12: under the hard limit at vy 0 (14 x 1.5 = 21), past it at vy -65 (14 x 0.78 = 10.92)
        let mut c = WreckChecker::default();
        assert!(!c.frame(&WreckInputs { rate_pitch: 12.0, ..calm() }, DT));
        assert!(c.frame(&WreckInputs { rate_pitch: 12.0, vy: -65.0, ..calm() }, DT));
    }

    #[test]
    fn doughnut_skips_yaw_and_the_checks_after_it() {
        let mut c = WreckChecker::default();
        // yaw past hard (8.5 x 1.5 = 12.75) and front load past hard: the doughnut ends the checks at the yaw
        let w = WreckInputs { rate_yaw: 20.0, doughnut: true, n_front: 9000.0, ..calm() };
        assert!(!c.frame(&w, DT));
        assert!(c.frame(&WreckInputs { doughnut: false, ..w }, DT));
        assert_eq!(c.reason, reason::YAW_POS);
    }

    #[test]
    fn touch_builds_up_then_cools_down_across_a_respawn() {
        let mut c = WreckChecker::default();
        let w = WreckInputs { touch: true, ..calm() };
        // 0.25 s of touching (16 frames at 60 Hz) wrecks
        let n = (0..60).position(|_| c.frame(&w, DT)).unwrap();
        assert!((14..=15).contains(&n));
        assert_eq!(c.reason, reason::CONTACT);
        // the 4 s impact timer survives the respawn: touching right after does not build up
        c.reset();
        assert!(!(0..60).any(|_| c.frame(&w, DT)));
    }

    #[test]
    fn bail_combo_acts_on_a_fresh_press() {
        let mut c = WreckChecker { bail_combo: true, ..Default::default() };
        assert!(c.frame(&calm(), DT));
        assert_eq!(c.reason, reason::CONTACT);
        // still held after the respawn: the latch toggled off is not a fresh press until it toggles again
        c.reset();
        assert!(!c.frame(&calm(), DT));
        assert!(c.frame(&calm(), DT));
    }

    #[test]
    fn mean_load_can_pick_no_type() {
        // the random pick includes "none": over many frames past the hard mean-load limit, some frames pass
        let w = WreckInputs { n_front: 4000.0, n_rear: 4000.0, ..calm() };
        let mut none = 0;
        let mut c = WreckChecker::default();
        for _ in 0..64 {
            if c.frame(&w, DT) {
                assert!(matches!(c.reason, reason::LOAD_A | reason::LOAD_B | reason::FRONT_LOAD));
                c.reset();
            } else {
                none += 1;
            }
        }
        assert!(none > 0 && none < 64);
    }
}
