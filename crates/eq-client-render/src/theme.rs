//! The client's look: one palette and one type scale. Windows, the HUD and
//! their helpers name these instead of writing colours and sizes, so the
//! chrome stays consistent and changes in one place.
//!
//! The world's own colours (lights, markers, placeholder models) stay with
//! the scene, and colours that come from the game's data (chat channels,
//! dyes) stay data.
//!
//! Every text is in Arial, as the official client writes, read from
//! Windows' own fonts at run time and never bundled; without it, Bevy's
//! built-in face stays.
use bevy::{asset::AssetId, ecs::system::EntityCommands, prelude::*};
use eq_client_core::combat::ConColor;
use std::path::{Path, PathBuf};

// Surfaces, from the back.

/// A window's body.
pub(crate) const PANEL: Color = Color::srgba(0.025, 0.032, 0.04, 0.94);
/// What covers the world entirely, such as character selection.
pub(crate) const COVER: Color = Color::srgb(0.025, 0.032, 0.04);
/// A window's title bar, or a panel standing on a cover.
pub(crate) const TITLE_BAR: Color = Color::srgb(0.055, 0.067, 0.078);
/// A group or a slot's ground inside a window.
pub(crate) const INSET: Color = Color::srgb(0.08, 0.10, 0.13);
/// An empty slot, a bar's track or a text field: the deepest surface.
pub(crate) const WELL: Color = Color::srgb(0.04, 0.05, 0.06);
/// Small surfaces over the world or other windows: tooltips, the item on
/// the cursor, notices, counts on icons.
pub(crate) const SCRIM: Color = Color::srgba(0.02, 0.03, 0.04, 0.9);
/// What lies over a control the server cannot do.
pub(crate) const VEIL: Color = Color::srgba(0.03, 0.035, 0.04, 0.82);

/// A window's or panel's surface and edge; its node gives the edge a width.
pub(crate) fn surface() -> (BackgroundColor, BorderColor) {
    (BackgroundColor(PANEL), BorderColor::all(EDGE))
}

// Edges.

/// A window's or slot's edge.
pub(crate) const EDGE: Color = Color::srgb(0.23, 0.25, 0.26);
/// The edge of a swatch or other coloured surface, light enough to show
/// against it.
pub(crate) const EDGE_LIGHT: Color = Color::srgb(0.5, 0.55, 0.6);
/// A slot that holds something.
pub(crate) const EDGE_HELD: Color = Color::srgb(0.48, 0.43, 0.31);
/// A slot under the pointer, or the tab being shown.
pub(crate) const EDGE_HOVER: Color = Color::srgb(0.88, 0.77, 0.45);
/// Whatever has the input: the item on the cursor, the chat being typed in.
pub(crate) const FOCUS: Color = Color::srgb(0.45, 0.82, 1.0);

// Controls.

/// A button at rest.
pub(crate) const BUTTON: Color = Color::srgb(0.10, 0.13, 0.17);
/// A button under the pointer.
pub(crate) const BUTTON_HOVER: Color = Color::srgb(0.16, 0.21, 0.28);
/// A button that is on: the selected row or tab, an open window's selector.
pub(crate) const BUTTON_ON: Color = Color::srgb(0.20, 0.29, 0.40);
/// A button that cannot be used now.
pub(crate) const BUTTON_OFF: Color = Color::srgb(0.055, 0.065, 0.08);
/// A button that destroys something.
pub(crate) const BUTTON_DANGER: Color = Color::srgb(0.22, 0.10, 0.10);

// Text.

/// Running text: chat, statuses, the HUD's figures.
pub(crate) const INK: Color = Color::srgb(0.72, 0.75, 0.77);
/// A window's own words: labels, buttons, names of slots and bags.
pub(crate) const INK_BRIGHT: Color = Color::srgb(0.88, 0.89, 0.91);
/// What can wait: placeholders, the names of empty slots, unusable buttons.
pub(crate) const INK_DIM: Color = Color::srgb(0.50, 0.54, 0.58);
/// Names of things and headings: items, the target, tooltips.
pub(crate) const INK_WARM: Color = Color::srgb(0.88, 0.85, 0.73);
/// A side of a trade that has clicked Trade.
pub(crate) const AGREED: Color = Color::srgb(0.45, 0.86, 0.47);
/// An item link in chat, as the official client colours it.
pub(crate) const LINK: Color = Color::srgb_u8(190, 80, 255);

// Gems and action slots.

/// Ready to use.
pub(crate) const READY: Color = Color::srgb(0.09, 0.16, 0.22);
/// Recovering, or waiting on a cast.
pub(crate) const WAITING: Color = Color::srgb(0.20, 0.14, 0.08);

// Bars.

pub(crate) const HP: Color = Color::srgb(0.60, 0.20, 0.20);
pub(crate) const MANA: Color = Color::srgb(0.20, 0.36, 0.70);
pub(crate) const STAMINA: Color = Color::srgb(0.65, 0.49, 0.18);
pub(crate) const EXPERIENCE: Color = Color::srgb(0.30, 0.52, 0.36);
pub(crate) const CAST: Color = Color::srgb(0.30, 0.50, 0.78);

/// A cast the server has not taken yet, at its pulse's brightness (0 to 1).
pub(crate) fn awaiting(pulse: f32) -> Color {
    Color::srgb(pulse * 0.6, pulse * 0.8, pulse)
}

/// How every button, tab, row and selector lights: unusable, on, under the
/// pointer, or at rest, in that order.
pub(crate) fn button(usable: bool, on: bool, interaction: Interaction) -> Color {
    if !usable {
        BUTTON_OFF
    } else if on {
        BUTTON_ON
    } else if interaction == Interaction::None {
        BUTTON
    } else {
        BUTTON_HOVER
    }
}

/// How a gem or action slot lights: under the pointer, empty, waiting, or
/// ready, in that order.
pub(crate) fn readiness(hovered: bool, empty: bool, waiting: bool) -> Color {
    if hovered {
        BUTTON_HOVER
    } else if empty {
        WELL
    } else if waiting {
        WAITING
    } else {
        READY
    }
}

/// A considered spawn's colour, as the official client shows a level
/// difference; names not yet considered are [`INK_WARM`].
pub(crate) fn con(color: Option<ConColor>) -> Color {
    match color {
        None => INK_WARM,
        Some(ConColor::Gray) => Color::srgb(0.6, 0.6, 0.6),
        Some(ConColor::Green) => Color::srgb(0.3, 0.85, 0.3),
        Some(ConColor::LightBlue) => Color::srgb(0.45, 0.85, 1.0),
        Some(ConColor::Blue) => Color::srgb(0.35, 0.5, 1.0),
        Some(ConColor::White | ConColor::Other(_)) => Color::srgb(0.95, 0.95, 0.95),
        Some(ConColor::Yellow) => Color::srgb(1.0, 0.9, 0.2),
        Some(ConColor::Red) => Color::srgb(1.0, 0.3, 0.25),
    }
}

/// The type scale: every text in the client is one of these sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Size {
    /// Numbers and keys on icons and slots.
    Caption,
    /// Window titles, section headings, the HUD's figures.
    Small,
    /// Running text: chat, statuses, lists.
    Body,
    /// Labels and buttons.
    Label,
    /// A window's subject: the target's name, the spellbook's page.
    Heading,
    /// Buttons and statuses on a cover.
    Large,
    /// A cover's title.
    Display,
}

impl Size {
    /// The size in logical pixels.
    pub(crate) const fn px(self) -> f32 {
        match self {
            Self::Caption => 8.0,
            Self::Small => 10.0,
            Self::Body => 11.0,
            Self::Label => 12.0,
            Self::Heading => 13.0,
            Self::Large => 15.0,
            Self::Display => 20.0,
        }
    }
}

/// Where Windows keeps Arial, under the system root the environment names
/// (`SystemRoot`, else `windir`).
fn arial_file(system_root: Option<&Path>) -> Option<PathBuf> {
    Some(system_root?.join("Fonts").join("arial.ttf"))
}

/// Makes Arial the face of every text that names none of its own, as the
/// official client writes, where Windows has it. It replaces Bevy's
/// built-in face, so it runs after the text plugin is added and before any
/// text is laid out.
pub(crate) fn install_font(app: &mut App) {
    let root = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("windir"));
    let Some(file) = arial_file(root.as_deref().map(Path::new)) else {
        info!("Not on Windows: text keeps the built-in face");
        return;
    };
    match std::fs::read(&file) {
        Ok(bytes) => {
            let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
            if let Err(error) = fonts.insert(AssetId::default(), Font::from_bytes(bytes)) {
                warn!("Could not use {}: {error}", file.display());
            }
        }
        Err(error) => info!(
            "No Arial at {}: {error}; text keeps the built-in face",
            file.display()
        ),
    }
}

/// The client's font at this size.
pub(crate) fn font(size: Size) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size.px()),
        ..default()
    }
}

/// A text in this size and colour.
pub(crate) fn text(
    value: impl Into<String>,
    size: Size,
    ink: Color,
) -> (Text, TextFont, TextColor) {
    (Text::new(value), font(size), TextColor(ink))
}

/// A window's own words, in [`INK_BRIGHT`].
pub(crate) fn label(parent: &mut ChildSpawnerCommands, value: &str, size: Size) {
    parent.spawn(text(value, size, INK_BRIGHT));
}

/// A button carrying its marker, with its words; it lights by [`button`]
/// once a system does so, and starts at rest.
pub(crate) fn button_with<'a>(
    parent: &'a mut ChildSpawnerCommands,
    marker: impl Bundle,
    value: &str,
    size: Size,
) -> EntityCommands<'a> {
    let mut button = parent.spawn((
        Button,
        marker,
        Node {
            padding: UiRect::axes(px(8), px(3)),
            ..default()
        },
        BackgroundColor(BUTTON),
    ));
    button.with_child(text(value, size, INK_BRIGHT));
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_light_unusable_then_on_then_hovered() {
        assert_eq!(button(false, true, Interaction::Hovered), BUTTON_OFF);
        assert_eq!(button(true, true, Interaction::Hovered), BUTTON_ON);
        assert_eq!(button(true, false, Interaction::Pressed), BUTTON_HOVER);
        assert_eq!(button(true, false, Interaction::None), BUTTON);
    }

    #[test]
    fn arial_is_read_from_the_windows_fonts() {
        assert_eq!(
            arial_file(Some(Path::new("C:/Windows"))),
            Some(Path::new("C:/Windows").join("Fonts").join("arial.ttf"))
        );
        assert_eq!(arial_file(None), None);
    }

    #[test]
    fn the_type_scale_grows() {
        let sizes = [
            Size::Caption,
            Size::Small,
            Size::Body,
            Size::Label,
            Size::Heading,
            Size::Large,
            Size::Display,
        ];
        assert!(sizes.windows(2).all(|pair| pair[0].px() < pair[1].px()));
    }
}
