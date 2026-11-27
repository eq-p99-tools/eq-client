//! Script acts keep calibrated movement while a text field owns native keys.
use super::*;

#[test]
fn script_walk_6721_text_fields_keep_forward_proposals_without_reading_their_keys() {
    for field in ["chat", "map", "spell", "quantity", "report", "script"] {
        let mut scene = Scene::with_speed(0, true, 6.);
        let route = eq_client_core::movement::Route::new(Vec3::ZERO, Vec3::Z * 100., 1., 6.);
        scene.app.insert_resource(
            crate::script::Script::new(vec![crate::script::Step::WalkPlace {
                destination: crate::script::PlaceDestination::Coordinates([0., 100., 0.]),
                range: 1.,
                duration: Duration::from_secs(60),
            }])
            .holding_for_test(vec![Act::Forward], None)
            .routing_for_test(route),
        );
        {
            let mut typing = scene.app.world_mut().resource_mut::<crate::keys::Typing>();
            match field {
                "chat" => typing.composing = true,
                "map" => typing.map_search = true,
                "spell" => typing.spell_search = true,
                "quantity" => typing.counting = true,
                "report" => typing.reporting = true,
                "script" => typing.editing_script = true,
                _ => unreachable!(),
            }
        }
        // A text field's raw Back key cannot reverse the authored forward act.
        latency_tests::key(&mut scene.app, KeyCode::ArrowDown, true);
        let start = scene.owner.position();
        for _ in 0..120 {
            scene.step(false);
        }
        let end = scene.owner.position();
        assert!(
            end.y - start.y > 1.,
            "{field}: script movement was lost: {start:?} -> {end:?}"
        );
        assert_eq!(scene.refusals, 0);
        assert!(
            scene
                .submissions
                .iter()
                .any(|entry| entry.accepted.position.y > start.y)
        );
    }
}
