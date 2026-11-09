//! Shared window resize geometry and saved dimensions.
use bevy::prelude::*;

/// Client usability minimum, not native constants or decoration measurements.
/// The authored initial rectangle is never used as a minimum.
pub(super) const MINIMUM: Vec2 = Vec2::new(160.0, 100.0);
/// A logical-pixel band inside the frame. Controls retain their focus priority.
const BAND: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Size {
    pub scope: u64,
    pub dimensions: Vec2,
}

impl Size {
    pub(super) fn valid(self) -> bool {
        self.dimensions.is_finite()
            && self.dimensions.cmpge(MINIMUM).all()
            && (self.dimensions.x + self.dimensions.y).is_finite()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Edges {
    pub x: i8,
    pub y: i8,
}

impl Edges {
    /// Corners select both axes. Points beyond the frame never start a resize.
    pub(super) fn at(point: Vec2, size: Vec2) -> Option<Self> {
        if !point.is_finite()
            || !size.is_finite()
            || point.cmplt(Vec2::ZERO).any()
            || point.cmpgt(size).any()
        {
            return None;
        }
        let axis = |point: f32, length: f32| {
            if point <= BAND.min(length * 0.5) {
                -1
            } else {
                i8::from(point >= length - BAND.min(length * 0.5))
            }
        };
        let edges = Self {
            x: axis(point.x, size.x),
            y: axis(point.y, size.y),
        };
        (edges.x != 0 || edges.y != 0).then_some(edges)
    }

    /// Changes selected edges only, including when the pointer crosses its
    /// opposite edge. Keep the selected top/left reachable on screen.
    pub(super) fn dragged(self, start: Rect, delta: Vec2, minimum: Vec2) -> Rect {
        let minimum = minimum.max(MINIMUM);
        let mut result = start;
        for (axis, side) in [(0, self.x), (1, self.y)] {
            if side < 0 {
                result.min[axis] = (start.min[axis] + delta[axis])
                    .clamp(0.0, (start.max[axis] - minimum[axis]).max(0.0));
            } else if side > 0 {
                result.max[axis] =
                    (start.max[axis] + delta[axis]).max(start.min[axis] + minimum[axis]);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::super::{DragHandle, DragState, Frame, PositionLocked, input};
    use super::*;

    fn fixture(scale: f32, sizable: bool) -> (App, Entity) {
        let mut app = crate::testing::app();
        app.add_systems(Update, input);
        let world = app.world_mut();
        world
            .query::<&mut Window>()
            .single_mut(world)
            .unwrap()
            .resolution
            .set_physical_resolution(3000, 2000);
        let mut frame = Frame::default();
        frame.sizing(sizable.then_some(7), Vec2::new(400.0, 250.0));
        let entity = app
            .world_mut()
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(100),
                    top: px(80),
                    width: px(400),
                    height: px(250),
                    ..default()
                },
                frame,
                Interaction::None,
                ComputedNode {
                    size: Vec2::new(400.0, 250.0) * scale,
                    inverse_scale_factor: scale.recip(),
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(300.0, 205.0) * scale),
            ))
            .id();
        app.world_mut()
            .entity_mut(entity)
            .insert(DragHandle(entity));
        app.update();
        (app, entity)
    }

    fn point(app: &mut App, logical: Vec2, scale: f32) {
        let world = app.world_mut();
        world
            .query::<&mut Window>()
            .single_mut(world)
            .unwrap()
            .set_physical_cursor_position(Some((logical * scale).as_dvec2()));
    }

    fn press(app: &mut App, entity: Entity, logical: Vec2, scale: f32) {
        point(app, logical, scale);
        *app.world_mut().get_mut::<Interaction>(entity).unwrap() = Interaction::Pressed;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    }

    #[test]
    fn a_native_runbook_resize_respects_its_minimum_and_survives_recreation() {
        use super::super::{Layouts, WindowId, layout, resizable_frame};

        let mut app = crate::testing::app();
        app.init_resource::<Layouts>()
            .add_systems(Update, (input, layout::remember).chain());
        let spawn = |app: &mut App| {
            resizable_frame(
                &mut app.world_mut().commands(),
                WindowId::Runbook,
                Node {
                    min_width: px(240),
                    min_height: px(260),
                    ..default()
                },
                Vec2::new(380.0, 360.0),
                u64::from_be_bytes(*b"runbook1"),
            )
        };
        let frame = spawn(&mut app);
        app.update();
        app.world_mut().entity_mut(frame).insert((
            ComputedNode {
                size: Vec2::new(380.0, 360.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(290.0, 260.0)),
        ));
        press(&mut app, frame, Vec2::new(478.0, 438.0), 1.0);
        point(&mut app, Vec2::new(150.0, 120.0), 1.0);
        app.update();
        let size = |app: &App, entity| {
            let node = app.world().get::<Node>(entity).unwrap();
            (node.width, node.height)
        };
        assert_eq!(size(&app, frame), (px(240), px(260)));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        app.world_mut().despawn(frame);
        let recreated = spawn(&mut app);
        app.update();
        assert_eq!(size(&app, recreated), (px(240), px(260)));
        assert_eq!(
            app.world()
                .get::<Frame>(recreated)
                .unwrap()
                .preferred_size(),
            Some(Vec2::new(240.0, 260.0))
        );
    }

    #[test]
    fn resize_gestures_keep_opposite_edges_at_small_and_large_scales() {
        for scale in [0.75, 1.0, 1.5, 2.25] {
            for x in -1..=1 {
                for y in -1..=1 {
                    if x == 0 && y == 0 {
                        continue;
                    }
                    let (mut app, entity) = fixture(scale, true);
                    let coordinate = |side: i8, low: f32, high: f32| match side.cmp(&0) {
                        std::cmp::Ordering::Less => low,
                        std::cmp::Ordering::Greater => high,
                        std::cmp::Ordering::Equal => low.midpoint(high),
                    };
                    let start = Vec2::new(coordinate(x, 100.0, 500.0), coordinate(y, 80.0, 330.0));
                    press(&mut app, entity, start, scale);
                    assert_eq!(
                        app.world()
                            .resource::<DragState>()
                            .active
                            .as_ref()
                            .unwrap()
                            .edges,
                        Some(Edges { x, y })
                    );
                    point(&mut app, start + Vec2::new(25.0, 15.0), scale);
                    app.update();
                    let node = app.world().get::<Node>(entity).unwrap();
                    let expected = Edges { x, y }.dragged(
                        Rect::new(100.0, 80.0, 500.0, 330.0),
                        Vec2::new(25.0, 15.0),
                        MINIMUM,
                    );
                    assert_eq!(
                        (node.left, node.top, node.width, node.height),
                        (
                            px(expected.min.x),
                            px(expected.min.y),
                            px(expected.width()),
                            px(expected.height())
                        )
                    );
                    assert_eq!(
                        app.world().get::<Frame>(entity).unwrap().preferred_size(),
                        Some(expected.size())
                    );
                    point(&mut app, start, scale);
                    app.update();
                    let restored = app.world().get::<Node>(entity).unwrap();
                    assert_eq!(
                        (restored.left, restored.top, restored.width, restored.height),
                        (px(100), px(80), px(400), px(250))
                    );
                    assert_eq!(
                        app.world().get::<Frame>(entity).unwrap().preferred_size(),
                        Some(Vec2::new(400.0, 250.0))
                    );
                }
            }
        }
    }

    #[test]
    fn resize_capture_cancels_on_lifecycle_and_never_restarts_a_held_press() {
        for change in 0..9 {
            let (mut app, entity) = fixture(1.0, true);
            press(&mut app, entity, Vec2::new(500.0, 200.0), 1.0);
            assert!(app.world().resource::<DragState>().active.is_some());
            match change {
                0 => {
                    let world = app.world_mut();
                    world
                        .query::<&mut Window>()
                        .single_mut(world)
                        .unwrap()
                        .focused = false;
                }
                1 => {
                    app.world_mut().entity_mut(entity).insert(PositionLocked);
                }
                2 => {
                    app.world_mut()
                        .get_mut::<ComputedNode>(entity)
                        .unwrap()
                        .inverse_scale_factor = 0.5;
                }
                3 => {
                    app.world_mut()
                        .get_mut::<Frame>(entity)
                        .unwrap()
                        .sizing(None, Vec2::new(400.0, 250.0));
                }
                4 => {
                    app.world_mut().get_mut::<Frame>(entity).unwrap().minimized = true;
                }
                5 => {
                    app.world_mut()
                        .resource_mut::<crate::profile_files::Profile>()
                        .set_changed();
                }
                6 => {
                    app.world_mut().resource_mut::<crate::skin::UiSkin>().0 = "another".into();
                }
                7 => {
                    app.world_mut().insert_resource(UiScale(1.25));
                }
                _ => {
                    app.world_mut().despawn(entity);
                }
            }
            point(&mut app, Vec2::new(650.0, 200.0), 1.0);
            app.update();
            assert!(
                app.world().resource::<DragState>().active.is_none(),
                "change {change}"
            );
            app.update();
            assert!(app.world().resource::<DragState>().active.is_none());
        }
    }

    #[test]
    fn fixed_edges_move_and_control_presses_do_not_resize() {
        let (mut app, entity) = fixture(1.0, false);
        press(&mut app, entity, Vec2::new(500.0, 200.0), 1.0);
        assert!(
            app.world()
                .resource::<DragState>()
                .active
                .as_ref()
                .unwrap()
                .edges
                .is_none()
        );
        point(&mut app, Vec2::new(530.0, 215.0), 1.0);
        app.update();
        let node = app.world().get::<Node>(entity).unwrap();
        assert_eq!((node.width, node.height), (px(400), px(250)));
        assert!(app.world().get::<Frame>(entity).unwrap().size.is_none());

        let (mut app, entity) = fixture(1.0, true);
        let control = app.world_mut().spawn((Button, Interaction::None)).id();
        press(&mut app, control, Vec2::new(500.0, 200.0), 1.0);
        assert!(app.world().resource::<DragState>().active.is_none());
        assert_eq!(app.world().get::<Node>(entity).unwrap().width, px(400));
    }

    #[test]
    fn resize_release_outside_ends_capture_and_later_motion_is_idle() {
        let (mut app, entity) = fixture(1.0, true);
        press(&mut app, entity, Vec2::new(500.0, 330.0), 1.0);
        point(&mut app, Vec2::new(650.0, 430.0), 1.0);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        point(&mut app, Vec2::new(900.0, 700.0), 1.0);
        app.update();
        assert!(app.world().resource::<DragState>().active.is_none());
        assert_eq!(app.world().get::<Node>(entity).unwrap().width, px(550));
    }

    #[test]
    fn resize_translated_scaled_frame_keeps_the_opposite_rendered_edge() {
        use bevy::math::Affine2;
        let (mut app, entity) = fixture(1.5, true);
        let scale = 1.5;
        let basis = Vec2::new(1.75, 0.8);
        let center = Vec2::new(300.0, 205.0) * scale + Vec2::new(170.0, 23.0);
        let transform = Affine2::from_scale_angle_translation(basis, 0.0, center);
        *app.world_mut()
            .get_mut::<UiGlobalTransform>(entity)
            .unwrap() = transform.into();
        let start = transform.transform_point2(Vec2::new(-198.0, 0.0) * scale);
        press(&mut app, entity, start / scale, scale);
        assert_eq!(
            app.world()
                .resource::<DragState>()
                .active
                .as_ref()
                .unwrap()
                .edges,
            Some(Edges { x: -1, y: 0 })
        );
        point(
            &mut app,
            (start + basis * Vec2::new(30.0, 0.0) * scale) / scale,
            scale,
        );
        app.update();
        let node = app.world().get::<Node>(entity).unwrap();
        let (Val::Px(left), Val::Px(width)) = (node.left, node.width) else {
            panic!("pixel rectangle")
        };
        assert_eq!(width, 370.0);
        let old_right = 100.0 + 200.0 + basis.x * 200.0;
        let new_right = left + width * 0.5 + basis.x * width * 0.5;
        assert!((new_right - old_right).abs() < 0.001);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn resize_actual_focus_keeps_controls_and_the_top_frame_as_the_only_owner() {
        // Real Bevy focus with fabricated bounds: routing evidence, not layout,
        // GPU, native pointer capture, or a server compatibility check.
        use bevy::{
            app::{HierarchyPropagatePlugin, PropagateSet},
            camera::RenderTarget,
            input::touch::Touches,
            ui::{FocusPolicy, UiStack, ui_focus_system, update::propagate_ui_target_cameras},
        };
        let (mut app, lower) = fixture(1.0, true);
        app.init_resource::<Touches>()
            .init_resource::<UiStack>()
            .init_resource::<UiScale>()
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
                PostUpdate,
            ))
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
                PostUpdate,
            ))
            .add_systems(PreUpdate, ui_focus_system.before(crate::clicks::track))
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
        app.world_mut().entity_mut(lower).insert((
            Button,
            FocusPolicy::Block,
            InheritedVisibility::VISIBLE,
        ));
        let mut top_state = Frame::default();
        top_state.sizing(Some(7), Vec2::new(400.0, 250.0));
        let upper = app
            .world_mut()
            .spawn((
                Node {
                    left: px(100),
                    top: px(80),
                    width: px(400),
                    height: px(250),
                    ..default()
                },
                top_state,
                Button,
                FocusPolicy::Block,
                InheritedVisibility::VISIBLE,
                ComputedNode {
                    size: Vec2::new(400.0, 250.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(300.0, 205.0)),
            ))
            .id();
        app.world_mut().entity_mut(upper).insert(DragHandle(upper));
        let control = app
            .world_mut()
            .spawn((
                Button,
                FocusPolicy::Block,
                InheritedVisibility::VISIBLE,
                ChildOf(upper),
                ComputedNode {
                    size: Vec2::new(18.0, 22.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(500.0, 200.0)),
            ))
            .id();
        app.update();
        *app.world_mut().resource_mut::<UiStack>() = UiStack {
            partition: std::iter::once(0..3).collect(),
            uinodes: vec![lower, upper, control],
        };
        point(&mut app, Vec2::new(499.0, 200.0), 1.0);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert_eq!(
            *app.world().get::<Interaction>(control).unwrap(),
            Interaction::Pressed
        );
        assert!(app.world().resource::<DragState>().active.is_none());
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        *app.world_mut().resource_mut::<UiStack>() = UiStack {
            partition: std::iter::once(0..2).collect(),
            uinodes: vec![lower, upper],
        };
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let active = app.world().resource::<DragState>().active.as_ref().unwrap();
        assert_eq!(active.frame, upper);
        assert_eq!(active.edges, Some(Edges { x: 1, y: 0 }));
        assert_eq!(app.world().get::<Node>(lower).unwrap().width, px(400));
    }

    #[test]
    fn resize_capture_blocks_world_input_after_the_cursor_leaves_the_frame() {
        #[derive(Resource, Default)]
        struct Blocked(bool);
        let (mut app, entity) = fixture(1.0, true);
        app.init_resource::<Blocked>().add_systems(
            Update,
            (|ui: super::super::pointer::PointerUi, mut blocked: ResMut<Blocked>| {
                blocked.0 = ui.contains(Vec2::new(900.0, 700.0));
            })
            .after(input),
        );
        press(&mut app, entity, Vec2::new(500.0, 200.0), 1.0);
        assert!(app.world().resource::<Blocked>().0);
        point(&mut app, Vec2::new(900.0, 700.0), 1.0);
        app.update();
        assert!(app.world().resource::<Blocked>().0);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        // The resized east edge can reach this point; check an off-frame point
        // through a fresh owner-free sample instead of inferring click-through.
        assert!(app.world().resource::<DragState>().active.is_none());
    }

    #[test]
    fn resize_edges_corners_and_band_endpoints() {
        let size = Vec2::new(400.0, 300.0);
        for (point, expected) in [
            ((0.0, 150.0), (-1, 0)),
            ((400.0, 150.0), (1, 0)),
            ((200.0, 0.0), (0, -1)),
            ((200.0, 300.0), (0, 1)),
            ((6.0, 6.0), (-1, -1)),
            ((394.0, 6.0), (1, -1)),
            ((6.0, 294.0), (-1, 1)),
            ((394.0, 294.0), (1, 1)),
        ] {
            assert_eq!(
                Edges::at(Vec2::from(point), size),
                Some(Edges {
                    x: expected.0,
                    y: expected.1
                })
            );
        }
        for point in [Vec2::new(-0.01, 20.0), Vec2::new(401.0, 20.0), size * 0.5] {
            assert_eq!(Edges::at(point, size), None);
        }
    }

    #[test]
    fn resize_opposite_edges_stay_anchored_at_limits() {
        let start = Rect::new(200.0, 180.0, 600.0, 480.0);
        for x in -1..=1 {
            for y in -1..=1 {
                let edges = Edges { x, y };
                let result = edges.dragged(start, Vec2::new(900.0, 900.0), MINIMUM);
                assert!(result.size().cmpge(MINIMUM).all());
                for (axis, side) in [(0, x), (1, y)] {
                    if side <= 0 {
                        assert_eq!(result.max[axis], start.max[axis]);
                    }
                    if side >= 0 {
                        assert_eq!(result.min[axis], start.min[axis]);
                    }
                }
            }
        }
    }
}
