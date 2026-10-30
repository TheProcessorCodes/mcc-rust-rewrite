//! The map menu: `halo3` with no map opens it. Three tabs (Halo 3, Halo 2,
//! Campaign) hold a grid of the maps found in the user's MCC install, each
//! shown with MCC's own preview picture (`levelstexturepack`, read at run
//! time, never copied into the repo). Start Game runs `halo3 <map>` and
//! closes the menu; Exit closes the game.
//!
//! Keys: arrows / WASD / d-pad / left stick move, Q / E / LB / RB change tab,
//! Enter / Space / A start, Escape / B exit. The mouse selects a tile with a
//! click and starts it with a double click.
//!
//! `halo3 --menu-shot FILE [--menu-tab N] [--menu-pick N] [--size WxH]`
//! draws the menu without a window into a PNG.

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::ui::UiGlobalTransform;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};

const TABS: [&str; 3] = ["HALO 3", "HALO 2", "CAMPAIGN"];
const COLUMNS: usize = 6;

/// (map argument, shown name, MCC preview texture without `_LARGE`).
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
    ("h3:130_epilogue", "Epilogue", Some("H3_C11_EPILOGUE")),
];

const KINDS: [&str; 3] = ["Halo 3 multiplayer", "Halo 2 multiplayer", "Halo 3 campaign"];

const BACKGROUND: Color = Color::srgb(0.043, 0.059, 0.082);
const BAR: Color = Color::srgb(0.071, 0.098, 0.141);
const LINE: Color = Color::srgb(0.149, 0.192, 0.259);
const DIM: Color = Color::srgb(0.65, 0.70, 0.78);
const FAINT: Color = Color::srgb(0.34, 0.39, 0.48);
const LIT: Color = Color::srgb(1.0, 0.82, 0.35);
const EMPTY: Color = Color::srgb(0.10, 0.13, 0.18);

/// One map the menu offers.
struct Map {
    arg: &'static str,
    name: &'static str,
    picture: Option<Handle<Image>>,
}

#[derive(Resource)]
struct Menu {
    tabs: [Vec<Map>; 3],
    tab: usize,
    /// The selected tile on each tab.
    pick: [usize; 3],
    /// Arguments passed on to the game after the map (`--foot`, ...).
    pass: Vec<String>,
    /// The last click: tile and time, for the double click.
    click: Option<(usize, f64)>,
}

impl Menu {
    fn selected(&self) -> Option<&Map> {
        self.tabs[self.tab].get(self.pick[self.tab])
    }
}

#[derive(Component)]
struct TabLabel(usize);
#[derive(Component)]
struct TabPage(usize);
#[derive(Component)]
struct Tile(usize, usize);
#[derive(Component)]
struct TileLabel(usize, usize);
#[derive(Component)]
/// The bar's text: 0 the map's name, 1 which game and mode.
struct Chosen(usize);
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

    /// `NAME_LARGE` as RGBA.
    fn picture(&self, name: &str) -> Option<Image> {
        let want = format!("{name}_LARGE\0");
        let at = self.perm.windows(want.len()).position(|w| w == want.as_bytes())?.checked_sub(4)?;
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

/// Where the headless picture goes, and its size.
#[derive(Resource)]
struct MenuShot {
    path: PathBuf,
    size: UVec2,
    frames: u32,
    target: Option<Handle<Image>>,
}

/// Runs the menu (no map on the command line). `args` are the options
/// given, passed on to the game.
pub fn run(args: &[String]) -> anyhow::Result<()> {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let shot = value("--menu-shot").map(|path| {
        let size = value("--size")
            .and_then(|s| s.split_once('x').and_then(|(w, h)| Some(UVec2::new(w.parse().ok()?, h.parse().ok()?))))
            .unwrap_or(UVec2::new(1920, 1080));
        MenuShot { path: path.into(), size, frames: 0, target: None }
    });
    let mut tab = value("--menu-tab").and_then(|s| s.parse().ok()).unwrap_or(0usize).min(2);
    let pick = value("--menu-pick").and_then(|s| s.parse().ok()).unwrap_or(0usize);
    let menu_flags = ["--menu-shot", "--menu-tab", "--menu-pick", "--size"];
    let mut pass = Vec::new();
    let mut skip = false;
    for a in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        if menu_flags.contains(&a.as_str()) {
            skip = true;
        } else {
            pass.push(a.clone());
        }
    }

    let mut app = App::new();
    match &shot {
        Some(_) => app
            .add_plugins(
                DefaultPlugins
                    .set(WindowPlugin {
                        primary_window: None,
                        exit_condition: bevy::window::ExitCondition::DontExit,
                        ..default()
                    })
                    .disable::<bevy::winit::WinitPlugin>()
                    .disable::<bevy::audio::AudioPlugin>(),
            )
            .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::from_millis(1))),
        None => app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Halo 3".into(),
                        present_mode: PresentMode::AutoVsync,
                        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                        ..default()
                    }),
                    ..default()
                })
                .disable::<bevy::audio::AudioPlugin>(),
        ),
    };

    // The maps this install has, with their pictures.
    let root = mcc_root();
    let pack = Pack::open(&root);
    if pack.is_none() {
        eprintln!("menu: no map pictures under {}", root.join(r"Data\UI\texturepacks").display());
    }
    let mut images = app.world_mut().resource_mut::<Assets<Image>>();
    let tabs = [HALO3, HALO2, CAMPAIGN].map(|entries| {
        entries
            .iter()
            .filter(|(arg, ..)| crate::resolve_map(arg).exists())
            .map(|&(arg, name, texture)| Map {
                arg,
                name,
                picture: texture
                    .and_then(|t| pack.as_ref()?.picture(t))
                    .map(|i| images.add(i)),
            })
            .collect::<Vec<_>>()
    });
    eprintln!(
        "menu: {} Halo 3, {} Halo 2, {} campaign maps under {}",
        tabs[0].len(),
        tabs[1].len(),
        tabs[2].len(),
        root.display()
    );
    if tabs[tab].is_empty() {
        tab = (0..3).find(|&t| !tabs[t].is_empty()).unwrap_or(0);
    }
    let mut picks = [0; 3];
    picks[tab] = pick.min(tabs[tab].len().saturating_sub(1));
    app.insert_resource(Menu { tabs, tab, pick: picks, pass, click: None })
        .add_systems(Startup, spawn)
        .add_systems(Update, (navigate, mouse, show).chain());
    if let Some(shot) = shot {
        app.insert_resource(shot).add_systems(Update, capture.after(show));
    } else {
        app.add_systems(Update, act.after(show));
    }
    app.run();
    Ok(())
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
            commands.spawn((Camera2d, RenderTarget::Image(handle.into()))).id()
        }
        None => commands.spawn(Camera2d).id(),
    };
    commands
        .spawn((
            UiTargetCamera(camera),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BACKGROUND),
        ))
        .with_children(|root| {
            // The tabs.
            root.spawn(Node {
                width: Val::Percent(92.0),
                max_width: Val::Vh(165.0),
                margin: UiRect::top(Val::Vh(4.0)),
                column_gap: Val::Px(36.0),
                align_items: AlignItems::FlexEnd,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            })
            .insert(BorderColor::all(LINE))
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
                            .with_children(|tile| {
                                let frame = (
                                    Tile(t, i),
                                    Node {
                                        width: Val::Percent(100.0),
                                        aspect_ratio: Some(668.0 / 292.0),
                                        border: UiRect::all(Val::Px(3.0)),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    },
                                    BorderColor::all(Color::NONE),
                                );
                                match &map.picture {
                                    Some(picture) => {
                                        tile.spawn(frame).with_child((
                                            ImageNode::new(picture.clone()),
                                            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                                        ));
                                    }
                                    None => {
                                        tile.spawn((frame, BackgroundColor(EMPTY)))
                                            .with_child(text(&map.name.to_uppercase(), 16.0, FAINT));
                                    }
                                }
                                tile.spawn((
                                    TileLabel(t, i),
                                    Node { margin: UiRect::top(Val::Px(5.0)), ..default() },
                                    text(map.name, 17.0, DIM),
                                ));
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
                    (MenuButton::Start, "A", Color::srgb(0.37, 0.75, 0.29), "START GAME", "Enter"),
                    (MenuButton::Exit, "B", Color::srgb(0.85, 0.33, 0.31), "EXIT", "Esc"),
                ] {
                    bar.spawn((
                        button,
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(10.0),
                            padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BorderColor::all(Color::NONE),
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
                        b.spawn(text(label, 20.0, if button == MenuButton::Start { LIT } else { DIM }));
                        b.spawn(text(&format!("({key})"), 15.0, FAINT));
                    });
                }
            });
        });
}

/// Keys and pads: move, change tab.
fn navigate(keys: Res<ButtonInput<KeyCode>>, gamepads: Query<&Gamepad>, mut menu: ResMut<Menu>, mut stick: Local<[bool; 4]>) {
    let key = |k: &[KeyCode]| k.iter().any(|k| keys.just_pressed(*k));
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    // The left stick steps once each time it is pushed past half way.
    let axis = |a: GamepadAxis| gamepads.iter().map(|p| p.get(a).unwrap_or(0.0)).fold(0.0, |m: f32, v| if v.abs() > m.abs() { v } else { m });
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
    menu.pick[t] = at.clamp(0, n as isize - 1) as usize;
}

/// The mouse: a click on a tab shows it, on a tile picks it, a double click
/// on a tile or a click on Start Game starts, a click on Exit exits.
#[allow(clippy::too_many_arguments)]
fn mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time<Real>>,
    mut menu: ResMut<Menu>,
    tabs: Query<(&TabLabel, &ComputedNode, &UiGlobalTransform)>,
    tiles: Query<(&Tile, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>,
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
    if let Some((tile, ..)) = tiles.iter().find(|(tile, n, t, v)| v.get() && tile.0 == menu.tab && n.contains_point(**t, at)) {
        let now = time.elapsed_secs_f64();
        let double = menu.click.is_some_and(|(i, then)| i == tile.1 && now - then < 0.4);
        menu.pick[tile.0] = tile.1;
        menu.click = Some((tile.1, now));
        if double {
            start(&menu, &mut exit);
        }
        return;
    }
    match bar.iter().find(|(_, n, t)| n.contains_point(**t, at)).map(|(b, ..)| *b) {
        Some(MenuButton::Start) => start(&menu, &mut exit),
        Some(MenuButton::Exit) => {
            exit.write(AppExit::Success);
        }
        None => {}
    }
}

/// Enter / A starts, Escape / B exits.
fn act(keys: Res<ButtonInput<KeyCode>>, gamepads: Query<&Gamepad>, menu: Res<Menu>, mut exit: MessageWriter<AppExit>) {
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) || pad(GamepadButton::South) {
        start(&menu, &mut exit);
    } else if keys.just_pressed(KeyCode::Escape) || pad(GamepadButton::East) {
        exit.write(AppExit::Success);
    }
}

/// Runs `halo3 <map>` in a new process and closes the menu.
fn start(menu: &Menu, exit: &mut MessageWriter<AppExit>) {
    let Some(map) = menu.selected() else {
        return;
    };
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            error!("menu: cannot find the game's exe: {e}");
            return;
        }
    };
    eprintln!("menu: starting {} ({})", map.name, map.arg);
    match std::process::Command::new(exe).arg(map.arg).args(&menu.pass).spawn() {
        Ok(_) => {
            exit.write(AppExit::Success);
        }
        Err(e) => error!("menu: cannot start {}: {e}", map.arg),
    }
}

/// Draws the state: the tab shown, the gold frame on the pick, the bar.
#[allow(clippy::type_complexity)]
fn show(
    menu: Res<Menu>,
    mut pages: Query<(&TabPage, &mut Node)>,
    mut labels: Query<(&TabLabel, &mut TextColor, &mut BorderColor), (Without<TileLabel>, Without<Tile>)>,
    mut tiles: Query<(&Tile, &mut BorderColor), Without<TabLabel>>,
    mut names: Query<(&TileLabel, &mut TextColor), (Without<TabLabel>, Without<Chosen>)>,
    mut chosen: Query<(&Chosen, &mut Text)>,
) {
    if !menu.is_changed() {
        return;
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
    for (part, mut text) in &mut chosen {
        text.0 = match (menu.selected(), part.0) {
            (Some(map), 0) => map.name.to_string(),
            (Some(_), _) => KINDS[menu.tab].to_string(),
            (None, _) => String::new(),
        };
    }
}

/// `--menu-shot`: after a few frames, save the menu and quit.
fn capture(mut commands: Commands, mut shot: ResMut<MenuShot>, mut exit: MessageWriter<AppExit>) {
    shot.frames += 1;
    if shot.frames == 20
        && let Some(target) = shot.target.clone()
    {
        eprintln!("menu: shot {}", shot.path.display());
        commands
            .spawn(Screenshot::image(target))
            .observe(save_to_disk(shot.path.clone()))
            .observe(|_: On<ScreenshotCaptured>, mut shot: ResMut<MenuShot>| {
                shot.frames = u32::MAX - 100;
            });
    }
    if shot.frames > u32::MAX - 90 {
        exit.write(AppExit::Success);
    }
}
