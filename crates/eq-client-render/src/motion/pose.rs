//! Local velocity snapshots played with the sample whose position is drawn.
//! They never feed movement, permissions, or outgoing commands.

use bevy::prelude::*;

/// Motion at the drawn point of an own-player sample, in units a second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Snapshot {
    pub horizontal: Vec2,
    pub vertical: f32,
}

/// The velocity history of one collision-checked sample. Interpolating its
/// snapshots preserves a short rising pose even when the sample's endpoint
/// has already passed it. No animation event or minimum hold is introduced.
#[derive(Clone, Debug, Default)]
pub(super) struct Path(Vec<(f32, Snapshot)>);

impl Path {
    pub(super) fn push(&mut self, seconds: f32, snapshot: Snapshot) {
        self.0.push((seconds, snapshot));
    }

    pub(super) fn at(&self, seconds: f32) -> Option<Snapshot> {
        let &(mut before, mut snapshot) = self.0.first()?;
        for &(after, next) in self.0.iter().skip(1) {
            if seconds < after {
                let share = ((seconds - before) / (after - before)).clamp(0.0, 1.0);
                return Some(Snapshot {
                    horizontal: snapshot.horizontal.lerp(next.horizontal, share),
                    vertical: snapshot.vertical + (next.vertical - snapshot.vertical) * share,
                });
            }
            before = after;
            snapshot = next;
        }
        Some(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        AirborneController, CollisionWorld, Controls, Player, fall_step_observed,
        fall_step_with_water, interpolate, physics,
    };
    use super::*;
    use crate::online::{OnlineState, testing as session};
    use eq_client_assets::ui::InstalledClient;
    use std::time::{Duration, Instant};

    fn floor() -> CollisionWorld {
        CollisionWorld::new([
            // Wound to face up, so the floor stays support once downward
            // faces stop counting as floor (6.651).
            [
                [-100.0, 0.0, -100.0],
                [100.0, 0.0, 100.0],
                [100.0, 0.0, -100.0],
            ],
            [
                [-100.0, 0.0, -100.0],
                [-100.0, 0.0, 100.0],
                [100.0, 0.0, 100.0],
            ],
        ])
        .unwrap()
    }

    fn render_app(path: Path, endpoint: Vec3, span: f32) -> (App, Entity) {
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 11, session::player(7));
        let mut controls = Controls {
            sample_pose: Some(path),
            sent_span: span,
            ..default()
        };
        controls.preview_sample(
            Some(11),
            Instant::now(),
            Transform::IDENTITY,
            crate::world_position(endpoint.to_array(), 0.0),
        );
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<crate::spellbook::SpellNames>()
            .insert_resource(online)
            .insert_resource(controls)
            .add_systems(Update, (interpolate, crate::character::animate).chain());
        crate::character::testing::stores(app.world_mut());
        let asset = eq_client_assets::characters::CharacterAsset::testing_animations(&[
            "L01", "L02", "L03", "L04", "L05",
        ]);
        let prepared = crate::character::testing::prepared_from(app.world_mut(), asset);
        let root = crate::character::testing::drawn(app.world_mut(), &prepared);
        app.world_mut().entity_mut(root).insert(Player);
        (app, root)
    }

    #[test]
    fn own_jump_sampling_matrix_keeps_early_pose_and_rolls_back_refusals() {
        for (milliseconds, frame_micros) in [50_u16, 100, 150, 250]
            .into_iter()
            .flat_map(|ms| [16_667_u32, 33_334].map(move |micros| (ms, micros)))
        {
            for running in [false, true] {
                let span = f32::from(milliseconds) / 1000.0;
                let delta = if running {
                    Vec3::X * 30.0 * span
                } else {
                    Vec3::ZERO
                };
                let mut controller = AirborneController::default();
                let mut unchanged = controller.clone();
                let mut path = Path::default();
                let physics = physics(InstalledClient::Titanium, None);
                let endpoint = fall_step_observed(
                    &mut controller,
                    &floor(),
                    physics,
                    None,
                    Vec3::ZERO,
                    delta,
                    span,
                    6.0,
                    true,
                    false,
                    Some(&mut path),
                );
                let baseline = fall_step_with_water(
                    &mut unchanged,
                    &floor(),
                    physics,
                    None,
                    Vec3::ZERO,
                    delta,
                    span,
                    6.0,
                    true,
                    false,
                );
                // Observing presentation leaves every motion result unchanged.
                assert_eq!(endpoint, baseline);
                assert_eq!(
                    controller.velocity().to_bits(),
                    unchanged.velocity().to_bits()
                );
                assert_eq!(controller.drift(), unchanged.drift());
                assert_eq!(controller.peak(), unchanged.peak());
                for delay in [1, 3] {
                    for refused in [false, true] {
                        let (mut app, root) = render_app(path.clone(), endpoint, span);
                        let created = app.world().resource::<Controls>().preview.unwrap().created;
                        let mut seen_rise = false;
                        for frame in 0..18 {
                            app.world_mut()
                                .resource_mut::<Time>()
                                .advance_by(Duration::from_micros(u64::from(frame_micros)));
                            app.update();
                            if app.world().resource::<Controls>().drawn_pose().is_some() {
                                let clip = crate::character::testing::clip(app.world(), root);
                                seen_rise |= clip == if running { "L03" } else { "L04" };
                            }
                            if frame == delay {
                                let position = if refused { Vec3::ZERO } else { endpoint };
                                assert!(
                                    app.world_mut()
                                        .resource_mut::<Controls>()
                                        .reconcile_preview(
                                            Some(11),
                                            created,
                                            crate::world_position(position.to_array(), 0.0),
                                            refused
                                        )
                                );
                                if refused {
                                    assert!(
                                        app.world().resource::<Controls>().drawn_pose().is_none()
                                    );
                                }
                            }
                        }
                        assert!(
                            seen_rise,
                            "{milliseconds}ms running={running} delay={delay}"
                        );
                        if refused {
                            assert!(app.world().resource::<Controls>().drawn_pose().is_none());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn own_jump_queued_paths_land_and_reset_without_replaying_the_rise() {
        let mut controller = AirborneController::default();
        let mut feet = Vec3::ZERO;
        let mut controls = Controls::default();
        for index in 0..12 {
            let mut path = Path::default();
            feet = fall_step_observed(
                &mut controller,
                &floor(),
                physics(InstalledClient::Titanium, None),
                None,
                feet,
                Vec3::X * 3.0,
                0.1,
                6.0,
                index == 0,
                false,
                Some(&mut path),
            );
            controls.sample_pose = Some(path);
            controls.sent_span = 0.1;
            controls.display_sample(
                Transform::IDENTITY,
                crate::world_position(feet.to_array(), 0.0),
            );
        }
        let mut shown = Transform::IDENTITY;
        let visual = controls.visual.as_mut().unwrap();
        visual.advance(0.033, &mut shown);
        let early = visual.pose.as_ref().unwrap().at(visual.elapsed).unwrap();
        assert!(
            early.vertical >= eq_client_assets::falls::JUMP_STRENGTH,
            "queued landing must not replace the drawn launch"
        );
        visual.advance(2.0, &mut shown);
        let pose = visual.pose.as_ref().unwrap().at(visual.elapsed).unwrap();
        assert!(pose.vertical.abs() < f32::EPSILON);
        assert_eq!(shown.translation, feet);
        controls.drawn_pose = Some(pose);
        controls.reset(Some(30.0));
        assert!(controls.drawn_pose().is_none());
        assert!(controls.sample_pose.is_none());
    }

    #[test]
    fn own_jump_snapshots_clear_on_paused_prediction_and_flight() {
        let snapshot = Snapshot {
            horizontal: Vec2::X * 30.0,
            vertical: 44.55,
        };
        let mut path = Path::default();
        path.push(0.0, snapshot);
        let mut controls = Controls {
            sample_pose: Some(path.clone()),
            sent_span: 0.1,
            ..default()
        };
        controls.display_sample(
            Transform::IDENTITY,
            crate::world_position([1.0, 1.0, 0.0], 0.0),
        );
        controls.sample_pose = Some(path.clone());
        controls.drawn_pose = Some(snapshot);
        controls.choose_movement(eq_client_core::movement::MovementOptions {
            fly: true,
            ..default()
        });
        assert!(controls.drawn_pose().is_none());
        assert!(controls.sample_pose.is_none());
        assert!(controls.visual.as_ref().unwrap().pose.is_none());
        controls.choose_movement(eq_client_core::movement::MovementOptions::default());
        assert!(controls.drawn_pose().is_none());
        controls.sample_pose = Some(path);
        controls.drawn_pose = Some(snapshot);
        controls.pause_prediction();
        assert!(controls.drawn_pose().is_none());
        assert!(controls.sample_pose.is_none());
        assert!(controls.visual.is_none());
    }

    #[test]
    fn own_jump_retry_does_not_inherit_an_old_refusal_in_its_display() {
        let mut path = Path::default();
        path.push(
            0.0,
            Snapshot {
                horizontal: Vec2::X * 30.0,
                vertical: 44.55,
            },
        );
        let mut controls = Controls {
            refused: Some("old refusal".into()),
            sample_pose: Some(path),
            sent_span: 0.1,
            ..default()
        };
        controls.preview_sample(
            Some(11),
            Instant::now(),
            Transform::IDENTITY,
            crate::world_position([1.0, 1.0, 0.0], 0.0),
        );
        assert!(controls.visual.as_ref().unwrap().pose.is_some());
    }

    #[test]
    fn own_jump_selector_leaves_the_outgoing_command_list_identical_across_matrix() {
        use bevy::ecs::system::RunSystemOnce;
        let stamp = Instant::now();
        for milliseconds in [50_u16, 100, 150, 250] {
            for delay in [1, 3] {
                for refused in [false, true] {
                    let mut lists = Vec::new();
                    for enabled in [false, true] {
                        let (mut app, requests) =
                            super::super::tests::standing_jump(InstalledClient::Titanium);
                        app.world_mut()
                            .resource_mut::<ButtonInput<KeyCode>>()
                            .press(KeyCode::KeyW);
                        app.world_mut().resource_mut::<Controls>().test_span =
                            Some(f32::from(milliseconds) / 1000.0);
                        app.update();
                        crate::character::testing::stores(app.world_mut());
                        app.init_resource::<Time>()
                            .init_resource::<crate::spellbook::SpellNames>();
                        let asset =
                            eq_client_assets::characters::CharacterAsset::testing_animations(&[
                                "L01", "L02", "L03", "L04", "L05",
                            ]);
                        let prepared =
                            crate::character::testing::prepared_from(app.world_mut(), asset);
                        let root = crate::character::testing::drawn(app.world_mut(), &prepared);
                        app.world_mut().entity_mut(root).insert(Player);
                        app.world_mut()
                            .resource_mut::<Controls>()
                            .set_test_pose(None);
                        app.world_mut()
                            .run_system_once(crate::character::animate)
                            .unwrap();
                        for _ in 0..delay {
                            app.world_mut().resource_mut::<Controls>().set_test_pose(
                                enabled.then_some(Snapshot {
                                    horizontal: Vec2::new(30.0, 0.0),
                                    vertical: 44.55,
                                }),
                            );
                            app.world_mut()
                                .resource_mut::<Time>()
                                .advance_by(Duration::from_millis(17));
                            app.world_mut()
                                .run_system_once(crate::character::animate)
                                .unwrap();
                        }
                        if refused {
                            let preview = app.world().resource::<Controls>().preview.unwrap();
                            app.world_mut()
                                .resource_mut::<Controls>()
                                .reconcile_preview(
                                    Some(11),
                                    preview.created,
                                    eq_client_core::WorldPosition::default(),
                                    true,
                                );
                        }
                        app.world_mut()
                            .run_system_once(crate::character::animate)
                            .unwrap();
                        let mut commands: Vec<_> = requests.try_iter().collect();
                        assert!(commands.iter().any(|command| matches!(
                            command,
                            eq_client_core::ClientCommand::Jump { .. }
                        )));
                        // Only the wall-clock request stamps differ between the
                        // two executions. Preserve and compare every action field.
                        for command in &mut commands {
                            match command {
                                eq_client_core::ClientCommand::Move(request) => {
                                    request.created = stamp;
                                }
                                eq_client_core::ClientCommand::Jump { created, .. } => {
                                    *created = stamp;
                                }
                                _ => {}
                            }
                        }
                        lists.push(format!("{commands:?}"));
                    }
                    assert_eq!(
                        lists[0], lists[1],
                        "{milliseconds}ms delay={delay} refused={refused}"
                    );
                }
            }
        }
    }

    fn ordinary_pose_clock(mut controls: ResMut<Controls>) {
        // Negative control: the merged #710 selector reads the unchanged
        // orientation timeline, even after the translation path is finished.
        controls.drawn_pose = controls.visual.as_ref().and_then(|visual| {
            visual
                .pose
                .as_ref()
                .and_then(|path| path.at(visual.elapsed))
        });
    }

    fn resume_translation(app: &mut App, root: Entity) {
        let from = *app.world().get::<Transform>(root).unwrap();
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.sent_span = 0.1;
        controls.visual.as_mut().unwrap().release_translation(false);
        controls.display_sample(from, crate::world_position((Vec3::X * 2.0).to_array(), 0.0));
    }

    #[test]
    fn release_rest_stops_walk_and_run_at_the_translation_endpoint() {
        for walking in [false, true] {
            for snapshots in [false, true] {
                for baseline in [false, true] {
                    let mut path = Path::default();
                    if snapshots {
                        path.push(
                            0.0,
                            Snapshot {
                                horizontal: Vec2::X * 10.0,
                                vertical: 0.0,
                            },
                        );
                    }
                    let (mut app, root) = render_app(path, Vec3::X, 0.25);
                    app.world_mut().resource_mut::<Controls>().walking = walking;
                    if baseline {
                        app.add_systems(
                            Update,
                            ordinary_pose_clock
                                .after(interpolate)
                                .before(crate::character::animate),
                        );
                    }
                    let original_world = app
                        .world()
                        .resource::<OnlineState>()
                        .world()
                        .player()
                        .unwrap()
                        .position;
                    let tick = |app: &mut App| {
                        app.world_mut()
                            .resource_mut::<Time>()
                            .advance_by(Duration::from_micros(16_667));
                        app.update();
                    };
                    for _ in 0..4 {
                        tick(&mut app);
                    }
                    let moving_clip = if walking { "L01" } else { "L02" };
                    assert_eq!(
                        crate::character::testing::clip(app.world(), root),
                        moving_clip
                    );
                    let rotation = app.world().get::<Transform>(root).unwrap().rotation;
                    {
                        let mut controls = app.world_mut().resource_mut::<Controls>();
                        let visual = controls.visual.as_mut().unwrap();
                        // render_app's target rotation is the ordinary sample
                        // facing; make the initial pose agree for this flat path.
                        visual.from.rotation = visual.to.rotation;
                        visual.release_translation(true);
                        assert!(visual.translation.is_some());
                    }
                    for _ in 0..3 {
                        tick(&mut app);
                    }
                    assert_eq!(
                        app.world().get::<Transform>(root).unwrap().translation,
                        Vec3::X
                    );
                    assert_eq!(
                        app.world().get::<Transform>(root).unwrap().rotation,
                        rotation
                    );
                    assert!(
                        app.world()
                            .resource::<Controls>()
                            .visual
                            .as_ref()
                            .unwrap()
                            .elapsed
                            < 0.25
                    );
                    assert_eq!(
                        crate::character::testing::clip(app.world(), root),
                        if baseline { moving_clip } else { "P01" },
                        "walking={walking} snapshots={snapshots} baseline={baseline}"
                    );
                    assert_eq!(
                        app.world()
                            .resource::<OnlineState>()
                            .world()
                            .player()
                            .unwrap()
                            .position,
                        original_world
                    );
                    if !baseline {
                        for _ in 0..2 {
                            tick(&mut app);
                        }
                        assert_eq!(crate::character::testing::clip(app.world(), root), "P01");
                        // New submitted deck-free travel resumes animation;
                        // the standing snapshot cannot stick over a fresh leg.
                        resume_translation(&mut app, root);
                        for _ in 0..2 {
                            tick(&mut app);
                        }
                        assert_eq!(
                            crate::character::testing::clip(app.world(), root),
                            moving_clip
                        );
                    }
                }
            }
        }
    }
}
