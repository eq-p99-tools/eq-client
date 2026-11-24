//! The same run-length dropdown above the Editor and Execution views.

use bevy::{input_focus::InputFocus, prelude::*, ui::FocusPolicy};

use super::{Tab, Workbench, view};
use crate::{
    script::RunLength,
    theme,
    windows::{Shown, WindowId},
};

#[derive(Component)]
struct Selector {
    menu: Entity,
    tab: Tab,
}

#[derive(Component)]
struct Menu;

#[derive(Resource, Default)]
struct Open(Option<Entity>);

pub(super) fn install(app: &mut App) {
    app.init_resource::<Open>().add_systems(
        Update,
        input
            .after(crate::chat::input)
            .after(super::claim_keyboard)
            .before(crate::escape::route)
            .in_set(crate::Stage::Typing),
    );
}

pub(super) fn selector(commands: &mut Commands, tab: Tab, length: RunLength) -> Entity {
    let text = view::label(commands, length.label(), 13.0, theme::INK_BRIGHT);
    commands.entity(text).insert(view::LengthLabel);
    // The bundled font lacks arrow glyphs. Two native borders draw the chevron.
    let arrow = commands
        .spawn((
            Node {
                width: px(7),
                height: px(7),
                border: UiRect {
                    right: px(1),
                    bottom: px(1),
                    ..default()
                },
                ..default()
            },
            BorderColor::all(theme::INK_BRIGHT),
            UiTransform::from_rotation(Rot2::degrees(45.0)),
        ))
        .id();
    let choices: Vec<_> = length
        .choices()
        .into_iter()
        .map(|length| {
            let row = view::button(commands, &length.label(), view::Action::SetLength(length));
            commands.entity(row).insert((
                crate::clicks::OnPress,
                Node {
                    width: percent(100),
                    padding: UiRect::axes(px(9), px(5)),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            row
        })
        .collect();
    let menu = commands
        .spawn((
            Menu,
            Button,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: percent(100),
                right: px(0),
                min_width: percent(100),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                ..default()
            },
            ZIndex(1),
            FocusPolicy::Block,
            BackgroundColor(theme::BUTTON),
            BorderColor::all(theme::EDGE),
        ))
        .add_children(&choices)
        .id();
    commands
        .spawn((
            Button,
            view::Action::Length,
            Selector { menu, tab },
            crate::clicks::OnPress,
            crate::tooltip::Tooltip("Choose how long the next run may go.".to_owned()),
            Node {
                padding: UiRect::axes(px(9), px(5)),
                border: UiRect::all(px(1)),
                column_gap: px(7),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme::BUTTON),
            BorderColor::all(theme::EDGE),
        ))
        .add_children(&[text, arrow, menu])
        .id()
}

/// Opening never changes the length; only choosing a row does. One dropdown
/// is open at a time, and Escape dismisses it before it can close the window.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn input(
    selectors: crate::clicks::Clicks<Selector>,
    actions: crate::clicks::Clicks<view::Action>,
    mut open: ResMut<Open>,
    state: Res<Workbench>,
    status: Res<crate::script::RunStatus>,
    shown: Res<Shown>,
    script: Option<Res<crate::script::Script>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut typing: ResMut<crate::keys::Typing>,
    mut focus: ResMut<InputFocus>,
    mut menus: Query<(Entity, &mut Node), With<Menu>>,
    buttons: Query<(&view::Action, &Interaction)>,
    all_selectors: Query<&Selector>,
) {
    let visible = shown.displayed(WindowId::Scripts);
    let usable = visible && !state.busy() && script.is_none() && !status.phase().active();
    let escape = open.0.is_some()
        && keys.just_pressed(KeyCode::Escape)
        && windows.single().is_ok_and(|window| window.focused);
    if escape {
        typing.escape_consumed = true;
    }
    let outside = mouse.just_pressed(MouseButton::Left)
        && !buttons.iter().any(|(action, interaction)| {
            matches!(action, view::Action::Length | view::Action::SetLength(_))
                && *interaction == Interaction::Pressed
        });
    let selected = actions
        .iter()
        .any(|action| matches!(action, view::Action::SetLength(_)));
    let same_tab = open.0.is_none_or(|menu| {
        all_selectors
            .iter()
            .any(|selector| selector.menu == menu && selector.tab == state.tab)
    });
    if !usable || escape || outside || selected || !same_tab {
        open.0 = None;
    } else if let Some(selector) = selectors.iter().next() {
        open.0 = (open.0 != Some(selector.menu)).then_some(selector.menu);
        focus.clear();
    }
    for (entity, mut node) in &mut menus {
        node.display = if open.0 == Some(entity) {
            Display::Flex
        } else {
            Display::None
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        clicks::testing::{click, press, release},
        script::{RunControl, RunStatus, Script},
    };

    fn app(length: RunLength) -> App {
        let mut app = crate::testing::app();
        app.insert_resource(Workbench {
            run_length: length,
            ..default()
        })
        .init_resource::<InputFocus>()
        .init_resource::<Assets<Font>>()
        .init_resource::<RunStatus>()
        .init_resource::<RunControl>()
        .add_systems(Startup, view::spawn)
        .add_systems(
            Update,
            (super::super::controls, view::length_label)
                .chain()
                .after(input),
        );
        install(&mut app);
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Scripts);
        app.update();
        app
    }

    fn selector(app: &mut App, tab: Tab) -> (Entity, Entity) {
        app.world_mut()
            .query::<(Entity, &Selector)>()
            .iter(app.world())
            .find_map(|(entity, selector)| (selector.tab == tab).then_some((entity, selector.menu)))
            .unwrap()
    }

    fn choice(app: &mut App, menu: Entity, wanted: RunLength) -> Entity {
        app.world_mut()
            .query::<(Entity, &ChildOf, &view::Action)>()
            .iter(app.world())
            .find_map(|(entity, parent, action)| {
                (parent.parent() == menu
                    && matches!(action, view::Action::SetLength(length) if *length == wanted))
                .then_some(entity)
            })
            .unwrap()
    }

    fn open(app: &App, menu: Entity) -> bool {
        app.world().get::<Node>(menu).unwrap().display != Display::None
    }

    #[test]
    fn both_dropdowns_offer_every_length_and_keep_a_custom_cli_choice() {
        for initial in [RunLength::Default, RunLength::Minutes(45)] {
            let mut app = app(initial);
            let mut wanted = vec![
                RunLength::Default,
                RunLength::Minutes(30),
                RunLength::Minutes(60),
                RunLength::Minutes(120),
                RunLength::Minutes(240),
                RunLength::Minutes(480),
                RunLength::Minutes(1440),
                RunLength::Unlimited,
            ];
            if initial != RunLength::Default {
                wanted.insert(2, initial);
            }
            for tab in [Tab::Editor, Tab::Execution] {
                app.world_mut().resource_mut::<Workbench>().tab = tab;
                let (button, menu) = selector(&mut app, tab);
                assert!(!open(&app, menu));
                click(&mut app, button);
                assert!(open(&app, menu));
                assert_eq!(app.world().resource::<Workbench>().run_length, initial);
                let choices: Vec<_> = app
                    .world()
                    .get::<Children>(menu)
                    .unwrap()
                    .iter()
                    .map(|row| match app.world().get::<view::Action>(row).unwrap() {
                        view::Action::SetLength(length) => *length,
                        _ => panic!("not a length choice"),
                    })
                    .collect();
                assert_eq!(choices, wanted);
                // The next tab closes the previous menu; opening a menu never cycles the value.
                click(&mut app, button);
            }
        }
    }

    #[test]
    fn choosing_a_length_updates_both_labels_and_the_next_run() {
        let mut app = app(RunLength::Default);
        for tab in [Tab::Editor, Tab::Execution] {
            for length in RunLength::Default.choices() {
                app.world_mut().resource_mut::<Workbench>().tab = tab;
                let (button, menu) = selector(&mut app, tab);
                click(&mut app, button);
                let row = choice(&mut app, menu, length);
                click(&mut app, row);
                assert!(!open(&app, menu));
                assert_eq!(app.world().resource::<Workbench>().run_length, length);
                let labels: Vec<_> = app
                    .world_mut()
                    .query_filtered::<&Text, With<view::LengthLabel>>()
                    .iter(app.world())
                    .map(|text| text.0.clone())
                    .collect();
                assert_eq!(labels, [length.label(), length.label()]);
            }
        }
        let run = app
            .world_mut()
            .query::<(Entity, &view::Action)>()
            .iter(app.world())
            .find_map(|(entity, action)| matches!(action, view::Action::Run).then_some(entity))
            .unwrap();
        click(&mut app, run);
        assert_eq!(
            app.world().resource::<Script>().status().length(),
            RunLength::Unlimited
        );
    }

    #[test]
    fn a_new_session_resets_the_choice_and_uses_its_own_cli_length() {
        let mut previous = app(RunLength::Default);
        let (button, menu) = selector(&mut previous, Tab::Editor);
        click(&mut previous, button);
        let row = choice(&mut previous, menu, RunLength::Unlimited);
        click(&mut previous, row);
        assert_eq!(
            previous.world().resource::<Workbench>().run_length,
            RunLength::Unlimited
        );
        drop(previous);

        for configured in [RunLength::Default, RunLength::Minutes(45)] {
            let mut next = crate::testing::app();
            next.world_mut()
                .resource_mut::<crate::ViewerSettings>()
                .0
                .script_run_length = configured;
            // Each new launch goes through production state construction;
            // the prior session's UI choice is not a stored preference.
            super::super::seed(&mut next);
            assert_eq!(next.world().resource::<Workbench>().run_length, configured);
        }
    }

    #[test]
    fn outside_escape_hidden_and_tab_changes_dismiss_without_changing_length() {
        let mut app = app(RunLength::Minutes(45));
        app.init_resource::<crate::escape::Escape>().add_systems(
            Update,
            (crate::escape::route, crate::skinned::close)
                .chain()
                .after(input),
        );
        let (button, menu) = selector(&mut app, Tab::Editor);
        click(&mut app, button);
        press(&mut app, None);
        assert!(!open(&app, menu));
        release(&mut app, None);
        click(&mut app, button);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(!open(&app, menu));
        assert!(
            app.world()
                .resource::<crate::keys::Typing>()
                .escape_consumed
        );
        assert!(app.world().resource::<Shown>().displayed(WindowId::Scripts));
        assert_eq!(
            *app.world().resource::<crate::escape::Escape>(),
            crate::escape::Escape::Unused
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        click(&mut app, button);
        app.world_mut().resource_mut::<Workbench>().tab = Tab::Execution;
        app.update();
        assert!(!open(&app, menu));
        app.world_mut().resource_mut::<Workbench>().tab = Tab::Editor;
        click(&mut app, button);
        app.world_mut()
            .resource_mut::<Shown>()
            .close(WindowId::Scripts);
        app.update();
        assert!(!open(&app, menu));
        assert_eq!(
            app.world().resource::<Workbench>().run_length,
            RunLength::Minutes(45)
        );
    }

    #[test]
    fn active_runs_and_file_work_refuse_length_changes() {
        let mut app = app(RunLength::Default);
        let (button, menu) = selector(&mut app, Tab::Editor);
        click(&mut app, button);
        let program =
            crate::script::Program::parse("wait(1000)\n", std::path::Path::new(".")).unwrap();
        let script = Script::for_viewer(
            program,
            &app.world().resource::<crate::ViewerSettings>().0,
            false,
        )
        .run_length(RunLength::Minutes(45));
        app.insert_resource(script);
        let row = choice(&mut app, menu, RunLength::Unlimited);
        click(&mut app, row);
        assert!(!open(&app, menu));
        assert_eq!(
            app.world().resource::<Workbench>().run_length,
            RunLength::Default
        );
        assert_eq!(
            app.world().resource::<Script>().status().length(),
            RunLength::Minutes(45)
        );
        click(&mut app, button);
        assert!(!open(&app, menu));
        app.world_mut().remove_resource::<Script>();
        app.world_mut()
            .resource_mut::<Workbench>()
            .job
            .keep_pending(0);
        click(&mut app, button);
        click(&mut app, row);
        assert!(!open(&app, menu));
        assert_eq!(
            app.world().resource::<Workbench>().run_length,
            RunLength::Default
        );
    }
}
