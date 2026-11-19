//! The menus before a map: `mcc-rust-rewrite` with no map opens the main
//! menu (Campaign, Multiplayer maps, Settings, Exit). Campaign and
//! Multiplayer maps open the map chooser: the maps found in the user's MCC
//! install in a list down the left (Multiplayer has a tab per game: Halo 3,
//! Halo 2, Halo CE, Reach), and on the right the picked map's name and MCC's
//! own preview picture (`levelstexturepack`, read at run time, never copied
//! into the repo). Start Game decodes the map on a thread behind a loading
//! screen, then puts it into this same app (`main.rs` `install`,
//! `mcc_core::map`). Every screen is built from `mcc_core::ui` (font,
//! colours, sizes as parts of the window's height).
//!
//! MCC lets the player leave out any game but Halo 3 (or part of one): a map
//! whose `.map` file is not in its game's maps folder is still listed, grayed
//! out, and starting it shows a notice to install it with MCC instead of
//! loading.
//!
//! Keys, main menu: up / down (arrows, W / S, d-pad, left stick) move,
//! Enter / Space / A or a click open the row, Escape / B go to Exit and from
//! Exit leave the game. Chooser: up / down move through the maps; left /
//! right, Q / E, Tab / Shift+Tab, Page Up / Page Down or LB / RB change tab
//! (the hints by the tabs show Q / E, or LB / RB when a controller was used
//! last, `key_hint.rs`); Enter / Space / A start; Escape / B go back to the
//! main menu; F1 / X open Settings. The mouse selects a map with a click and
//! starts it with a double click. On the notice, Enter / Space / Escape /
//! A / B or a click close it.
//!
//! The Campaign's list has headings: the missions that have been played
//! through (Halo CE, Halo 3), then the untested ones; F2 / Y change the
//! difficulty. A list taller than the screen scrolls: moving past its bottom
//! (or top) row moves it one row, and the mouse wheel scrolls it.
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
//! down move, Enter / A (or a click on the focused row) changes it, and left /
//! right step a value up or down. Reset and delete ask for a second press.
//!
//! `mcc-rust-rewrite --menu-shot FILE [--menu-tab N] [--menu-pick N] [--size WxH]`
//! draws the menu without a window into a PNG; `--menu-loading` draws the
//! loading screen instead, `--menu-missing` the picked map's not-installed
//! notice, `--menu-settings` the settings (with `--menu-step 1`, the Skate 3
//! step as Settings opens it), `--menu-setup [--menu-step N]` the setup
//! screen, and `--menu-start` loads the picked map the way Start Game does
//! and saves the first view of it. `--menu-row N` puts the focus on row N of
//! the settings (and shows its page) or the setup step. `--menu-update TAG`
//! draws the new-version pop-up for release TAG (`update.rs`).
//!
//! When a newer release is out on GitHub (`update.rs`), a pop-up over the
//! maps says so once: Enter / A or its button opens the release's page in
//! the browser, Escape / B or Close shuts it.

mod setup;
mod support;
mod update;

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
use mcc_core::ui::{self, bold, text, u};
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

const BACKGROUND: Color = ui::BACKGROUND;
const LINE: Color = ui::LINE;
const DIM: Color = ui::TEXT;
const FAINT: Color = ui::QUIET;
const LIT: Color = ui::ACCENT;
/// The loading ring's unlit segments.
const RING_DARK: Color = Color::srgb(0.13, 0.20, 0.29);

/// The spinner: a Halo ring of segments with a bright arc running round it.
const RING_SEGMENTS: usize = 32;
const RING_RADIUS: f32 = 52.0;
/// Turns per second.
const RING_SPEED: f32 = 0.6;

/// A missing map's picture is drawn through this tint.
const MISSING_TINT: Color = Color::srgb(0.30, 0.30, 0.32);
/// The tiles not picked are drawn a little darker than the picked one.
const TILE_TINT: Color = Color::srgb(0.66, 0.70, 0.76);

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
    /// The main menu: Campaign, Multiplayer maps, Settings, Exit.
    Main,
    /// The map chooser: the multiplayer tabs, or the Campaign tab alone.
    Choosing,
    Loading,
    /// The notice over the chooser: this game's map is not installed.
    Missing(Game),
    /// The settings screen.
    Settings,
    /// The first-run setup screen (`setup.rs`).
    Setup,
    /// The pop-up over the chooser: a newer release is out
    /// ([`Menu::update`]).
    Update,
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
    /// The main menu's focused row, an index into [`MAIN_ROWS`].
    main_row: usize,
    /// The multiplayer tab the chooser showed last (the Campaign has its own
    /// row on the main menu).
    mp_tab: usize,
    /// Where Settings and the new-version pop-up go back to: the main menu
    /// or the chooser, whichever they were opened over.
    under: Phase,
    /// The new-version check, while it runs on its thread (`update.rs`).
    update_check: Option<update::UpdateCheck>,
    /// A newer release: waiting for the chooser, then shown in the pop-up
    /// until it is closed (once per launch).
    update: Option<update::NewRelease>,
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
/// A section's heading on a tab's page: tab, its first map.
#[derive(Component)]
struct SectionHead(usize, usize);
/// A map's place in a tab's grid (its tile and name): tab, map.
#[derive(Component)]
struct TileCell(usize, usize);
/// A map's tile: its framed picture.
#[derive(Component)]
struct Tile(usize, usize);
/// The picture in a map's tile.
#[derive(Component)]
struct TilePicture(usize, usize);
/// The name across the foot of a map's tile.
#[derive(Component)]
struct TileName(usize, usize);
/// The chooser's title: MULTIPLAYER MAPS or CAMPAIGN.
#[derive(Component)]
struct ChooserTitle;
/// The bumper hints beside the chooser's tabs (hidden on Campaign, which has
/// no tabs).
#[derive(Component)]
struct TabExtra;
/// The main menu.
#[derive(Component)]
struct MainScreen;
/// A row of the main menu, an index into [`MAIN_ROWS`], and its name.
#[derive(Component)]
struct MainRow(usize);
#[derive(Component)]
struct MainLabel(usize);
/// Under the main menu's rows: what the focused one is.
#[derive(Component)]
struct MainAbout;
/// The main menu's picture (the map the chooser opens on) and its caption.
#[derive(Component)]
struct MainPicture;
#[derive(Component)]
struct MainCaption;
/// The picked map's words: 0 its name, 1 which game and mode, 2 a note
/// (not installed, or why it did not load).
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
    /// The main menu's A: open the focused row.
    Select,
    /// The campaign difficulty, only on the Campaign tab.
    Difficulty,
    Settings,
    Start,
    Exit,
}
/// The difficulty button's text.
#[derive(Component)]
struct DifficultyLabel;
/// The new-version pop-up over the chooser.
#[derive(Component)]
struct UpdateNotice;
/// The pop-up's line naming the versions.
#[derive(Component)]
struct UpdateLine;
/// The pop-up's buttons.
#[derive(Component, Clone, Copy, PartialEq)]
enum UpdateButton {
    Open,
    Close,
}
/// The settings screen.
#[derive(Component)]
struct SettingsScreen;
/// One row of the settings screen, an index into `SETTING_NAMES`.
#[derive(Component)]
struct SettingRow(usize);
#[derive(Component)]
struct SettingValue(usize);
/// On the right of the settings: the focused row's name and what it is.
#[derive(Component)]
struct SettingTitle;
#[derive(Component)]
struct SettingAbout;
/// The footer's A button, and its label: what A does to the focused row.
#[derive(Component)]
struct SettingsAct;
#[derive(Component)]
struct SettingActionLabel;
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

/// The main menu's rows and what each one is.
const MAIN_ROWS: [(&str, &str); 4] = [
    ("CAMPAIGN", "The missions: the Warthog runs, played through from start to finish, and the rest to explore."),
    ("MULTIPLAYER MAPS", "Skate, ride and walk the multiplayer maps of Halo 3, Halo 2, Halo CE and Reach."),
    ("SETTINGS", "Your games, player, controls, display and support."),
    ("EXIT", "Close the game."),
];
const MAIN_CAMPAIGN: usize = 0;
const MAIN_MAPS: usize = 1;
const MAIN_SETTINGS: usize = 2;

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

/// What each row shows now, in short: the saved folder or choice, or why
/// there is none ([`setting_about`] says what the row is).
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
    let on_off = |on: bool| if on { "On" } else { "Off" }.to_string();
    let settings = game_paths::settings();
    let reflex = match game_paths::reflex_chosen() {
        Some(p) if game_paths::reflex_imported() => format!("Imported from {}", p.display()),
        chosen => shown(chosen.filter(|_| game_paths::reflex_ready()))
            .unwrap_or_else(|| if game_paths::reflex_skipped() { "Not used" } else { "Not set" }.to_string()),
    };
    let skate_folder = if skate_ready {
        settings.get(game_paths::key::SKATE_SOURCE).map_or_else(|| "Set up".to_string(), str::to_owned)
    } else if game_paths::skate_skipped() {
        "Not used".to_string()
    } else {
        "Not set".to_string()
    };
    let display = view::Display::load();
    [
        shown(game_paths::mcc_root()).unwrap_or_else(|| "Not set".to_string()),
        reflex,
        skate_folder,
        game_paths::username(),
        mcc_rider::ARMOUR_COLOURS[mcc_rider::armour()].0.to_string(),
        unit.label().to_string(),
        if skate_ready { on_off(skate.0) } else { "Not available".to_string() },
        on_off(game_paths::trick_display()),
        language_label(game_paths::language()).to_string(),
        if reflex_physics { "Reflex" } else { "Arcade" }.to_string(),
        format!("{} %", look.mouse),
        format!("{} %", look.stick),
        on_off(look.invert),
        if display.windowed { "Windowed" } else { "Borderless fullscreen" }.to_string(),
        on_off(display.vsync),
        format!("{} %", look.fov),
        game_paths::settings_dir().display().to_string(),
        String::new(),
        String::new(),
        converted.to_string(),
    ]
}

/// What a row is, for the panel beside the rows.
fn setting_about(row: usize, skate_ready: bool, skating: bool) -> &'static str {
    match row {
        0 => "Required. The maps, their pictures and their sounds are read from your own Master Chief Collection \
              while you play. Nothing in it is changed.",
        1 => "Optional. With your MX vs ATV Reflex the dirt bike is its Dakumoto 450F, with its engine recordings. \
              Without it the bike is our own model and has no engine sound.",
        SKATE_ROW if skate_ready => "Your Skate 3 adds the skateboard and the grind rails. Change it to convert it \
                                     again or to go without it.",
        SKATE_ROW => "Optional. Your own Skate 3 adds the skateboard and grind rails on every map.",
        USERNAME_ROW => "The name the game shows for you.",
        ARMOUR_ROW => "The Spartan's armour on multiplayer maps, from the next map you load.",
        UNITS_ROW => "The speedometer shows miles per hour or kilometres per hour.",
        SKATING_ROW if !skate_ready => "Skating needs Skate 3. Set it up to add the skateboard.",
        SKATING_ROW if skating => "Maps load with the skateboard and grind rails. J (or both sticks clicked) gets on the board.",
        SKATING_ROW => "Maps load without the skateboard or grind rails, and are ready sooner.",
        TRICKS_ROW => "Shows each trick's name and points on screen while skating, from the next map you load.",
        LANGUAGE_ROW => "The voices in Halo 3 and Halo: Combat Evolved, where your MCC has that language installed.",
        PHYSICS_ROW => "Reflex is MX vs ATV Reflex's own bike simulation; Arcade is the bike's own physics. The pause \
                        menu's Dirt bike tab switches it too.",
        MOUSE_ROW => "How fast the mouse turns the camera, against its stock speed.",
        STICK_ROW => "How fast the right stick turns the camera, against its stock speed.",
        INVERT_ROW => "On: pushing up looks down, with the mouse and the stick.",
        DISPLAY_ROW => "Fill the screen without a border, or run in a window.",
        VSYNC_ROW => "On: the frame rate keeps to the screen's, with no tearing. Off: frames show as fast as they \
                      come, and may tear.",
        FOV_ROW => "How wide every camera sees, against its own: flying, on foot, the bike and the board. Wider shows more.",
        FOLDER_ROW => "Where your settings, logs and crash reports are kept.",
        DIAGNOSTICS_ROW => "Copies the version, your setup and where the log is, to paste into a bug report.",
        RESET_ROW => "Puts every setting back to its default. Your game folders stay.",
        DELETE_ROW => "The Skate 3 and Reflex data converted at setup. Deleting it means playing without them until \
                       you set them up again on the Games page.",
        _ => "",
    }
}

/// Whether left and right change a row (the Controls and Display pages'):
/// its value is drawn between arrows while it has the focus.
fn steps(row: usize) -> bool {
    matches!(row, MOUSE_ROW | STICK_ROW | INVERT_ROW | DISPLAY_ROW | VSYNC_ROW | FOV_ROW)
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
        let with_value = [
            "--menu-shot",
            "--menu-tab",
            "--menu-pick",
            "--menu-back",
            "--menu-step",
            "--menu-row",
            "--menu-update",
            "--size",
        ];
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
        // A shot that names a map, or starts one, is of the chooser; any
        // other, and the game itself, opens on the main menu.
        let to_chooser = headless(args)
            && ["--menu-tab", "--menu-pick", "--menu-loading", "--menu-start", "--menu-missing"]
                .iter()
                .any(|flag| args.iter().any(|a| a == flag));
        let first_run = args.iter().any(|a| a == "--menu-setup") || (!headless(args) && mcc_setup::needs_setup());
        let phase = match tabs[tab].get(picks[tab]) {
            _ if first_run => Phase::Setup,
            _ if args.iter().any(|a| a == "--menu-settings") => Phase::Settings,
            Some(map) if shot.as_ref().is_some_and(|s| s.missing) => Phase::Missing(map.game),
            _ if to_chooser => Phase::Choosing,
            _ => Phase::Main,
        };
        let main_open = |menu: Res<Menu>| menu.phase == Phase::Main;
        // Not in the frame the main menu opened them: the press that chose
        // the row would start the picked map, or change the first setting.
        let choosing = |menu: Res<Menu>| menu.phase == Phase::Choosing && !menu.fresh;
        let missing = |menu: Res<Menu>| matches!(menu.phase, Phase::Missing(_));
        let settings_open = |menu: Res<Menu>| menu.phase == Phase::Settings && !menu.fresh;
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
            main_row: if tab == CAMPAIGN_TAB { MAIN_CAMPAIGN } else { MAIN_MAPS },
            mp_tab: if tab == CAMPAIGN_TAB { 0 } else { tab },
            under: if to_chooser { Phase::Choosing } else { Phase::Main },
            // Not in a headless shot, nor with `--menu-update TAG`, which
            // pretends TAG is out (to see the pop-up while ours is the newest).
            update_check: if headless(args) || value(args, "--menu-update").is_some() { None } else { update::start() },
            update: value(args, "--menu-update").map(|tag| update::NewRelease { tag }),
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
                main_input.run_if(main_open),
                (navigate, mouse, wheel, act).run_if(choosing),
                settings_input.run_if(settings_open),
                setup::input.run_if(setup_open),
                setup::finish.run_if(|menu: Res<Menu>| menu.setup_finished),
                dismiss.run_if(missing),
                update_poll,
                update_input.run_if(|menu: Res<Menu>| menu.phase == Phase::Update),
                settle,
                show,
                follow,
                show_notice,
                show_update,
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

/// The pad's face buttons, as the footers' badges colour them.
const PAD_A: Color = Color::srgb(0.37, 0.75, 0.29);
const PAD_B: Color = Color::srgb(0.85, 0.33, 0.31);
const PAD_X: Color = Color::srgb(0.30, 0.55, 0.90);
const PAD_Y: Color = Color::srgb(0.93, 0.76, 0.20);

/// A map picture's shape in MCC's texture pack.
const PICTURE_ASPECT: f32 = 668.0 / 292.0;

/// A full-screen layer of the menu, hidden until its phase shows it.
fn layer() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        display: Display::None,
        ..default()
    }
}

/// A layer holding a pop-up in its middle, over a darkened screen.
fn popup_layer() -> impl Bundle {
    (
        Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..layer() },
        BackgroundColor(Color::srgba(0.0, 0.01, 0.03, 0.72)),
    )
}

/// A footer's button: its badge (the pad's `letter` in `colour`, or `key`)
/// and its label.
fn footer_button(
    bar: &mut ChildSpawnerCommands,
    marker: impl Bundle,
    letter: &'static str,
    key: &'static str,
    colour: Color,
    label: impl Bundle,
) {
    bar.spawn((
        marker,
        Node { align_items: AlignItems::Center, column_gap: u(12.0), padding: UiRect::axes(u(8.0), u(10.0)), ..default() },
    ))
    .with_children(|b| {
        key_hint::badge(b, letter, key, colour);
        b.spawn(label);
    });
}

/// A header's title, sitting on the rule like the tabs.
fn title(head: &mut ChildSpawnerCommands, marker: impl Bundle, name: &str) {
    head.spawn((
        marker,
        Node { padding: UiRect::bottom(u(10.0)), margin: UiRect::right(u(24.0)), ..default() },
        bold(name, 40.0, ui::BRIGHT),
    ));
}

/// The hint beside a header's tabs: the pad's bumper or the key.
fn tab_hint(head: &mut ChildSpawnerCommands, marker: impl Bundle, pad: &'static str, keys: &'static str) {
    head.spawn((marker, KeyHint { pad, keys }, Node { padding: UiRect::bottom(u(18.0)), ..default() }, text(keys, 17.0, FAINT)));
}

/// A pop-up's panel: its title on a strip, then what `body` puts under it.
fn popup(screen: &mut ChildSpawnerCommands, name: &str, body: impl FnOnce(&mut ChildSpawnerCommands)) {
    screen
        .spawn((
            Node { width: u(860.0), flex_direction: FlexDirection::Column, border: UiRect::all(u(1.5)), ..default() },
            BackgroundColor(Color::srgb(0.035, 0.078, 0.130)),
            BorderColor::all(LINE),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Node { padding: UiRect::axes(u(36.0), u(20.0)), border: UiRect::bottom(u(1.5)), ..default() },
                    BackgroundColor(ui::FOCUS.with_alpha(0.22)),
                    BorderColor::all(LINE),
                ))
                .with_child(bold(name, 30.0, ui::BRIGHT));
            panel
                .spawn(Node { flex_direction: FlexDirection::Column, row_gap: u(26.0), padding: UiRect::all(u(36.0)), ..default() })
                .with_children(body);
        });
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
            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            BackgroundColor(BACKGROUND),
            ui::backdrop(),
            GlobalZIndex(200),
        ))
        .with_children(|root| {
            root.spawn((MainScreen, layer())).with_children(|screen| spawn_main(screen, &menu));
            root.spawn((Chooser, layer())).with_children(|chooser| spawn_chooser(chooser, &menu));
            root.spawn((LoadingScreen, layer(), BackgroundColor(BACKGROUND), ui::backdrop())).with_children(spawn_loading);
            root.spawn((SettingsScreen, layer())).with_children(spawn_settings);
            root.spawn((setup::SetupScreen, layer())).with_children(setup::spawn);
            root.spawn((Notice, popup_layer())).with_children(spawn_notice);
            root.spawn((UpdateNotice, popup_layer())).with_children(spawn_update);
        });
}

/// The main menu: the game's name, the big rows down the left (what each one
/// is, under them), and the picked map's picture on the right.
fn spawn_main(screen: &mut ChildSpawnerCommands, menu: &Menu) {
    screen
        .spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            padding: UiRect::horizontal(u(96.0)),
            column_gap: u(84.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|body| {
            body.spawn(Node { width: u(640.0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, ..default() })
                .with_children(|left| {
                    left.spawn(bold("MCC RUST REWRITE", 62.0, ui::BRIGHT));
                    left.spawn((
                        Node { margin: UiRect::new(u(4.0), Val::ZERO, u(2.0), u(54.0)), ..default() },
                        text("HALO MAPS  /  SKATEBOARD  /  DIRT BIKE", 19.0, ui::ACCENT),
                    ));
                    for (i, (name, _)) in MAIN_ROWS.iter().enumerate() {
                        left.spawn((MainRow(i), ui::row(74.0))).with_child((MainLabel(i), bold(name, 34.0, DIM)));
                    }
                    left.spawn((
                        MainAbout,
                        Node { margin: UiRect::new(u(26.0), Val::ZERO, u(34.0), Val::ZERO), min_height: u(60.0), ..default() },
                        text("", 21.0, FAINT),
                    ));
                });
            body.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: u(14.0), ..default() })
                .with_children(|right| {
                    right
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                aspect_ratio: Some(PICTURE_ASPECT),
                                border: UiRect::all(u(1.5)),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            BackgroundColor(ui::WELL),
                            BorderColor::all(LINE),
                        ))
                        .with_child((
                            MainPicture,
                            ImageNode { color: Color::NONE, ..default() },
                            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                        ));
                    right.spawn((MainCaption, text("", 20.0, FAINT)));
                });
        });
    screen.spawn((ui::footer_node(), ui::bar())).with_children(|bar| {
        bar.spawn((Node { margin: UiRect::right(Val::Auto), ..default() }, text(game_paths::VERSION, 18.0, FAINT)));
        footer_button(bar, MenuButton::Select, "A", "Enter", PAD_A, bold("SELECT", 20.0, ui::BRIGHT));
        footer_button(bar, MenuButton::Exit, "B", "Esc", PAD_B, text("EXIT", 20.0, DIM));
    });
    let _ = menu;
}

/// The settings screen: the pages' tab bar; the page's rows down the left
/// (name, what it is now; [`show_settings`] shows the page's); on the right
/// what the focused row is and the note; the footer with what A does.
fn spawn_settings(screen: &mut ChildSpawnerCommands) {
    screen.spawn((ui::header_node(), ui::bar())).with_children(|head| {
        title(head, (), "SETTINGS");
        tab_hint(head, (), "LB", "Q");
        for (i, (name, _)) in SETTING_PAGES.iter().enumerate() {
            head.spawn((SettingTab(i), ui::tab_node(), BorderColor::all(Color::NONE), bold(name, 24.0, FAINT)));
        }
        tab_hint(head, (), "RB", "E");
    });
    screen
        .spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            padding: UiRect::new(u(72.0), u(72.0), u(40.0), u(30.0)),
            column_gap: u(56.0),
            ..default()
        })
        .with_children(|body| {
            body.spawn(Node { width: Val::Percent(58.0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: u(4.0), ..default() })
                .with_children(|list| {
                    for (i, name) in SETTING_NAMES.iter().enumerate() {
                        list.spawn((SettingRow(i), ui::row(64.0))).with_children(|row| {
                            row.spawn((Node { width: u(310.0), flex_shrink: 0.0, ..default() }, bold(name, 24.0, ui::BRIGHT)));
                            row.spawn((
                                SettingValue(i),
                                Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() },
                                TextLayout::justify(Justify::Right),
                                text("", 21.0, DIM),
                            ));
                        });
                    }
                });
            body.spawn((
                Node {
                    flex_grow: 1.0,
                    flex_basis: Val::Px(0.0),
                    align_self: AlignSelf::FlexStart,
                    min_height: u(320.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: u(18.0),
                    padding: UiRect::all(u(34.0)),
                    border: UiRect::all(u(1.5)),
                    ..default()
                },
                ui::panel(),
            ))
            .with_children(|about| {
                about.spawn((SettingTitle, bold("", 32.0, ui::BRIGHT)));
                about.spawn((SettingAbout, text("", 21.0, DIM)));
                about.spawn((SettingsNote, Node { margin: UiRect::top(u(10.0)), ..default() }, text("", 21.0, LIT)));
            });
        });
    screen.spawn((ui::footer_node(), ui::bar())).with_children(|bar| {
        // Left and right step a value (sensitivity, field of view), on the
        // left of the bar.
        bar.spawn((SettingsStepHint, Node { align_items: AlignItems::Center, column_gap: u(10.0), margin: UiRect::right(Val::Auto), ..default() }))
            .with_children(|hint| {
                hint.spawn((KeyHint { pad: "D-PAD LEFT / RIGHT", keys: "LEFT / RIGHT" }, text("LEFT / RIGHT", 18.0, FAINT)));
                hint.spawn(text("TO STEP A VALUE", 18.0, FAINT));
            });
        footer_button(bar, SettingsAct, "A", "Enter", PAD_A, (SettingActionLabel, bold("", 20.0, ui::BRIGHT)));
        footer_button(bar, SettingsBack, "B", "Esc", PAD_B, text("BACK", 20.0, DIM));
    });
}

// The not-installed notice: what to do, which game, and OK.
fn spawn_notice(screen: &mut ChildSpawnerCommands) {
    popup(screen, "You must install these Halo maps with MCC", |panel| {
        panel.spawn((NoticeGame, text("", 22.0, DIM)));
        panel.spawn(Node { justify_content: JustifyContent::FlexEnd, ..default() }).with_children(|row| {
            footer_button(row, (), "A", "Enter", PAD_A, bold("OK", 20.0, ui::BRIGHT));
        });
    });
}

/// The new-version pop-up: what is out, and its two buttons.
fn spawn_update(screen: &mut ChildSpawnerCommands) {
    popup(screen, "A new version is available", |panel| {
        panel.spawn((UpdateLine, text("", 22.0, DIM)));
        panel.spawn(Node { justify_content: JustifyContent::FlexEnd, column_gap: u(28.0), ..default() }).with_children(|row| {
            footer_button(row, UpdateButton::Open, "A", "Enter", PAD_A, bold("OPEN RELEASE PAGE", 20.0, ui::BRIGHT));
            footer_button(row, UpdateButton::Close, "B", "Esc", PAD_B, text("CLOSE", 20.0, DIM));
        });
    });
}

/// The map chooser: its title and the games' tabs; a grid of the tab's maps'
/// tiles (a page per tab, each section under its heading, scrolling when
/// they do not fit); the footer with the picked map's name and kind and the
/// buttons.
fn spawn_chooser(root: &mut ChildSpawnerCommands, menu: &Menu) {
    root.spawn((ui::header_node(), ui::bar())).with_children(|head| {
        title(head, ChooserTitle, "MULTIPLAYER MAPS");
        tab_hint(head, TabExtra, "LB", "Q");
        for (i, name) in TABS.iter().enumerate().take(CAMPAIGN_TAB) {
            head.spawn((TabLabel(i), ui::tab_node(), BorderColor::all(Color::NONE), bold(name, 24.0, FAINT)));
        }
        tab_hint(head, TabExtra, "RB", "E");
    });
    // A page per tab: each section's heading and its grid of tiles.
    root.spawn(Node {
        flex_grow: 1.0,
        min_height: Val::Px(0.0),
        flex_direction: FlexDirection::Column,
        padding: UiRect::new(u(72.0), u(72.0), u(30.0), u(20.0)),
        ..default()
    })
    .with_children(|body| {
        for (t, maps) in menu.tabs.iter().enumerate() {
            body.spawn((
                TabPage(t),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: u(30.0),
                    flex_grow: 1.0,
                    flex_basis: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    overflow: Overflow::scroll_y(),
                    display: Display::None,
                    ..default()
                },
                ScrollPosition::default(),
            ))
            .with_children(|page| {
                let mut first = 0;
                for (heading, entries) in SECTIONS[t] {
                    let range = first..(first + entries.len()).min(maps.len());
                    first += entries.len();
                    page.spawn(Node { flex_direction: FlexDirection::Column, flex_shrink: 0.0, row_gap: u(14.0), ..default() })
                        .with_children(|section| {
                            if let Some(heading) = heading {
                                section.spawn((
                                    SectionHead(t, range.start),
                                    Node { padding: UiRect::bottom(u(8.0)), border: UiRect::bottom(u(1.5)), ..default() },
                                    BorderColor::all(LINE),
                                    bold(&heading.to_uppercase(), 19.0, ui::ACCENT),
                                ));
                            }
                            section
                                .spawn(Node {
                                    display: Display::Grid,
                                    grid_template_columns: RepeatedGridTrack::flex(COLUMNS as u16, 1.0),
                                    column_gap: u(18.0),
                                    row_gap: u(18.0),
                                    ..default()
                                })
                                .with_children(|grid| {
                                    for i in range {
                                        grid.spawn((TileCell(t, i), Node { flex_direction: FlexDirection::Column, ..default() }))
                                            .with_children(|cell| spawn_tile(cell, t, i, &maps[i]));
                                    }
                                });
                        });
                }
            });
        }
    });
    // The footer: the picked map's name and kind (and a note when it cannot
    // start), then the difficulty (Campaign), Settings, Start Game and Back.
    root.spawn((ui::footer_node(), ui::bar())).with_children(|bar| {
        bar.spawn(Node { margin: UiRect::right(Val::Auto), flex_direction: FlexDirection::Column, row_gap: u(2.0), ..default() })
            .with_children(|about| {
                about.spawn(Node { column_gap: u(16.0), align_items: AlignItems::Baseline, ..default() }).with_children(|line| {
                    line.spawn((Chosen(0), bold("", 28.0, ui::BRIGHT)));
                    line.spawn((Chosen(1), text("", 18.0, LIT)));
                });
                about.spawn((Chosen(2), text("", 17.0, DIM)));
            });
        footer_button(
            bar,
            MenuButton::Difficulty,
            "Y",
            "F2",
            PAD_Y,
            (DifficultyLabel, text(&difficulty_label(menu.difficulty), 20.0, DIM)),
        );
        footer_button(bar, MenuButton::Settings, "X", "F1", PAD_X, text("SETTINGS", 20.0, DIM));
        footer_button(bar, MenuButton::Start, "A", "Enter", PAD_A, (StartLabel, bold("START GAME", 20.0, ui::BRIGHT)));
        footer_button(bar, MenuButton::Exit, "B", "Esc", PAD_B, text("BACK", 20.0, DIM));
    });
}

/// A map's tile: its picture in a frame (a plain well when MCC has none),
/// with its name across the foot, and NOT INSTALLED when MCC lacks it.
fn spawn_tile(cell: &mut ChildSpawnerCommands, t: usize, i: usize, map: &Map) {
    cell.spawn((
        Tile(t, i),
        Node {
            width: Val::Percent(100.0),
            aspect_ratio: Some(PICTURE_ASPECT),
            border: UiRect::all(u(3.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(ui::WELL),
        BorderColor::all(Color::NONE),
    ))
    .with_children(|frame| {
        if let Some(picture) = &map.picture {
            frame.spawn((
                TilePicture(t, i),
                ImageNode { color: TILE_TINT, ..ImageNode::new(picture.clone()) },
                Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            ));
        }
        // The name on a shaded strip along the bottom.
        frame
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    padding: UiRect::new(u(12.0), u(12.0), u(16.0), u(7.0)),
                    align_items: AlignItems::Baseline,
                    column_gap: u(10.0),
                    ..default()
                },
                BackgroundGradient::from(LinearGradient::to_bottom(vec![
                    ColorStop::auto(Color::NONE),
                    ColorStop::auto(Color::srgba(0.0, 0.01, 0.03, 0.85)),
                ])),
            ))
            .with_children(|strip| {
                strip.spawn((
                    TileName(t, i),
                    Node { flex_grow: 1.0, ..default() },
                    bold(&map.name.to_uppercase(), 18.0, if map.installed { ui::TEXT } else { ui::OFF }),
                ));
                if !map.installed {
                    strip.spawn(text("NOT INSTALLED", 13.0, ui::OFF));
                }
            });
    });
}

/// The loading screen: the map's picture across the screen behind a shade,
/// its name and what is being got ready at the bottom left, the ring at the
/// bottom right.
fn spawn_loading(screen: &mut ChildSpawnerCommands) {
    screen
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_child((
            LoadingPicture,
            ImageNode { color: Color::NONE, ..default() },
            Node { width: Val::Percent(100.0), aspect_ratio: Some(PICTURE_ASPECT), ..default() },
        ));
    screen.spawn((
        Node { position_type: PositionType::Absolute, bottom: Val::ZERO, width: Val::Percent(100.0), height: Val::Percent(62.0), ..default() },
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            ColorStop::new(ui::BACKDROP_BOTTOM.with_alpha(0.0), Val::Percent(0.0)),
            ColorStop::new(ui::BACKDROP_BOTTOM.with_alpha(0.96), Val::Percent(78.0)),
        ])),
    ));
    screen
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: u(96.0),
            bottom: u(92.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|words| {
            words.spawn(text("LOADING", 21.0, LIT));
            words.spawn((LoadingName, bold("", 88.0, ui::BRIGHT)));
            words.spawn((LoadingStatus, Node { min_height: u(30.0), ..default() }, text("", 22.0, DIM)));
        });
    let size = RING_RADIUS * 2.0 + 24.0;
    screen
        .spawn(Node { position_type: PositionType::Absolute, right: u(96.0), bottom: u(96.0), width: u(size), height: u(size), ..default() })
        .with_children(|ring| {
            let arc = std::f32::consts::TAU * RING_RADIUS / RING_SEGMENTS as f32;
            for n in 0..RING_SEGMENTS {
                let a = n as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
                let (w, h) = (arc * 0.7, 6.0);
                ring.spawn((
                    RingSegment(n),
                    Node {
                        position_type: PositionType::Absolute,
                        left: u(size / 2.0 + RING_RADIUS * a.cos() - w / 2.0),
                        top: u(size / 2.0 + RING_RADIUS * a.sin() - h / 2.0),
                        width: u(w),
                        height: u(h),
                        ..default()
                    },
                    UiTransform::from_rotation(Rot2::radians(a + std::f32::consts::FRAC_PI_2)),
                    BackgroundColor(RING_DARK),
                ));
            }
        });
}

/// What the keys, the pad and the left stick ask of a list this frame.
#[derive(Default)]
struct Moves {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

/// Arrows / WASD / d-pad, and the left stick once each time it is pushed
/// past half way (`stick`: which ways it was already held).
fn moves(keys: &ButtonInput<KeyCode>, gamepads: &Query<&Gamepad>, stick: &mut [bool; 4]) -> Moves {
    let key = |k: [KeyCode; 2]| keys.any_just_pressed(k);
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
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
    Moves {
        left: key([KeyCode::ArrowLeft, KeyCode::KeyA]) || pad(GamepadButton::DPadLeft) || pushed[0],
        right: key([KeyCode::ArrowRight, KeyCode::KeyD]) || pad(GamepadButton::DPadRight) || pushed[1],
        up: key([KeyCode::ArrowUp, KeyCode::KeyW]) || pad(GamepadButton::DPadUp) || pushed[2],
        down: key([KeyCode::ArrowDown, KeyCode::KeyS]) || pad(GamepadButton::DPadDown) || pushed[3],
    }
}

/// Opens a row of the main menu: the Campaign's maps, the multiplayer maps
/// on the tab last shown, Settings, or out of the game.
fn open_main_row(menu: &mut Menu, row: usize, exit: &mut MessageWriter<AppExit>) {
    menu.main_row = row;
    match row {
        MAIN_CAMPAIGN => {
            menu.tab = CAMPAIGN_TAB;
            menu.phase = Phase::Choosing;
        }
        MAIN_MAPS => {
            menu.tab = menu.mp_tab.min(CAMPAIGN_TAB - 1);
            menu.phase = Phase::Choosing;
        }
        MAIN_SETTINGS => open_settings(menu),
        _ => {
            exit.write(AppExit::Success);
        }
    }
    menu.click = None;
    menu.fresh = true;
}

/// The frame is over for the input systems: a screen opened in it takes
/// presses from the next one.
fn settle(mut menu: ResMut<Menu>) {
    if menu.fresh {
        menu.fresh = false;
    }
}

/// The main menu: up / down move, Enter / A (or a click) open the row; the
/// mouse lights the row it is over. Escape / B go to Exit, and from Exit
/// leave the game.
#[allow(clippy::too_many_arguments)]
fn main_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut menu: ResMut<Menu>,
    rows: Query<(&MainRow, &ComputedNode, &UiGlobalTransform)>,
    bar: Query<(&MenuButton, &ComputedNode, &UiGlobalTransform)>,
    mut exit: MessageWriter<AppExit>,
    mut stick: Local<[bool; 4]>,
    mut cursor_was: Local<Option<Vec2>>,
) {
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    let last = MAIN_ROWS.len() - 1;
    let go = moves(&keys, &gamepads, &mut stick);
    if go.up && menu.main_row > 0 {
        menu.main_row -= 1;
    }
    if go.down && menu.main_row < last {
        menu.main_row += 1;
    }
    let cursor = windows.single().ok().and_then(Window::physical_cursor_position);
    let over = cursor.and_then(|at| rows.iter().find(|(_, n, t)| n.contains_point(**t, at)).map(|(r, ..)| r.0));
    // The mouse takes the focus only when it moves, so it does not fight the keys.
    if cursor != *cursor_was {
        *cursor_was = cursor;
        if let Some(row) = over.filter(|r| *r != menu.main_row) {
            menu.main_row = row;
        }
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) || pad(GamepadButton::South) {
        let row = menu.main_row;
        open_main_row(&mut menu, row, &mut exit);
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || pad(GamepadButton::East) {
        if menu.main_row == last {
            exit.write(AppExit::Success);
        } else {
            menu.main_row = last;
        }
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if let Some(row) = over {
        open_main_row(&mut menu, row, &mut exit);
        return;
    }
    let Some(at) = cursor else {
        return;
    };
    match bar.iter().find(|(_, n, t)| n.contains_point(**t, at)).map(|(b, ..)| *b) {
        Some(MenuButton::Select) => {
            let row = menu.main_row;
            open_main_row(&mut menu, row, &mut exit);
        }
        Some(MenuButton::Exit) => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

/// The chooser's keys and pads: the arrows, WASD, the d-pad and the left
/// stick move between the tiles; Q / E, Tab, Page Up / Page Down or LB / RB
/// change the game's tab (the Campaign has none).
fn navigate(keys: Res<ButtonInput<KeyCode>>, gamepads: Query<&Gamepad>, mut menu: ResMut<Menu>, mut stick: Local<[bool; 4]>) {
    let key = |k: &[KeyCode]| k.iter().any(|k| keys.just_pressed(*k));
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    let go = moves(&keys, &gamepads, &mut stick);
    if key(&[KeyCode::F1]) || pad(GamepadButton::West) {
        open_settings(&mut menu);
        return;
    }
    if menu.tab == CAMPAIGN_TAB {
        if key(&[KeyCode::F2]) || pad(GamepadButton::North) {
            menu.next_difficulty();
        }
    } else {
        let tabs = CAMPAIGN_TAB;
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let tab_key = keys.just_pressed(KeyCode::Tab);
        if key(&[KeyCode::KeyE, KeyCode::PageDown]) || (tab_key && !shift) || pad(GamepadButton::RightTrigger) {
            menu.tab = (menu.tab + 1) % tabs;
        }
        if key(&[KeyCode::KeyQ, KeyCode::PageUp]) || (tab_key && shift) || pad(GamepadButton::LeftTrigger) {
            menu.tab = (menu.tab + tabs - 1) % tabs;
        }
        if menu.mp_tab != menu.tab {
            menu.mp_tab = menu.tab;
        }
    }
    let t = menu.tab;
    if menu.tabs[t].is_empty() {
        return;
    }
    let (dx, dy) = (go.right as isize - go.left as isize, go.down as isize - go.up as isize);
    let at = moved(&rows(t), menu.pick[t], dx, dy).min(menu.tabs[t].len() - 1);
    if at != menu.pick[t] {
        menu.pick[t] = at;
    }
}

/// Tiles across a tab's page.
const COLUMNS: usize = 6;

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

/// The mouse: a click on a tab shows it, on a tile picks it, a double click
/// on a tile or a click on Start Game starts, a click on Back goes back to
/// the main menu.
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
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(at) = windows.single().ok().and_then(Window::physical_cursor_position) else {
        return;
    };
    if menu.tab != CAMPAIGN_TAB
        && let Some((label, ..)) = tabs.iter().find(|(_, n, t)| n.contains_point(**t, at))
    {
        menu.tab = label.0;
        menu.mp_tab = label.0;
        return;
    }
    let tab = menu.tab;
    // A tile scrolled out of the page is not there to click.
    let on_page = pages.iter().any(|(page, n, t)| page.0 == tab && n.contains_point(*t, at));
    if let Some((row, ..)) = tiles.iter().find(|(tile, n, t)| on_page && tile.0 == tab && n.contains_point(**t, at)) {
        let now = time.elapsed_secs_f64();
        let double = menu.click.is_some_and(|(i, then)| i == row.1 && now - then < 0.4);
        menu.pick[row.0] = row.1;
        menu.click = Some((row.1, now));
        if double {
            start(&mut commands, &mut menu);
        }
        return;
    }
    match bar.iter().find(|(_, n, t)| n.contains_point(**t, at)).map(|(b, ..)| *b) {
        Some(MenuButton::Difficulty) => menu.next_difficulty(),
        Some(MenuButton::Settings) => open_settings(&mut menu),
        Some(MenuButton::Start) => start(&mut commands, &mut menu),
        Some(MenuButton::Exit) => menu.phase = Phase::Main,
        Some(MenuButton::Select) | None => {}
    }
}

/// The difficulty button's text.
fn difficulty_label(difficulty: usize) -> String {
    format!("DIFFICULTY: {}", DIFFICULTIES[difficulty].to_uppercase())
}

fn open_settings(menu: &mut Menu) {
    menu.under = if menu.phase == Phase::Choosing { Phase::Choosing } else { Phase::Main };
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
    rows_on_screen: Query<(&SettingRow, &ComputedNode, &UiGlobalTransform)>,
    act_button: Query<(&ComputedNode, &UiGlobalTransform), With<SettingsAct>>,
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
        menu.phase = menu.under;
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
    if let Some((s, ..)) = rows_on_screen.iter().find(|(s, n, t)| on_page(s.0) && n.contains_point(**t, at)) {
        // A click on a row takes the focus; on the focused row it changes it.
        if s.0 != menu.setting {
            go_to_row(&mut menu, s.0);
        } else {
            select(&mut menu, &mut live, s.0);
        }
    } else if act_button.iter().any(|(n, t)| n.contains_point(*t, at)) {
        select(&mut menu, &mut live, row);
    } else if back.iter().any(|(n, t)| n.contains_point(*t, at)) {
        menu.phase = menu.under;
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
/// row and what it is, the note, and what A does.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn show_settings(
    menu: Res<Menu>,
    unit: Res<SpeedUnit>,
    skate: Res<SkateEnabled>,
    tuning: Option<Res<mcc_bike::tune::BikeTuning>>,
    look: Res<LookSettings>,
    mut tabs: Query<(&SettingTab, &mut TextColor, &mut BorderColor), Without<SettingRow>>,
    mut screen: Query<&mut Node, (With<SettingsScreen>, Without<SettingRow>, Without<SettingsStepHint>)>,
    mut step_hint: Query<&mut Node, (With<SettingsStepHint>, Without<SettingRow>, Without<SettingsScreen>)>,
    mut rows: Query<(&SettingRow, &mut BackgroundColor, &mut BorderColor, &mut Node), Without<SettingsScreen>>,
    mut texts: Query<
        (AnyOf<(&SettingValue, &SettingTitle, &SettingAbout, &SettingsNote, &SettingActionLabel)>, &mut Text, &mut TextColor),
        Without<SettingTab>,
    >,
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
    let focused = menu.setting;
    let page = page_of(focused);
    for mut node in &mut step_hint {
        let any = SETTING_PAGES[page].1.clone().any(steps);
        node.display = if any { Display::Flex } else { Display::None };
    }
    for (tab, mut colour, mut border) in &mut tabs {
        (colour.0, *border) = ui::tab_lit(tab.0 == page, false);
    }
    for (row, mut fill, mut border, mut node) in &mut rows {
        let shown = page_of(row.0) == page && row_offered(row.0);
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        ui::focus(row.0 == focused, &mut fill, &mut border);
    }
    let reflex_physics = tuning.map_or_else(|| mcc_bike::tune::load().model, |t| t.0.model) >= 1.5;
    let skate_ready = game_paths::skate_ready();
    let now = setting_values(*unit, *skate, reflex_physics, skate_ready, *look, &menu.converted);
    let busy = menu.picking.as_ref().is_some_and(|p| p.what == Pick::Setting(focused));
    for ((value, title, about, note, action), mut text, mut colour) in &mut texts {
        if let Some(&SettingValue(i)) = value {
            text.0 = match &menu.editing {
                Some(typing) if i == USERNAME_ROW => format!("{typing}_"),
                _ if i == focused && steps(i) => format!("\u{2039}   {}   \u{203a}", now[i]),
                _ => now[i].clone(),
            };
            colour.0 = if now[i].starts_with("Not") {
                FAINT
            } else if i == focused {
                ui::BRIGHT
            } else {
                DIM
            };
        } else if title.is_some() {
            text.0 = SETTING_NAMES[focused].to_string();
        } else if about.is_some() {
            text.0 = setting_about(focused, skate_ready, skate.0).to_string();
        } else if note.is_some() {
            text.0.clone_from(&menu.note);
        } else if action.is_some() {
            text.0 = if busy {
                "WORKING..."
            } else if focused == USERNAME_ROW && menu.editing.is_some() {
                "SAVE"
            } else if menu.confirm == Some(focused) {
                "CONFIRM"
            } else {
                button_label(focused, skate_ready)
            }
            .to_string();
        }
    }
}

/// Enter / A starts, Escape / B goes back to the main menu.
fn act(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut menu: ResMut<Menu>,
    shot: Option<ResMut<MenuShot>>,
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
    } else if keys.any_just_pressed([KeyCode::Escape, KeyCode::Backspace]) || pad(GamepadButton::East) {
        menu.phase = Phase::Main;
        menu.error = None;
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

/// The new-version check's answer; a newer release shows its pop-up once the
/// chooser is up (not over the setup, settings or a loading map).
fn update_poll(mut menu: ResMut<Menu>) {
    if let Some(check) = &menu.update_check {
        let answer = match check.0.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(found) => Some(found),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
            },
            Err(_) => Some(None),
        };
        if let Some(found) = answer {
            menu.update_check = None;
            menu.update = found;
        }
    }
    if menu.update.is_some() && matches!(menu.phase, Phase::Main | Phase::Choosing) {
        menu.under = menu.phase;
        menu.phase = Phase::Update;
        menu.fresh = true;
    }
}

/// The new-version pop-up: Enter / Space / A or its button open the
/// release's page; Escape / B or Close shut it. Either way it is gone for
/// this launch.
fn update_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    gamepads: Query<&Gamepad>,
    clickable: Query<(&UpdateButton, &ComputedNode, &UiGlobalTransform)>,
    mut menu: ResMut<Menu>,
) {
    // The press that was down as it opened is not one that answers it.
    if menu.fresh {
        menu.fresh = false;
        return;
    }
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    let clicked = buttons
        .just_pressed(MouseButton::Left)
        .then(|| windows.single().ok().and_then(Window::physical_cursor_position))
        .flatten()
        .and_then(|at| clickable.iter().find(|(_, n, t)| n.contains_point(**t, at)).map(|(b, ..)| *b));
    let open = clicked == Some(UpdateButton::Open)
        || keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space])
        || pad(GamepadButton::South);
    let close = clicked == Some(UpdateButton::Close) || keys.just_pressed(KeyCode::Escape) || pad(GamepadButton::East);
    if !open && !close {
        return;
    }
    if open && let Some(release) = &menu.update {
        let page = release.page();
        match support::open_link(&page) {
            Ok(()) => eprintln!("menu: opened {page}"),
            Err(e) => eprintln!("menu: cannot open {page}: {e}"),
        }
    }
    menu.update = None;
    menu.phase = menu.under;
    menu.click = None;
}

/// Q in the pause menu (`pause.rs`): unload the map and show the menu again.
fn back_to_menu(world: &mut World) {
    if world.remove_resource::<BackToMenu>().is_none() {
        return;
    }
    mcc_core::map::unload(world);
    if let Ok(mut window) = world.query_filtered::<&mut Window, With<PrimaryWindow>>().single_mut(world) {
        window.title = "MCC Rust Rewrite".to_string();
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

/// Draws the state: which screen is up; on the main menu its focused row,
/// what it is and the picture; on the chooser its title, the tab shown, the
/// picked map's tile, its words in the footer, and the footer's buttons.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn show(
    menu: Res<Menu>,
    mut screens: Query<
        (AnyOf<(&MainScreen, &Chooser, &LoadingScreen, &TabPage, &MenuButton, &TabExtra, &TabLabel)>, &mut Node),
    >,
    mut pictures: Query<(AnyOf<(&MainPicture, &LoadingPicture)>, &mut ImageNode), Without<TilePicture>>,
    mut tile_pictures: Query<(&TilePicture, &mut ImageNode), (Without<MainPicture>, Without<LoadingPicture>)>,
    mut rows: Query<(&MainRow, &mut BackgroundColor, &mut BorderColor), (Without<TabLabel>, Without<SettingRow>, Without<Tile>)>,
    mut tiles: Query<(&Tile, &mut BorderColor), (Without<TabLabel>, Without<MainRow>)>,
    mut labels: Query<(&TabLabel, &mut TextColor, &mut BorderColor), (Without<MainRow>, Without<Tile>)>,
    mut texts: Query<
        (
            AnyOf<(
                &Chosen,
                &LoadingName,
                &DifficultyLabel,
                &MainLabel,
                &MainAbout,
                &MainCaption,
                &ChooserTitle,
                &TileName,
                &StartLabel,
            )>,
            &mut Text,
            &mut TextColor,
        ),
        Without<TabLabel>,
    >,
) {
    if !menu.is_changed() {
        return;
    }
    let campaign = menu.tab == CAMPAIGN_TAB;
    // The pop-ups sit over the screen they came up on.
    let phase = match menu.phase {
        Phase::Missing(_) => Phase::Choosing,
        Phase::Update => menu.under,
        phase => phase,
    };
    for ((main, chooser, loading, page, button, ..), mut node) in &mut screens {
        let shown = if main.is_some() {
            phase == Phase::Main
        } else if chooser.is_some() {
            phase == Phase::Choosing
        } else if loading.is_some() {
            phase == Phase::Loading
        } else if let Some(page) = page {
            page.0 == menu.tab
        } else if let Some(button) = button {
            *button != MenuButton::Difficulty || campaign
        } else {
            // The games' tabs and their hints: not on the Campaign.
            !campaign
        };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    let selected = menu.selected();
    for ((main, _), mut image) in &mut pictures {
        // Behind the loading screen's words, and quieter on the main menu.
        let tint = if main.is_some() { Color::srgb(0.82, 0.86, 0.92) } else { Color::srgb(0.62, 0.66, 0.72) };
        match selected.and_then(|m| m.picture.clone()) {
            Some(handle) => {
                image.image = handle;
                image.color = tint;
            }
            None => image.color = Color::NONE,
        }
    }
    for (row, mut fill, mut border) in &mut rows {
        ui::focus(row.0 == menu.main_row, &mut fill, &mut border);
    }
    // The picked tile is framed in the accent and drawn at full brightness;
    // a map MCC lacks stays dark.
    for (tile, mut border) in &mut tiles {
        let on = tile.1 == menu.pick[tile.0];
        *border = BorderColor::all(if on { ui::ACCENT } else { Color::NONE });
    }
    for (picture, mut image) in &mut tile_pictures {
        let on = picture.1 == menu.pick[picture.0];
        let installed = menu.tabs[picture.0].get(picture.1).is_some_and(|m| m.installed);
        image.color = match (installed, on) {
            (false, _) => MISSING_TINT,
            (true, true) => Color::WHITE,
            (true, false) => TILE_TINT,
        };
    }
    for (label, mut colour, mut border) in &mut labels {
        // On Campaign the tabs are hidden: their nodes are left alone.
        (colour.0, *border) = ui::tab_lit(label.0 == menu.tab, false);
    }
    for ((part, loading, difficulty, main, about, caption, heading, name, start), mut text, mut colour) in &mut texts {
        if let Some(part) = part {
            let note = |map: &Map| match &menu.error {
                Some(error) => error.clone(),
                None if !map.installed => format!("{} isn't installed in your Master Chief Collection.", game_title(map.game)),
                None => String::new(),
            };
            text.0 = match (selected, part.0) {
                (Some(map), 0) => map.name.to_uppercase(),
                (Some(map), 1) => kind(menu.tab, map).to_uppercase(),
                (Some(map), _) => note(map),
                (None, _) => String::new(),
            };
            if part.0 == 2 {
                colour.0 = if menu.error.is_some() { ui::ERROR } else { DIM };
            }
        } else if loading.is_some() {
            text.0 = selected.map_or(String::new(), |m| m.name.to_uppercase());
        } else if difficulty.is_some() {
            text.0 = difficulty_label(menu.difficulty);
        } else if let Some(row) = main {
            colour.0 = if row.0 == menu.main_row { ui::BRIGHT } else { DIM };
        } else if about.is_some() {
            text.0 = MAIN_ROWS[menu.main_row].1.to_string();
        } else if caption.is_some() {
            text.0 = selected.map_or(String::new(), |m| format!("{}  /  {}", m.name.to_uppercase(), kind(menu.tab, m).to_uppercase()));
        } else if heading.is_some() {
            text.0 = if campaign { "CAMPAIGN" } else { "MULTIPLAYER MAPS" }.to_string();
        } else if let Some(name) = name {
            let installed = menu.tabs[name.0].get(name.1).is_some_and(|m| m.installed);
            colour.0 = match (name.1 == menu.pick[name.0], installed) {
                (true, true) => ui::BRIGHT,
                (true, false) => ui::QUIET,
                (false, true) => ui::TEXT,
                (false, false) => ui::OFF,
            };
        } else if start.is_some() {
            colour.0 = if selected.is_some_and(|m| m.installed) { ui::BRIGHT } else { ui::OFF };
        }
    }
}

/// Scrolls the shown page so the picked tile is all in view: picking a tile
/// below the page's bottom row moves the page up by that row, and the same
/// upwards (with the section's heading, for its first row). Waits for the
/// page to be laid out (just shown, or rebuilt).
#[allow(clippy::type_complexity)]
fn follow(
    menu: Res<Menu>,
    mut pages: Query<(Entity, &TabPage, &ComputedNode, &UiGlobalTransform, &mut ScrollPosition)>,
    cells: Query<(&TileCell, &ComputedNode, &UiGlobalTransform)>,
    sections: Query<(&SectionHead, &ComputedNode, &UiGlobalTransform)>,
    mut done: Local<Option<(Entity, usize, usize)>>,
) {
    let (t, i) = (menu.tab, menu.pick[menu.tab]);
    let Some((page, _, node, at, mut scroll)) = pages.iter_mut().find(|(_, p, ..)| p.0 == t) else {
        return;
    };
    if *done == Some((page, t, i)) {
        return;
    }
    let Some((_, row, row_at)) = cells.iter().find(|(c, ..)| c.0 == t && c.1 == i) else {
        return;
    };
    if node.size.y <= 0.0 || row.size.y <= 0.0 {
        return;
    }
    // Top and bottom on screen, in physical pixels.
    let span = |n: &ComputedNode, at: &UiGlobalTransform| (at.translation.y - n.size.y / 2.0, at.translation.y + n.size.y / 2.0);
    let (page_top, page_bottom) = span(node, at);
    let (mut top, bottom) = span(row, row_at);
    // A section's first row brings its heading with it.
    if let Some((_, n, a)) = sections.iter().find(|(s, ..)| s.0 == t && i >= s.1 && i - s.1 < COLUMNS) {
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

fn show_update(
    menu: Res<Menu>,
    mut notice: Query<&mut Node, With<UpdateNotice>>,
    mut line: Query<&mut Text, With<UpdateLine>>,
) {
    if !menu.is_changed() {
        return;
    }
    let release = menu.update.as_ref().filter(|_| menu.phase == Phase::Update);
    for mut node in &mut notice {
        node.display = if release.is_some() { Display::Flex } else { Display::None };
    }
    if let Some(release) = release {
        for mut text in &mut line {
            text.0 = format!("{} is out on GitHub. You have {}.", release.tag, game_paths::VERSION);
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
        colour.0 = RING_DARK.mix(&LIT, glow * glow);
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

    /// Every settings row says what it is, and its value is drawn between
    /// arrows only where left and right change it.
    #[test]
    fn every_setting_says_what_it_is() {
        for row in 0..SETTING_NAMES.len() {
            for (ready, skating) in [(false, false), (true, false), (true, true)] {
                assert!(!setting_about(row, ready, skating).is_empty(), "{}", SETTING_NAMES[row]);
            }
        }
        assert!(steps(FOV_ROW) && steps(VSYNC_ROW) && !steps(USERNAME_ROW) && !steps(RESET_ROW));
        assert_eq!(MAIN_ROWS[MAIN_CAMPAIGN].0, "CAMPAIGN");
        assert_eq!(MAIN_ROWS[MAIN_MAPS].0, "MULTIPLAYER MAPS");
        assert_eq!(MAIN_ROWS[MAIN_SETTINGS].0, "SETTINGS");
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
