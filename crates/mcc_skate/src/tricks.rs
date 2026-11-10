//! The trick call-outs, as Skate 3 shows them: each trick's name and points
//! as it is called, the tricks of the sequence in progress with its score and
//! combo multiplier, and the line's running total, which stays a moment
//! after the line ends and fades out. The scoring is the engine's own
//! (`skate_host::bridge::Score`, published with every pose); this only draws
//! it.
//!
//! The engine names tricks by their string IDs (`ID_TRICK_FLIP_KICKFLIP`);
//! Skate 3's text for them is in its localisation, which setup does not
//! convert, so [`trick_name`] spells them out from the ID.
//!
//! `MCC_SHOT_TRICKS=1` makes a `--skate` shot flick the right stick (an
//! ollie, then a kickflip, ...) so the pictures show the call-outs.

use bevy::prelude::*;
use skate_host::bridge::Score;

/// Seconds the line's total stays after the line ends, and of that, how
/// long it takes to fade out.
const LINGER: f32 = 3.0;
const FADE: f32 = 1.0;
/// Seconds after a sequence lands without another trick before the line
/// ends, while the engine's own line timer is not running (it starts only
/// once the combo multiplier is up).
const LINE_GRACE: f32 = 2.5;
/// Tricks of the sequence shown above the latest one.
const CHAIN: usize = 4;

const LIT: Color = Color::srgb(0.98, 0.98, 0.95);
const DIM: Color = Color::srgb(0.75, 0.78, 0.82);
const GOLD: Color = Color::srgb(1.0, 0.8, 0.25);
const BAIL: Color = Color::srgb(1.0, 0.3, 0.25);

/// The call-outs on screen: fed the engine's score with each pose.
#[derive(Resource, Default)]
pub struct Tricks {
    /// The engine's counters last seen (tricks called, sequences published);
    /// `None` until the first score after getting on the board.
    seen: Option<(u32, u32)>,
    /// The sequence's tricks and their points, latest last.
    chain: Vec<(String, f32)>,
    /// The sequence is still open (no landing or bail published since its
    /// last trick).
    open: bool,
    /// The sequence's running score and multiplier.
    sequence: f32,
    multiplier: f32,
    /// The line: every sequence landed since it began.
    line: f32,
    /// Seconds since the last trick or landing, and since the line ended
    /// (`None` while it goes on).
    idle: f32,
    ended: Option<f32>,
    /// The last sequence was a bail.
    bailed: bool,
}

impl Tricks {
    /// Nothing on screen: the board was left, or a new ride began.
    pub fn clear(&mut self) {
        *self = Tricks::default();
    }

    /// Takes the engine's score after `dt` seconds of riding.
    pub fn take(&mut self, score: &Score, dt: f32) {
        let Some((tricks, sequences)) = self.seen else {
            // The first score of a ride: what came before is not his.
            self.seen = Some((score.tricks, score.sequences));
            return;
        };
        self.idle += dt;
        if let Some(t) = self.ended.as_mut() {
            *t += dt;
            if *t > LINGER {
                let seen = self.seen;
                self.clear();
                self.seen = seen;
            }
        }
        if score.tricks != tricks {
            if !self.open {
                self.chain.clear();
                self.bailed = false;
                if self.ended.is_some() {
                    self.line = 0.0;
                    self.ended = None;
                }
            }
            self.open = true;
            self.idle = 0.0;
            self.chain.push((shown_name(score), score.points));
            if self.chain.len() > CHAIN + 1 {
                self.chain.remove(0);
            }
        }
        if score.sequences != sequences {
            self.open = false;
            self.idle = 0.0;
            self.bailed = score.bailed;
            if score.bailed {
                // A bail ends the line with nothing.
                self.line = 0.0;
                self.ended = Some(0.0);
            } else {
                self.sequence = score.last_sequence;
                self.line += score.last_sequence;
            }
        } else if self.open {
            self.sequence = score.sequence;
            self.multiplier = score.multiplier;
        }
        self.seen = Some((score.tricks, score.sequences));
        if !self.open
            && self.ended.is_none()
            && !self.chain.is_empty()
            && score.line_seconds <= 0.0
            && self.idle > LINE_GRACE
        {
            self.ended = Some(0.0);
        }
    }

    /// How opaque the call-outs are: 1 until the line has lingered, then
    /// fading to 0.
    fn alpha(&self) -> f32 {
        match self.ended {
            Some(t) => ((LINGER - t) / FADE).clamp(0.0, 1.0),
            None => 1.0,
        }
    }
}

/// A trick ID as words: `ID_TRICK_FLIP_360_INWARD_HEELFLIP` is
/// "360 Inward Heelflip", `ID_TRICK_GRIND_50_50` "50-50",
/// `ID_TRICK_GRAB_FS_SHIFTY_TAIL_GRAB` "Frontside Shifty Tail Grab".
pub fn trick_name(id: &str) -> String {
    let mut rest = id.strip_prefix("ID_TRICK_").unwrap_or(id);
    for kind in ["GROUND_TRICK_", "FLIP_", "GRAB_", "GRIND_", "TYPE_"] {
        if let Some(r) = rest.strip_prefix(kind) {
            rest = r;
            break;
        }
    }
    let rest = rest
        .replace("50_50", "50-50")
        .replace("5_0", "5-0")
        .replace("POP_SHUVIT", "POP_SHOVE-IT")
        .replace("SHUVIT", "SHOVE-IT")
        .replace("POPSHUV", "POP_SHOVE-IT");
    let words: Vec<String> = rest
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| match w {
            "FS" => "Frontside".into(),
            "BS" => "Backside".into(),
            "N" => "Nollie".into(),
            "NG" => "Nose Grab".into(),
            "TG" => "Tail Grab".into(),
            "ONEFOOT" => "One Foot".into(),
            "DARKCATCH" => "Darkslide Catch".into(),
            "DARKTOLIGHT" => "Dark to Light".into(),
            "FINGERFLIP" => "Fingerflip".into(),
            "NOCOMPLY" => "No Comply".into(),
            "DROPIN" => "Drop In".into(),
            "HIPPYJUMP" => "Hippy Jump".into(),
            "WALLRIDE" => "Wallride".into(),
            _ => {
                // Words with a number stuck on (KICKFLIP2) keep it apart.
                let (word, number) = match w.find(|c: char| c.is_ascii_digit()) {
                    Some(i) if i > 0 => (&w[..i], &w[i..]),
                    _ => (w, ""),
                };
                let mut chars = word.chars();
                let mut s: String = match chars.next() {
                    Some(c) => c.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
                    None => String::new(),
                };
                if !number.is_empty() {
                    s.push(' ');
                    s.push_str(number);
                }
                s
            }
        })
        .collect();
    let name = words.join(" ");
    if name.is_empty() { id.to_string() } else { name }
}

/// The trick as called: its name, switch or fakie in front as Skate 3 says
/// it (nollie tricks already have it in the name).
fn shown_name(score: &Score) -> String {
    let name = trick_name(&score.trick);
    let [switch, fakie, _] = score.stance;
    if switch {
        format!("Switch {name}")
    } else if fakie {
        format!("Fakie {name}")
    } else {
        name
    }
}

/// Points with thousands separated: 12,450.
fn points(p: f32) -> String {
    let n = p.max(0.0).round() as u64;
    let digits = n.to_string();
    let mut s = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            s.push(',');
        }
        s.push(c);
    }
    s
}

#[derive(Component)]
pub(crate) struct TrickHud;
#[derive(Component)]
pub(crate) struct ChainText;
#[derive(Component)]
pub(crate) struct NameText;
#[derive(Component)]
pub(crate) struct ScoreText;
#[derive(Component)]
pub(crate) struct LineText;

/// The call-outs in the lower middle of the screen, hidden until a trick.
pub(crate) fn spawn(mut commands: Commands) {
    commands
        .spawn((
            TrickHud,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                bottom: Val::Percent(18.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.0),
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|p| {
            let text = |size: f32, colour: Color| {
                (
                    Text::new(""),
                    TextFont::from_font_size(size),
                    TextColor(colour),
                    TextShadow::default(),
                    TextLayout::justify(Justify::Center),
                )
            };
            p.spawn((LineText, text(26.0, GOLD)));
            p.spawn((ChainText, text(22.0, DIM)));
            p.spawn((NameText, text(38.0, LIT)));
            p.spawn((ScoreText, text(28.0, LIT)));
        });
}

/// A headless `--shot` draws into an image, not a window: the call-outs go
/// on the 3D camera to be in the picture (as the shield meter does).
pub(crate) fn onto_shot_camera(
    mut commands: Commands,
    windows: Query<(), With<bevy::window::PrimaryWindow>>,
    huds: Query<Entity, (With<TrickHud>, Without<bevy::ui::UiTargetCamera>)>,
    cameras: Query<Entity, With<Camera3d>>,
) {
    if !windows.is_empty() {
        return;
    }
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    for h in &huds {
        commands.entity(h).insert(bevy::ui::UiTargetCamera(camera));
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn show(
    tricks: Res<Tricks>,
    mut root: Query<&mut Visibility, With<TrickHud>>,
    mut texts: Query<(
        &mut Text,
        &mut TextColor,
        Has<LineText>,
        Has<ChainText>,
        Has<NameText>,
        Has<ScoreText>,
    )>,
) {
    let shown = !tricks.chain.is_empty() && tricks.alpha() > 0.0;
    for mut v in &mut root {
        v.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    }
    if !shown {
        return;
    }
    let alpha = tricks.alpha();
    let (latest, earlier) = tricks.chain.split_last().expect("not empty");
    let chain = earlier.iter().rev().take(CHAIN).rev().map(|(n, _)| n.as_str()).collect::<Vec<_>>();
    let name = if latest.1 > 0.0 {
        format!("{}  +{}", latest.0, points(latest.1))
    } else {
        latest.0.clone()
    };
    let score = if tricks.bailed {
        "Bail".to_string()
    } else if tricks.open && tricks.multiplier > 1.01 {
        format!("{}  x{:.1}", points(tricks.sequence), tricks.multiplier)
    } else {
        points(tricks.sequence)
    };
    let line = if tricks.line > 0.0 && tricks.line != tricks.sequence {
        format!("Line {}", points(tricks.line))
    } else {
        String::new()
    };
    for (mut text, mut colour, is_line, is_chain, is_name, is_score) in &mut texts {
        let (s, base) = if is_line {
            (line.clone(), GOLD)
        } else if is_chain {
            (if chain.is_empty() { String::new() } else { format!("{} +", chain.join(" + ")) }, DIM)
        } else if is_name {
            (name.clone(), LIT)
        } else if is_score {
            (score.clone(), if tricks.bailed { BAIL } else { LIT })
        } else {
            continue;
        };
        if text.0 != s {
            text.0 = s;
        }
        colour.set_if_neq(TextColor(base.with_alpha(alpha)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trick_ids_read_as_words() {
        assert_eq!(trick_name("ID_TRICK_FLIP_KICKFLIP"), "Kickflip");
        assert_eq!(trick_name("ID_TRICK_FLIP_360_INWARD_HEELFLIP"), "360 Inward Heelflip");
        assert_eq!(trick_name("ID_TRICK_GRIND_50_50"), "50-50");
        assert_eq!(trick_name("ID_TRICK_GRIND_5_0"), "5-0");
        assert_eq!(trick_name("ID_TRICK_FLIP_FS_POP_SHUVIT"), "Frontside Pop Shove-it");
        assert_eq!(trick_name("ID_TRICK_GRAB_FS_SHIFTY_TAIL_GRAB"), "Frontside Shifty Tail Grab");
        assert_eq!(trick_name("ID_TRICK_GROUND_TRICK_NOSE_MANUAL"), "Nose Manual");
        assert_eq!(trick_name("ID_TRICK_FLIP_KICKFLIP2_UNDERFLIP"), "Kickflip 2 Underflip");
        assert_eq!(trick_name("ID_TRICK_NOCOMPLY"), "No Comply");
        assert_eq!(trick_name("something else"), "Something else");
    }

    #[test]
    fn points_have_thousands_separated() {
        assert_eq!(points(0.0), "0");
        assert_eq!(points(999.4), "999");
        assert_eq!(points(12450.0), "12,450");
        assert_eq!(points(1_234_567.0), "1,234,567");
    }

    fn score(tricks: u32, sequences: u32) -> Score {
        Score {
            tricks,
            sequences,
            trick: "ID_TRICK_FLIP_KICKFLIP".into(),
            points: 20.0,
            multiplier: 1.0,
            ..default()
        }
    }

    /// Two tricks land as two sequences of one line; the line ends a while
    /// after the last and fades out; the next trick starts afresh.
    #[test]
    fn a_line_adds_up_then_fades() {
        let mut t = Tricks::default();
        t.take(&score(5, 3), 0.1);
        assert!(t.chain.is_empty(), "tricks from before the ride are not shown");
        t.take(&Score { sequence_active: true, sequence: 20.0, ..score(6, 3) }, 0.1);
        assert_eq!(t.chain.len(), 1);
        assert!(t.open);
        t.take(&Score { last_sequence: 22.0, ..score(6, 4) }, 0.1);
        assert!(!t.open);
        assert_eq!(t.line, 22.0);
        t.take(&score(7, 4), 0.1);
        assert_eq!(t.chain.len(), 1, "a new sequence starts its own list");
        t.take(&Score { last_sequence: 30.0, ..score(7, 5) }, 0.1);
        assert_eq!(t.line, 52.0);
        for _ in 0..30 {
            t.take(&score(7, 5), 0.1);
        }
        assert!(t.ended.is_some());
        assert_eq!(t.alpha(), 1.0);
        for _ in 0..25 {
            t.take(&score(7, 5), 0.1);
        }
        assert!(t.alpha() < 1.0 && t.alpha() > 0.0, "{}", t.alpha());
        for _ in 0..10 {
            t.take(&score(7, 5), 0.1);
        }
        assert!(t.chain.is_empty(), "gone after lingering");
        t.take(&score(8, 5), 0.1);
        assert_eq!(t.line, 0.0);
        assert_eq!(t.chain.len(), 1);
    }

    #[test]
    fn a_bail_scores_nothing() {
        let mut t = Tricks::default();
        t.take(&score(0, 0), 0.1);
        t.take(&score(1, 0), 0.1);
        t.take(&Score { bailed: true, ..score(1, 1) }, 0.1);
        assert!(t.bailed);
        assert_eq!(t.line, 0.0);
        assert!(t.ended.is_some());
    }
}
