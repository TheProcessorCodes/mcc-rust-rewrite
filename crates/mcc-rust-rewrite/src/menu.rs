//! The map menu: `mcc-rust-rewrite` with no map opens it. Five tabs (Halo 3, Halo 2,
//! Halo CE, Reach, Campaign) hold a grid of the maps found in the user's MCC install, each
//! shown with MCC's own preview picture (`levelstexturepack`, read at run
//! time, never copied into the repo). Start Game decodes the map on a thread
//! behind a loading screen, then puts it into this same app (`main.rs`
//! `install`, `mcc_core::map`); Exit closes the game.
//!
//! MCC lets the player leave out any game but Halo 3 (or part of one): a map
//! whose `.map` file is not in its game's maps folder is still listed, grayed
//! out, and starting it shows a notice to install it with MCC instead of
//! loading.
//!
//! Keys: arrows / WASD / d-pad / left stick move, Q / E, Tab / Shift+Tab,
//! Page Up / Page Down or LB / RB change tab (the hints by the tabs show Q / E,
//! or LB / RB when a controller was used last, `key_hint.rs`),
//! Enter / Space / A start, Escape / B exit. The mouse selects a tile with a
//! click and starts it with a double click. On the notice, Enter / Space /
//! Escape / A / B or a click close it.
//!
//! The Campaign tab groups its maps under headings: the missions that have
//! been played through (Halo CE, Halo 3), then the untested ones. A page
//! taller than the screen scrolls: moving past its bottom (or top) row moves
//! it one row, and the mouse wheel scrolls it.
//!
//! Until the player's games are set up (`mcc_setup`) the menu opens on
//! its setup screen instead (`setup.rs`): Halo MCC, then Skate 3, then MX vs
//! ATV Reflex; only MCC is required. While Skate 3 is not set up (skipped,
//! or its data gone), the settings screen's Skate 3 and Skating rows say so
//! and open that screen's Skate 3 step, to add it later. Set up, the Skate 3
//! row opens the same step, to convert it again or to go without it.
//!
//! The settings screen has pages, like the map tabs: Games (the three
//! games' folders), Player, Controls (look speed, invert look), Display
//! (display mode, vsync, field of view) and Support (open the data folder,
//! copy diagnostics, reset the settings, delete the converted game data;
//! `support.rs`). Q / E, Tab, Page Up / Page Down or LB / RB change page, up /
//! down move, Enter / A or a click on the row's button changes it, and left /
//! right step a value up or down. Reset and delete ask for a second press.
//!
//! `mcc-rust-rewrite --menu-shot FILE [--menu-tab N] [--menu-pick N] [--size WxH]`
//! draws the menu without a window into a PNG; `--menu-loading` draws the
//! loading screen instead, `--menu-missing` the picked map's not-installed
//! notice, `--menu-settings` the settings (with `--menu-step 1`, the Skate 3
//! step as Settings opens it), `--menu-setup [--menu-step N]` the setup
//! screen, and `--menu-start` loads the picked map the way Start Game does
//! and saves the first view of it. `--menu-row N` puts the focus on row N of
//! the settings (and shows its page) or the setup step.

mod setup;
mod support;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;

use mcc_core::skate::SkateEnabled;
use mcc_core::units::SpeedUnit;
use mcc_core::view::{self, LookSettings};
use mcc_core::map::{BackToMenu, MapAppExt, MapInstalled, MapMenu, MapReady, Playing, Warmup};

use crate::Game;
use crate::key_hint::{self, KeyHint};

const TABS: [&str; 5] = ["HALO 3", "HALO 2", "HALO CE", "REACH", "CAMPAIGN"];
/// The Campaign tab's index in `TABS`.
const CAMPAIGN_TAB: usize = TABS.len() - 1;

/// The campaign difficulties, as `--difficulty` takes them.
const DIFFICULTIES: [&str; 4] = ["easy", "normal", "heroic", "legendary"];

/// The saved campaign difficulty (`game_paths::key::DIFFICULTY`), an index
/// into [`DIFFICULTIES`]; normal when there is none.
fn saved_difficulty() -> usize {
    let saved = game_paths::settings().get(game_paths::key::DIFFICULTY).map(|s| s.trim().to_ascii_lowercase());
    saved.and_then(|s| DIFFICULTIES.iter().position(|d| *d == s)).unwrap_or(1)
}

/// Where a map name sits in the menu: its tab and tile.
fn find_entry(arg: &str) -> Option<(usize, usize)> {
    SECTIONS.iter().enumerate().find_map(|(t, sections)| {
        sections.iter().flat_map(|(_, entries)| entries.iter()).position(|e| e.0 == arg).map(|i| (t, i))
    })
}
const COLUMNS: usize = 6;

/// (map argument, shown name, MCC preview texture without `_LARGE` /
/// `LARGE`).
type Entry = (&'static str, &'static str, Option<&'static str>);

const HALO3: &[Entry] = &[
    ("h3:descent", "Assembly", Some("H3MP_ASSEMBLY")),
    ("h3:sidewinder", "Avalanche", Some("H3MP_AVALANCHE")),
    ("h3:lockout", "Blackout", Some("H3MP_BLACKOUT")),
    ("h3:fortress", "Citadel", Some("H3MP_CITADEL")),
    ("h3:chillout", "Cold Storage", Some("H3MP_COLDSTORAGE")),
    ("h3:construct", "Construct", Some("H3MP_CONSTRUCT")),
    ("h3:s3d_edge", "Edge", Some("H3MP_S3D_EDGE")),
    ("h3:salvation", "Epitaph", Some("H3MP_EPITAPH")),
    ("h3:warehouse", "Foundry", Some("H3MP_FOUNDRY")),
    ("h3:ghosttown", "Ghost Town", Some("H3MP_GHOSTTOWN")),
    ("h3:guardian", "Guardian", Some("H3MP_GUARDIAN")),
    ("h3:midship", "Heretic", Some("H3MP_HERETIC")),
    ("h3:deadlock", "High Ground", Some("H3MP_HIGHGROUND")),
    ("h3:s3d_turf", "Icebox", Some("H3MP_S3D_ICEBOX")),
    ("h3:isolation", "Isolation", Some("H3MP_ISOLATION")),
    ("h3:zanzibar", "Last Resort", Some("H3MP_LASTRESORT")),
    ("h3:docks", "Longshore", Some("H3MP_LONGSHORE")),
    ("h3:chill", "Narrows", Some("H3MP_NARROWS")),
    ("h3:spacecamp", "Orbital", Some("H3MP_ORBITAL")),
    ("h3:armory", "Rat's Nest", Some("H3MP_RATSNEST")),
    ("h3:sandbox", "Sandbox", Some("H3MP_SANDBOX")),
    ("h3:shrine", "Sandtrap", Some("H3MP_SANDTRAP")),
    ("h3:snowbound", "Snowbound", Some("H3MP_SNOWBOUND")),
    ("h3:bunkerworld", "Standoff", Some("H3MP_STANDOFF")),
    ("h3:cyberdyne", "The Pit", Some("H3MP_THEPIT")),
    ("h3:riverworld", "Valhalla", Some("H3MP_VALHALLA")),
    ("h3:s3d_waterfall", "Waterfall", Some("H3MP_S3D_WATERFALL")),
];

const HALO2: &[Entry] = &[
    ("h2:ascension", "Ascension", Some("H2MP_ASCENSION")),
    ("h2:backwash", "Backwash", Some("H2MP_BACKWASH")),
    ("h2:beavercreek", "Beaver Creek", Some("H2MP_BEAVERCREEK")),
    ("h2:burial_mounds", "Burial Mounds", Some("H2MP_BURIALMOUNDS")),
    ("h2:coagulation", "Coagulation", Some("H2MP_COAGULATION")),
    ("h2:colossus", "Colossus", Some("H2MP_COLOSSUS")),
    ("h2:containment", "Containment", Some("H2MP_CONTAINMENT")),
    ("h2:derelict", "Desolation", Some("H2MP_DESOLATION")),
    ("h2:street_sweeper", "District", Some("H2MP_DISTRICT")),
    ("h2:elongation", "Elongation", Some("H2MP_ELONGATION")),
    ("h2:foundation", "Foundation", Some("H2MP_FOUNDATIONS")),
    ("h2:gemini", "Gemini", Some("H2MP_GEMINI")),
    ("h2:headlong", "Headlong", Some("H2MP_HEADLONG")),
    ("h2:cyclotron", "Ivory Tower", Some("H2MP_IVORYTOWER")),
    ("h2:lockout", "Lockout", Some("H2MP_LOCKOUT")),
    ("h2:midship", "Midship", Some("H2MP_MIDSHIP")),
    ("h2:dune", "Relic", Some("H2MP_RELIC")),
    ("h2:deltatap", "Sanctuary", Some("H2MP_SANCTUARY")),
    ("h2:triplicate", "Terminal", Some("H2MP_TERMINAL")),
    ("h2:highplains", "Tombstone", Some("H2MP_TOMBSTONE")),
    ("h2:turf", "Turf", Some("H2MP_TURF")),
    ("h2:needle", "Uplift", Some("H2MP_UPLIFT")),
    ("h2:warlock", "Warlock", Some("H2MP_WARLOCK")),
    ("h2:waterworks", "Waterworks", Some("H2MP_WATERWORKS")),
    ("h2:zanzibar", "Zanzibar", Some("H2MP_ZANZIBAR")),
];

/// Halo CE's multiplayer maps (MCC's `halo1\maps`).
const HALO1: &[Entry] = &[
    ("h1:beavercreek", "Battle Creek", Some("H1MP_BATTLECREEK")),
    ("h1:bloodgulch", "Blood Gulch", Some("H1MP_BLOODGULCH")),
    ("h1:boardingaction", "Boarding Action", Some("H1MP_BOARDINGACTION")),
    ("h1:chillout", "Chill Out", Some("H1MP_CHILLOUT")),
    ("h1:putput", "Chiron TL-34", Some("H1MP_CHIRONTL34")),
    ("h1:damnation", "Damnation", Some("H1MP_DAMNATION")),
    ("h1:dangercanyon", "Danger Canyon", Some("H1MP_DANGERCANYON")),
    ("h1:deathisland", "Death Island", Some("H1MP_DEATHISLAND")),
    ("h1:carousel", "Derelict", Some("H1MP_DERELICT")),
    ("h1:gephyrophobia", "Gephyrophobia", Some("H1MP_GEPHYROPHOBIA")),
    ("h1:hangemhigh", "Hang 'Em High", Some("H1MP_HANGEMHIGH")),
    ("h1:icefields", "Ice Fields", Some("H1MP_ICEFIELDS")),
    ("h1:infinity", "Infinity", Some("H1MP_INFINITY")),
    ("h1:longest", "Longest", Some("H1MP_LONGEST")),
    ("h1:prisoner", "Prisoner", Some("H1MP_PRISONER")),
    ("h1:ratrace", "Rat Race", Some("H1MP_RATRACE")),
    ("h1:sidewinder", "Sidewinder", Some("H1MP_SIDEWINDER")),
    ("h1:timberland", "Timberland", Some("H1MP_TIMBERLAND")),
    ("h1:wizard", "Wizard", Some("H1MP_WIZARD")),
];

/// Halo Reach's multiplayer maps (MCC's `haloreach\maps`; the Firefight
/// maps are left out).
const REACH: &[Entry] = &[
    ("hr:dlc_slayer", "Anchor 9", Some("HREACHMP_ANCHOR9")),
    ("hr:cex_beaver_creek", "Battle Canyon", Some("HREACHMP_BATTLECANYON")),
    ("hr:50_panopticon", "Boardwalk", Some("HREACHMP_BOARDWALK")),
    ("hr:70_boneyard", "Boneyard", Some("HREACHMP_BONEYARD")),
    ("hr:cex_headlong", "Breakneck", Some("HREACHMP_BREAKNECK")),
    ("hr:dlc_invasion", "Breakpoint", Some("HREACHMP_BREAKPOINT")),
    ("hr:condemned", "Condemned", Some("HREACHMP_CONDEMNED")),
    ("hr:45_launch_station", "Countdown", Some("HREACHMP_COUNTDOWN")),
    ("hr:forge_halo", "Forge World", Some("HREACHMP_FORGEWORLD")),
    ("hr:trainingpreserve", "Highlands", Some("HREACHMP_HIGHLANDS")),
    ("hr:cex_hangemhigh", "High Noon", Some("HREACHMP_HIGHNOON")),
    ("hr:cex_damnation", "Penance", Some("HREACHMP_PENANCE")),
    ("hr:30_settlement", "Powerhouse", Some("HREACHMP_POWERHOUSE")),
    ("hr:52_ivory_tower", "Reflection", Some("HREACHMP_REFLECTION")),
    ("hr:cex_timberland", "Ridgeline", Some("HREACHMP_RIDGELINE")),
    ("hr:cex_prisoner", "Solitary", Some("HREACHMP_SOLITARY")),
    ("hr:35_island", "Spire", Some("HREACHMP_SPIRE")),
    ("hr:20_sword_slayer", "Sword Base", Some("HREACHMP_SWORDBASE")),
    ("hr:dlc_medium", "Tempest", Some("HREACHMP_TEMPEST")),
    ("hr:45_aftship", "Zealot", Some("HREACHMP_ZEALOT")),
];

/// The Campaign tab, under headings: the missions we have played through
/// by game, then the rest, which nobody has tried yet (Halo 3's in story
/// order; Crow's Nest has no preview in MCC).
const CAMPAIGN: &[Section] = &[
    // Halo CE's: The Maw from the Warthogs on (`main.rs` `MISSIONS`).
    (Some("HALO CE"), &[("h1:warthog_run", "The Warthog Run", Some("H1_C10_THEMAW"))]),
    // A mission of ours (`MISSIONS`): Halo from the Warthog run on.
    (Some("HALO 3"), &[("warthog_run", "The Warthog Run", Some("H3_C10_HALO3"))]),
    (
        Some("UNTESTED"),
        &[
            ("h3:005_intro", "Arrival", Some("H3_C01_ARRIVAL")),
            ("h3:010_jungle", "Sierra 117", Some("H3_C02_SIERRA117")),
            ("h3:020_base", "Crow's Nest", None),
            ("h3:030_outskirts", "Tsavo Highway", Some("H3_C04_TSAVOHIGHWAY")),
            ("h3:040_voi", "The Storm", Some("H3_C05_THESTORM")),
            ("h3:050_floodvoi", "Floodgate", Some("H3_C06_FLOODGATE")),
            ("h3:070_waste", "The Ark", Some("H3_C07_THEARK")),
            ("h3:100_citadel", "The Covenant", Some("H3_C08_THECOVENANT")),
            ("h3:110_hc", "Cortana", Some("H3_C09_CORTANA")),
            ("h3:120_halo", "Halo", Some("H3_C10_HALO3")),
            ("h3:130_epilogue", "Epilogue", Some("H3_C11_EPILOGUE")),
        ],
    ),
];

/// A run of a tab's maps under a heading (`None`: no heading).
type Section = (Option<&'static str>, &'static [Entry]);

/// Every tab's sections, in [`TABS`] order. A tab's maps are its sections'
/// entries one after the other.
const SECTIONS: [&[Section]; TABS.len()] = [&[(None, HALO3)], &[(None, HALO2)], &[(None, HALO1)], &[(None, REACH)], CAMPAIGN];

/// A tab's rows of tiles as (first map, how many): each section starts a new
/// row.
fn rows(tab: usize) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    let mut first = 0;
    for (_, entries) in SECTIONS[tab] {
        for at in (0..entries.len()).step_by(COLUMNS) {
            rows.push((first + at, (entries.len() - at).min(COLUMNS)));
        }
        first += entries.len();
    }
    rows
}

/// The tile `dx` tiles along and `dy` rows down from `at`: along runs on
/// across sections, down keeps the column (or the last tile of a shorter
/// row); both stop at the ends.
fn moved(rows: &[(usize, usize)], at: usize, dx: isize, dy: isize) -> usize {
    let Some(&(last, len)) = rows.last() else {
        return 0;
    };
    let at = (at as isize + dx).clamp(0, (last + len) as isize - 1) as usize;
    if dy == 0 {
        return at;
    }
    let r = rows.iter().rposition(|&(first, _)| first <= at).unwrap_or(0);
    let column = at - rows[r].0;
    let (first, len) = rows[(r as isize + dy).clamp(0, rows.len() as isize - 1) as usize];
    first + column.min(len - 1)
}

const KINDS: [&str; 5] = [
    "Halo 3 multiplayer",
    "Halo 2 multiplayer",
    "Halo CE multiplayer",
    "Halo Reach multiplayer",
    "Halo 3 campaign",
];

/// What the footer calls a tile: its tab's kind, except Halo CE's mission
/// on the Campaign tab.
fn kind(tab: usize, map: &Map) -> &'static str {
    match map.game {
        Game::Halo1 if tab == KINDS.len() - 1 => "Halo CE campaign",
        _ => KINDS[tab],
    }
}

const BACKGROUND: Color = Color::srgb(0.043, 0.059, 0.082);
const BAR: Color = Color::srgb(0.071, 0.098, 0.141);
const LINE: Color = Color::srgb(0.149, 0.192, 0.259);
const DIM: Color = Color::srgb(0.65, 0.70, 0.78);
const FAINT: Color = Color::srgb(0.34, 0.39, 0.48);
const LIT: Color = Color::srgb(1.0, 0.82, 0.35);
const EMPTY: Color = Color::srgb(0.10, 0.13, 0.18);

/// The spinner: a Halo ring of segments with a bright arc running round it.
const RING_SEGMENTS: usize = 32;
const RING_RADIUS: f32 = 70.0;
/// Turns per second.
const RING_SPEED: f32 = 0.6;

/// A missing map's picture is drawn through this tint.
const MISSING_TINT: Color = Color::srgb(0.30, 0.30, 0.32);

/// One map the menu offers.
struct Map {
    arg: &'static str,
    name: &'static str,
    picture: Option<Handle<Image>>,
    game: Game,
    /// Its `.map` file is in the player's MCC ([`installed`]).
    installed: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Choosing,
    Loading,
    /// The notice over the chooser: this game's map is not installed.
    Missing(Game),
    /// The settings screen.
    Settings,
    /// The first-run setup screen (`setup.rs`).
    Setup,
}

/// The map file a menu entry plays: its own, or a mission's
/// (`main.rs` `MISSIONS`).
fn map_of(arg: &str) -> &str {
    crate::mission(arg).map_or(arg, |m| m.map)
}

/// The game a menu entry's map belongs to (every map has its prefix).
fn game_of(arg: &str) -> Game {
    crate::codename(map_of(arg)).0.unwrap_or(Game::Halo3)
}

/// The game as MCC names it, for the notice.
fn game_title(game: Game) -> &'static str {
    match game {
        Game::Halo1 => "Halo: Combat Evolved",
        Game::Halo2 => "Halo 2",
        Game::Halo3 => "Halo 3",
        Game::Reach => "Halo: Reach",
    }
}

/// Whether a menu entry's map is in the player's MCC: its `.map` file in its
/// game's maps folder ([`crate::maps_dir`]). Checked per map, as MCC can
/// install part of a game.
fn installed(arg: &str) -> bool {
    let (game, file) = crate::codename(map_of(arg));
    has_map(&crate::maps_dir(game.unwrap_or(Game::Halo3)), file)
}

/// `dir` holds `file.map` (an empty `dir`, before setup, holds nothing).
fn has_map(dir: &Path, file: &str) -> bool {
    !dir.as_os_str().is_empty() && dir.join(format!("{file}.map")).is_file()
}

#[derive(Resource)]
struct Menu {
    tabs: [Vec<Map>; TABS.len()],
    tab: usize,
    /// The selected tile on each tab.
    pick: [usize; TABS.len()],
    phase: Phase,
    /// Arguments for the map (`--foot`, `--zone`, ...).
    args: Vec<String>,
    /// The last click: tile and time, for the double click.
    click: Option<(usize, f64)>,
    /// Why the last map did not load.
    error: Option<String>,
    /// The notice, or the setup screen from Settings, opened this frame: the
    /// press that opened it does not act on it too ([`dismiss`],
    /// `setup::input`).
    fresh: bool,
    /// The settings row with the focus.
    setting: usize,
    /// A folder picker (and, for Skate 3, its conversion) running on its thread.
    picking: Option<Picking>,
    /// What the settings screen says under the rows.
    note: String,
    /// The username being typed (Enter saves it, Escape drops it).
    editing: Option<String>,
    /// The setup screen's step: 0 Halo MCC, 1 Skate 3, 2 MX vs ATV Reflex.
    step: usize,
    /// `note` is a problem, not news.
    note_is_error: bool,
    /// Every game is set up: the setup screen gives way to the maps.
    setup_finished: bool,
    /// The setup screen's Skate 3 step opened from Settings
    /// (`setup::open_from_settings`): the settings row to go back to.
    setup_return: Option<usize>,
    /// "Without Skate 3" was chosen once with Skate 3 set up and said what
    /// it deletes: choosing it again does it (`setup::choose`).
    confirm_skip: bool,
    /// The settings row (Reset settings, Delete converted data) that was
    /// pressed once and said what it does: pressing it again does it.
    confirm: Option<usize>,
    /// What converted game data there is (`support::converted`), worked
    /// out when Settings opens and after deleting it.
    converted: String,
    /// The campaign difficulty, an index into [`DIFFICULTIES`]: the Campaign
    /// tab's levels start on it (Y or F2 changes it, and it is saved).
    difficulty: usize,
    /// Starting a map saves it as the last one, to open on next time; not in
    /// a headless shot, which never writes the setup.
    remember: bool,
}

/// A folder picker (and, for Skate 3, its conversion), on its thread.
struct Picking {
    what: Pick,
    result: Mutex<Receiver<Result<Option<PathBuf>, String>>>,
}

/// What a [`Picking`] is choosing for.
#[derive(Clone, Copy, PartialEq)]
enum Pick {
    /// A settings row.
    Setting(usize),
    /// A setup screen row.
    Setup(setup::SetupRow),
}

impl Menu {
    fn selected(&self) -> Option<&Map> {
        self.tabs[self.tab].get(self.pick[self.tab])
    }

    /// The command line the picked map loads with: a campaign level gets
    /// the chosen difficulty, unless `--difficulty` was given.
    fn map_args(&self) -> Vec<String> {
        let mut args = self.args.clone();
        if self.tab == CAMPAIGN_TAB && !args.iter().any(|a| a == "--difficulty") {
            args.extend(["--difficulty".to_string(), DIFFICULTIES[self.difficulty].to_string()]);
        }
        args
    }

    /// The next difficulty, saved.
    fn next_difficulty(&mut self) {
        self.difficulty = (self.difficulty + 1) % DIFFICULTIES.len();
        if self.remember
            && let Err(e) = game_paths::save_setting(game_paths::key::DIFFICULTY, DIFFICULTIES[self.difficulty])
        {
            eprintln!("menu: the difficulty was not saved: {e}");
        }
    }
}

/// The map being decoded on its thread.
#[derive(Resource)]
struct Loading(Mutex<Receiver<anyhow::Result<crate::LoadedMap>>>);

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct MenuCamera;
#[derive(Component)]
struct Chooser;
#[derive(Component)]
struct LoadingScreen;
#[derive(Component)]
struct LoadingName;
/// Under the name: what the map is still getting ready ([`Warmup`]).
#[derive(Component)]
struct LoadingStatus;
#[derive(Component)]
struct LoadingPicture;
#[derive(Component)]
struct RingSegment(usize);
#[derive(Component)]
struct TabLabel(usize);
/// A tab's page of maps, scrolled to keep the picked tile in view
/// ([`follow`]).
#[derive(Component)]
struct TabPage(usize);
/// A section on a tab's page: tab, its first map.
#[derive(Component)]
struct SectionBox(usize, usize);
/// A tile with its name under it: tab, map.
#[derive(Component)]
struct TileCell(usize, usize);
#[derive(Component)]
struct Tile(usize, usize);
#[derive(Component)]
struct TileLabel(usize, usize);
/// The bar's text: 0 the map's name, 1 which game and mode.
#[derive(Component)]
struct Chosen(usize);
#[derive(Component)]
struct StartLabel;
/// The not-installed notice over the chooser.
#[derive(Component)]
struct Notice;
/// The notice's line naming the game.
#[derive(Component)]
struct NoticeGame;
#[derive(Component, Clone, Copy, PartialEq)]
enum MenuButton {
    /// The campaign difficulty, only on the Campaign tab.
    Difficulty,
    Settings,
    Start,
    Exit,
}
/// The difficulty button's text.
#[derive(Component)]
struct DifficultyLabel;
/// The settings screen.
#[derive(Component)]
struct SettingsScreen;
/// One row of the settings screen, an index into `SETTING_NAMES`.
#[derive(Component)]
struct SettingRow(usize);
#[derive(Component)]
struct SettingValue(usize);
/// A row's Select button, and its label.
#[derive(Component)]
struct SettingSelect(usize);
#[derive(Component)]
struct SettingSelectLabel(usize);
#[derive(Component)]
struct SettingsNote;
#[derive(Component)]
struct SettingsBack;
/// A page's name in the settings' tab bar, an index into `SETTING_PAGES`.
#[derive(Component)]
struct SettingTab(usize);
/// The bar's "left / right step a value", shown on the pages with values.
#[derive(Component)]
struct SettingsStepHint;

/// The settings, in row order, page by page (`SETTING_PAGES`): three games'
/// folders; the username, the armour colour, the speed unit, skating on or
/// off, the dialogue language, the bike's physics; the mouse's and the
/// stick's speed and invert look; the display mode, vsync and the field of
/// view; the data folder, diagnostics, reset and the converted data.
const SETTING_NAMES: [&str; 20] = [
    "Halo MCC",
    "MX vs ATV Reflex",
    "Skate 3",
    "Username",
    "Armour colour",
    "Speedometer",
    "Skating",
    "Trick display",
    "Dialogue language",
    "Bike physics",
    "Mouse sensitivity",
    "Stick sensitivity",
    "Invert look",
    "Display mode",
    "VSync",
    "Field of view",
    "Data folder",
    "Diagnostics",
    "Reset settings",
    "Converted data",
];
/// The settings' pages: their names in the tab bar and their rows.
const SETTING_PAGES: [(&str, std::ops::Range<usize>); 5] = [
    ("GAMES", 0..3),
    ("PLAYER", 3..10),
    ("CONTROLS", 10..13),
    ("DISPLAY", 13..16),
    ("SUPPORT", 16..20),
];
/// The row holding Skate 3's folder.
const SKATE_ROW: usize = 2;
/// The row holding the username.
const USERNAME_ROW: usize = 3;
/// The row choosing the Spartan's armour colour.
const ARMOUR_ROW: usize = 4;
/// The row holding the speedometer's unit.
const UNITS_ROW: usize = 5;
/// The row turning skating on and off.
const SKATING_ROW: usize = 6;
/// The row turning the skate tricks' names and score on (off by default).
const TRICKS_ROW: usize = 7;
/// The row choosing the language Halo's dialogue plays in.
const LANGUAGE_ROW: usize = 8;
/// The row switching the bike between Arcade and Reflex physics, the last
/// on the Player page; only shown with Reflex set up (it is optional).
const PHYSICS_ROW: usize = 9;
/// The mouse's turning speed (`mcc_core::view::LookSettings`).
const MOUSE_ROW: usize = 10;
/// The right stick's turning speed.
const STICK_ROW: usize = 11;
/// Looking up and down turned round.
const INVERT_ROW: usize = 12;
/// Borderless fullscreen or a window (`mcc_core::view::Display`).
const DISPLAY_ROW: usize = 13;
const VSYNC_ROW: usize = 14;
/// Every camera's field of view.
const FOV_ROW: usize = 15;
/// Opens the data folder.
const FOLDER_ROW: usize = 16;
/// Copies the diagnostics summary.
const DIAGNOSTICS_ROW: usize = 17;
/// Puts the settings back to their defaults (asks first).
const RESET_ROW: usize = 18;
/// Deletes the converted Skate 3 and Reflex data (asks first).
const DELETE_ROW: usize = 19;

/// A dialogue language (`game_paths::LANGUAGES`) as the row shows it.
fn language_label(language: &str) -> &'static str {
    match language {
        "french" => "French",
        "german" => "German",
        "italian" => "Italian",
        "mexican" => "Spanish (Mexico)",
        "spanish" => "Spanish (Spain)",
        _ => "English",
    }
}

/// The language after `language` in `game_paths::LANGUAGES`, going round.
fn next_language(language: &str) -> &'static str {
    let all = game_paths::LANGUAGES;
    let at = all.iter().position(|l| *l == language).unwrap_or(all.len() - 1);
    all[(at + 1) % all.len()]
}

/// Whether the physics row is there: Reflex is set up.
fn physics_offered() -> bool {
    game_paths::reflex_ready()
}

/// Whether a settings row is shown now (the physics row needs Reflex).
fn row_offered(row: usize) -> bool {
    row < SETTING_NAMES.len() && (row != PHYSICS_ROW || physics_offered())
}

/// The page a settings row is on.
fn page_of(row: usize) -> usize {
    SETTING_PAGES.iter().position(|(_, rows)| rows.contains(&row)).unwrap_or(0)
}

/// The rows a page shows now.
fn page_rows(page: usize) -> Vec<usize> {
    SETTING_PAGES[page].1.clone().filter(|r| row_offered(*r)).collect()
}

/// A row's button: games pick a folder, the username is typed, the unit
/// changes. While Skate 3 is not set up (`skate_ready`), Skate 3 and Skating
/// both set it up first (the setup screen's Skate 3 step); set up, Skate 3's
/// row opens that step to convert it again or to go without it. The Support
/// page's rows each do one thing.
fn button_label(row: usize, skate_ready: bool) -> &'static str {
    match row {
        SKATE_ROW | SKATING_ROW if !skate_ready => "SET UP",
        USERNAME_ROW => "EDIT",
        SKATE_ROW | ARMOUR_ROW | UNITS_ROW | SKATING_ROW | TRICKS_ROW | LANGUAGE_ROW | PHYSICS_ROW => "CHANGE",
        MOUSE_ROW | STICK_ROW | INVERT_ROW | DISPLAY_ROW | VSYNC_ROW | FOV_ROW => "CHANGE",
        FOLDER_ROW => "OPEN",
        DIAGNOSTICS_ROW => "COPY",
        RESET_ROW => "RESET",
        DELETE_ROW => "DELETE",
        _ => "SELECT",
    }
}

/// What each row shows now: the saved folder, or why there is none.
/// `skate_ready`: Skate 3 is set up (`game_paths::skate_ready`); without it
/// skating is not there to turn on. `converted`: what converted data there
/// is (`support::converted`).
fn setting_values(
    unit: SpeedUnit,
    skate: SkateEnabled,
    reflex_physics: bool,
    skate_ready: bool,
    look: LookSettings,
    converted: &str,
) -> [String; SETTING_NAMES.len()] {
    let shown = |p: Option<PathBuf>| p.map(|p| p.display().to_string());
    let settings = game_paths::settings();
    let reflex = match game_paths::reflex_chosen() {
        Some(p) if game_paths::reflex_imported() => format!("Imported from {}", p.display()),
        chosen => shown(chosen.filter(|_| game_paths::reflex_ready())).unwrap_or_else(|| {
            if game_paths::reflex_skipped() { "Not used (optional)" } else { "Not set (optional)" }.to_string()
        }),
    };
    let skate_folder = if skate_ready {
        settings
            .get(game_paths::key::SKATE_SOURCE)
            .map_or_else(|| "Set up (folder not recorded, select to choose it)".to_string(), str::to_owned)
    } else if game_paths::skate_skipped() {
        "Not used (optional): adds the skateboard".to_string()
    } else {
        "Not set (optional): adds the skateboard".to_string()
    };
    let skating = if !skate_ready {
        "Not available: needs Skate 3 (SET UP adds it)"
    } else if skate.0 {
        "On (skateboard and grind rails on every map)"
    } else {
        "Off (maps load without the skateboard or grind rails)"
    };
    let display = view::Display::load();
    [
        shown(game_paths::mcc_root()).unwrap_or_else(|| "Not set".to_string()),
        reflex,
        skate_folder,
        game_paths::username(),
        format!("{} (the Spartan on multiplayer maps, from the next map you load)", mcc_rider::ARMOUR_COLOURS[mcc_rider::armour()].0),
        format!("{} (miles per hour or kilometres per hour)", unit.label()),
        skating.to_string(),
        if game_paths::trick_display() {
            "On (each trick's name and points on screen while skating)"
        } else {
            "Off (no trick names or score on screen)"
        }
        .to_string(),
        format!("{} (the voices in Halo 3 and Halo: Combat Evolved)", language_label(game_paths::language())),
        if reflex_physics {
            "Reflex (MX vs ATV Reflex's own bike simulation)"
        } else {
            "Arcade (the bike's own physics)"
        }
        .to_string(),
        format!("{} % (how fast the mouse turns the camera; left / right step it)", look.mouse),
        format!("{} % (how fast the right stick turns the camera; left / right step it)", look.stick),
        if look.invert { "On (pushing up looks down, mouse and stick)" } else { "Off (pushing up looks up)" }.to_string(),
        if display.windowed { "Windowed" } else { "Borderless fullscreen" }.to_string(),
        if display.vsync {
            "On (the frame rate keeps to the screen's, no tearing)"
        } else {
            "Off (frames as fast as they come, may tear)"
        }
        .to_string(),
        format!("{} % of each camera's own (wider shows more; left / right step it)", look.fov),
        game_paths::settings_dir().display().to_string(),
        "Copies the version, your setup and where the log is, to paste into a bug report".to_string(),
        "Puts every setting back to its default; keeps your game folders".to_string(),
        converted.to_string(),
    ]
}

/// MCC's install root, as first-run setup saved it (`game_paths`).
fn mcc_root() -> PathBuf {
    game_paths::mcc_root().unwrap_or_default()
}

/// MCC's map pictures: `levelstexturepack.perm.bin` holds a header per
/// texture (a magic, the name at +4, width / height u16 at +0x34, byte size
/// at +0x48, offset into `.temp.bin` at +0x50); the data is DXT1.
struct Pack {
    perm: Vec<u8>,
    temp: Vec<u8>,
}

impl Pack {
    fn open(root: &Path) -> Option<Pack> {
        let dir = root.join(r"Data\UI\texturepacks");
        let perm = std::fs::read(dir.join("levelstexturepack.perm.bin")).ok()?;
        let temp = std::fs::read(dir.join("levelstexturepack.temp.bin")).ok()?;
        Some(Pack { perm, temp })
    }

    /// `NAME_LARGE` (or Reach's `NAMELARGE`) as RGBA.
    fn picture(&self, name: &str) -> Option<Image> {
        let find = |want: String| self.perm.windows(want.len()).position(|w| w == want.as_bytes());
        let at = find(format!("{name}_LARGE\0")).or_else(|| find(format!("{name}LARGE\0")))?.checked_sub(4)?;
        let e = self.perm.get(at..at + 0x54)?;
        let u16_at = |o: usize| u16::from_le_bytes([e[o], e[o + 1]]) as u32;
        let u32_at = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap()) as usize;
        let (w, h) = (u16_at(0x34), u16_at(0x36));
        let (size, offset) = (u32_at(0x48), u32_at(0x50));
        let data = self.temp.get(offset..offset + size)?;
        let rgba = blam_texture::rgba::decode_level(blam_texture::Format::Bc1, w, h, data).ok()?;
        Some(Image::new(
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            TextureDimension::D2,
            rgba,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        ))
    }
}

/// `--menu-shot`: where the headless picture goes, its size, and what it
/// shows.
#[derive(Resource)]
struct MenuShot {
    path: PathBuf,
    size: UVec2,
    loading: bool,
    /// `--menu-missing`: the picked map's not-installed notice.
    missing: bool,
    start: bool,
    frames: u32,
    /// Frames since the map was ready (`--menu-start`).
    ready_frames: u32,
    /// Maps started so far.
    starts: u32,
    /// `--menu-back TAB,PICK`: go back to the menu from the first map and
    /// start this one.
    back: Option<(usize, usize)>,
    /// The frame it went back to the menu.
    backed_at: Option<u32>,
    saved: bool,
    target: Option<Handle<Image>>,
}

fn value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

/// `--menu-shot`: the menu draws into a PNG without a window.
pub fn headless(args: &[String]) -> bool {
    args.iter().any(|a| a == "--menu-shot")
}

/// The plugins for [`headless`]: no window, no event loop, no sound device.
pub fn add_headless_plugins(app: &mut App) -> &mut App {
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::audio::AudioPlugin>(),
    )
    .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::from_millis(1)))
}

pub struct MenuPlugin {
    args: Vec<String>,
}

impl MenuPlugin {
    /// `args`: the command line, passed on to the map (menu options taken
    /// out).
    pub fn new(args: &[String]) -> MenuPlugin {
        MenuPlugin { args: args.to_vec() }
    }
}

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        let args = &self.args;
        let shot = value(args, "--menu-shot").map(|path| MenuShot {
            path: path.into(),
            size: value(args, "--size")
                .and_then(|s| s.split_once('x').and_then(|(w, h)| Some(UVec2::new(w.parse().ok()?, h.parse().ok()?))))
                .unwrap_or(UVec2::new(1920, 1080)),
            loading: args.iter().any(|a| a == "--menu-loading"),
            missing: args.iter().any(|a| a == "--menu-missing"),
            start: args.iter().any(|a| a == "--menu-start"),
            frames: 0,
            ready_frames: 0,
            starts: 0,
            back: value(args, "--menu-back").and_then(|s| {
                let (t, p) = s.split_once(',')?;
                Some((t.parse().ok()?, p.parse().ok()?))
            }),
            backed_at: None,
            saved: false,
            target: None,
        });
        // The last map started, unless the command line picks one (shots).
        let last = game_paths::settings()
            .get(game_paths::key::LAST_MAP)
            .and_then(find_entry)
            .filter(|_| !headless(args));
        let mut tab = value(args, "--menu-tab")
            .and_then(|s| s.parse().ok())
            .or(last.map(|l| l.0))
            .unwrap_or(0usize)
            .min(TABS.len() - 1);
        let pick = value(args, "--menu-pick")
            .and_then(|s| s.parse().ok())
            .or(last.filter(|l| l.0 == tab).map(|l| l.1))
            .unwrap_or(0usize);
        let with_value =
            ["--menu-shot", "--menu-tab", "--menu-pick", "--menu-back", "--menu-step", "--menu-row", "--size"];
        let alone = ["--menu-loading", "--menu-start", "--menu-missing", "--menu-settings", "--menu-setup"];
        let mut pass = Vec::new();
        let mut skip = false;
        for a in args {
            if std::mem::take(&mut skip) || alone.contains(&a.as_str()) {
                continue;
            }
            if with_value.contains(&a.as_str()) {
                skip = true;
            } else {
                pass.push(a.clone());
            }
        }

        // Every map, with its picture; the ones this install lacks are
        // grayed out.
        let root = mcc_root();
        let pack = Pack::open(&root);
        if pack.is_none() {
            eprintln!("menu: no map pictures under {}", root.join(r"Data\UI\texturepacks").display());
        }
        let tabs = build_tabs(pack.as_ref(), &mut app.world_mut().resource_mut::<Assets<Image>>());
        let count = |t: usize| format!("{}/{}", tabs[t].iter().filter(|m| m.installed).count(), tabs[t].len());
        eprintln!(
            "menu: installed {} Halo 3, {} Halo 2, {} Halo CE, {} Reach, {} campaign maps under {}",
            count(0),
            count(1),
            count(2),
            count(3),
            count(4),
            root.display()
        );
        if tabs[tab].is_empty() {
            tab = (0..TABS.len()).find(|&t| !tabs[t].is_empty()).unwrap_or(0);
        }
        let mut picks = [0; TABS.len()];
        picks[tab] = pick.min(tabs[tab].len().saturating_sub(1));
        // `--menu-shot --menu-missing`: open on the picked map's notice.
        // The games not set up yet: the setup screen first (`--menu-setup`
        // draws it in a shot).
        let first_run = args.iter().any(|a| a == "--menu-setup") || (!headless(args) && mcc_setup::needs_setup());
        let phase = match tabs[tab].get(picks[tab]) {
            _ if first_run => Phase::Setup,
            _ if args.iter().any(|a| a == "--menu-settings") => Phase::Settings,
            Some(map) if shot.as_ref().is_some_and(|s| s.missing) => Phase::Missing(map.game),
            _ => Phase::Choosing,
        };
        let choosing = |menu: Res<Menu>| menu.phase == Phase::Choosing;
        let missing = |menu: Res<Menu>| matches!(menu.phase, Phase::Missing(_));
        let settings_open = |menu: Res<Menu>| menu.phase == Phase::Settings;
        let setup_open = |menu: Res<Menu>| menu.phase == Phase::Setup;
        // What `install` (main.rs) puts in goes when the map is unloaded.
        app.insert_resource(MapMenu)
            .remove_on_map_unload::<crate::falls::KillZones>()
            .remove_on_map_unload::<mcc_core::lifts::Lifts>()
            .remove_on_map_unload::<crate::world::stream::ZoneTriggers>()
            .remove_on_map_unload::<crate::world::PendingScene>()
            .remove_on_map_unload::<crate::rider::PendingChief>()
            .remove_on_map_unload::<crate::skate::SkateInput>()
            .remove_on_map_unload::<crate::bike::BikeInput>()
            .remove_on_map_unload::<crate::swim::WaterWorld>()
            .remove_on_map_unload::<crate::swim::FallbackSpot>()
            .remove_on_map_unload::<crate::weapons::PendingArsenal>()
            .remove_on_map_unload::<mcc_effects::PendingEffects>();
        let mut menu = Menu {
            tabs,
            tab,
            pick: picks,
            phase,
            args: pass,
            click: None,
            error: None,
            fresh: false,
            setting: 0,
            picking: None,
            note: String::new(),
            editing: None,
            step: 0,
            note_is_error: false,
            setup_finished: false,
            setup_return: None,
            confirm_skip: false,
            confirm: None,
            converted: String::new(),
            difficulty: saved_difficulty(),
            remember: !headless(args),
        };
        let step = value(args, "--menu-step");
        if first_run {
            setup::open(&mut menu, step.and_then(|s| s.parse().ok()));
        } else if menu.phase == Phase::Settings && step.is_some() {
            // `--menu-settings --menu-step 1`: the Skate 3 step, the one
            // Settings opens.
            setup::open_from_settings(&mut menu, SKATE_ROW, "");
        }
        // `--menu-row N`: the focus on that row (and its page), or the last
        // one the screen has (a row past it would be chosen as a game's
        // folder).
        if let Some(row) = value(args, "--menu-row").and_then(|s| s.parse::<usize>().ok()) {
            menu.setting = if menu.phase == Phase::Setup {
                row.min(setup::row_count(&menu) - 1)
            } else {
                (0..=row.min(SETTING_NAMES.len() - 1)).rev().find(|r| row_offered(*r)).unwrap_or(0)
            };
        }
        if menu.phase == Phase::Settings {
            menu.converted = support::converted();
        }
        app.insert_resource(menu)
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            (
                settings_poll.run_if(|menu: Res<Menu>| menu.picking.is_some()),
                (navigate, mouse, wheel, act).run_if(choosing),
                settings_input.run_if(settings_open),
                setup::input.run_if(setup_open),
                setup::finish.run_if(|menu: Res<Menu>| menu.setup_finished),
                dismiss.run_if(missing),
                show,
                follow,
                show_notice,
                show_settings,
                setup::show,
                spin,
            )
                .chain(),
        )
        .add_systems(Last, (back_to_menu, finish_loading, leave).chain());
        if let Some(shot) = shot {
            app.insert_resource(shot).add_systems(Update, capture.after(show));
        }
    }
}

/// Every tab's maps, with their pictures; the ones this install lacks are
/// grayed out.
fn build_tabs(pack: Option<&Pack>, images: &mut Assets<Image>) -> [Vec<Map>; TABS.len()] {
    SECTIONS.map(|sections| {
        sections
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .map(|&(arg, name, texture)| Map {
                arg,
                name,
                picture: texture.and_then(|t| pack?.picture(t)).map(|i| images.add(i)),
                game: game_of(arg),
                installed: installed(arg),
            })
            .collect::<Vec<_>>()
    })
}

fn text(s: &str, size: f32, colour: Color) -> impl Bundle {
    (Text::new(s), TextFont::from_font_size(size), TextColor(colour))
}

fn spawn(mut commands: Commands, menu: Res<Menu>, shot: Option<ResMut<MenuShot>>, mut images: ResMut<Assets<Image>>) {
    let camera = match shot {
        Some(mut shot) => {
            let mut image = Image::new_target_texture(shot.size.x, shot.size.y, TextureFormat::Rgba8UnormSrgb, None);
            image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            let handle = images.add(image);
            shot.target = Some(handle.clone());
            commands.spawn((MenuCamera, Camera2d, RenderTarget::Image(handle.into()))).id()
        }
        None => commands.spawn((MenuCamera, Camera2d)).id(),
    };
    commands
        .spawn((
            MenuRoot,
            UiTargetCamera(camera),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(BACKGROUND),
            GlobalZIndex(200),
        ))
        .with_children(|root| {
            root.spawn((
                Chooser,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|chooser| spawn_chooser(chooser, &menu));
            root.spawn((
                LoadingScreen,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    row_gap: Val::Px(28.0),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(BACKGROUND),
            ))
            .with_children(spawn_loading);
            root.spawn((
                Notice,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(BACKGROUND.with_alpha(0.8)),
            ))
            .with_children(spawn_notice);
            root.spawn((
                SettingsScreen,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(BACKGROUND),
            ))
            .with_children(spawn_settings);
            root.spawn((
                setup::SetupScreen,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(BACKGROUND),
            ))
            .with_children(setup::spawn);
        });
}

//// The settings screen: the pages' tab bar, a row per setting (name, what it
/// is now, its button; [`show_settings`] shows the page's), the note and
/// Back.
fn spawn_settings(screen: &mut ChildSpawnerCommands) {
    screen
        .spawn((
            Node {
                width: Val::Percent(92.0),
                max_width: Val::Vh(165.0),
                margin: UiRect::top(Val::Vh(4.0)),
                column_gap: Val::Px(36.0),
                align_items: AlignItems::FlexEnd,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(LINE),
        ))
        .with_children(|tabs| {
            tabs.spawn((
                Node { padding: UiRect::new(Val::Px(2.0), Val::Px(12.0), Val::Px(6.0), Val::Px(13.0)), ..default() },
                text("SETTINGS", 22.0, Color::WHITE),
            ));
            tabs.spawn((KeyHint { pad: "LB", keys: "Q" }, text("Q", 15.0, FAINT)));
            for (i, (name, _)) in SETTING_PAGES.iter().enumerate() {
                tabs.spawn((
                    SettingTab(i),
                    Node {
                        padding: UiRect::new(Val::Px(2.0), Val::Px(2.0), Val::Px(6.0), Val::Px(10.0)),
                        border: UiRect::bottom(Val::Px(3.0)),
                        margin: UiRect::bottom(Val::Px(-1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::NONE),
                    text(name, 22.0, DIM),
                ));
            }
            tabs.spawn((KeyHint { pad: "RB", keys: "E" }, text("E", 15.0, FAINT)));
        });
    screen.spawn(Node { height: Val::Vh(3.0), flex_shrink: 0.0, ..default() });
    let skate_ready = game_paths::skate_ready();
    for (i, name) in SETTING_NAMES.iter().enumerate() {
        screen
            .spawn((
                SettingRow(i),
                Node {
                    width: Val::Percent(92.0),
                    max_width: Val::Vh(165.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(24.0),
                    padding: UiRect::axes(Val::Px(20.0), Val::Px(14.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(Color::NONE),
            ))
            .with_children(|row| {
                row.spawn((Node { width: Val::Px(240.0), flex_shrink: 0.0, ..default() }, text(name, 22.0, Color::WHITE)));
                row.spawn((
                    SettingValue(i),
                    Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() },
                    text("", 18.0, DIM),
                ));
                row.spawn((
                    SettingSelect(i),
                    Node {
                        padding: UiRect::axes(Val::Px(26.0), Val::Px(9.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(5.0)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BorderColor::all(LINE),
                ))
                .with_child((SettingSelectLabel(i), text(button_label(i, skate_ready), 18.0, DIM)));
            });
    }
    screen.spawn((
        SettingsNote,
        Node { width: Val::Percent(92.0), max_width: Val::Vh(165.0), margin: UiRect::top(Val::Px(16.0)), ..default() },
        text("", 17.0, DIM),
    ));
    screen.spawn(Node { flex_grow: 1.0, ..default() });
    screen
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Vh(11.0),
                padding: UiRect::horizontal(Val::Percent(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(BAR),
            BorderColor::all(LINE),
        ))
        .with_children(|bar| {
            // Left and right step a value (sensitivity, field of view), on
            // the left of the bar.
            bar.spawn((
                SettingsStepHint,
                Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), margin: UiRect::right(Val::Auto), ..default() },
            ))
                .with_children(|hint| {
                    hint.spawn(key_hint::hint("D-PAD LEFT / RIGHT", "LEFT / RIGHT", 17.0, FAINT));
                    hint.spawn(text("TO STEP A VALUE", 17.0, FAINT));
                });
            bar.spawn((
                SettingsBack,
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
                    ..default()
                },
            ))
            .with_children(|b| {
                key_hint::badge(b, "B", "Esc", Color::srgb(0.85, 0.33, 0.31));
                b.spawn(text("BACK", 20.0, DIM));
            });
        });
}

// The not-installed notice: what to do, which game, and OK.
fn spawn_notice(screen: &mut ChildSpawnerCommands) {
    screen
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(18.0),
                padding: UiRect::axes(Val::Px(56.0), Val::Px(36.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(BAR),
            BorderColor::all(LINE),
        ))
        .with_children(|panel| {
            panel.spawn(text("You must install these Halo maps with MCC", 28.0, Color::WHITE));
            panel.spawn((NoticeGame, text("", 19.0, DIM)));
            panel
                .spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    margin: UiRect::top(Val::Px(14.0)),
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
                    ..default()
                })
                .with_children(|b| {
                    key_hint::badge(b, "A", "Enter", Color::srgb(0.37, 0.75, 0.29));
                    b.spawn(text("OK", 20.0, LIT));
                });
        });
}

fn spawn_chooser(root: &mut ChildSpawnerCommands, menu: &Menu) {
    // The tabs.
    root.spawn((
        Node {
            width: Val::Percent(92.0),
            max_width: Val::Vh(165.0),
            margin: UiRect::top(Val::Vh(4.0)),
            column_gap: Val::Px(36.0),
            align_items: AlignItems::FlexEnd,
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(LINE),
    ))
    .with_children(|tabs| {
        tabs.spawn((KeyHint { pad: "LB", keys: "Q" }, text("Q", 15.0, FAINT)));
        for (i, name) in TABS.iter().enumerate() {
            tabs.spawn((
                TabLabel(i),
                Node {
                    padding: UiRect::new(Val::Px(2.0), Val::Px(2.0), Val::Px(6.0), Val::Px(10.0)),
                    border: UiRect::bottom(Val::Px(3.0)),
                    margin: UiRect::bottom(Val::Px(-1.0)),
                    ..default()
                },
                BorderColor::all(Color::NONE),
                text(name, 22.0, DIM),
            ));
        }
        tabs.spawn((KeyHint { pad: "RB", keys: "E" }, text("E", 15.0, FAINT)));
        // The version, at the right end of the bar.
        tabs.spawn((
            Node { margin: UiRect::new(Val::Auto, Val::Px(2.0), Val::ZERO, Val::Px(10.0)), ..default() },
            text(game_paths::VERSION, 15.0, FAINT),
        ));
    });
    // A page per tab: each section's heading and its grid of tiles,
    // scrolling up and down when they do not fit.
    for (t, maps) in menu.tabs.iter().enumerate() {
        root.spawn((
            TabPage(t),
            Node {
                width: Val::Percent(92.0),
                max_width: Val::Vh(165.0),
                margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Vh(3.0), Val::Vh(1.6)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Vh(3.0),
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
        ))
        .with_children(|page| {
            let mut first = 0;
            for (heading, entries) in SECTIONS[t] {
                let range = first..(first + entries.len()).min(maps.len());
                first += entries.len();
                page.spawn((
                    SectionBox(t, range.start),
                    Node { flex_direction: FlexDirection::Column, flex_shrink: 0.0, row_gap: Val::Px(10.0), ..default() },
                ))
                .with_children(|section| {
                    if let Some(heading) = heading {
                        section.spawn((
                            Node { padding: UiRect::bottom(Val::Px(6.0)), border: UiRect::bottom(Val::Px(1.0)), ..default() },
                            BorderColor::all(LINE),
                            text(heading, 16.0, FAINT),
                        ));
                    }
                    section
                        .spawn(Node {
                            display: Display::Grid,
                            grid_template_columns: RepeatedGridTrack::flex(COLUMNS as u16, 1.0),
                            column_gap: Val::Vh(1.6),
                            row_gap: Val::Vh(1.6),
                            ..default()
                        })
                        .with_children(|grid| {
                            for i in range {
                                grid.spawn((TileCell(t, i), Node { flex_direction: FlexDirection::Column, ..default() }))
                                    .with_children(|tile| spawn_tile(tile, t, i, &maps[i]));
                            }
                        });
                });
            }
        });
    }
    // The bar: the chosen map, Start Game and Exit.
    root.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Vh(11.0),
            padding: UiRect::horizontal(Val::Percent(4.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(40.0),
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(BAR),
        BorderColor::all(LINE),
    ))
    .with_children(|bar| {
        bar.spawn(Node { flex_grow: 1.0, column_gap: Val::Px(14.0), align_items: AlignItems::Baseline, ..default() })
            .with_children(|name| {
                name.spawn((Chosen(0), text("", 26.0, Color::WHITE)));
                name.spawn((Chosen(1), text("", 18.0, DIM)));
            });
        for (button, letter, colour, label, key) in [
            (MenuButton::Difficulty, "Y", Color::srgb(0.93, 0.76, 0.20), "", "F2"),
            (MenuButton::Settings, "X", Color::srgb(0.30, 0.55, 0.90), "SETTINGS", "F1"),
            (MenuButton::Start, "A", Color::srgb(0.37, 0.75, 0.29), "START GAME", "Enter"),
            (MenuButton::Exit, "B", Color::srgb(0.85, 0.33, 0.31), "EXIT", "Esc"),
        ] {
            bar.spawn((
                button,
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
                    ..default()
                },
            ))
            .with_children(|b| {
                key_hint::badge(b, letter, key, colour);
                if button == MenuButton::Difficulty {
                    b.spawn((DifficultyLabel, text(&difficulty_label(menu.difficulty), 20.0, DIM)));
                } else if button == MenuButton::Start {
                    b.spawn((StartLabel, text(label, 20.0, LIT)));
                } else {
                    b.spawn(text(label, 20.0, DIM));
                }
            });
    }
    });
}

/// A map's tile: its picture (or its name on a plain card) and its name
/// under it.
fn spawn_tile(tile: &mut ChildSpawnerCommands, t: usize, i: usize, map: &Map) {
    tile.spawn((
        Tile(t, i),
        Node {
            width: Val::Percent(100.0),
            aspect_ratio: Some(668.0 / 292.0),
            border: UiRect::all(Val::Px(3.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..default()
        },
        BorderColor::all(Color::NONE),
        BackgroundColor(if map.picture.is_some() { Color::NONE } else { EMPTY }),
    ))
    .with_children(|frame| {
        match &map.picture {
            Some(picture) => {
                let tint = if map.installed { Color::WHITE } else { MISSING_TINT };
                frame.spawn((
                    ImageNode { color: tint, ..ImageNode::new(picture.clone()) },
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                ));
            }
            None => {
                let colour = if map.installed { FAINT } else { FAINT.mix(&BACKGROUND, 0.5) };
                frame.spawn(text(&map.name.to_uppercase(), 16.0, colour));
            }
        }
    });
    tile.spawn((
        TileLabel(t, i),
        Node { margin: UiRect::top(Val::Px(5.0)), ..default() },
        text(map.name, 17.0, if map.installed { DIM } else { FAINT }),
    ));
}

/// The loading screen: the map's picture, its name, the ring.
fn spawn_loading(screen: &mut ChildSpawnerCommands) {
    screen.spawn((
        LoadingPicture,
        ImageNode::default(),
        Node {
            width: Val::Vh(62.0),
            aspect_ratio: Some(668.0 / 292.0),
            ..default()
        },
    ));
    screen.spawn((LoadingName, text("", 34.0, Color::WHITE)));
    screen.spawn((LoadingStatus, text("", 18.0, DIM)));
    let size = RING_RADIUS * 2.0 + 24.0;
    screen
        .spawn(Node {
            width: Val::Px(size),
            height: Val::Px(size),
            margin: UiRect::top(Val::Px(12.0)),
            ..default()
        })
        .with_children(|ring| {
            let arc = std::f32::consts::TAU * RING_RADIUS / RING_SEGMENTS as f32;
            for n in 0..RING_SEGMENTS {
                let a = n as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
                let (w, h) = (arc * 0.7, 7.0);
                ring.spawn((
                    RingSegment(n),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(size / 2.0 + RING_RADIUS * a.cos() - w / 2.0),
                        top: Val::Px(size / 2.0 + RING_RADIUS * a.sin() - h / 2.0),
                        width: Val::Px(w),
                        height: Val::Px(h),
                        ..default()
                    },
                    UiTransform::from_rotation(Rot2::radians(a + std::f32::consts::FRAC_PI_2)),
                    BackgroundColor(LINE),
                ));
            }
        });
    screen.spawn(text("LOADING", 18.0, DIM));
}

/// Keys and pads: move, change tab.
fn navigate(keys: Res<ButtonInput<KeyCode>>, gamepads: Query<&Gamepad>, mut menu: ResMut<Menu>, mut stick: Local<[bool; 4]>) {
    let key = |k: &[KeyCode]| k.iter().any(|k| keys.just_pressed(*k));
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    // The left stick steps once each time it is pushed past half way.
    let axis = |a: GamepadAxis| {
        gamepads
            .iter()
            .map(|p| p.get(a).unwrap_or(0.0))
            .fold(0.0, |m: f32, v| if v.abs() > m.abs() { v } else { m })
    };
    let (x, y) = (axis(GamepadAxis::LeftStickX), axis(GamepadAxis::LeftStickY));
    let held = [x < -0.5, x > 0.5, y > 0.5, y < -0.5];
    let pushed: Vec<bool> = held.iter().zip(stick.iter()).map(|(h, was)| *h && !was).collect();
    *stick = held;

    if key(&[KeyCode::F1]) || pad(GamepadButton::West) {
        open_settings(&mut menu);
        return;
    }
    if menu.tab == CAMPAIGN_TAB && (key(&[KeyCode::F2]) || pad(GamepadButton::North)) {
        menu.next_difficulty();
    }
    let tabs = TABS.len();
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let tab_key = keys.just_pressed(KeyCode::Tab);
    if key(&[KeyCode::KeyE, KeyCode::PageDown]) || (tab_key && !shift) || pad(GamepadButton::RightTrigger) {
        menu.tab = (menu.tab + 1) % tabs;
    }
    if key(&[KeyCode::KeyQ, KeyCode::PageUp]) || (tab_key && shift) || pad(GamepadButton::LeftTrigger) {
        menu.tab = (menu.tab + tabs - 1) % tabs;
    }
    if menu.tabs[menu.tab].is_empty() {
        return;
    }
    let t = menu.tab;
    let (mut dx, mut dy) = (0, 0);
    if key(&[KeyCode::ArrowLeft, KeyCode::KeyA]) || pad(GamepadButton::DPadLeft) || pushed[0] {
        dx -= 1;
    }
    if key(&[KeyCode::ArrowRight, KeyCode::KeyD]) || pad(GamepadButton::DPadRight) || pushed[1] {
        dx += 1;
    }
    if key(&[KeyCode::ArrowUp, KeyCode::KeyW]) || pad(GamepadButton::DPadUp) || pushed[2] {
        dy -= 1;
    }
    if key(&[KeyCode::ArrowDown, KeyCode::KeyS]) || pad(GamepadButton::DPadDown) || pushed[3] {
        dy += 1;
    }
    let at = moved(&rows(t), menu.pick[t], dx, dy);
    if at != menu.pick[t] {
        menu.pick[t] = at;
    }
}

/// The mouse: a click on a tab shows it, on a tile picks it, a double click
/// on a tile or a click on Start Game starts, a click on Exit exits.
#[allow(clippy::too_many_arguments)]
fn mouse(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time<Real>>,
    mut menu: ResMut<Menu>,
    tabs: Query<(&TabLabel, &ComputedNode, &UiGlobalTransform)>,
    tiles: Query<(&Tile, &ComputedNode, &UiGlobalTransform)>,
    pages: Query<(&TabPage, &ComputedNode, &UiGlobalTransform)>,
    bar: Query<(&MenuButton, &ComputedNode, &UiGlobalTransform)>,
    mut exit: MessageWriter<AppExit>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(at) = windows.single().ok().and_then(Window::physical_cursor_position) else {
        return;
    };
    if let Some((label, ..)) = tabs.iter().find(|(_, n, t)| n.contains_point(**t, at)) {
        menu.tab = label.0;
        return;
    }
    let tab = menu.tab;
    // A tile scrolled out of the page is not there to click.
    let on_page = pages.iter().any(|(page, n, t)| page.0 == tab && n.contains_point(*t, at));
    if let Some((tile, ..)) = tiles.iter().find(|(tile, n, t)| on_page && tile.0 == tab && n.contains_point(**t, at)) {
        let now = time.elapsed_secs_f64();
        let double = menu.click.is_some_and(|(i, then)| i == tile.1 && now - then < 0.4);
        menu.pick[tile.0] = tile.1;
        menu.click = Some((tile.1, now));
        if double {
            start(&mut commands, &mut menu);
        }
        return;
    }
    match bar.iter().find(|(_, n, t)| n.contains_point(**t, at)).map(|(b, ..)| *b) {
        Some(MenuButton::Difficulty) => menu.next_difficulty(),
        Some(MenuButton::Settings) => open_settings(&mut menu),
        Some(MenuButton::Start) => start(&mut commands, &mut menu),
        Some(MenuButton::Exit) => {
            exit.write(AppExit::Success);
        }
        None => {}
    }
}

/// The difficulty button's text.
fn difficulty_label(difficulty: usize) -> String {
    format!("DIFFICULTY: {}", DIFFICULTIES[difficulty].to_uppercase())
}

fn open_settings(menu: &mut Menu) {
    menu.phase = Phase::Settings;
    menu.setting = 0;
    menu.confirm = None;
    menu.note.clear();
    menu.converted = support::converted();
}

/// What the settings change at once besides the saved file: the resources
/// the game reads them from, and the window. Only what is changed is marked
/// changed.
struct Live<'a> {
    unit: Mut<'a, SpeedUnit>,
    skate: Mut<'a, SkateEnabled>,
    tuning: Option<Mut<'a, mcc_bike::tune::BikeTuning>>,
    look: Mut<'a, LookSettings>,
    window: Option<Mut<'a, Window>>,
}

/// The settings screen: Q / E (Tab, Page Up / Down, LB / RB, or a click on
/// a page's name) change page, up / down move between its rows, Enter / A
/// (or a click) on a row's button changes it, left / right step a value,
/// Escape / B (or Back) leaves.
#[allow(clippy::too_many_arguments)]
fn settings_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<KeyboardInput>,
    buttons: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut menu: ResMut<Menu>,
    (unit, skate, tuning, look): (
        ResMut<SpeedUnit>,
        ResMut<SkateEnabled>,
        Option<ResMut<mcc_bike::tune::BikeTuning>>,
        ResMut<LookSettings>,
    ),
    selects: Query<(&SettingSelect, &ComputedNode, &UiGlobalTransform)>,
    tabs: Query<(&SettingTab, &ComputedNode, &UiGlobalTransform)>,
    back: Query<(&ComputedNode, &UiGlobalTransform), With<SettingsBack>>,
) {
    if menu.editing.is_some() {
        type_username(&mut menu, typed.read());
        return;
    }
    typed.clear();
    if menu.picking.is_some() {
        return;
    }
    let window = windows.single_mut().ok();
    let cursor = window.as_ref().and_then(|w| w.physical_cursor_position());
    let mut live = Live { unit: unit.into(), skate: skate.into(), tuning: tuning.map(Into::into), look: look.into(), window };
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    if keys.any_just_pressed([KeyCode::Escape, KeyCode::Backspace]) || pad(GamepadButton::East) {
        menu.phase = Phase::Choosing;
        return;
    }
    let pages = SETTING_PAGES.len();
    let page = page_of(menu.setting);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let tab_key = keys.just_pressed(KeyCode::Tab);
    let mut to_page = None;
    if keys.any_just_pressed([KeyCode::KeyE, KeyCode::PageDown]) || (tab_key && !shift) || pad(GamepadButton::RightTrigger) {
        to_page = Some((page + 1) % pages);
    }
    if keys.any_just_pressed([KeyCode::KeyQ, KeyCode::PageUp]) || (tab_key && shift) || pad(GamepadButton::LeftTrigger) {
        to_page = Some((page + pages - 1) % pages);
    }
    let rows = page_rows(page);
    let at = rows.iter().position(|r| *r == menu.setting).unwrap_or(0);
    let mut to_row = None;
    if keys.any_just_pressed([KeyCode::ArrowUp, KeyCode::KeyW]) || pad(GamepadButton::DPadUp) {
        to_row = rows.get(at.saturating_sub(1)).copied();
    }
    if keys.any_just_pressed([KeyCode::ArrowDown, KeyCode::KeyS]) || pad(GamepadButton::DPadDown) {
        to_row = rows.get((at + 1).min(rows.len().saturating_sub(1))).copied();
    }
    let clicked = buttons.just_pressed(MouseButton::Left).then_some(cursor).flatten();
    if let Some(at) = clicked
        && let Some((tab, ..)) = tabs.iter().find(|(_, n, t)| n.contains_point(**t, at))
    {
        to_page = Some(tab.0);
    }
    if let Some(p) = to_page.filter(|p| *p != page) {
        go_to_row(&mut menu, page_rows(p).first().copied().unwrap_or(0));
        return;
    }
    if let Some(row) = to_row.filter(|r| *r != menu.setting) {
        go_to_row(&mut menu, row);
        return;
    }
    let row = menu.setting;
    let by = if keys.any_just_pressed([KeyCode::ArrowLeft, KeyCode::KeyA]) || pad(GamepadButton::DPadLeft) {
        -1
    } else if keys.any_just_pressed([KeyCode::ArrowRight, KeyCode::KeyD]) || pad(GamepadButton::DPadRight) {
        1
    } else {
        0
    };
    if by != 0 {
        adjust(&mut menu, &mut live, row, by);
        return;
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) || pad(GamepadButton::South) {
        select(&mut menu, &mut live, row);
        return;
    }
    let Some(at) = clicked else {
        return;
    };
    let on_page = |r: usize| page_of(r) == page && row_offered(r);
    if let Some((s, ..)) = selects.iter().find(|(s, n, t)| on_page(s.0) && n.contains_point(**t, at)) {
        if s.0 != menu.setting {
            go_to_row(&mut menu, s.0);
        }
        select(&mut menu, &mut live, s.0);
    } else if back.iter().any(|(n, t)| n.contains_point(*t, at)) {
        menu.phase = Phase::Choosing;
    }
}

/// Puts the focus on `row`: a question waiting for a second press is
/// dropped, and so is the note it left.
fn go_to_row(menu: &mut Menu, row: usize) {
    menu.setting = row;
    if menu.confirm.take().is_some() {
        menu.note.clear();
    }
}
// Keys typed into the username: letters and spaces add, Backspace removes,
/// Enter saves it at once, Escape keeps the old name.
fn type_username<'a>(menu: &mut Menu, events: impl Iterator<Item = &'a KeyboardInput>) {
    for event in events.filter(|e| e.state.is_pressed()) {
        let Some(name) = menu.editing.as_mut() else {
            return;
        };
        match &event.logical_key {
            Key::Enter => {
                let typed = menu.editing.take().unwrap_or_default();
                menu.note = match game_paths::set_username(&typed) {
                    Ok(name) => format!("Saved username {name}."),
                    Err(e) => e,
                };
            }
            Key::Escape => {
                menu.editing = None;
                menu.note.clear();
            }
            Key::Backspace => {
                name.pop();
            }
            Key::Space => name.push(' '),
            Key::Character(c) => {
                if name.chars().count() < game_paths::USERNAME_MAX {
                    name.extend(c.chars().filter(|c| !c.is_control() && *c != '"'));
                }
            }
            _ => {}
        }
    }
}

/// Opens the folder picker for a game on its own thread; the choice is
/// checked and saved there, and [`settings_poll`] picks up the outcome.
/// Skate 3 opens the setup screen's Skate 3 step instead
/// (`setup::open_from_settings`), to add it, convert it again or go without
/// it; so does Skating while Skate 3 is not set up. The Controls and
/// Display rows step on ([`adjust`]), and the Support rows do their thing
/// ([`support_row`]).
fn select(menu: &mut Menu, live: &mut Live, row: usize) {
    if matches!(row, MOUSE_ROW | STICK_ROW | INVERT_ROW | DISPLAY_ROW | VSYNC_ROW | FOV_ROW) {
        adjust(menu, live, row, 0);
        return;
    }
    if matches!(row, FOLDER_ROW | DIAGNOSTICS_ROW | RESET_ROW | DELETE_ROW) {
        support_row(menu, live, row);
        return;
    }
    if row == PHYSICS_ROW {
        // The tuning the bike rides on, or the saved one when this app has
        // no bike (the file is what the next start reads either way).
        let mut loaded = None;
        let t = match live.tuning.as_deref_mut() {
            Some(t) => &mut t.0,
            None => loaded.insert(mcc_bike::tune::load()),
        };
        let reflex = t.model < 1.5;
        menu.note = if !mcc_bike::tune::switch_physics(t, reflex) {
            "Reflex's bike physics could not be read from your MX vs ATV Reflex folder.".to_string()
        } else if reflex {
            "The bike rides on Reflex's own simulation. The pause menu's Dirt bike tab switches it too.".to_string()
        } else {
            "The bike rides on its own arcade physics. The pause menu's Dirt bike tab switches it too.".to_string()
        };
        return;
    }
    if row == SKATE_ROW || (row == SKATING_ROW && !game_paths::skate_ready()) {
        // The setup screen's Skate 3 step, with its converter row, its
        // progress and "Without Skate 3". The saved Skating choice stays as
        // it was, for when Skate 3 is added.
        let note = if row == SKATING_ROW { "Skating needs Skate 3." } else { "" };
        setup::open_from_settings(menu, row, note);
        return;
    }
    if row == SKATING_ROW {
        let on = !live.skate.0;
        live.skate.0 = on;
        menu.note = match game_paths::set_skating_enabled(on) {
            Ok(()) if on => "Skating is on. J (or both sticks clicked) gets on the board.".to_string(),
            Ok(()) => "Skating is off. Maps load without the skateboard or grind rails.".to_string(),
            Err(e) => e,
        };
        return;
    }
    if row == TRICKS_ROW {
        let on = !game_paths::trick_display();
        menu.note = match game_paths::save_setting(game_paths::key::TRICK_DISPLAY, if on { "on" } else { "off" }) {
            Ok(()) if on => "Skating shows each trick's name and points from the next map you load.".to_string(),
            Ok(()) => "Skating shows no trick names or score from the next map you load.".to_string(),
            Err(e) => e,
        };
        return;
    }
    if row == ARMOUR_ROW {
        let next = mcc_rider::ARMOUR_COLOURS[(mcc_rider::armour() + 1) % mcc_rider::ARMOUR_COLOURS.len()].0;
        menu.note = match game_paths::save_setting(game_paths::key::ARMOUR, &next.to_ascii_lowercase()) {
            Ok(()) => format!("The Spartan wears {next} on the next map you load."),
            Err(e) => e,
        };
        return;
    }
    if row == LANGUAGE_ROW {
        let language = next_language(game_paths::language());
        menu.note = match game_paths::set_language(language) {
            // `%MCC_LANGUAGE%` (dev) is read before the saved choice.
            Ok(()) if game_paths::language() != language => {
                "Saved, but the MCC_LANGUAGE environment variable is set and comes first.".to_string()
            }
            Ok(()) => format!(
                "Dialogue plays in {} on the next map you load, where your MCC has that language installed.",
                language_label(language)
            ),
            Err(e) => e,
        };
        return;
    }
    if row == UNITS_ROW {
        let unit = live.unit.toggled();
        *live.unit = unit;
        menu.note = match unit.save() {
            Ok(()) => format!("Speedometer now shows {}.", unit.label()),
            Err(e) => e,
        };
        return;
    }
    if row == USERNAME_ROW {
        menu.editing = Some(game_paths::username());
        menu.note = "Type your name. Enter saves it, Escape cancels.".to_string();
        return;
    }
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(match row {
            0 => mcc_setup::select_mcc(),
            _ => mcc_setup::select_reflex(),
        });
    });
    menu.note = format!("Choose your {} folder in the window that opened.", SETTING_NAMES[row]);
    menu.picking = Some(Picking { what: Pick::Setting(row), result: Mutex::new(receive) });
}

/// Steps a Controls or Display row: `by` -1 or 1 (left / right) moves a
/// percentage down or up, stopping at the ends; 0 (its button) moves it up
/// and round. A switch flips either way. Saved and used at once; the window
/// changes mode and vsync there and then. Other rows ignore left and right.
fn adjust(menu: &mut Menu, live: &mut Live, row: usize, by: i32) {
    let mut look = *live.look;
    let mut display = view::Display::load();
    let note = match row {
        MOUSE_ROW => {
            look.mouse = view::step(&view::SENSITIVITY_STEPS, look.mouse, by);
            format!("The mouse turns the camera at {} % of its stock speed.", look.mouse)
        }
        STICK_ROW => {
            look.stick = view::step(&view::SENSITIVITY_STEPS, look.stick, by);
            format!("The right stick turns the camera at {} % of its stock speed.", look.stick)
        }
        INVERT_ROW => {
            look.invert = !look.invert;
            if look.invert {
                "Look is inverted: pushing the stick or the mouse up looks down."
            } else {
                "Look is not inverted: pushing up looks up."
            }
            .to_string()
        }
        FOV_ROW => {
            look.fov = view::step(&view::FOV_STEPS, look.fov, by);
            format!("Every camera sees {} % as wide as its own: flying, on foot, the bike and the board.", look.fov)
        }
        DISPLAY_ROW => {
            display.windowed = !display.windowed;
            if display.windowed { "The game runs in a window." } else { "The game fills the screen, borderless." }.to_string()
        }
        VSYNC_ROW => {
            display.vsync = !display.vsync;
            if display.vsync {
                "VSync is on: the frame rate keeps to the screen's."
            } else {
                "VSync is off: frames show as soon as they are drawn."
            }
            .to_string()
        }
        _ => return,
    };
    let saved = if matches!(row, DISPLAY_ROW | VSYNC_ROW) {
        if let Some(window) = live.window.as_deref_mut() {
            display.apply(window);
        }
        display.save()
    } else {
        if *live.look != look {
            *live.look = look;
        }
        look.save()
    };
    menu.note = match saved {
        Ok(()) => note,
        Err(e) => e,
    };
}

/// The Support page's rows: open the data folder, copy the diagnostics, and
/// (each asking first: the first press says what it does, the second does
/// it) reset the settings or delete the converted game data.
fn support_row(menu: &mut Menu, live: &mut Live, row: usize) {
    let asked = menu.confirm.take() == Some(row);
    match row {
        FOLDER_ROW => {
            let dir = game_paths::settings_dir();
            menu.note = match support::open_folder(&dir) {
                Ok(()) => format!("Opened {} (logs and crash reports are in its logs folder).", dir.display()),
                Err(e) => e,
            };
        }
        DIAGNOSTICS_ROW => {
            let text = support::diagnostics(&live.look);
            eprintln!("settings: diagnostics\n{text}");
            menu.note = match support::copy(&text) {
                Ok(()) => "Copied. Paste it into your bug report, with the log from the data folder.".to_string(),
                Err(e) => {
                    // No clipboard: a file to attach instead.
                    let file = game_paths::settings_dir().join("diagnostics.txt");
                    match std::fs::write(&file, &text) {
                        Ok(()) => format!("Could not copy ({e}); saved it to {} instead.", file.display()),
                        Err(_) => format!("Could not copy: {e}"),
                    }
                }
            };
        }
        RESET_ROW if !asked => {
            menu.confirm = Some(row);
            menu.note = "This puts every setting back to its default. Your game folders and what was converted \
                         from them stay. Press RESET again to do it."
                .to_string();
        }
        RESET_ROW => {
            menu.note = match game_paths::reset_settings() {
                Ok(()) => {
                    // What the game holds now, read again from the reset file.
                    *live.unit = SpeedUnit::load();
                    live.skate.0 = game_paths::skating_enabled();
                    *live.look = LookSettings::load();
                    if let Some(window) = live.window.as_deref_mut() {
                        view::Display::load().apply(window);
                    }
                    menu.difficulty = saved_difficulty();
                    eprintln!("settings: reset to the defaults");
                    "Every setting is back to its default.".to_string()
                }
                Err(e) => e,
            };
        }
        DELETE_ROW if !asked => {
            menu.confirm = Some(row);
            menu.note = "This deletes the Skate 3 and Reflex data converted at setup. You then play without them \
                         until you set them up again on the Games page. Press DELETE again to do it."
                .to_string();
        }
        DELETE_ROW => {
            let result = support::delete_converted();
            live.skate.0 = game_paths::skating_enabled();
            menu.converted = support::converted();
            menu.note = match result {
                Ok(said) => {
                    eprintln!("settings: {said}");
                    said
                }
                Err(e) => e,
            };
        }
        _ => {}
    }
}

/// A picker has finished: say how it went. A new MCC folder changes which
/// maps are installed and their pictures, so the map lists are built again.
/// (Skate 3 is chosen on the setup screen's step: `setup::finish`.)
fn settings_poll(world: &mut World) {
    let menu = world.resource::<Menu>();
    let Some(picking) = &menu.picking else {
        return;
    };
    let what = picking.what;
    let outcome = match picking.result.lock().unwrap().try_recv() {
        Ok(outcome) => outcome,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("The folder picker stopped unexpectedly.".to_string()),
    };
    let changed = matches!(outcome, Ok(Some(_)));
    let mut menu = world.resource_mut::<Menu>();
    menu.picking = None;
    let row = match what {
        Pick::Setting(row) => row,
        Pick::Setup(row) => {
            setup::picked(&mut menu, row, outcome);
            return;
        }
    };
    menu.note = match outcome {
        Ok(Some(path)) => {
            eprintln!("settings: {} is now {}", SETTING_NAMES[row], path.display());
            format!("Saved {}.", SETTING_NAMES[row])
        }
        Ok(None) => String::new(),
        Err(e) => {
            eprintln!("settings: {e}");
            e
        }
    };
    if changed {
        // Choosing Reflex imports it into the data folder.
        menu.converted = support::converted();
    }
    if changed && row == 0 {
        refresh_maps(world);
    }
}

/// Builds the map lists and the menu's screens again, from the MCC folder
/// as saved now.
fn refresh_maps(world: &mut World) {
    let pack = Pack::open(&mcc_root());
    let tabs = build_tabs(pack.as_ref(), &mut world.resource_mut::<Assets<Image>>());
    let old: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<MenuRoot>, With<MenuCamera>)>>()
        .iter(world)
        .collect();
    for e in old {
        world.entity_mut(e).despawn();
    }
    world.resource_mut::<Menu>().tabs = tabs;
    if let Err(e) = world.run_system_cached(spawn) {
        error!("menu: cannot rebuild the menu: {e}");
    }
}

/// Draws the settings screen: shown in [`Phase::Settings`], the focused
/// row's page (its name lit in the tab bar), the saved values, the focused
/// row, the note.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn show_settings(
    menu: Res<Menu>,
    unit: Res<SpeedUnit>,
    skate: Res<SkateEnabled>,
    tuning: Option<Res<mcc_bike::tune::BikeTuning>>,
    look: Res<LookSettings>,
    mut tabs: Query<
        (&SettingTab, &mut TextColor, &mut BorderColor),
        (Without<SettingValue>, Without<SettingSelectLabel>, Without<SettingRow>, Without<SettingSelect>, Without<SettingsNote>),
    >,
    mut screen: Query<&mut Node, (With<SettingsScreen>, Without<SettingRow>, Without<SettingsStepHint>)>,
    mut step_hint: Query<&mut Node, (With<SettingsStepHint>, Without<SettingRow>, Without<SettingsScreen>)>,
    mut rows: Query<(&SettingRow, &mut BackgroundColor, &mut BorderColor, &mut Node), (Without<SettingSelect>, Without<SettingsScreen>)>,
    mut values: Query<(&SettingValue, &mut Text, &mut TextColor), (Without<SettingsNote>, Without<SettingSelectLabel>)>,
    mut selects: Query<(&SettingSelect, &mut BorderColor), Without<SettingRow>>,
    mut labels: Query<(&SettingSelectLabel, &mut Text, &mut TextColor), (Without<SettingValue>, Without<SettingsNote>)>,
    mut note: Query<&mut Text, (With<SettingsNote>, Without<SettingValue>, Without<SettingSelectLabel>)>,
) {
    let tuned = tuning.as_ref().is_some_and(|t| t.is_changed());
    if !menu.is_changed() && !unit.is_changed() && !skate.is_changed() && !look.is_changed() && !tuned {
        return;
    }
    let open = menu.phase == Phase::Settings;
    for mut node in &mut screen {
        node.display = if open { Display::Flex } else { Display::None };
    }
    if !open {
        return;
    }
    let busy = |i: usize| menu.picking.as_ref().is_some_and(|p| p.what == Pick::Setting(i));
    let page = page_of(menu.setting);
    for mut node in &mut step_hint {
        let steps = SETTING_PAGES[page].1.contains(&MOUSE_ROW) || SETTING_PAGES[page].1.contains(&FOV_ROW);
        node.display = if steps { Display::Flex } else { Display::None };
    }
    for (tab, mut colour, mut border) in &mut tabs {
        let on = tab.0 == page;
        colour.0 = if on { LIT } else { DIM };
        *border = BorderColor::all(if on { LIT } else { Color::NONE });
    }
    for (row, mut background, mut border, mut node) in &mut rows {
        let shown = page_of(row.0) == page && row_offered(row.0);
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let on = row.0 == menu.setting;
        background.0 = if on { BAR } else { Color::NONE };
        *border = BorderColor::all(if on { LINE } else { Color::NONE });
    }
    let reflex_physics = tuning.map_or_else(|| mcc_bike::tune::load().model, |t| t.0.model) >= 1.5;
    let skate_ready = game_paths::skate_ready();
    let now = setting_values(*unit, *skate, reflex_physics, skate_ready, *look, &menu.converted);
    for (value, mut text, mut colour) in &mut values {
        text.0.clone_from(&now[value.0]);
        if value.0 == USERNAME_ROW {
            if let Some(typing) = &menu.editing {
                text.0 = format!("{typing}_");
            }
        }
        colour.0 = if now[value.0].starts_with("Not") { FAINT } else { DIM };
    }
    for (select, mut border) in &mut selects {
        *border = BorderColor::all(if select.0 == menu.setting { LIT } else { LINE });
    }
    for (label, mut text, mut colour) in &mut labels {
        let editing = label.0 == USERNAME_ROW && menu.editing.is_some();
        text.0 = if busy(label.0) {
            "WORKING..."
        } else if editing {
            "ENTER"
        } else if menu.confirm == Some(label.0) {
            "CONFIRM"
        } else {
            button_label(label.0, skate_ready)
        }
        .to_string();
        colour.0 = if label.0 == menu.setting { LIT } else { DIM };
    }
    for mut text in &mut note {
        text.0.clone_from(&menu.note);
    }
}

/// Enter / A starts, Escape / B exits.
fn act(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut menu: ResMut<Menu>,
    shot: Option<ResMut<MenuShot>>,
    mut exit: MessageWriter<AppExit>,
) {
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    // `--menu-shot`: start the map (twice with `--menu-back`).
    let auto = shot.is_some_and(|mut s| {
        let planned = if s.loading { 1 } else if s.start { 1 + s.back.is_some() as u32 } else { 0 };
        let go = s.starts < planned;
        s.starts += go as u32;
        go
    });
    if auto || keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) || pad(GamepadButton::South) {
        start(&mut commands, &mut menu);
    } else if keys.just_pressed(KeyCode::Escape) || pad(GamepadButton::East) {
        exit.write(AppExit::Success);
    }
}

/// Starts decoding the chosen map on a thread; the loading screen shows
/// until it is in. A map MCC does not have opens the notice instead.
fn start(commands: &mut Commands, menu: &mut Menu) {
    let Some(map) = menu.selected() else {
        return;
    };
    if !map.installed {
        eprintln!("menu: {} ({}) is not installed in MCC", map.name, map.arg);
        menu.phase = Phase::Missing(map.game);
        menu.fresh = true;
        return;
    }
    if let Some(loading) = begin(menu) {
        commands.insert_resource(loading);
    }
}

/// Starts decoding the picked map on its thread and shows the loading
/// screen.
fn begin(menu: &mut Menu) -> Option<Loading> {
    let map = menu.selected()?;
    let arg = map.arg;
    eprintln!("menu: loading {} ({arg})", map.name);
    if menu.remember
        && let Err(e) = game_paths::save_setting(game_paths::key::LAST_MAP, arg)
    {
        eprintln!("menu: the last map was not saved: {e}");
    }
    let (send, receive) = std::sync::mpsc::channel();
    let args = menu.map_args();
    std::thread::spawn(move || {
        let _ = send.send(crate::load(arg, &args));
    });
    menu.phase = Phase::Loading;
    menu.error = None;
    Some(Loading(Mutex::new(receive)))
}

/// The notice: Enter / Space / Escape / A / B or a click close it, back to
/// the chooser.
fn dismiss(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    mut menu: ResMut<Menu>,
) {
    // The press that opened it, this frame, is not one that closes it.
    if menu.fresh {
        menu.fresh = false;
        return;
    }
    let pad = gamepads.iter().any(|p| p.any_just_pressed([GamepadButton::South, GamepadButton::East]));
    let key = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space, KeyCode::Escape]);
    if pad || key || buttons.just_pressed(MouseButton::Left) {
        menu.phase = Phase::Choosing;
        menu.click = None;
    }
}

/// Q in the pause menu (`pause.rs`): unload the map and show the menu again.
fn back_to_menu(world: &mut World) {
    if world.remove_resource::<BackToMenu>().is_none() {
        return;
    }
    mcc_core::map::unload(world);
    if let Ok(mut window) = world.query_filtered::<&mut Window, With<PrimaryWindow>>().single_mut(world) {
        window.title = "Halo 3".to_string();
    }
    if let Ok(mut cursor) = world
        .query_filtered::<&mut bevy::window::CursorOptions, With<PrimaryWindow>>()
        .single_mut(world)
    {
        cursor.grab_mode = bevy::window::CursorGrabMode::None;
        cursor.visible = true;
    }
    let mut menu = world.resource_mut::<Menu>();
    menu.phase = Phase::Choosing;
    menu.error = None;
    menu.click = None;
    if let Err(e) = world.run_system_cached(spawn) {
        error!("menu: cannot show the menu again: {e}");
    }
}

/// Restart on the completion screen (`finish.rs`): the map goes and the
/// menu's loading screen shows while `arg` (the map or mission it came
/// from) loads again.
pub(crate) fn reload(world: &mut World, arg: &str) {
    mcc_core::map::unload(world);
    let mut menu = world.resource_mut::<Menu>();
    // The tile it came from (still picked unless that was another tab's).
    if let Some((t, i)) = (0..TABS.len()).find_map(|t| Some((t, menu.tabs[t].iter().position(|m| m.arg == arg)?))) {
        menu.tab = t;
        menu.pick[t] = i;
    }
    menu.click = None;
    if let Err(e) = world.run_system_cached(spawn) {
        error!("menu: cannot show the loading screen: {e}");
    }
    if let Some(loading) = begin(&mut world.resource_mut::<Menu>()) {
        world.insert_resource(loading);
    }
}

/// The map is decoded: put it into the app (or back to the menu with the
/// error).
fn finish_loading(world: &mut World) {
    let Some(received) = world.get_resource::<Loading>().map(|l| l.0.lock().unwrap().try_recv()) else {
        return;
    };
    let result = match received {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        // The loading thread died (a panic) before it sent anything.
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(anyhow::anyhow!("the map crashed while loading")),
    };
    world.remove_resource::<Loading>();
    let args = world.resource::<Menu>().map_args();
    let failed = match result {
        Ok(map) => {
            let title = format!("MCC Rust Rewrite - {}", map.scene_name());
            if let Ok(mut window) = world.query_filtered::<&mut Window, With<PrimaryWindow>>().single_mut(world) {
                window.title = title;
            }
            crate::install(world, map, &args).err()
        }
        Err(e) => Some(e),
    };
    if let Some(e) = failed {
        eprintln!("menu: the map did not load: {e:#}");
        let mut menu = world.resource_mut::<Menu>();
        menu.phase = Phase::Choosing;
        menu.error = Some(format!("Did not load: {e}"));
    }
}

/// Once play begins (the map's [`Warmup`] work, the bike's collision and
/// the skate session, is done, or `mcc_core::map` gave up waiting), the
/// menu, its camera and the loading screen go. Meanwhile the screen says
/// what it is waiting for.
fn leave(
    mut commands: Commands,
    ready: Option<Res<MapReady>>,
    playing: Option<Res<Playing>>,
    warmup: Option<Res<Warmup>>,
    shot: Option<Res<MenuShot>>,
    mut status: Query<&mut Text, With<LoadingStatus>>,
    roots: Query<Entity, Or<(With<MenuRoot>, With<MenuCamera>)>>,
) {
    // `--menu-shot --menu-loading` keeps the loading screen for its picture.
    if ready.is_none() || shot.is_some_and(|s| s.loading) || roots.is_empty() {
        return;
    }
    if playing.is_none() {
        if let Some(warmup) = warmup.filter(|w| !w.0.is_empty()) {
            let waiting: Vec<&str> = warmup.0.iter().copied().collect();
            let line = format!("Preparing {}", waiting.join(" and "));
            for mut text in &mut status {
                if text.0 != line {
                    text.0 = line.clone();
                }
            }
        }
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
}

/// Draws the state: chooser or loading screen, the tab shown, the gold
/// frame on the pick, the bar.
#[allow(clippy::type_complexity)]
fn show(
    menu: Res<Menu>,
    mut chooser: Query<&mut Node, (With<Chooser>, Without<LoadingScreen>, Without<TabPage>)>,
    mut loading: Query<&mut Node, (With<LoadingScreen>, Without<Chooser>, Without<TabPage>)>,
    mut picture: Query<&mut ImageNode, With<LoadingPicture>>,
    mut pages: Query<(&TabPage, &mut Node), (Without<Chooser>, Without<LoadingScreen>)>,
    mut labels: Query<(&TabLabel, &mut TextColor, &mut BorderColor), (Without<TileLabel>, Without<Tile>, Without<StartLabel>)>,
    mut tiles: Query<(&Tile, &mut BorderColor), Without<TabLabel>>,
    mut names: Query<(&TileLabel, &mut TextColor), (Without<TabLabel>, Without<StartLabel>)>,
    mut start_label: Query<&mut TextColor, (With<StartLabel>, Without<TabLabel>, Without<TileLabel>)>,
    mut texts: Query<(Option<&Chosen>, Option<&LoadingName>, Option<&DifficultyLabel>, &mut Text)>,
    mut bar: Query<(&MenuButton, &mut Node), (Without<Chooser>, Without<LoadingScreen>, Without<TabPage>)>,
) {
    if !menu.is_changed() {
        return;
    }
    for (button, mut node) in &mut bar {
        if *button == MenuButton::Difficulty {
            node.display = if menu.tab == CAMPAIGN_TAB { Display::Flex } else { Display::None };
        }
    }
    let loading_now = menu.phase == Phase::Loading;
    let over = matches!(menu.phase, Phase::Settings | Phase::Setup);
    for mut node in &mut chooser {
        node.display = if loading_now || over { Display::None } else { Display::Flex };
    }
    for mut node in &mut loading {
        node.display = if loading_now { Display::Flex } else { Display::None };
    }
    let selected = menu.selected();
    for mut image in &mut picture {
        if let Some(handle) = selected.and_then(|m| m.picture.clone()) {
            image.image = handle;
        }
    }
    for (page, mut node) in &mut pages {
        node.display = if page.0 == menu.tab { Display::Flex } else { Display::None };
    }
    for (label, mut colour, mut border) in &mut labels {
        let on = label.0 == menu.tab;
        colour.0 = if on { LIT } else { DIM };
        *border = BorderColor::all(if on { LIT } else { Color::NONE });
    }
    for (tile, mut border) in &mut tiles {
        let on = tile.1 == menu.pick[tile.0];
        *border = BorderColor::all(if on { LIT } else { Color::NONE });
    }
    for (label, mut colour) in &mut names {
        let installed = menu.tabs[label.0].get(label.1).is_some_and(|m| m.installed);
        colour.0 = match (label.1 == menu.pick[label.0], installed) {
            (true, true) => LIT,
            (true, false) => LIT.mix(&FAINT, 0.5),
            (false, true) => DIM,
            (false, false) => FAINT,
        };
    }
    for mut colour in &mut start_label {
        colour.0 = if selected.is_some_and(|m| m.installed) { LIT } else { FAINT };
    }
    for (part, name, difficulty, mut text) in &mut texts {
        if difficulty.is_some() {
            text.0 = difficulty_label(menu.difficulty);
            continue;
        }
        if name.is_some() {
            text.0 = selected.map_or(String::new(), |m| m.name.to_uppercase());
            continue;
        }
        let Some(part) = part else {
            continue;
        };
        text.0 = match (selected, part.0) {
            (Some(map), 0) => map.name.to_string(),
            (Some(_), _) if menu.error.is_some() => menu.error.clone().unwrap_or_default(),
            (Some(map), _) if !map.installed => format!("{} - not installed in MCC", kind(menu.tab, map)),
            (Some(map), _) => kind(menu.tab, map).to_string(),
            (None, _) => String::new(),
        };
    }
}

/// Scrolls the shown page so the picked tile is all in view: picking a
/// tile in the row below the page's bottom row moves the page up by that
/// row, and the same upwards (with the section's heading, for its first
/// row). Waits for the page to be laid out (just shown, or rebuilt).
#[allow(clippy::type_complexity)]
fn follow(
    menu: Res<Menu>,
    mut pages: Query<(Entity, &TabPage, &ComputedNode, &UiGlobalTransform, &mut ScrollPosition)>,
    cells: Query<(&TileCell, &ComputedNode, &UiGlobalTransform)>,
    sections: Query<(&SectionBox, &ComputedNode, &UiGlobalTransform)>,
    mut done: Local<Option<(Entity, usize, usize)>>,
) {
    let (t, i) = (menu.tab, menu.pick[menu.tab]);
    let Some((page, _, node, at, mut scroll)) = pages.iter_mut().find(|(_, p, ..)| p.0 == t) else {
        return;
    };
    if *done == Some((page, t, i)) {
        return;
    }
    let Some((_, cell, cell_at)) = cells.iter().find(|(c, ..)| c.0 == t && c.1 == i) else {
        return;
    };
    if node.size.y <= 0.0 || cell.size.y <= 0.0 {
        return;
    }
    // Top and bottom on screen, in physical pixels.
    let span = |n: &ComputedNode, at: &UiGlobalTransform| (at.translation.y - n.size.y / 2.0, at.translation.y + n.size.y / 2.0);
    let (page_top, page_bottom) = span(node, at);
    let (mut top, bottom) = span(cell, cell_at);
    // A section's first row brings its heading with it.
    if let Some((section, n, a)) = sections.iter().filter(|(s, ..)| s.0 == t && s.1 <= i).max_by_key(|(s, ..)| s.1)
        && i - section.1 < COLUMNS
    {
        top = span(n, a).0;
    }
    let by = if bottom > page_bottom {
        bottom - page_bottom
    } else if top < page_top {
        top - page_top
    } else {
        0.0
    };
    scroll.y = (scroll.y + by * node.inverse_scale_factor).max(0.0);
    *done = Some((page, t, i));
}

/// The mouse wheel scrolls the shown page.
fn wheel(menu: Res<Menu>, scrolled: Res<AccumulatedMouseScroll>, mut pages: Query<(&TabPage, &mut ScrollPosition)>) {
    if scrolled.delta.y == 0.0 {
        return;
    }
    let step = match scrolled.unit {
        MouseScrollUnit::Line => 60.0,
        MouseScrollUnit::Pixel => 1.0,
    };
    for (page, mut scroll) in &mut pages {
        if page.0 == menu.tab {
            scroll.y = (scroll.y - scrolled.delta.y * step).max(0.0);
        }
    }
}

/// The notice shows while the phase is [`Phase::Missing`], naming the game.
fn show_notice(menu: Res<Menu>, mut notice: Query<&mut Node, With<Notice>>, mut line: Query<&mut Text, With<NoticeGame>>) {
    if !menu.is_changed() {
        return;
    }
    let game = match menu.phase {
        Phase::Missing(game) => Some(game),
        _ => None,
    };
    for mut node in &mut notice {
        node.display = if game.is_some() { Display::Flex } else { Display::None };
    }
    if let Some(game) = game {
        for mut text in &mut line {
            text.0 = format!("{} isn't installed in your Master Chief Collection.", game_title(game));
        }
    }
}

/// The ring: a bright arc runs round it, fading behind.
fn spin(menu: Res<Menu>, time: Res<Time<Real>>, mut segments: Query<(&RingSegment, &mut BackgroundColor)>) {
    if menu.phase != Phase::Loading {
        return;
    }
    let head = (time.elapsed_secs() * RING_SPEED).fract() * RING_SEGMENTS as f32;
    for (segment, mut colour) in &mut segments {
        // How far behind the head, in segments.
        let behind = (head - segment.0 as f32).rem_euclid(RING_SEGMENTS as f32);
        let glow = (1.0 - behind / (RING_SEGMENTS as f32 * 0.45)).max(0.0);
        colour.0 = LINE.mix(&LIT, glow * glow);
    }
}

/// `--menu-shot`: save the menu (or the loading screen, or with
/// `--menu-start` the map's first view) and quit.
fn capture(
    mut commands: Commands,
    mut shot: ResMut<MenuShot>,
    mut menu: ResMut<Menu>,
    ready: Option<Res<MapReady>>,
    installed: Option<Res<MapInstalled>>,
    cameras: Query<Entity, (With<Camera3d>, Without<MenuCamera>)>,
    mut exit: MessageWriter<AppExit>,
) {
    shot.frames += 1;
    if shot.saved {
        if shot.frames > 10 {
            exit.write(AppExit::Success);
        }
        return;
    }
    let Some(target) = shot.target.clone() else {
        return;
    };
    let due = if shot.start {
        // Give the world a few seconds of frames to stream in.
        if ready.is_some()
            && shot.starts == 1
            && shot.ready_frames == 120
            && let Some((tab, pick)) = shot.back
        {
            eprintln!("menu: shot: back to the menu");
            commands.insert_resource(BackToMenu);
            menu.tab = tab.min(TABS.len() - 1);
            let t = menu.tab;
            menu.pick[t] = pick;
            shot.ready_frames = 0;
            shot.backed_at = Some(shot.frames);
            return;
        }
        if ready.is_some() {
            shot.ready_frames += 1;
            if shot.ready_frames == 1 {
                for camera in &cameras {
                    commands.entity(camera).insert(RenderTarget::Image(target.clone().into()));
                }
            }
        }
        // Back on the menu: the menu.
        let menu_again = shot.backed_at.is_some_and(|at| shot.frames == at + 40)
            && ready.is_none()
            && menu.phase == Phase::Choosing;
        menu_again || (shot.ready_frames == 300 && shot.starts == 1 + shot.back.is_some() as u32)
    } else if shot.loading {
        menu.phase == Phase::Loading && installed.is_none() && shot.frames == 30
    } else {
        shot.frames == 20
    };
    if menu.phase == Phase::Choosing && menu.error.is_some() {
        eprintln!("menu: shot: the map did not load");
        exit.write(AppExit::from_code(1));
        return;
    }
    if due {
        eprintln!("menu: shot {}", shot.path.display());
        commands
            .spawn(Screenshot::image(target))
            .observe(save_to_disk(shot.path.clone()))
            .observe(|_: On<ScreenshotCaptured>, mut shot: ResMut<MenuShot>| {
                shot.saved = true;
                shot.frames = 0;
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every entry names its game, and each tab holds one game's maps, so
    /// the installed check looks in the right folder.
    #[test]
    fn entries_name_their_game() {
        for (sections, game) in SECTIONS.iter().zip([Game::Halo3, Game::Halo2, Game::Halo1, Game::Reach, Game::Halo3]) {
            for (arg, ..) in sections.iter().flat_map(|(_, entries)| entries.iter()) {
                // A mission (warthog_run) is its map's game; the Campaign
                // tab also holds Halo CE's Warthog run.
                let game = if *arg == "h1:warthog_run" { Game::Halo1 } else { game };
                assert_eq!(crate::codename(map_of(arg)).0, Some(game), "{arg}");
            }
        }
    }

    /// Without Skate 3, its row and Skating set it up first; with it, its
    /// row changes it and Skating switches skating as before.
    #[test]
    fn skate_rows_set_up_skate_3_first() {
        assert_eq!(SETTING_NAMES[SKATE_ROW], "Skate 3");
        assert_eq!(SETTING_NAMES[SKATING_ROW], "Skating");
        assert_eq!(button_label(SKATE_ROW, false), "SET UP");
        assert_eq!(button_label(SKATING_ROW, false), "SET UP");
        assert_eq!(button_label(SKATE_ROW, true), "CHANGE");
        assert_eq!(button_label(SKATING_ROW, true), "CHANGE");
        for ready in [false, true] {
            assert_eq!(button_label(0, ready), "SELECT");
            assert_eq!(button_label(USERNAME_ROW, ready), "EDIT");
            assert_eq!(button_label(PHYSICS_ROW, ready), "CHANGE");
        }
    }

    /// The pages hold every settings row once, in order, and the rows'
    /// constants name the rows they mean.
    #[test]
    fn settings_pages_cover_every_row() {
        let mut next = 0;
        for (page, (_, rows)) in SETTING_PAGES.iter().enumerate() {
            assert_eq!(rows.start, next);
            assert!(!rows.is_empty());
            for row in rows.clone() {
                assert_eq!(page_of(row), page);
            }
            next = rows.end;
        }
        assert_eq!(next, SETTING_NAMES.len());
        assert_eq!(SETTING_NAMES[PHYSICS_ROW], "Bike physics");
        assert_eq!(SETTING_NAMES[MOUSE_ROW], "Mouse sensitivity");
        assert_eq!(SETTING_NAMES[FOV_ROW], "Field of view");
        assert_eq!(SETTING_NAMES[DELETE_ROW], "Converted data");
        // The physics row is the last on its page, so hiding it leaves no gap.
        assert_eq!(SETTING_PAGES[page_of(PHYSICS_ROW)].1.end, PHYSICS_ROW + 1);
        assert_eq!(button_label(DELETE_ROW, true), "DELETE");
        assert_eq!(button_label(FOV_ROW, false), "CHANGE");
    }

    /// Up and down keep the column and stop at a section's shorter row;
    /// along runs across sections.
    #[test]
    fn moving_between_sections() {
        // The Campaign tab: rows of 1, 1, 6 and 5 maps.
        let campaign = rows(4);
        assert_eq!(campaign, [(0, 1), (1, 1), (2, 6), (8, 5)]);
        assert_eq!(moved(&campaign, 0, 0, 1), 1);
        assert_eq!(moved(&campaign, 1, 0, 1), 2);
        assert_eq!(moved(&campaign, 7, 0, -1), 1);
        assert_eq!(moved(&campaign, 7, 0, 1), 12);
        assert_eq!(moved(&campaign, 4, 0, 1), 10);
        assert_eq!(moved(&campaign, 12, 0, 1), 12);
        assert_eq!(moved(&campaign, 0, 0, -1), 0);
        assert_eq!(moved(&campaign, 1, 1, 0), 2);
        assert_eq!(moved(&campaign, 12, 1, 0), 12);
        // One section: rows of six.
        assert_eq!(moved(&rows(0), 3, 0, 1), 9);
    }

    #[test]
    fn has_map_wants_the_file() {
        let dir = std::env::temp_dir().join(format!("mcc-menu-has-map-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("folder.map")).unwrap();
        std::fs::write(dir.join("here.map"), b"").unwrap();
        assert!(has_map(&dir, "here"));
        assert!(!has_map(&dir, "gone"));
        // A folder of that name is not the map.
        assert!(!has_map(&dir, "folder"));
        // No maps folder (before setup) has no maps.
        assert!(!has_map(Path::new(""), "here"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
