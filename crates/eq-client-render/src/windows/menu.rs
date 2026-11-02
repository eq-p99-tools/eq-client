//! A persistent corner launcher; its entries use the same window toggles as keys.
//! Artwork comes from the installed EQ main button, never bundled textures.

use bevy::prelude::*;
use eq_client_assets::sidl::Element;

use super::{Shown, Toggle, WindowId, pointer::TakesWheel, stack::SelectorLabel};
use crate::theme::{self, Size};

#[derive(Resource, Default)]
pub(crate) struct Menu {
    pub(crate) open: bool,
}

#[derive(Component)]
struct Launcher;
#[derive(Component)]
struct Fallback;
#[derive(Component)]
struct Panel;
/// The menu's panel: a script's `click window <key>` or `click actions`
/// reaches its entries while it is closed, as it reached the HUD's buttons
/// before the menu held them; the player opens the menu by hand.
#[derive(Component)]
pub(crate) struct ScriptReach;
#[derive(Component)]
struct Dismiss;
#[derive(Component)]
struct Entry(WindowId);
#[derive(Component)]
struct Mark(WindowId);

/// Keep the launcher alive across HUD rebuilds, login and character selection.
pub(super) fn install(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(
            Update,
            input
                .after(super::stack::toggle)
                .in_set(crate::Stage::Route),
        )
        .add_systems(
            Update,
            (present, artwork, scroll).in_set(crate::Stage::Present),
        );
}

/// Build a corner button and a menu whose outside-click shield prevents fallthrough.
fn spawn(mut commands: Commands) {
    commands.spawn((
        Button,
        Dismiss,
        TakesWheel,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        GlobalZIndex(850),
    ));
    commands
        .spawn((
            Panel,
            ScriptReach,
            super::KeepsPress,
            TakesWheel,
            ScrollPosition::default(),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                right: px(12),
                bottom: px(62),
                width: px(250),
                max_height: percent(72),
                overflow: Overflow::scroll_y(),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(6)),
                row_gap: px(3),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::COVER),
            BorderColor::all(theme::EDGE),
            GlobalZIndex(851),
        ))
        .with_children(entries);
    commands
        .spawn((
            Button,
            Launcher,
            crate::skinned::SkinId {
                window: WindowId::Selector,
                id: "EQButton".into(),
            },
            TakesWheel,
            crate::tooltip::Tooltip("Windows: scripts, map, options and more".into()),
            super::placed(
                WindowId::Selector,
                Node {
                    width: px(42),
                    height: px(42),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ),
            BackgroundColor(theme::BUTTON),
            GlobalZIndex(launcher_layer(true)),
        ))
        .with_child((Fallback, theme::text("EQ", Size::Label, theme::INK_BRIGHT)));
}

/// The window registry defines the menu; interaction-specific windows stay excluded.
fn entries(panel: &mut ChildSpawnerCommands) {
    panel.spawn(theme::text("Windows", Size::Label, theme::INK_DIM));
    for id in WindowId::ALL
        .into_iter()
        .filter(|id| id.describe().toggle != Toggle::Never)
    {
        panel
            .spawn((
                Button,
                super::SelectorButton(id),
                Entry(id),
                Node {
                    padding: UiRect::axes(px(8), px(5)),
                    column_gap: px(8),
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(theme::BUTTON),
            ))
            .with_children(|entry| {
                entry.spawn((
                    Mark(id),
                    Node {
                        width: px(9),
                        height: px(9),
                        border: UiRect::all(px(1)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BorderColor::all(theme::EDGE),
                    BackgroundColor(Color::NONE),
                ));
                entry.spawn((
                    SelectorLabel(id),
                    theme::text(super::stack::name(id), Size::Label, theme::INK_BRIGHT),
                ));
            });
    }
}

/// Close after choosing an entry, clicking outside, or consuming this layer's Escape.
#[allow(clippy::needless_pass_by_value)]
fn input(
    launcher: crate::clicks::Clicks<Launcher>,
    outside: crate::clicks::Clicks<Dismiss>,
    entries: crate::clicks::Clicks<Entry>,
    escape: Res<crate::escape::Escape>,
    mut menu: ResMut<Menu>,
) {
    if launcher.any() {
        menu.open = !menu.open;
    } else if outside.any()
        || entries.any()
        || *escape == crate::escape::Escape::Close(WindowId::Selector)
    {
        menu.open = false;
    }
}

/// Context windows are listed only in-world; Scripts also works before login.
fn available(id: WindowId, in_world: bool, shown: &Shown) -> bool {
    (in_world || id == WindowId::Scripts)
        && (id.describe().toggle != Toggle::Hides || shown.is_open(id))
}

/// Below movable HUD windows in-world; above only the pre-login backdrop otherwise.
const fn launcher_layer(in_world: bool) -> i32 {
    if in_world {
        super::Layer::Hud.base() - 1
    } else {
        super::Layer::Screen.base()
    }
}

const fn opacity(interaction: Interaction) -> f32 {
    if matches!(interaction, Interaction::None) {
        0.65
    } else {
        0.85
    }
}

type PopupNodes<'w, 's> =
    Query<'w, 's, &'static mut Node, (Or<(With<Panel>, With<Dismiss>)>, Without<Entry>)>;

#[allow(clippy::needless_pass_by_value)]
fn present(
    menu: Res<Menu>,
    shown: Res<Shown>,
    (front, online): (Res<crate::login::FrontEnd>, Res<crate::online::OnlineState>),
    mut panels: PopupNodes,
    mut entries: Query<(&Entry, &mut Node)>,
    mut marks: Query<(&Mark, &mut BackgroundColor)>,
    mut launchers: Query<&mut GlobalZIndex, With<Launcher>>,
) {
    for mut node in &mut panels {
        node.display = if menu.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    let in_world = front.screen(&online) == crate::login::Screen::World;
    for mut layer in &mut launchers {
        layer.0 = launcher_layer(in_world);
    }
    for (Entry(id), mut node) in &mut entries {
        node.display = if available(*id, in_world, &shown) {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (Mark(id), mut color) in &mut marks {
        color.0 = if shown.displayed(*id) {
            theme::FOCUS
        } else {
            Color::NONE
        };
    }
}

/// Resolve button states through the existing skin cache, including custom skins.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn artwork(
    mut commands: Commands,
    menu: Res<Menu>,
    settings: Res<crate::ViewerSettings>,
    skin: Res<crate::skin::UiSkin>,
    mut screens: ResMut<crate::skinned::Screens>,
    mut art: crate::sheets::Art,
    launchers: Query<(Entity, Ref<Interaction>), With<Launcher>>,
    mut fallbacks: Query<(&mut Visibility, &mut TextColor), With<Fallback>>,
) {
    if !menu.is_changed()
        && !settings.is_changed()
        && !skin.is_changed()
        && launchers
            .iter()
            .all(|(_, interaction)| !interaction.is_changed())
    {
        return;
    }
    let button = settings.0.eq_directory.as_deref().and_then(|directory| {
        screens
            .screen(directory, &skin.0, ("EQUI_EQMainWnd.xml", "EQMainWnd"))?
            .pieces
            .iter()
            .find_map(|(_, element)| match element {
                Element::Button(button) if button.id.as_deref() == Some("EQButton") => Some(button),
                _ => None,
            })
    });
    for (entity, interaction) in &launchers {
        let image = button.and_then(|button| {
            let look = &button.look;
            let down = menu.open || *interaction == Interaction::Pressed;
            let piece = match (down, *interaction != Interaction::None) {
                (true, true) => look.pressed_flyby.as_ref().or(look.pressed.as_ref()),
                (true, false) => look.pressed.as_ref(),
                (false, true) => look.flyby.as_ref(),
                (false, false) => None,
            }
            .or(look.normal.as_ref());
            piece.and_then(|piece| art.visible_cut(piece))
        });
        let has_image = image.is_some();
        let alpha = opacity(*interaction);
        if let Some(mut image) = image {
            image.color.set_alpha(alpha);
            commands
                .entity(entity)
                .insert((image, BackgroundColor(Color::NONE)));
        } else {
            commands.entity(entity).remove::<ImageNode>();
            commands.entity(entity).insert(BackgroundColor(
                theme::button(true, menu.open, *interaction).with_alpha(alpha),
            ));
        }
        for (mut visible, mut color) in &mut fallbacks {
            color.0.set_alpha(alpha);
            *visible = if has_image {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn scroll(
    wheel: Res<super::pointer::Wheel>,
    mut panels: Query<(&ComputedNode, &mut ScrollPosition), With<Panel>>,
) {
    if let Some((node, mut position)) = wheel.surface.and_then(|entity| panels.get_mut(entity).ok())
    {
        super::scroll_by(&mut position, node, wheel.pixels);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_stays_translucent_below_movable_windows() {
        for layer in [
            super::super::Layer::Hud,
            super::super::Layer::Floating,
            super::super::Layer::Tool,
        ] {
            assert!(launcher_layer(true) < layer.base());
        }
        assert!(launcher_layer(false) < super::super::Layer::Tool.base());
        for state in [
            Interaction::None,
            Interaction::Hovered,
            Interaction::Pressed,
        ] {
            assert!(opacity(state) > 0.0 && opacity(state) < 1.0);
        }
    }

    #[test]
    fn scripts_start_closed_and_remain_available_before_login() {
        let shown = Shown::default();
        assert!(!shown.displayed(WindowId::Scripts));
        assert!(available(WindowId::Scripts, false, &shown));
        assert!(!available(WindowId::Map, false, &shown));
        assert!(available(WindowId::Map, true, &shown));
        assert!(!available(WindowId::PetInfo, true, &shown));
    }

    #[test]
    fn menu_opens_scripts_through_the_shared_toggle_and_dismisses() {
        let mut app = crate::testing::app();
        app.add_systems(Startup, spawn)
            .add_systems(Update, (super::super::stack::toggle, input).chain());
        app.update();
        let launcher = app
            .world_mut()
            .query_filtered::<Entity, With<Launcher>>()
            .single(app.world())
            .unwrap();
        let entry = app
            .world_mut()
            .query::<(Entity, &Entry)>()
            .iter(app.world())
            .find_map(|(entity, Entry(id))| (*id == WindowId::Scripts).then_some(entity))
            .unwrap();
        crate::clicks::testing::click(&mut app, launcher);
        assert!(app.world().resource::<Menu>().open);
        crate::clicks::testing::click(&mut app, entry);
        assert!(!app.world().resource::<Menu>().open);
        assert!(app.world().resource::<Shown>().displayed(WindowId::Scripts));
        crate::clicks::testing::click(&mut app, launcher);
        *app.world_mut().resource_mut::<crate::escape::Escape>() =
            crate::escape::Escape::Close(WindowId::Selector);
        app.update();
        assert!(!app.world().resource::<Menu>().open);
        assert!(app.world().resource::<Shown>().displayed(WindowId::Scripts));
        *app.world_mut().resource_mut::<crate::escape::Escape>() = crate::escape::Escape::Unused;
        crate::clicks::testing::click(&mut app, launcher);
        let outside = app
            .world_mut()
            .query_filtered::<Entity, With<Dismiss>>()
            .single(app.world())
            .unwrap();
        crate::clicks::testing::click(&mut app, outside);
        assert!(!app.world().resource::<Menu>().open);
        assert!(app.world().resource::<Shown>().displayed(WindowId::Scripts));
    }
}
