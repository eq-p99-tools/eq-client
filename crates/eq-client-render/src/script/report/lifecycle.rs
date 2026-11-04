//! Bounded snapshots of model facts, without inferred ownership or correlation.
use eq_client_core::{deletion::Scope, world::ClientWorld};
use serde_json::{Value, json};

const MAX_ENTRIES: usize = 16;
const MAX_NAME_BYTES: usize = 63;
const MAX_ZONE_BYTES: usize = 128;
const MAX_JSON_BYTES: usize = 8192;

#[derive(Clone, Copy)]
struct View {
    scope: Scope,
    entering: bool,
}

pub(super) fn state(online: &crate::online::OnlineState) -> String {
    let view = online.selection.as_ref().map(|selection| View {
        scope: selection.scope(),
        entering: selection.entering(),
    });
    snapshot(online.world(), view, online.enabled)
}

fn snapshot(world: &ClientWorld, view: Option<View>, online_enabled: bool) -> String {
    if let Err(reason) = validate(world) {
        return incomplete(reason).to_string();
    }
    let list = world.characters();
    let matches = list.zip(view).is_some_and(|(list, view)| {
        list.scope() == view.scope
            && !world.ended()
            && world.session_id().is_none()
            && world.player().is_none()
    });
    let selection_state = match (list.is_some(), matches, view) {
        (true, true, Some(View { entering: true, .. })) => "entering_view",
        (true, true, _) => "selection_view",
        (true, false, _) => "cached_snapshot",
        (false, _, Some(_)) => "view_without_snapshot",
        (false, _, None) => "absent",
    };
    let offered = list.map(|list| {
        let capacity = list.creation.map(eq_client_core::creation::Rules::slots);
        let free_slots = capacity.map(|capacity| {
            (0..capacity)
                .filter(|slot| {
                    !list
                        .characters
                        .iter()
                        .any(|entry| usize::from(entry.slot) == *slot)
                })
                .collect::<Vec<_>>()
        });
        let entries = list
            .characters
            .iter()
            .map(|entry| {
                json!({
                    "slot": entry.slot, "name": entry.name,
                    "level": entry.level, "zone_id": entry.zone_id,
                })
            })
            .collect::<Vec<_>>();
        json!({
            "scope": list.scope(), "entries": entries,
            "creation_rules": list.creation, "deletion_rules": list.deletion,
            "capacity": capacity, "capacity_source": capacity.map(|_| "creation_rules"),
            "free_slots": free_slots, "free_slot_count": free_slots.as_ref().map(Vec::len),
        })
    });
    // The model retains this answer across admission/camp/loss. Its counter
    // alone is not a request, list, connection or run identity.
    let creation = world.creation().map(|answer| {
        json!({
            "currency": "history_unscoped", "count": answer.count,
            "name": answer.name, "outcome": answer.outcome,
            "request_id": null, "scope": null,
        })
    });
    let player = world.player().map(|player| {
        json!({
            "currency": if world.connected() && !world.ended() && world.session_id().is_some() {
                "current_admission"
            } else { "retained_admission" },
            "admission_id": world.session_id(), "spawn_id": player.spawn_id,
            "name": player.name, "level": player.level, "race": player.race,
            "gender": player.gender, "class": player.class,
            "zone": (!world.zone().is_empty()).then_some(world.zone()),
        })
    });
    let mut report = envelope(true, None);
    report["lifecycle"] = json!({
        "online_enabled": online_enabled, "connected": world.connected(),
        "ended": world.ended(), "admission_id": world.session_id(),
        "has_player": world.player().is_some(), "in_world": world.in_world(),
        "dead": world.is_dead(), "transfer_pending": world.pending_transfer().is_some(),
        "active_world_scope": null, "last_link": null,
    });
    report["selection"] = json!({
        "state": selection_state, "snapshot": offered,
        "view": view.map(|view| json!({"scope": view.scope, "entering": view.entering})),
    });
    report["creation_answer"] = creation.unwrap_or(Value::Null);
    report["player"] = player.unwrap_or(Value::Null);
    bounded(&report)
}

fn validate(world: &ClientWorld) -> Result<(), &'static str> {
    if let Some(list) = world.characters() {
        if list.characters.len() > MAX_ENTRIES {
            return Err("entry_limit");
        }
        if list.connection_id == 0 {
            return Err("invalid_scope");
        }
        let capacity = list.creation.map(eq_client_core::creation::Rules::slots);
        if capacity.is_some_and(|capacity| capacity > MAX_ENTRIES) {
            return Err("capacity_limit");
        }
        for (index, entry) in list.characters.iter().enumerate() {
            if !name(&entry.name) {
                return Err("invalid_character_name");
            }
            if capacity.is_some_and(|capacity| usize::from(entry.slot) >= capacity) {
                return Err("capacity_mismatch");
            }
            if list.characters[..index].iter().any(|previous| {
                previous.slot == entry.slot || previous.name.eq_ignore_ascii_case(&entry.name)
            }) {
                return Err("ambiguous_selection");
            }
        }
    }
    if world.creation().is_some_and(|answer| !name(&answer.name)) {
        return Err("invalid_creation_name");
    }
    if world.player().is_some_and(|player| !name(&player.name)) {
        return Err("invalid_player_name");
    }
    if !text(world.zone(), MAX_ZONE_BYTES) {
        return Err("invalid_zone");
    }
    Ok(())
}

fn name(value: &str) -> bool {
    !value.is_empty() && text(value, MAX_NAME_BYTES)
}

fn text(value: &str, limit: usize) -> bool {
    value.len() <= limit && value.is_ascii() && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn envelope(complete: bool, reason: Option<&str>) -> Value {
    json!({
        "version": 1, "complete": complete, "incomplete_reason": reason,
        "consumer_reliance": if complete { "external_binding_required" } else { "forbidden" },
        "limits": {"entries": MAX_ENTRIES, "name_bytes": MAX_NAME_BYTES,
            "zone_bytes": MAX_ZONE_BYTES, "json_bytes": MAX_JSON_BYTES},
        "lifecycle": null, "selection": null, "creation_answer": null, "player": null,
    })
}

fn incomplete(reason: &'static str) -> Value {
    envelope(false, Some(reason))
}

fn bounded(report: &Value) -> String {
    let encoded = report.to_string();
    if encoded.len() <= MAX_JSON_BYTES {
        encoded
    } else {
        incomplete("json_limit").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::{
        CharacterChoice, WorldEvent, WorldUpdate, creation, deletion,
        world::{Link, NoSpells},
    };
    use std::time::Instant;

    fn apply(world: &mut ClientWorld, update: &WorldUpdate) {
        world.apply(update, Instant::now(), &NoSpells);
    }

    fn offered(
        world: &mut ClientWorld,
        scope: Scope,
        names: &[&str],
        creation: Option<creation::Rules>,
        deletion: Option<deletion::Rules>,
    ) {
        apply(
            world,
            &WorldUpdate::Game(WorldEvent::CharacterSelection {
                connection_id: scope.connection_id,
                selection_id: scope.selection_id,
                creation,
                deletion,
                characters: names
                    .iter()
                    .enumerate()
                    .map(|(slot, name)| CharacterChoice {
                        slot: u8::try_from(slot).unwrap(),
                        name: (*name).into(),
                        level: None,
                        zone_id: None,
                        look: None,
                    })
                    .collect(),
            }),
        );
    }

    fn scope(selection_id: u64) -> Scope {
        Scope {
            connection_id: 7,
            selection_id,
        }
    }

    fn report(world: &ClientWorld, view: Option<View>) -> Value {
        let encoded = snapshot(world, view, true);
        assert!(encoded.len() <= MAX_JSON_BYTES);
        serde_json::from_str(&encoded).unwrap()
    }

    fn creation(world: &mut ClientWorld, name: &str) {
        apply(
            world,
            &WorldUpdate::Game(WorldEvent::CharacterCreation {
                name: name.into(),
                outcome: creation::CreationOutcome::Created,
            }),
        );
    }

    fn admitted(world: &mut ClientWorld, name: &str, zone: &str) {
        let mut player = crate::online::testing::player(9);
        player.name = name.into();
        apply(
            world,
            &WorldUpdate::Game(WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 12,
                zone: zone.into(),
                player: Box::new(player),
                far_clip: None,
                spell_landing_flag: None,
            }),
        );
        apply(world, &WorldUpdate::Connection(Link::Connected));
    }

    #[test]
    fn complete_offer_preserves_exact_entries_and_source_policy() {
        for rules in [
            creation::Rules::Classic,
            creation::Rules::Titanium,
            creation::Rules::EqMac,
        ] {
            let mut world = ClientWorld::default();
            let delete = if rules == creation::Rules::EqMac {
                deletion::Rules::EqMac
            } else {
                deletion::Rules::Titanium
            };
            apply(
                &mut world,
                &WorldUpdate::Game(WorldEvent::CharacterSelection {
                    connection_id: 7,
                    selection_id: 1,
                    creation: Some(rules),
                    deletion: Some(delete),
                    characters: vec![
                        CharacterChoice {
                            slot: 3,
                            name: "Protected".into(),
                            level: Some(42),
                            zone_id: Some(22),
                            look: None,
                        },
                        CharacterChoice {
                            slot: 0,
                            name: "Disposable".into(),
                            level: None,
                            zone_id: None,
                            look: None,
                        },
                    ],
                }),
            );
            let value = report(
                &world,
                Some(View {
                    scope: scope(1),
                    entering: false,
                }),
            );
            let list = &value["selection"]["snapshot"];
            assert_eq!(value["complete"], true);
            assert_eq!(value["selection"]["state"], "selection_view");
            assert_eq!(
                list["scope"],
                json!({"connection_id": 7, "selection_id": 1})
            );
            assert_eq!(
                list["entries"],
                json!([
                    {"slot":3,"name":"Protected","level":42,"zone_id":22},
                    {"slot":0,"name":"Disposable","level":null,"zone_id":null},
                ])
            );
            assert_eq!(list["creation_rules"], serde_json::to_value(rules).unwrap());
            assert_eq!(
                list["deletion_rules"],
                serde_json::to_value(delete).unwrap()
            );
            assert_eq!(list["capacity"], rules.slots());
            assert_eq!(list["free_slots"], json!([1, 2, 4, 5, 6, 7]));
            assert_eq!(list["free_slot_count"], 6);
            assert!(value["lifecycle"]["active_world_scope"].is_null());
        }
    }

    #[test]
    fn p99_classic_creation_and_admission_do_not_invent_ownership_correlation() {
        let mut world = ClientWorld::default();
        offered(
            &mut world,
            scope(1),
            &["Protected"],
            Some(creation::Rules::Classic),
            Some(deletion::Rules::Titanium),
        );
        let baseline = report(
            &world,
            Some(View {
                scope: scope(1),
                entering: false,
            }),
        );
        assert_eq!(
            baseline["selection"]["snapshot"]["creation_rules"],
            "Classic"
        );
        assert_eq!(
            baseline["selection"]["snapshot"]["deletion_rules"],
            "Titanium"
        );
        creation(&mut world, "Disposable");
        offered(
            &mut world,
            scope(2),
            &["Protected", "Disposable"],
            Some(creation::Rules::Classic),
            Some(deletion::Rules::Titanium),
        );
        let refreshed = report(
            &world,
            Some(View {
                scope: scope(2),
                entering: false,
            }),
        );
        assert_eq!(
            refreshed["selection"]["snapshot"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(refreshed["creation_answer"]["scope"].is_null());
        admitted(&mut world, "Disposable", "qeynos");
        let entered = report(&world, None);
        assert_eq!(entered["player"]["name"], "Disposable");
        assert_eq!(entered["player"]["level"], 1);
        assert!(entered["creation_answer"]["request_id"].is_null());
        assert!(entered["selection"]["snapshot"].is_null());
        assert_eq!(entered["consumer_reliance"], "external_binding_required");
    }

    #[test]
    fn source_zero_and_escaped_text_are_preserved_without_invented_defaults() {
        let mut world = ClientWorld::default();
        let name = "Quoted\\Name\"";
        apply(
            &mut world,
            &WorldUpdate::Game(WorldEvent::CharacterSelection {
                connection_id: 7,
                selection_id: 0,
                creation: None,
                deletion: None,
                characters: vec![CharacterChoice {
                    slot: 0,
                    name: name.into(),
                    level: Some(0),
                    zone_id: Some(0),
                    look: None,
                }],
            }),
        );
        let value = report(&world, None);
        let snapshot = &value["selection"]["snapshot"];
        assert_eq!(value["complete"], true);
        assert_eq!(snapshot["scope"]["selection_id"], 0);
        assert_eq!(snapshot["entries"][0]["name"], name);
        assert_eq!(snapshot["entries"][0]["level"], 0);
        assert_eq!(snapshot["entries"][0]["zone_id"], 0);
    }

    #[test]
    fn empty_unsupported_and_absent_lists_are_different() {
        let mut world = ClientWorld::default();
        let absent = report(&world, None);
        assert!(absent["selection"]["snapshot"].is_null());
        offered(
            &mut world,
            scope(1),
            &[],
            Some(creation::Rules::Titanium),
            Some(deletion::Rules::Titanium),
        );
        let empty = report(&world, None);
        assert_eq!(empty["selection"]["snapshot"]["entries"], json!([]));
        assert_eq!(empty["selection"]["snapshot"]["free_slot_count"], 8);
        offered(&mut world, scope(2), &["Protected"], None, None);
        let unsupported = report(&world, None);
        let list = &unsupported["selection"]["snapshot"];
        assert_eq!(unsupported["complete"], true);
        assert_eq!(list["entries"].as_array().unwrap().len(), 1);
        for field in [
            "capacity",
            "free_slots",
            "free_slot_count",
            "creation_rules",
            "deletion_rules",
        ] {
            assert!(list[field].is_null(), "{field}");
        }
    }

    #[test]
    fn view_cache_entry_loss_and_restart_preserve_currency_limits() {
        let mut world = ClientWorld::default();
        offered(&mut world, scope(1), &["Protected"], None, None);
        let view = View {
            scope: scope(1),
            entering: false,
        };
        assert_eq!(
            report(&world, Some(view))["selection"]["state"],
            "selection_view"
        );
        assert_eq!(
            report(&world, None)["selection"]["state"],
            "cached_snapshot"
        );
        let entering = View {
            entering: true,
            ..view
        };
        assert_eq!(
            report(&world, Some(entering))["selection"]["state"],
            "entering_view"
        );
        // A generic progress/reset is not evidence of a new live world socket.
        apply(&mut world, &WorldUpdate::Connection(Link::LoggingIn));
        let retained = report(&world, Some(view));
        assert_eq!(
            retained["selection"]["snapshot"]["scope"]["selection_id"],
            1
        );
        assert!(retained["lifecycle"]["active_world_scope"].is_null());
        offered(&mut world, scope(2), &["Protected"], None, None);
        assert_eq!(
            report(&world, Some(view))["selection"]["state"],
            "cached_snapshot"
        );
        apply(&mut world, &WorldUpdate::Connection(Link::Ended));
        assert!(report(&world, Some(view))["selection"]["snapshot"].is_null());
        assert_eq!(
            report(&world, Some(view))["selection"]["state"],
            "view_without_snapshot"
        );
        world.restart();
        assert_eq!(report(&world, None)["selection"]["state"], "absent");
    }

    #[test]
    fn creation_answers_remain_uncorrelated_history_until_restart() {
        let mut world = ClientWorld::default();
        assert!(report(&world, None)["creation_answer"].is_null());
        creation(&mut world, "Disposable");
        creation(&mut world, "Disposable");
        admitted(&mut world, "Disposable", "qeynos");
        apply(
            &mut world,
            &WorldUpdate::Game(WorldEvent::Camp(eq_client_core::CampStatus::Camped)),
        );
        apply(&mut world, &WorldUpdate::Connection(Link::Ended));
        apply(&mut world, &WorldUpdate::Connection(Link::LoggingIn));
        offered(
            &mut world,
            Scope {
                connection_id: 8,
                selection_id: 1,
            },
            &["Disposable"],
            None,
            None,
        );
        let answer = &report(&world, None)["creation_answer"];
        assert_eq!(answer["count"], 2);
        assert_eq!(answer["name"], "Disposable");
        assert_eq!(answer["outcome"], "Created");
        assert_eq!(answer["currency"], "history_unscoped");
        assert!(answer["request_id"].is_null());
        assert!(answer["scope"].is_null());
        world.restart();
        assert!(report(&world, None)["creation_answer"].is_null());
    }

    #[test]
    fn admitted_and_retained_player_identity_are_explicit() {
        let mut world = ClientWorld::default();
        offered(&mut world, scope(1), &["Disposable"], None, None);
        admitted(&mut world, "Disposable", "qeynos");
        let value = report(&world, None);
        assert_eq!(value["lifecycle"]["admission_id"], 12);
        assert_eq!(value["lifecycle"]["has_player"], true);
        assert_eq!(value["lifecycle"]["in_world"], true);
        assert!(value["selection"]["snapshot"].is_null());
        assert_eq!(
            value["player"],
            json!({"currency":"current_admission","admission_id":12,"spawn_id":9,
            "name":"Disposable","level":1,"race":1,"gender":0,"class":1,"zone":"qeynos"})
        );
        apply(&mut world, &WorldUpdate::Connection(Link::Zoning));
        let value = report(&world, None);
        assert_eq!(value["lifecycle"]["has_player"], true);
        assert_eq!(value["lifecycle"]["connected"], false);
        assert_eq!(value["player"]["currency"], "retained_admission");
        apply(&mut world, &WorldUpdate::Connection(Link::Ended));
        let ended = report(&world, None);
        assert_eq!(ended["lifecycle"]["ended"], true);
        assert_eq!(ended["player"]["currency"], "retained_admission");
        apply(
            &mut world,
            &WorldUpdate::Game(WorldEvent::Camp(eq_client_core::CampStatus::Camped)),
        );
        assert!(report(&world, None)["player"].is_null());
    }

    #[test]
    fn malformed_or_oversized_text_forbids_reliance_without_echoing_it() {
        for invalid in [
            String::new(),
            "bad\nname".into(),
            "bad\0name".into(),
            "é".into(),
            "x".repeat(65536),
        ] {
            let mut world = ClientWorld::default();
            offered(&mut world, scope(1), &[&invalid], None, None);
            let value = report(&world, None);
            assert_eq!(value["complete"], false);
            assert_eq!(value["consumer_reliance"], "forbidden");
            assert_eq!(value["incomplete_reason"], "invalid_character_name");
            assert!(value["selection"].is_null());
        }
        let mut world = ClientWorld::default();
        creation(&mut world, "x\u{1b}name");
        assert_eq!(
            report(&world, None)["incomplete_reason"],
            "invalid_creation_name"
        );
        let mut world = ClientWorld::default();
        admitted(&mut world, "bad\nname", "qeynos");
        assert_eq!(
            report(&world, None)["incomplete_reason"],
            "invalid_player_name"
        );
        let mut world = ClientWorld::default();
        admitted(&mut world, "Disposable", &"x".repeat(MAX_ZONE_BYTES + 1));
        assert_eq!(report(&world, None)["incomplete_reason"], "invalid_zone");
    }

    #[test]
    fn excess_or_ambiguous_entries_are_not_truncated_into_complete_evidence() {
        let mut world = ClientWorld::default();
        let names = (0..=MAX_ENTRIES)
            .map(|index| format!("Fixture{index}"))
            .collect::<Vec<_>>();
        let names = names.iter().map(String::as_str).collect::<Vec<_>>();
        offered(&mut world, scope(1), &names, None, None);
        assert_eq!(report(&world, None)["incomplete_reason"], "entry_limit");
        offered(
            &mut world,
            scope(2),
            &["Protected", "protected"],
            None,
            None,
        );
        assert_eq!(
            report(&world, None)["incomplete_reason"],
            "ambiguous_selection"
        );
        offered(
            &mut world,
            scope(3),
            &names[..9],
            Some(creation::Rules::Titanium),
            None,
        );
        assert_eq!(
            report(&world, None)["incomplete_reason"],
            "capacity_mismatch"
        );
    }

    #[test]
    fn serialized_byte_limit_rejects_a_whole_envelope() {
        let mut value = envelope(true, None);
        value["player"] = json!({"name": "x".repeat(MAX_JSON_BYTES)});
        let encoded = bounded(&value);
        assert!(encoded.len() <= MAX_JSON_BYTES);
        let value: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(value["complete"], false);
        assert_eq!(value["consumer_reliance"], "forbidden");
        assert_eq!(value["incomplete_reason"], "json_limit");
        assert!(value["player"].is_null());
    }
}
