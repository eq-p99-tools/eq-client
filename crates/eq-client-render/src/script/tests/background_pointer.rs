//! Real Bevy focus and production input/drive/click order, with synthetic
//! backend messages and computed bounds. No renderer, OS window or live input.
use super::*;
use bevy::{
    app::{HierarchyPropagatePlugin, PropagateSet},
    camera::RenderTarget,
    input::{ButtonState, mouse::MouseButtonInput, touch::Touches},
    ui::{UiStack, UiSystems, ui_focus_system, update::propagate_ui_target_cameras},
};

#[derive(Default, Resource)]
struct Recorded {
    presses: Vec<Entity>,
    left_clicks: Vec<Entity>,
    right_releases: Vec<Entity>,
}

#[allow(clippy::needless_pass_by_value)]
fn record(
    controls: Query<(Entity, Ref<Interaction>)>,
    mouse: Res<ButtonInput<MouseButton>>,
    left: Res<crate::clicks::LeftButton>,
    mut seen: ResMut<Recorded>,
) {
    for (entity, interaction) in &controls {
        if interaction.is_changed() && *interaction == Interaction::Pressed {
            seen.presses.push(entity);
        }
        if left.clicked(entity) {
            seen.left_clicks.push(entity);
        }
        if mouse.just_released(MouseButton::Right) && *interaction == Interaction::Hovered {
            seen.right_releases.push(entity);
        }
    }
}

fn fixture(steps: Vec<Step>) -> (App, Entity, Entity, Entity) {
    let (mut app, window) = background(steps);
    app.init_resource::<Touches>()
        .init_resource::<UiStack>()
        .init_resource::<UiScale>()
        .init_resource::<Recorded>()
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
            PostUpdate,
        ))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
            PostUpdate,
        ))
        .add_systems(PreUpdate, ui_focus_system.in_set(UiSystems::Focus))
        .add_systems(Update, record)
        .add_systems(
            PostUpdate,
            propagate_ui_target_cameras
                .before(PropagateSet::<ComputedUiTargetCamera>::default())
                .before(PropagateSet::<ComputedUiRenderTargetInfo>::default()),
        );
    app.world_mut().spawn((
        Camera2d,
        Camera::default(),
        RenderTarget::default(),
        IsDefaultUiCamera,
    ));
    let (give, other) = give_and_attack(&mut app);
    *app.world_mut().resource_mut::<UiStack>() = UiStack {
        partition: std::iter::once(0..2).collect(),
        uinodes: vec![give, other],
    };
    (app, window, give, other)
}

#[test]
fn owned_background_clicks_and_holds_isolate_the_passive_physical_pointer() {
    for kind in 0..3 {
        for physical in [Vec2::new(440.0, 360.0), Vec2::splat(20.0)] {
            let (step, button) = match kind {
                0 => (Step::Click(ClickTarget::Give), MouseButton::Left),
                1 => (Step::RightClick(ClickTarget::Give), MouseButton::Right),
                _ => (
                    Step::HoldClick(ClickTarget::Give, Duration::from_millis(500)),
                    MouseButton::Left,
                ),
            };
            let (mut app, window, give, _) = fixture(vec![step]);
            set_focus(&mut app, window, true);
            app.world_mut()
                .get_mut::<Window>(window)
                .unwrap()
                .set_cursor_position(Some(physical));
            for ms in [0, 10, 260, 300, 500, 760, 770, 780] {
                frame_at(&mut app, ms);
                assert_eq!(
                    app.world().get::<Window>(window).unwrap().cursor_position(),
                    Some(physical)
                );
            }
            let seen = app.world().resource::<Recorded>();
            // Stable held interaction is essential: OnPress consumers must not repeat.
            assert_eq!(seen.presses, [give]);
            if button == MouseButton::Left {
                assert_eq!(seen.left_clicks, [give]);
                assert_eq!(seen.right_releases, [] as [Entity; 0]);
            } else {
                assert_eq!(seen.left_clicks, [] as [Entity; 0]);
                assert_eq!(seen.right_releases, [give]);
            }
            assert!(
                !app.world()
                    .resource::<ButtonInput<MouseButton>>()
                    .pressed(button)
            );
            assert_eq!(
                app.world().resource::<RunStatus>().phase(),
                Phase::Completed
            );
        }
    }
}

#[test]
fn native_takeover_cancels_without_redirecting_the_script_release() {
    let (mut app, window, give, other) = fixture(vec![Step::HoldClick(
        ClickTarget::Give,
        Duration::from_secs(1),
    )]);
    set_focus(&mut app, window, true);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(440.0, 360.0)));
    for ms in [0, 10, 260] {
        frame_at(&mut app, ms);
    }
    assert_eq!(interaction(&app, give), Interaction::Pressed);
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    frame_at(&mut app, 270);
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
    assert_eq!(interaction(&app, give), Interaction::None);
    // The physical press shares the already-held button. Do not invent a new
    // press edge or redirect the script's eventual release to the other target.
    assert_eq!(interaction(&app, other), Interaction::Hovered);
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
    assert_eq!(
        app.world().resource::<Recorded>().left_clicks,
        [] as [Entity; 0]
    );
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    frame_at(&mut app, 280);
    assert_eq!(app.world().resource::<Recorded>().presses, [give]);
    assert_eq!(
        app.world().resource::<Recorded>().left_clicks,
        [] as [Entity; 0]
    );
    // After cancellation, a fresh native click uses ordinary focus and delivery.
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    frame_at(&mut app, 290);
    assert_eq!(interaction(&app, other), Interaction::Pressed);
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    frame_at(&mut app, 300);
    let seen = app.world().resource::<Recorded>();
    assert_eq!(seen.presses, [give, other]);
    assert_eq!(seen.left_clicks, [other]);
}

#[test]
fn passive_background_clicks_keep_coverage_and_attended_pointer_isolation() {
    for attended in [true, false] {
        let (mut app, window, give, _) = fixture(vec![Step::Click(ClickTarget::Give)]);
        set_focus(&mut app, window, true);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(440.0, 360.0)));
        if attended {
            app.insert_resource(
                Script::new(vec![Step::Click(ClickTarget::Give)]).offline_preview(true),
            );
        } else {
            app.world_mut()
                .spawn((Button, laid_out(Vec2::new(240.0, 360.0), 100.0, 10)));
        }
        for ms in [0, 10, 260, 270] {
            frame_at(&mut app, ms);
        }
        assert_ne!(interaction(&app, give), Interaction::Pressed);
        assert_eq!(
            app.world().resource::<Recorded>().left_clicks,
            [] as [Entity; 0]
        );
        assert_eq!(
            app.world().resource::<RunStatus>().phase(),
            if attended {
                Phase::WaitingForPointer
            } else {
                Phase::Failed
            }
        );
    }
}
