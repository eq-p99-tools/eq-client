use super::*;

fn echo(world: &mut ClientWorld, revision: u64, unconscious: bool) -> Changes {
    let basis = world.hit_point_basis().unwrap();
    game(
        world,
        WorldEvent::OwnUnconscious {
            session_id: 1,
            revision,
            unconscious,
            basis,
        },
    )
}

#[test]
fn own_hp_control_distinguishes_unknown_gear_display_zero_waking_and_actual_death() {
    let mut world = admitted();
    game(&mut world, hit_points(9, -20, 150));
    assert!(!world.is_unconscious());
    assert_eq!(world.hit_point_basis(), None);
    game(&mut world, inventory(vec![chest()]));
    assert!(!world.is_unconscious());
    assert_eq!(world.hit_point_basis().unwrap().current(), Some(80));
    game(&mut world, inventory(Vec::new()));
    game(&mut world, hit_points(9, 1, 150));
    assert_eq!(world.health(9), Some(0));
    assert!(!world.is_unconscious());
    game(&mut world, hit_points(9, 0, 150));
    assert!(world.is_unconscious());
    let news = echo(&mut world, 1, true);
    assert!(matches!(
        news.notices[..],
        [Notice::ServerString {
            id: 12877,
            message_type: Some(15),
            ..
        }]
    ));
    assert!(!world.is_dead());
    assert_eq!(echo(&mut world, 1, true).notices, []);
    // The report can heal; the existing posture owner's wake releases control.
    game(&mut world, hit_points(9, 1, 150));
    assert!(world.is_unconscious());
    let news = echo(&mut world, 2, false);
    assert!(matches!(
        news.notices[..],
        [Notice::ServerString {
            id: 12876,
            message_type: Some(13),
            ..
        }]
    ));
    assert!(!world.is_unconscious());
    game(&mut world, WorldEvent::DeathBind { session_id: 1 });
    game(&mut world, hit_points(9, 50, 150));
    assert!(world.is_dead());
    assert!(!world.is_unconscious());
    game(&mut world, entered(2));
    assert!(!world.is_dead());
    assert!(!world.is_unconscious());
    assert_eq!(world.accepted_hit_point_basis(), None);
}

#[test]
fn hp_count_diagnostics_distinguish_computed_accepted_and_replaced_reports() {
    let mut world = admitted();
    game(&mut world, inventory(Vec::new()));
    game(&mut world, hit_points(9, 0, 31));
    let computed = world.hit_point_basis().unwrap();
    assert_eq!(computed.current(), Some(0));
    assert_eq!(world.accepted_hit_point_basis(), None);
    assert!(
        world.is_unconscious(),
        "the host holds input before acceptance"
    );

    echo(&mut world, 1, true);
    assert_eq!(world.accepted_hit_point_basis(), Some(computed));

    // Even an identical HP receipt replaces the local count. An accepted
    // state from the previous receipt must remain distinguishable from a
    // newly computed basis, despite the numerical values being identical.
    game(&mut world, hit_points(9, 0, 31));
    assert_eq!(world.hit_point_basis(), Some(computed));
    assert_eq!(world.accepted_hit_point_basis(), None);
    echo(&mut world, 1, true);
    assert_eq!(world.accepted_hit_point_basis(), Some(computed));

    game(&mut world, hit_points(9, 5, 31));
    let healed = world.hit_point_basis().unwrap();
    assert_eq!(healed.current(), Some(5));
    assert_eq!(world.accepted_hit_point_basis(), None);
    assert!(
        world.is_unconscious(),
        "positive HP still awaits the worker's wake"
    );
    echo(&mut world, 2, false);
    assert_eq!(world.accepted_hit_point_basis(), Some(healed));
    assert!(!world.is_unconscious());
    assert!(!world.is_dead());
}

#[test]
fn submitted_drowning_changes_local_hp_and_a_fresh_report_replaces_it() {
    let mut world = admitted();
    game(&mut world, inventory(Vec::new()));
    game(&mut world, hit_points(9, 1, 23));
    assert!(!world.is_unconscious());
    let report = WorldEvent::HazardReported {
        session_id: 1,
        hazard: crate::hazards::Hazard::Drowning,
        amount: 2,
        record: true,
    };
    game(&mut world, report.clone());
    assert_eq!(world.vitals().reported_hp.unwrap().current, 1);
    assert_eq!(world.counted_hit_points(), Some(1));
    assert_eq!(world.vitals().local_hp_damage, 2);
    assert_eq!(world.hit_point_basis().unwrap().current(), Some(-1));
    assert!(world.is_unconscious());
    assert!(!world.is_dead());
    assert!(world.hit_point_gauge().unwrap().value() < -9);
    // A foreign admission's report is not damage in this admission.
    game(
        &mut world,
        WorldEvent::HazardReported {
            session_id: 2,
            hazard: crate::hazards::Hazard::Drowning,
            amount: 50,
            record: true,
        },
    );
    assert_eq!(world.vitals().local_hp_damage, 2);
    game(&mut world, hit_points(9, 1, 23));
    assert_eq!(world.vitals().local_hp_damage, 0);
    assert_eq!(world.vitals().local_hp_base, None);
    assert_eq!(world.hit_point_basis().unwrap().current(), Some(1));
    assert!(!world.is_unconscious());
    // Neither depth of local damage nor a displayed zero can manufacture death.
    for _ in 0..20 {
        game(&mut world, report.clone());
    }
    assert!(!world.is_dead());
}

#[test]
fn repeated_submitted_damage_uses_the_previous_clamped_base_without_changing_the_report() {
    let mut world = admitted();
    let mut item = chest();
    item.details.bonuses.as_mut().unwrap().hit_points = -20;
    game(&mut world, inventory(vec![item]));
    game(&mut world, hit_points(9, 10_000_010, 10_000_020));
    for (damage, expected) in [(1, 9_999_980), (2, 9_999_979)] {
        game(
            &mut world,
            WorldEvent::HazardReported {
                session_id: 1,
                hazard: crate::hazards::Hazard::Drowning,
                amount: 1,
                record: false,
            },
        );
        assert_eq!(world.vitals().reported_hp.unwrap().current, 10_000_010);
        assert_eq!(world.counted_hit_points(), Some(9_999_990));
        assert_eq!(world.vitals().local_hp_damage, damage);
        assert_eq!(world.hit_point_basis().unwrap().current(), Some(expected));
        assert_eq!(
            world.hit_points(),
            Some((u32::try_from(expected).unwrap(), 10_000_000))
        );
        assert!(!world.is_unconscious());
        assert!(!world.is_dead());
    }
    assert_eq!(world.vitals().local_hp_base, Some(9_999_999));
}

#[test]
fn unresolved_equipped_hp_keeps_control_unknown_despite_a_zero_display() {
    for augmented in [false, true] {
        let mut world = admitted();
        let mut item = chest();
        if augmented {
            item.augments[0] = Some(42);
        } else {
            item.details.bonuses = None;
        }
        game(&mut world, inventory(vec![item]));
        game(&mut world, hit_points(9, -5, 100));
        assert_eq!(world.hit_points(), Some((0, 100)));
        assert_eq!(world.counted_hit_points(), None);
        assert_eq!(world.hit_point_basis(), None);
        assert!(!world.is_unconscious());
        assert!(!world.is_dead());
    }
}

#[test]
fn a_nonpositive_reconstructed_maximum_withholds_the_control_basis() {
    let mut world = admitted();
    game(&mut world, inventory(Vec::new()));
    game(&mut world, hit_points(9, 1, 0));
    assert_eq!(world.hit_point_basis(), None);
    assert_eq!(world.hit_point_gauge(), None);
    assert!(!world.is_unconscious());
    assert!(!world.is_dead());
}
