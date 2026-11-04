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
        Name::new("EQ menu outside-click shield"),
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
            Name::new("EQ menu panel"),
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

    /// Minimal laid-out controls using the real driver, release click rule,
    /// menu handler, shared window toggle and presentation in production order.
    fn scripted_menu() -> (App, Entity, Entity) {
        use crate::script::{ClickTarget, Script, Step};
        let mut app = crate::testing::app();
        app.insert_resource(Time::<Real>::new(std::time::Instant::now()))
            .init_resource::<crate::windows::Moves>()
            .init_resource::<crate::script::RunStatus>()
            .init_resource::<crate::script::RunControl>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_message::<bevy::window::WindowFocused>()
            .add_message::<AppExit>()
            .insert_resource(Script::new(vec![Step::Click(ClickTarget::Toggle(
                "inventory",
            ))]))
            .add_systems(PreUpdate, crate::script::drive.before(crate::clicks::track))
            .add_systems(
                Update,
                (
                    super::super::stack::toggle,
                    input,
                    present,
                    crate::script::observe,
                )
                    .chain(),
            );
        {
            let mut state = app.world_mut().resource_mut::<crate::online::OnlineState>();
            state.enabled = true;
            crate::online::testing::admit(&mut state, 1, crate::online::testing::player(7));
        }
        let geometry = |at: Vec2, side: f32, stack| {
            (
                InheritedVisibility::VISIBLE,
                ComputedNode {
                    size: Vec2::splat(side),
                    ..default()
                },
                UiGlobalTransform::from_xy(at.x, at.y),
                bevy::ui::ComputedStackIndex(stack),
            )
        };
        let launcher = app
            .world_mut()
            .spawn((
                Button,
                Launcher,
                GlobalZIndex(0),
                crate::skinned::SkinId {
                    window: WindowId::Selector,
                    id: "EQButton".into(),
                },
                geometry(Vec2::new(1200.0, 680.0), 40.0, 1),
            ))
            .id();
        let panel = app
            .world_mut()
            .spawn((
                Panel,
                Node {
                    display: Display::None,
                    ..default()
                },
                geometry(Vec2::new(400.0, 300.0), 200.0, 10),
            ))
            .id();
        for (id, at) in [
            (WindowId::Inventory, Vec2::new(400.0, 300.0)),
            (WindowId::Spellbook, Vec2::new(450.0, 300.0)),
            (WindowId::Options, Vec2::new(400.0, 350.0)),
        ] {
            app.world_mut().spawn((
                Button,
                Entry(id),
                super::super::SelectorButton(id),
                ChildOf(panel),
                geometry(at, 40.0, 11),
            ));
        }
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_physical_cursor_position(Some(bevy::math::DVec2::new(100.0, 100.0)));
        (app, window, launcher)
    }

    fn menu_frame(app: &mut App, ms: u64) {
        // The input plugin clears edge flags before each frame in the real app.
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        let mut time = app.world_mut().resource_mut::<Time<Real>>();
        let startup = time.startup();
        time.update_with_instant(startup + std::time::Duration::from_millis(ms));
        app.update();
    }

    fn hand_off_pointer(app: &mut App, window: Entity) {
        menu_frame(app, 0);
        // A focused Window flag alone is not an attended focus report.
        // Let the actual driver observe the matching backend event next frame.
        assert_eq!(
            app.world().resource::<crate::script::RunStatus>().phase(),
            crate::script::Phase::WaitingForFocus
        );
        assert!(
            app.world_mut()
                .write_message(bevy::window::WindowFocused {
                    window,
                    focused: true,
                })
                .is_some()
        );
        menu_frame(app, 1);
        menu_frame(app, 42_000);
        assert_eq!(
            app.world().resource::<crate::script::RunStatus>().phase(),
            crate::script::Phase::WaitingForPointer
        );
        assert!(!app.world().resource::<Menu>().open);
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_physical_cursor_position(None);
        for ms in [42_001, 42_251, 42_252] {
            menu_frame(app, ms);
        }
    }

    #[test]
    fn consecutive_scripted_window_clicks_reopen_the_menu_after_the_previous_release() {
        for source in [
            "click window spellbook\nclick window options\n",
            "hold_click 1 window spellbook\nclick window options\n",
        ] {
            let (mut app, window, _) = scripted_menu();
            let program = crate::script::Program::parse(source, std::path::Path::new(".")).unwrap();
            app.insert_resource(crate::script::Script::from_program(program));
            app.world_mut()
                .resource_mut::<Shown>()
                .open(WindowId::Spellbook);
            hand_off_pointer(&mut app, window);
            for ms in (42_253..=44_253).step_by(16) {
                menu_frame(&mut app, ms);
            }
            let status = app.world().resource::<crate::script::RunStatus>();
            assert_eq!(
                status.phase(),
                crate::script::Phase::Completed,
                "{source}: {:?}",
                status.events().back()
            );
            let shown = app.world().resource::<Shown>();
            assert!(!shown.displayed(WindowId::Spellbook));
            assert!(shown.displayed(WindowId::Options));
            assert!(!app.world().resource::<Menu>().open);
            assert!(
                !app.world()
                    .resource::<ButtonInput<MouseButton>>()
                    .pressed(MouseButton::Left)
            );
        }
    }

    #[test]
    fn a_scripted_menu_entry_survives_pointer_wait_and_observes_release_before_delivery() {
        let (mut app, window, _) = scripted_menu();
        hand_off_pointer(&mut app, window);
        // The real handler opens it in Update, after the driver's PreUpdate
        // release. The old queued timestamp failed the run earlier that frame
        // even though this menu is now visible.
        assert!(app.world().resource::<Menu>().open);
        assert!(
            app.world()
                .resource::<crate::script::RunStatus>()
                .phase()
                .active()
        );
        assert!(
            !app.world()
                .resource::<Shown>()
                .displayed(WindowId::Inventory)
        );
        for ms in [42_253, 42_254, 42_504, 42_505] {
            menu_frame(&mut app, ms);
        }
        // Visibility alone does not establish the requested action: the
        // actual shared toggle must receive the entry's own release.
        assert!(
            app.world()
                .resource::<Shown>()
                .displayed(WindowId::Inventory)
        );
        assert!(!app.world().resource::<Menu>().open);
        assert_ne!(
            app.world().resource::<crate::script::RunStatus>().phase(),
            crate::script::Phase::Failed
        );
    }

    #[test]
    fn an_unhandled_menu_opener_still_fails_one_second_after_its_release() {
        let (mut app, window, launcher) = scripted_menu();
        // It remains a reachable matching button, but no menu handler owns
        // it. Releasing input must not count as opening the menu.
        app.world_mut().entity_mut(launcher).remove::<Launcher>();
        hand_off_pointer(&mut app, window);
        for ms in [42_253, 43_251] {
            menu_frame(&mut app, ms);
            assert!(
                app.world()
                    .resource::<crate::script::RunStatus>()
                    .phase()
                    .active()
            );
            assert!(!app.world().resource::<Menu>().open);
        }
        menu_frame(&mut app, 43_252);
        let status = app.world().resource::<crate::script::RunStatus>();
        assert_eq!(status.phase(), crate::script::Phase::Failed);
        assert_eq!(
            status.events().back().unwrap().message,
            "the EQ menu did not open for the click"
        );
        assert!(
            !app.world()
                .resource::<Shown>()
                .displayed(WindowId::Inventory)
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
    }

    #[test]
    fn a_covered_menu_opener_still_refuses_the_scripted_click() {
        let (mut app, window, _) = scripted_menu();
        app.world_mut().spawn((
            Node::default(),
            bevy::ui::FocusPolicy::Block,
            InheritedVisibility::VISIBLE,
            ComputedNode {
                size: Vec2::splat(60.0),
                ..default()
            },
            UiGlobalTransform::from_xy(1200.0, 680.0),
            bevy::ui::ComputedStackIndex(20),
        ));
        hand_off_pointer(&mut app, window);
        let status = app.world().resource::<crate::script::RunStatus>();
        assert_eq!(status.phase(), crate::script::Phase::Failed);
        assert_eq!(
            status.events().back().unwrap().message,
            "the click target is covered"
        );
        assert!(!app.world().resource::<Menu>().open);
        assert!(
            !app.world()
                .resource::<Shown>()
                .displayed(WindowId::Inventory)
        );
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
