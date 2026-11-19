//! The menus' shared look: the font, the colours, the sizes and the few
//! pieces every screen is built from (title, tab, list row, panel, footer),
//! so the map menu, Settings, setup, the loading screen and the pause menu
//! read as one game.
//!
//! Sizes are given as pixels on a 1080-line screen and turned into parts of
//! the window's height ([`u`], [`fs`]), so a screen laid out once fits a
//! 720p window and a 4K one alike.
//!
//! The font is Barlow (SIL Open Font License, `fonts/OFL.txt`), compiled in
//! and put behind two fixed handles by [`UiThemePlugin`], so spawn helpers
//! need no asset access. The design is our own: nothing here is taken from
//! another game's menus.

use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::text::FontSize;

/// Barlow Medium: body text.
pub const FONT: Handle<Font> = uuid_handle!("6d1c2f0e-52a4-4d0b-9a43-1f0f6a3e9b01");
/// Barlow SemiBold: titles, tabs, the focused row.
pub const FONT_BOLD: Handle<Font> = uuid_handle!("6d1c2f0e-52a4-4d0b-9a43-1f0f6a3e9b02");

/// The screen behind everything: top and bottom of its gradient.
pub const BACKDROP_TOP: Color = Color::srgb(0.035, 0.075, 0.125);
pub const BACKDROP_BOTTOM: Color = Color::srgb(0.012, 0.024, 0.043);
/// A plain fill where a gradient cannot go (a camera's clear colour).
pub const BACKGROUND: Color = Color::srgb(0.020, 0.043, 0.075);
/// Panels and bars over the backdrop.
pub const PANEL: Color = Color::srgba(0.045, 0.095, 0.155, 0.82);
/// Hairlines: panel edges, the rule under the tabs.
pub const LINE: Color = Color::srgba(0.46, 0.66, 0.85, 0.28);
/// The accent: the focused row's bar, the lit tab, slider fills.
pub const ACCENT: Color = Color::srgb(0.56, 0.80, 1.0);
/// The focused row's fill.
pub const FOCUS: Color = Color::srgba(0.36, 0.62, 0.90, 0.30);
/// Text: bright, ordinary, quiet, and unavailable.
pub const BRIGHT: Color = Color::WHITE;
pub const TEXT: Color = Color::srgb(0.74, 0.82, 0.90);
pub const QUIET: Color = Color::srgb(0.45, 0.55, 0.66);
pub const OFF: Color = Color::srgb(0.27, 0.33, 0.41);
/// Something went wrong.
pub const ERROR: Color = Color::srgb(0.98, 0.52, 0.46);
/// Empty picture frames, slider tracks.
pub const WELL: Color = Color::srgba(0.60, 0.78, 1.0, 0.10);

/// `px` on a 1080-line screen, as a part of the window's height.
pub fn u(px: f32) -> Val {
    Val::Vh(px / 10.8)
}

/// A font size of `px` on a 1080-line screen.
pub fn fs(px: f32) -> FontSize {
    FontSize::Vh(px / 10.8)
}

/// Body text.
pub fn text(s: &str, size: f32, colour: Color) -> impl Bundle {
    (Text::new(s), TextFont { font: FONT.into(), font_size: fs(size), ..default() }, TextColor(colour))
}

/// Heavier text: titles, tabs, row names.
pub fn bold(s: &str, size: f32, colour: Color) -> impl Bundle {
    (Text::new(s), TextFont { font: FONT_BOLD.into(), font_size: fs(size), ..default() }, TextColor(colour))
}

/// The backdrop's gradient, for a full-screen node.
pub fn backdrop() -> BackgroundGradient {
    BackgroundGradient::from(LinearGradient::to_bottom(vec![
        ColorStop::new(BACKDROP_TOP, Val::Percent(0.0)),
        ColorStop::new(BACKDROP_BOTTOM, Val::Percent(100.0)),
    ]))
}

/// The focused row's fill.
pub fn focus_fill(on: bool) -> BackgroundColor {
    BackgroundColor(if on { FOCUS } else { Color::NONE })
}

/// A list row's node: full width, a bar down its left edge (lit by
/// [`focus`]), its children in a line. Spawn it with [`focus_fill`]`(false)`
/// and `BorderColor::all(Color::NONE)`.
pub fn row_node(height: f32) -> Node {
    Node {
        width: Val::Percent(100.0),
        min_height: u(height),
        flex_shrink: 0.0,
        align_items: AlignItems::Center,
        column_gap: u(20.0),
        padding: UiRect::new(u(22.0), u(22.0), u(4.0), u(4.0)),
        border: UiRect::left(u(4.0)),
        ..default()
    }
}

/// Everything a list row is spawned with besides its marker and children.
pub fn row(height: f32) -> impl Bundle {
    (row_node(height), focus_fill(false), BorderColor::all(Color::NONE))
}

/// Lights a list row ([`row`]) or puts it out.
pub fn focus(on: bool, fill: &mut BackgroundColor, border: &mut BorderColor) {
    let want = focus_fill(on);
    if *fill != want {
        *fill = want;
    }
    let edge = BorderColor::all(if on { ACCENT } else { Color::NONE });
    if *border != edge {
        *border = edge;
    }
}

/// A panel: the translucent fill with a hairline round it.
pub fn panel() -> impl Bundle {
    (BackgroundColor(PANEL), BorderColor::all(LINE))
}

/// The bar across the top of a screen: holds the title and the tabs, with a
/// rule under it.
pub fn header_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: u(118.0),
        flex_shrink: 0.0,
        padding: UiRect::new(u(72.0), u(72.0), u(0.0), u(0.0)),
        column_gap: u(34.0),
        align_items: AlignItems::FlexEnd,
        border: UiRect::bottom(u(1.5)),
        ..default()
    }
}

/// A tab's node in a header: its name sits on the rule, which is lit under
/// the shown tab ([`tab_lit`]).
pub fn tab_node() -> Node {
    Node {
        padding: UiRect::new(u(4.0), u(4.0), u(6.0), u(14.0)),
        border: UiRect::bottom(u(4.0)),
        margin: UiRect::bottom(u(-1.5)),
        ..default()
    }
}

/// The colours of a tab's name and its underline: shown, under the mouse,
/// or neither.
pub fn tab_lit(on: bool, hovered: bool) -> (Color, BorderColor) {
    match (on, hovered) {
        (true, _) => (BRIGHT, BorderColor::all(ACCENT)),
        (false, true) => (BRIGHT, BorderColor::all(Color::NONE)),
        (false, false) => (QUIET, BorderColor::all(Color::NONE)),
    }
}

/// The bar across the bottom of a screen: button hints, right-aligned.
pub fn footer_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: u(84.0),
        flex_shrink: 0.0,
        padding: UiRect::horizontal(u(72.0)),
        column_gap: u(36.0),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::FlexEnd,
        border: UiRect::top(u(1.5)),
        ..default()
    }
}

/// What a header or footer bar is spawned with besides its node.
pub fn bar() -> impl Bundle {
    (BackgroundColor(Color::srgba(0.0, 0.01, 0.03, 0.45)), BorderColor::all(LINE))
}

pub struct UiThemePlugin;

impl Plugin for UiThemePlugin {
    fn build(&self, app: &mut App) {
        let Some(mut fonts) = app.world_mut().get_resource_mut::<Assets<Font>>() else {
            return;
        };
        for (handle, bytes) in [
            (FONT, &include_bytes!("../fonts/Barlow-Medium.ttf")[..]),
            (FONT_BOLD, &include_bytes!("../fonts/Barlow-SemiBold.ttf")[..]),
        ] {
            if fonts.insert(handle.id(), Font::from_bytes(bytes.to_vec())).is_err() {
                warn!("ui: a menu font could not be put in");
            }
        }
    }
}
