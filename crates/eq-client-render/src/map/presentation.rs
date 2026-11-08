//! Per-character map appearance, independent of installed skin and game protocol.
use crate::{
    profile_files::{Profile, Store},
    theme,
    windows::{self, WindowId},
};
use bevy::prelude::*;
use std::time::Instant;

const SHARED: &str = "map-view.txt";
/// Neutral dark backing for a map left over the world, faded by the BG control.
pub(super) const OVERLAY: Color = Color::srgb_u8(16, 20, 26);

/// Lift map-file ink for the dark overlay while retaining each line's hue.
pub(super) fn ink(color: [u8; 3], plain: bool) -> [u8; 3] {
    if plain {
        color.map(|channel| 128 + channel / 2)
    } else {
        color
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Options {
    opacity: u8,
    plain: bool,
    locked: bool,
    outlines: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            opacity: 100,
            plain: false,
            locked: false,
            outlines: true,
        }
    }
}

impl Options {
    fn read(text: &str) -> Self {
        let mut options = Self::default();
        for (key, value) in text.lines().filter_map(|line| line.split_once('=')) {
            match (key.trim(), value.trim()) {
                ("opacity", value) => {
                    if let Ok(value) = value.parse::<u8>() {
                        options.opacity = value.min(100);
                    }
                }
                ("plain", value) => {
                    if let Ok(value) = value.parse() {
                        options.plain = value;
                    }
                }
                ("locked", value) => {
                    if let Ok(value) = value.parse() {
                        options.locked = value;
                    }
                }
                ("outlines", value) => {
                    if let Ok(value) = value.parse() {
                        options.outlines = value;
                    }
                }
                _ => (),
            }
        }
        options
    }

    fn text(self) -> String {
        format!(
            "opacity={}\nplain={}\nlocked={}\noutlines={}\n",
            self.opacity, self.plain, self.locked, self.outlines
        )
    }
}

#[derive(Resource, Default)]
pub(crate) struct Presentation {
    options: Options,
    dirty: bool,
    store: Store,
}

impl Presentation {
    pub(super) fn plain(&self) -> bool {
        self.options.plain
    }

    /// Tracking normally outlines matching scene models. This option only
    /// controls the scene presentation.
    pub(crate) fn outlines(&self) -> bool {
        self.options.outlines
    }
}

/// A decorative background only: text, markers and map lines stay fully legible.
#[derive(Component)]
pub(crate) enum Backdrop {
    /// Fade a fill without fading the map geometry rendered on the same node.
    Fill(Color),
    /// Fade an installed decorative texture, such as parchment.
    Artwork,
}

#[derive(Component, Clone, Copy)]
pub(crate) enum Action {
    Less,
    More,
    Style,
    Lock,
    Outlines,
}

#[derive(Component)]
pub(crate) enum Label {
    Opacity,
    Style,
    Lock,
    Outlines,
}

/// Save map presentation beside the existing per-character window placements.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn persist(
    settings: Res<crate::ViewerSettings>,
    profile: Res<Profile>,
    mut state: ResMut<Presentation>,
) {
    let state = state.bypass_change_detection();
    state.store.retry(Instant::now());
    if state.store.loaded && state.store.profile == profile.key() {
        if state.dirty {
            save(state);
        }
        return;
    }
    if state.store.loaded {
        save(state);
    } else {
        state.store.open(settings.0.settings_directory.as_ref());
    }
    state.store.profile = profile.key();
    let text = state
        .store
        .read(&file_name(&state.store))
        .or_else(|| state.store.read(SHARED))
        .unwrap_or_default();
    state.options = Options::read(&text);
    state.store.loaded_text(state.options.text());
    state.dirty = false;
}

fn file_name(store: &Store) -> String {
    crate::profile_files::name("map-view", SHARED, store.profile.as_ref())
}

fn save(state: &mut Presentation) {
    state.store.save(
        file_name(&state.store),
        state.options.text(),
        Instant::now(),
    );
    state.dirty = false;
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn actions(clicks: crate::clicks::Clicks<Action>, mut state: ResMut<Presentation>) {
    for action in clicks.iter() {
        match action {
            Action::Less => state.options.opacity = state.options.opacity.saturating_sub(10),
            Action::More => {
                state.options.opacity = state.options.opacity.saturating_add(10).min(100);
            }
            Action::Style => state.options.plain = !state.options.plain,
            Action::Lock => state.options.locked = !state.options.locked,
            Action::Outlines => state.options.outlines = !state.options.outlines,
        }
        state.dirty = true;
    }
}

/// Reapply after skin dressing; opacity never compounds across frames.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn update(
    mut commands: Commands,
    state: Res<Presentation>,
    mut backdrops: Query<(&Backdrop, Option<&mut ImageNode>, &mut BackgroundColor)>,
    frames: Query<(Entity, &WindowId, Has<windows::PositionLocked>)>,
    mut labels: Query<(&Label, &mut Text)>,
) {
    let alpha = f32::from(state.options.opacity) / 100.0;
    for (original, image, mut background) in &mut backdrops {
        let fill = match original {
            Backdrop::Fill(color) => color.with_alpha(color.alpha() * alpha),
            Backdrop::Artwork => {
                if let Some(mut image) = image {
                    image.color.set_alpha(alpha);
                }
                Color::NONE
            }
        };
        background.set_if_neq(BackgroundColor(fill));
    }
    for (entity, id, locked) in &frames {
        if *id == WindowId::Map && locked != state.options.locked {
            if state.options.locked {
                commands.entity(entity).insert(windows::PositionLocked);
            } else {
                commands.entity(entity).remove::<windows::PositionLocked>();
            }
        }
    }
    for (label, mut text) in &mut labels {
        let value = match label {
            Label::Opacity => format!("BG {}%", state.options.opacity),
            Label::Style => if state.options.plain { "Plain" } else { "Skin" }.into(),
            Label::Lock => if state.options.locked {
                "Unlock"
            } else {
                "Lock"
            }
            .into(),
            Label::Outlines => if state.options.outlines {
                "3D On"
            } else {
                "3D Off"
            }
            .into(),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}

/// The same compact controls appear on native and installed-skin maps.
pub(super) fn chrome(parent: &mut ChildSpawnerCommands) {
    use super::controls::button;
    use crate::tooltip::Tooltip;
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(6),
                right: px(6),
                top: px(42),
                column_gap: px(4),
                align_items: AlignItems::Center,
                ..default()
            },
            windows::KeepsPress,
            ZIndex(10),
        ))
        .with_children(|row| {
            row.spawn((
                Label::Opacity,
                theme::text("BG 100%", theme::Size::Small, theme::INK_BRIGHT),
            ));
            for (text, action, tip) in [
                (
                    "−",
                    Action::Less,
                    "Reduce background opacity. Map lines and markers stay readable.",
                ),
                ("+", Action::More, "Increase background opacity."),
                (
                    "Skin",
                    Action::Style,
                    "Switch between the installed skin and a plain overlay.",
                ),
                (
                    "Lock",
                    Action::Lock,
                    "Lock window position. Map panning and zooming still work.",
                ),
            ] {
                let mut control = row.spawn((action, button(text), Tooltip(tip.into())));
                match action {
                    Action::Style => {
                        control.insert(Label::Style);
                    }
                    Action::Lock => {
                        control.insert(Label::Lock);
                    }
                    _ => (),
                }
            }
            row.spawn((
                super::MapButton::Nearby,
                button("Nearby"),
                Tooltip("Center a readable local view on your character.".into()),
            ));
            row.spawn((
                super::MapButton::Reset,
                button("Fit all"),
                Tooltip(
                    "Fit every known position, including distant invisible server helpers.".into(),
                ),
            ));
            row.spawn((super::landmarks::Action::Toggle, button("Landmarks")));
            row.spawn((
                Label::Outlines,
                Action::Outlines,
                button("3D On"),
                Tooltip(
                    "Show an outline of tracked names through scenery in the main view.".into(),
                ),
            ));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_round_trips_and_rejects_invalid_values() {
        let options = Options {
            opacity: 30,
            plain: true,
            locked: true,
            outlines: false,
        };
        assert_eq!(Options::read(&options.text()), options);
        assert_eq!(
            Options::read("opacity=-20\nplain=invalid"),
            Options::default()
        );
        assert_eq!(Options::read("opacity=255").opacity, 100);
    }

    #[test]
    fn opacity_keeps_map_content_readable_and_does_not_compound() {
        let mut app = App::new();
        app.init_resource::<Presentation>()
            .add_systems(Update, update);
        app.world_mut().resource_mut::<Presentation>().options = Options {
            opacity: 30,
            locked: true,
            ..default()
        };
        let root = app.world_mut().spawn(WindowId::Map).id();
        let back = app
            .world_mut()
            .spawn((
                Backdrop::Artwork,
                ImageNode::default(),
                BackgroundColor(Color::WHITE),
            ))
            .id();
        let line = app
            .world_mut()
            .spawn((
                ImageNode::default(),
                Backdrop::Fill(Color::WHITE),
                BackgroundColor(Color::WHITE),
            ))
            .id();
        app.update();
        app.update();
        assert!(app.world().get::<windows::PositionLocked>(root).is_some());
        assert!(
            (app.world().get::<ImageNode>(back).unwrap().color.alpha() - 0.3).abs() < f32::EPSILON
        );
        assert_eq!(
            app.world().get::<ImageNode>(line).unwrap().color.alpha(),
            1.0
        );
        app.world_mut()
            .resource_mut::<Presentation>()
            .options
            .locked = false;
        app.update();
        assert!(app.world().get::<windows::PositionLocked>(root).is_none());
    }
}
