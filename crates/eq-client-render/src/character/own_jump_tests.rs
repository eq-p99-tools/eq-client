use super::*;
use crate::motion::{Controls, pose::Snapshot};
use std::time::Duration;

fn motion(horizontal: f32, vertical: f32) -> Snapshot {
    Snapshot {
        horizontal: Vec2::new(horizontal, 0.0),
        vertical,
    }
}

#[test]
fn own_jump_uses_exact_moving_and_standing_thresholds() {
    let strength = eq_client_assets::falls::JUMP_STRENGTH;
    let horizontal_boundary = 0.001 * TICKS_PER_SECOND;
    for (horizontal, vertical, expected) in [
        (30.0, strength, "L03"),
        (30.0, strength.next_down(), "L02"),
        (30.0, 0.0, "L02"),
        (30.0, -64.0, "L02"),
        (30.0, (-64.0_f32).next_down(), "L05"),
        (0.0, strength * 0.5, "L04"),
        (0.0, (strength * 0.5).next_down(), "P01"),
        (0.0, 0.0, "P01"),
        (0.0, -45.0, "P01"),
        (0.0, (-45.0_f32).next_down(), "L05"),
        (horizontal_boundary, 20.0, "L02"),
        (-horizontal_boundary, 20.0, "L02"),
        (horizontal_boundary.next_down(), 20.0, "L04"),
        // A half-strength launch does not halve the selector's actor strength.
        (30.0, 44.55 * 0.5, "L02"),
    ] {
        assert_eq!(
            own_air_clip(("L02", false), motion(horizontal, vertical), |_| true).0,
            expected,
            "{horizontal}, {vertical}"
        );
    }
    let y_boundary = Snapshot {
        horizontal: Vec2::new(0.0, -horizontal_boundary),
        vertical: 20.0,
    };
    assert_eq!(own_air_clip(("L02", false), y_boundary, |_| true).0, "L02");
    assert_eq!(
        own_air_clip(("L01", false), motion(10.0, 0.0), |_| true).0,
        "L01"
    );
}

#[test]
fn own_jump_preserves_special_poses_and_missing_clip_fallback() {
    for clip in [
        "D05", "P02", "P05", "L06", "L07", "L08", "P06", "L09", "C05", "T04",
    ] {
        let selected = (clip, true);
        assert_eq!(
            own_air_clip(selected, motion(30.0, 44.55), |_| true),
            selected
        );
        assert_eq!(
            own_air_clip(selected, motion(30.0, -80.0), |_| true),
            selected
        );
    }
    assert_eq!(
        own_air_clip(("L02", false), motion(30.0, 44.55), |_| false),
        ("P01", false)
    );
    assert_eq!(
        own_air_clip(("L02", false), motion(30.0, f32::NAN), |_| true),
        ("L02", false)
    );
}

#[test]
fn own_jump_animation_uses_drawn_velocity_and_updates_joints() {
    use crate::online::{OnlineState, testing as session};
    let mut app = crate::testing::app();
    testing::stores(app.world_mut());
    app.init_resource::<Controls>().add_systems(Update, animate);
    session::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        11,
        session::player(7),
    );
    let asset = CharacterAsset::testing_animations(&["L01", "L02", "L03", "L04", "L05"]);
    let prepared = testing::prepared_from(app.world_mut(), asset);
    let root = testing::drawn(app.world_mut(), &prepared);
    app.world_mut().entity_mut(root).insert(crate::Player);
    for (pose, expected) in [
        (motion(30.0, 44.55), "L03"),
        (motion(30.0, 0.0), "L02"),
        (motion(30.0, -80.0), "L05"),
        (motion(0.0, 27.0), "L04"),
        (motion(0.0, 0.0), "P01"),
    ] {
        app.world_mut()
            .resource_mut::<Controls>()
            .set_test_pose(Some(pose));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(40));
        app.update();
        assert_eq!(
            app.world().get::<AnimatedCharacter>(root).unwrap().clip.0,
            expected
        );
    }
    app.world_mut()
        .resource_mut::<Controls>()
        .set_test_pose(Some(motion(30.0, 44.55)));
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(40));
    app.update();
    let joint = app.world().get::<AnimatedCharacter>(root).unwrap().joints[1];
    let before = app.world().get::<Transform>(joint).unwrap().rotation;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(40));
    app.update();
    assert!(
        app.world()
            .get::<Transform>(joint)
            .unwrap()
            .rotation
            .angle_between(before)
            > 0.01
    );
}
