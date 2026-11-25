//! Held-forward animation through a release/re-press and delayed worker replies.
use super::{
    latency_tests::{key, timing_app},
    *,
};
use std::{collections::VecDeque, sync::mpsc};

fn model(app: &mut App) -> Entity {
    let world = app.world_mut();
    let (old, transform, body) = world
        .query_filtered::<(Entity, &Transform, &PlayerBody), With<Player>>()
        .single(world)
        .map(|(entity, transform, body)| (entity, *transform, *body))
        .unwrap();
    world.despawn(old);
    crate::character::testing::stores(world);
    let asset = eq_client_assets::characters::CharacterAsset::testing_animations(&[
        "L01", "L02", "L03", "L04", "L05",
    ]);
    let prepared = crate::character::testing::prepared_from(world, asset);
    let root = crate::character::testing::drawn(world, &prepared);
    world.entity_mut(root).insert((Player, transform, body));
    app.add_systems(
        Update,
        crate::character::animate
            .after(interpolate)
            .in_set(crate::systems::Stage::Present),
    );
    root
}

fn tick(app: &mut App, base: Instant, frame: usize) {
    let now = base + Duration::from_micros(u64::try_from(frame).unwrap() * 16_667);
    app.world_mut().resource_mut::<Controls>().test_now = Some(now);
    app.world_mut()
        .resource_mut::<crate::outbox::Outbox>()
        .set_test_time(now);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_micros(16_667));
    app.update();
}

fn answer(news: &mpsc::Sender<eq_client_core::WorldUpdate>, request: &MovementRequest) {
    news.send(eq_client_core::WorldUpdate::Game(
        eq_client_core::WorldEvent::MotionSent {
            session_id: request.session_id,
            created: request.created,
            position: request.position,
            refused: None,
        },
    ))
    .unwrap();
}

fn exhausted_cursor_stands(mut controls: ResMut<Controls>) {
    // Negative control: the returned #716 treated every exhausted cursor as
    // released, even though W remained held and the next sample was pending.
    if controls.visual.as_ref().is_some_and(|visual| {
        visual
            .translation
            .as_ref()
            .is_some_and(translation::Playhead::finished)
    }) {
        controls.drawn_pose = Some(pose::Snapshot::default());
    }
}

struct Replay {
    clips: Vec<&'static str>,
    phases: Vec<f32>,
    commands: Vec<String>,
    repress_gap_frames: usize,
    used_release: bool,
    used_repress: bool,
}

fn observe(app: &App, root: Entity, awaiting_sample: bool, result: &mut Replay) {
    let controls = app.world().resource::<Controls>();
    if let Some(playhead) = controls
        .visual
        .as_ref()
        .and_then(|visual| visual.translation.as_ref())
    {
        result.used_release |= playhead.rested();
        if app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
        {
            result.used_repress |= !playhead.rested();
            if awaiting_sample && playhead.finished() && !playhead.rested() {
                result.repress_gap_frames += 1;
            }
        }
    }
    result
        .clips
        .push(crate::character::testing::clip(app.world(), root));
    result
        .phases
        .push(crate::character::testing::elapsed(app.world(), root));
}

fn replay(walking: bool, delay: usize, baseline: bool, base: Instant) -> Replay {
    let (mut app, requests, news) = timing_app();
    let root = model(&mut app);
    {
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.walking = walking;
        controls.walk_speed = Some(4.0);
        controls.airborne = Some(AirborneController::default());
    }
    if baseline {
        app.add_systems(
            Update,
            exhausted_cursor_stands
                .after(interpolate)
                .before(crate::character::animate)
                .in_set(crate::systems::Stage::Present),
        );
    }
    let mut result = Replay {
        clips: vec![],
        phases: vec![],
        commands: vec![],
        repress_gap_frames: 0,
        used_release: false,
        used_repress: false,
    };
    let mut replies: VecDeque<(usize, MovementRequest)> = VecDeque::new();
    let mut awaiting_sample = true;
    for frame in 0..45 {
        while replies.front().is_some_and(|(due, _)| *due <= frame) {
            answer(&news, &replies.pop_front().unwrap().1);
        }
        if frame == 5 {
            // A continuing legal sample can cover up to 250 ms. Its ordinary
            // pose clock outlasts the shortened release cursor, reproducing
            // the retained playhead after re-press under the normal schedule.
            {
                let mut controls = app.world_mut().resource_mut::<Controls>();
                controls.moving = true;
                controls.last_sent = Some(base.checked_sub(Duration::from_millis(250)).unwrap());
            }
            key(&mut app, KeyCode::KeyW, true);
        }
        if frame == 8 {
            key(&mut app, KeyCode::KeyW, false);
        }
        if frame == 12 {
            key(&mut app, KeyCode::KeyW, true);
        }
        tick(&mut app, base, frame);
        for command in requests.try_iter() {
            result.commands.push(format!("{command:?}"));
            if let ClientCommand::Move(request) = command {
                if frame >= 12 {
                    awaiting_sample = false;
                }
                replies.push_back((frame + 1 + delay, request));
            }
        }
        observe(&app, root, frame >= 12 && awaiting_sample, &mut result);
    }
    result
}

#[test]
fn release_repress_keeps_walk_and_run_phase_through_one_and_three_late_frames() {
    let base = Instant::now();
    for walking in [false, true] {
        for delay in [1, 3] {
            let fixed = replay(walking, delay, false, base);
            let before = replay(walking, delay, true, base);
            assert_eq!(
                fixed.commands, before.commands,
                "animation changed commands"
            );
            assert!(fixed.used_release && fixed.used_repress);
            assert_eq!(fixed.repress_gap_frames, delay);
            let moving = if walking { "L01" } else { "L02" };
            // Re-press coincides with the ordinary 30 Hz pose opportunity.
            // Inspect every rendered frame thereafter, including the exact
            // one/three exhausted-cursor frames before the next legal sample.
            assert!(fixed.clips[12..].iter().all(|clip| *clip == moving));
            assert!(fixed.phases[12..].windows(2).all(|pair| pair[1] > pair[0]));
            assert!(before.clips[12..].contains(&"P01"));
            assert!(before.phases[12..].windows(2).any(|pair| pair[1] < pair[0]));
        }
    }
}
