//! The received AA facts in the ordinary script report (checklist 6.582):
//! numbers only, never names or installed text, so that a script can read
//! what a live row checks. Each list gives at most [`MAX_LISTED`] entries
//! and its full count.
use eq_client_core::alternate_advancement::State;
use serde_json::{Value, json};

/// The most entries a list gives; its count says how many there are.
const MAX_LISTED: usize = 64;

/// A list's count and its first [`MAX_LISTED`] entries.
fn bounded(entries: impl Iterator<Item = Value>) -> Value {
    let mut listed = Vec::new();
    let mut count = 0_usize;
    for entry in entries {
        if listed.len() < MAX_LISTED {
            listed.push(entry);
        }
        count += 1;
    }
    json!({ "count": count, "listed": listed })
}

/// A bounded snapshot of this admission's AA facts, both generations'.
pub(super) fn state(facts: &State) -> Value {
    let titanium = json!({
        "definitions": facts.definition_count(),
        "selections": facts.selections().map(|selections| bounded(
            selections.iter().map(|selected| json!(selected.id)),
        )),
        "trainable": bounded(
            facts.trainable().map(|rank| json!({ "id": rank.id, "cost": rank.cost })),
        ),
        "activatable": bounded(facts.activatable().map(|rank| json!(rank.id))),
        "points": facts.points().map(|points| json!({
            "aapoints_spent": points.aapoints_spent,
            "aapoints": points.aapoints,
            "exp_aa": points.exp_aa,
        })),
        "progress": facts.progress().map(|progress| json!({
            "experience": progress.experience,
            "unspent": progress.unspent,
            "percentage": progress.percentage,
        })),
        "unspent": facts.unspent(),
        "timers": bounded(facts.timers().map(|timer| json!({
            "ability": timer.ability,
            "begin": timer.begin,
            "end": timer.end,
        }))),
    });
    let mac = json!({
        "ranks": facts.mac_ranks().map(|ranks| json!({
            "spent": ranks.spent,
            "ranks": bounded(ranks.ranks.iter().map(|rank| json!({
                "id": rank.id,
                "value": rank.value,
            }))),
        })),
        "selections": facts.mac_selections().map(|selections| bounded(
            selections.iter().map(|rank| json!({ "id": rank.id, "value": rank.value })),
        )),
        "points": facts.mac_points().map(|points| json!({
            "aapoints": points.aapoints,
            "exp_aa": points.exp_aa,
            "per_aa": points.per_aa,
        })),
        "progress": facts.mac_progress().map(|progress| json!({
            "experience": progress.experience,
            "unspent": progress.unspent,
            "percentage": progress.percentage,
        })),
        "timers": bounded(facts.mac_timers().map(|timer| json!({
            "ability": timer.ability,
            "begin": timer.begin,
            "end": timer.end,
        }))),
    });
    json!({ "titanium": titanium, "mac": mac })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::alternate_advancement::{
        MacPoints, MacProgress, MacRank, MacRanks, MacTimer, Points, Progress, Rank, Selection,
        Timer, Update,
    };

    fn rank(id: u32, last_id: i32, cost: u32, spellid: i32) -> Update {
        Update::Definition {
            id,
            rank: Some(Rank {
                id,
                required_level: 51,
                classes: 1 << 12,
                sequence: id,
                rank: 1,
                hotkey_sid: 0,
                hotkey_sid2: 0,
                title_sid: 0,
                desc_sid: 0,
                cost,
                prereq_skill: 0,
                prereq_minpoints: 0,
                kind: 0,
                spellid,
                spell_type: 0,
                spell_refresh: 0,
                max_level: 0,
                last_id,
                next_id: -1,
                cost2: 0,
                effects: Vec::new(),
            }),
        }
    }

    #[test]
    fn an_unknown_report_is_empty_and_never_zeros() {
        let report = state(&State::default());
        let empty = json!({ "count": 0, "listed": [] });
        assert_eq!(
            report,
            json!({
                "titanium": {
                    "definitions": 0,
                    "selections": null,
                    "trainable": empty,
                    "activatable": empty,
                    "points": null,
                    "progress": null,
                    "unspent": null,
                    "timers": empty,
                },
                "mac": {
                    "ranks": null,
                    "selections": null,
                    "points": null,
                    "progress": null,
                    "timers": empty,
                },
            })
        );
    }

    #[test]
    fn titanium_facts_report_owned_trainable_activatable_points_and_timers() {
        let mut facts = State::default();
        for update in [
            // Rank 10 is owned (11 follows it) and has an ability; 20 is
            // offered and unowned; 21 is owned by exact id with none.
            Update::Selections(Some(vec![
                Selection { id: 10, value: 1 },
                Selection { id: 21, value: 1 },
            ])),
            rank(10, -1, 1, 2_000),
            rank(11, 10, 3, 2_000),
            rank(20, -1, 2, -1),
            rank(21, -1, 1, -1),
            Update::Points(Some(Points {
                aapoints_spent: 4,
                aapoints: 7,
                exp_aa: 100,
            })),
            Update::Progress(Some(Progress {
                experience: 250,
                unspent: 6,
                percentage: 100,
            })),
            Update::Timer(Timer {
                begin: 5,
                ability: 3,
                end: 905,
            }),
        ] {
            facts.apply(&update);
        }
        let titanium = &state(&facts)["titanium"];
        assert_eq!(titanium["definitions"], 4);
        assert_eq!(
            titanium["selections"],
            json!({ "count": 2, "listed": [10, 21] })
        );
        assert_eq!(
            titanium["trainable"],
            json!({ "count": 2, "listed": [{ "id": 11, "cost": 3 }, { "id": 20, "cost": 2 }] })
        );
        assert_eq!(
            titanium["activatable"],
            json!({ "count": 1, "listed": [10] })
        );
        assert_eq!(
            titanium["points"],
            json!({ "aapoints_spent": 4, "aapoints": 7, "exp_aa": 100 })
        );
        assert_eq!(
            titanium["progress"],
            json!({ "experience": 250, "unspent": 6, "percentage": 100 })
        );
        // The latest of the two, as the session reads it.
        assert_eq!(titanium["unspent"], 6);
        assert_eq!(
            titanium["timers"],
            json!({ "count": 1, "listed": [{ "ability": 3, "begin": 5, "end": 905 }] })
        );
    }

    #[test]
    fn mac_facts_report_ranks_points_progress_and_timers_as_sent() {
        let mut facts = State::default();
        for update in [
            Update::MacRanks(Some(MacRanks {
                spent: 9,
                ranks: vec![MacRank { id: 211, value: 3 }],
            })),
            // TAKP's profile value is the rank times 16, kept as sent.
            Update::MacSelections(Some(vec![MacRank { id: 12, value: 32 }])),
            Update::MacPoints(Some(MacPoints {
                aapoints: 5,
                exp_aa: 40,
                per_aa: 50,
            })),
            Update::MacProgress(Some(MacProgress {
                experience: 41,
                unspent: 5,
                percentage: 50,
            })),
            Update::MacTimer(MacTimer {
                begin: 1,
                ability: 12,
                end: 601,
            }),
        ] {
            facts.apply(&update);
        }
        let mac = &state(&facts)["mac"];
        assert_eq!(
            mac["ranks"],
            json!({ "spent": 9, "ranks": { "count": 1, "listed": [{ "id": 211, "value": 3 }] } })
        );
        assert_eq!(
            mac["selections"],
            json!({ "count": 1, "listed": [{ "id": 12, "value": 32 }] })
        );
        assert_eq!(
            mac["points"],
            json!({ "aapoints": 5, "exp_aa": 40, "per_aa": 50 })
        );
        assert_eq!(
            mac["progress"],
            json!({ "experience": 41, "unspent": 5, "percentage": 50 })
        );
        assert_eq!(
            mac["timers"],
            json!({ "count": 1, "listed": [{ "ability": 12, "begin": 1, "end": 601 }] })
        );
    }

    #[test]
    fn a_long_list_gives_its_first_sixty_four_and_its_count() {
        let mut facts = State::default();
        facts.apply(&Update::Selections(Some(Vec::new())));
        for id in 1..=100 {
            facts.apply(&rank(id, -1, 1, -1));
        }
        let trainable = &state(&facts)["titanium"]["trainable"];
        assert_eq!(trainable["count"], 100);
        let listed = trainable["listed"].as_array().unwrap();
        assert_eq!(listed.len(), MAX_LISTED);
        assert_eq!(listed[63]["id"], 64);
    }
}
