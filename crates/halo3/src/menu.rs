//! The map menu: `halo3` with no map opens it. Five tabs (Halo 3, Halo 2,
//! Halo CE, Reach, Campaign) hold a grid of the maps found in the user's MCC install, each
//! shown with MCC's own preview picture (`levelstexturepack`, read at run
//! time, never copied into the repo). Start Game decodes the map on a thread
//! behind a loading screen, then puts it into this same app (`main.rs`
//! `install`, `halo3_core::map`); Exit closes the game.
//!
//! Keys: arrows / WASD / d-pad / left stick move, Q / E / LB / RB change tab,
//! Enter / Space / A start, Escape / B exit. The mouse selects a tile with a
//! click and starts it with a double click.
//!
//! `halo3 --menu-shot FILE [--menu-tab N] [--menu-pick N] [--size WxH]`
//! draws the menu without a window into a PNG; `--menu-loading` draws the
//! loading screen instead, and `--menu-start` loads the picked map the way
//! Start Game does and saves the first view of it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;

use halo3_core::map::{BackToMenu, MapAppExt, MapInstalled, MapMenu, MapReady, Warmup};

const TABS: [&str; 5] = ["HALO 3", "HALO 2", "HALO CE", "REACH", "CAMPAIGN"];
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

/// Halo 3's missions in story order. Crow's Nest has no preview in MCC.
const CAMPAIGN: &[Entry] = &[
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
    // A mission of ours (`main.rs` `MISSIONS`): Halo from the Warthog run on.
    ("warthog_run", "The Warthog Run", Some("H3_C10_HALO3")),
    ("h3:130_epilogue", "Epilogue", Some("H3_C11_EPILOGUE")),
];

const KINDS: [&str; 5] = [
    "Halo 3 multiplayer",
    "Halo 2 multiplayer",
    "Halo CE multiplayer",
    "Halo Reach multiplayer",
    "Halo 3 campaign",
];

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

/// One map the menu offers.
struct Map {
    arg: &'static str,
    name: &'static str,
    picture: Option<Handle<Image>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Choosing,
    Loading,
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
}

impl Menu {
    fn selected(&self) -> Option<&Map> {
        self.tabs[self.tab].get(self.pick[self.tab])
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
#[derive(Component)]
struct TabPage(usize);
#[derive(Component)]
struct Tile(usize, usize);
#[derive(Component)]
struct TileLabel(usize, usize);
/// The bar's text: 0 the map's name, 1 which game and mode.
#[derive(Component)]
struct Chosen(usize);
#[derive(Component)]
struct StartLabel;
#[derive(Component, Clone, Copy, PartialEq)]
enum MenuButton {
    Start,
    Exit,
}

/// MCC's install root, from the Halo 3 maps folder (`halo3\maps`).
fn mcc_root() -> PathBuf {
    let maps = std::env::var_os("HALO3_MAPS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(halo3_core::DEFAULT_MAPS));
    maps.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or(maps)
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
        let mut tab = value(args, "--menu-tab").and_then(|s| s.parse().ok()).unwrap_or(0usize).min(TABS.len() - 1);
        let pick = value(args, "--menu-pick").and_then(|s| s.parse().ok()).unwrap_or(0usize);
        let with_value = ["--menu-shot", "--menu-tab", "--menu-pick", "--menu-back", "--size"];
        let alone = ["--menu-loading", "--menu-start"];
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

        // The maps this install has, with their pictures.
        let root = mcc_root();
        let pack = Pack::open(&root);
        if pack.is_none() {
            eprintln!("menu: no map pictures under {}", root.join(r"Data\UI\texturepacks").display());
        }
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let tabs = [HALO3, HALO2, HALO1, REACH, CAMPAIGN].map(|entries| {
            entries
                .iter()
                .filter(|(arg, ..)| crate::map_exists(arg))
                .map(|&(arg, name, texture)| Map {
                    arg,
                    name,
                    picture: texture.and_then(|t| pack.as_ref()?.picture(t)).map(|i| images.add(i)),
                })
                .collect::<Vec<_>>()
        });
        eprintln!(
            "menu: {} Halo 3, {} Halo 2, {} Halo CE, {} Reach, {} campaign maps under {}",
            tabs[0].len(),
            tabs[1].len(),
            tabs[2].len(),
            tabs[3].len(),
            tabs[4].len(),
            root.display()
        );
        if tabs[tab].is_empty() {
            tab = (0..TABS.len()).find(|&t| !tabs[t].is_empty()).unwrap_or(0);
        }
        let mut picks = [0; TABS.len()];
        picks[tab] = pick.min(tabs[tab].len().saturating_sub(1));
        let choosing = |menu: Res<Menu>| menu.phase == Phase::Choosing;
        // What `install` (main.rs) puts in goes when the map is unloaded.
        app.insert_resource(MapMenu)
            .remove_on_map_unload::<crate::falls::KillZones>()
            .remove_on_map_unload::<halo3_core::lifts::Lifts>()
            .remove_on_map_unload::<crate::world::stream::ZoneTriggers>()
            .remove_on_map_unload::<crate::world::PendingScene>()
            .remove_on_map_unload::<crate::rider::PendingChief>()
            .remove_on_map_unload::<crate::skate::SkateInput>()
            .remove_on_map_unload::<crate::bike::BikeInput>()
            .remove_on_map_unload::<crate::swim::WaterWorld>()
            .remove_on_map_unload::<crate::swim::FallbackSpot>()
            .remove_on_map_unload::<crate::weapons::PendingArsenal>();
        app.insert_resource(Menu {
            tabs,
            tab,
            pick: picks,
            phase: Phase::Choosing,
            args: pass,
            click: None,
            error: None,
        })
        .add_systems(Startup, spawn)
        .add_systems(Update, ((navigate, mouse, act).run_if(choosing), show, spin).chain())
        .add_systems(Last, (back_to_menu, finish_loading, leave).chain());
        if let Some(shot) = shot {
            app.insert_resource(shot).add_systems(Update, capture.after(show));
        }
    }
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
        tabs.spawn(text("LB", 15.0, FAINT));
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
        tabs.spawn(text("RB", 15.0, FAINT));
    });
    // A grid of tiles per tab.
    for (t, maps) in menu.tabs.iter().enumerate() {
        root.spawn((
            TabPage(t),
            Node {
                width: Val::Percent(92.0),
                max_width: Val::Vh(165.0),
                margin: UiRect::top(Val::Vh(3.0)),
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::flex(COLUMNS as u16, 1.0),
                column_gap: Val::Vh(1.6),
                row_gap: Val::Vh(1.6),
                flex_grow: 1.0,
                align_content: AlignContent::Start,
                overflow: Overflow::clip(),
                ..default()
            },
        ))
        .with_children(|grid| {
            if maps.is_empty() {
                grid.spawn(text("No maps for this game in the MCC install.", 18.0, DIM));
            }
            for (i, map) in maps.iter().enumerate() {
                grid.spawn(Node { flex_direction: FlexDirection::Column, ..default() })
                    .with_children(|tile| spawn_tile(tile, t, i, map));
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
                b.spawn((
                    Node {
                        width: Val::Px(26.0),
                        height: Val::Px(26.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(colour),
                ))
                .with_child(text(letter, 15.0, Color::BLACK));
                if button == MenuButton::Start {
                    b.spawn((StartLabel, text(label, 20.0, LIT)));
                } else {
                    b.spawn(text(label, 20.0, DIM));
                }
                b.spawn(text(&format!("({key})"), 15.0, FAINT));
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
                frame.spawn((
                    ImageNode::new(picture.clone()),
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                ));
            }
            None => {
                frame.spawn(text(&map.name.to_uppercase(), 16.0, FAINT));
            }
        }
    });
    tile.spawn((
        TileLabel(t, i),
        Node { margin: UiRect::top(Val::Px(5.0)), ..default() },
        text(map.name, 17.0, DIM),
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

    let tabs = TABS.len();
    if key(&[KeyCode::KeyE, KeyCode::Tab]) || pad(GamepadButton::RightTrigger) {
        menu.tab = (menu.tab + 1) % tabs;
    }
    if key(&[KeyCode::KeyQ]) || pad(GamepadButton::LeftTrigger) {
        menu.tab = (menu.tab + tabs - 1) % tabs;
    }
    let n = menu.tabs[menu.tab].len();
    if n == 0 {
        return;
    }
    let t = menu.tab;
    let mut at = menu.pick[t] as isize;
    if key(&[KeyCode::ArrowLeft, KeyCode::KeyA]) || pad(GamepadButton::DPadLeft) || pushed[0] {
        at -= 1;
    }
    if key(&[KeyCode::ArrowRight, KeyCode::KeyD]) || pad(GamepadButton::DPadRight) || pushed[1] {
        at += 1;
    }
    if key(&[KeyCode::ArrowUp, KeyCode::KeyW]) || pad(GamepadButton::DPadUp) || pushed[2] {
        at -= COLUMNS as isize;
    }
    if key(&[KeyCode::ArrowDown, KeyCode::KeyS]) || pad(GamepadButton::DPadDown) || pushed[3] {
        at += COLUMNS as isize;
    }
    let at = at.clamp(0, n as isize - 1) as usize;
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
    if let Some((tile, ..)) = tiles.iter().find(|(tile, n, t)| tile.0 == tab && n.contains_point(**t, at)) {
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
        Some(MenuButton::Start) => start(&mut commands, &mut menu),
        Some(MenuButton::Exit) => {
            exit.write(AppExit::Success);
        }
        None => {}
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
/// until it is in.
fn start(commands: &mut Commands, menu: &mut Menu) {
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
    let (send, receive) = std::sync::mpsc::channel();
    let args = menu.args.clone();
    std::thread::spawn(move || {
        let _ = send.send(crate::load(arg, &args));
    });
    menu.phase = Phase::Loading;
    menu.error = None;
    Some(Loading(Mutex::new(receive)))
}

/// Q in the pause menu (`pause.rs`): unload the map and show the menu again.
fn back_to_menu(world: &mut World) {
    if world.remove_resource::<BackToMenu>().is_none() {
        return;
    }
    halo3_core::map::unload(world);
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
    halo3_core::map::unload(world);
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
    let args = world.resource::<Menu>().args.clone();
    let failed = match result {
        Ok(map) => {
            let title = format!("Halo 3 - {}", map.scene_name());
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

/// Longest the loading screen waits for the map's [`Warmup`] work.
const WARMUP_LIMIT: f32 = 180.0;

/// Once the map has started and its [`Warmup`] work (the bike's collision,
/// the skate session) is done, the menu, its camera and the loading screen
/// go. Meanwhile the screen says what it is waiting for.
#[allow(clippy::too_many_arguments)]
fn leave(
    mut commands: Commands,
    ready: Option<Res<MapReady>>,
    warmup: Option<Res<Warmup>>,
    shot: Option<Res<MenuShot>>,
    time: Res<Time<Real>>,
    mut since: Local<Option<f32>>,
    mut status: Query<&mut Text, With<LoadingStatus>>,
    roots: Query<Entity, Or<(With<MenuRoot>, With<MenuCamera>)>>,
) {
    // `--menu-shot --menu-loading` keeps the loading screen for its picture.
    if ready.is_none() || shot.is_some_and(|s| s.loading) {
        *since = None;
        return;
    }
    if roots.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    let started = *since.get_or_insert(now);
    if let Some(warmup) = warmup.filter(|w| !w.0.is_empty()) {
        if now - started < WARMUP_LIMIT {
            let waiting: Vec<&str> = warmup.0.iter().copied().collect();
            let line = format!("Preparing {}", waiting.join(" and "));
            for mut text in &mut status {
                if text.0 != line {
                    text.0 = line.clone();
                }
            }
            return;
        }
        eprintln!("menu: still preparing {:?} after {WARMUP_LIMIT} s; into the map anyway", warmup.0);
    }
    eprintln!("menu: map ready, prepared in {:.1} s", now - started);
    *since = None;
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
    mut texts: Query<(Option<&Chosen>, Option<&LoadingName>, &mut Text)>,
) {
    if !menu.is_changed() {
        return;
    }
    let loading_now = menu.phase == Phase::Loading;
    for mut node in &mut chooser {
        node.display = if loading_now { Display::None } else { Display::Flex };
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
        node.display = if page.0 == menu.tab { Display::Grid } else { Display::None };
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
        colour.0 = if label.1 == menu.pick[label.0] { LIT } else { DIM };
    }
    for mut colour in &mut start_label {
        colour.0 = if selected.is_some() { LIT } else { FAINT };
    }
    for (part, name, mut text) in &mut texts {
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
            (Some(_), _) => KINDS[menu.tab].to_string(),
            (None, _) => String::new(),
        };
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
