//! Raw received duration records, separate from local elapsed estimates.
use eq_client_core::buffs::BuffTracker;
use serde_json::{Value, json};
use std::time::Instant;

pub(super) fn slots(tracker: &BuffTracker, now: Instant) -> Value {
    json!(tracker.slots().map(|slots| slots
        .iter()
        .map(|(slot, buff)| {
            let duration = tracker.received_duration(*slot, now);
            json!({
                "slot": slot,
                "spell_id": buff.spell_id,
                "duration_ticks": buff.duration_ticks,
                "duration_kind": if buff.duration_ticks < 0 { "negative_special" } else { "finite" },
                "observation_age_ms": duration.map(|duration| duration.observation_age.as_millis()),
                "estimated_remaining_ms": duration.and_then(|duration| duration.estimated_remaining)
                    .map(|remaining| remaining.as_millis()),
            })
        })
        .collect::<Vec<_>>()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::{
        Buff, BuffUpdate, SpellEffect,
        buffs::{Applies, Band, Landing},
    };
    use std::{collections::BTreeMap, time::Duration};

    fn buff(spell_id: u32, duration_ticks: i32) -> Buff {
        Buff {
            spell_id,
            caster_level: 1,
            effect_type: 2,
            bard_modifier: 10,
            duration_ticks,
            counters: 0,
            caster_id: 7,
        }
    }

    #[test]
    fn report_preserves_unknown_empty_signed_ticks_and_local_elapsed_provenance() {
        let start = Instant::now();
        let mut tracker = BuffTracker::default();
        assert_eq!(slots(&tracker, start), Value::Null);
        tracker.replace_snapshot(BTreeMap::new(), 25, start);
        assert_eq!(slots(&tracker, start), json!([]));
        tracker.replace_snapshot(
            BTreeMap::from([(3, buff(86, 10)), (4, buff(42, 0)), (5, buff(43, -2))]),
            25,
            start,
        );
        let report = slots(&tracker, start + Duration::from_secs(15));
        assert_eq!(
            report,
            json!([
                {"slot":3,"spell_id":86,"duration_ticks":10,"duration_kind":"finite",
                    "observation_age_ms":15000,"estimated_remaining_ms":45000},
                {"slot":4,"spell_id":42,"duration_ticks":0,"duration_kind":"finite",
                    "observation_age_ms":15000,"estimated_remaining_ms":0},
                {"slot":5,"spell_id":43,"duration_ticks":-2,"duration_kind":"negative_special",
                    "observation_age_ms":15000,"estimated_remaining_ms":null},
            ])
        );
        let later = slots(&tracker, start + Duration::from_secs(30));
        assert_eq!(later[0]["duration_ticks"], 10);
        assert_eq!(later[0]["observation_age_ms"], 30000);
        assert_eq!(later[0]["estimated_remaining_ms"], 30000);
    }

    #[test]
    fn report_omits_landings_until_an_actual_received_slot_write() {
        let start = Instant::now();
        let mut tracker = BuffTracker::empty_snapshot();
        tracker.observe_effect(
            SpellEffect {
                spell_id: 86,
                caster_level: 1,
                effect_flag: 4,
                instrument_modifier: 10,
                target_id: 7,
                caster_id: 7,
                spell_level: 1,
                push: None,
            },
            Landing {
                finder: eq_client_core::GameDialect::Titanium.buff_placement(),
                band: Some(Band {
                    beneficial: true,
                    short: false,
                    last: false,
                }),
                record_type: Some(2),
                caster_level: Some(1),
                ..Landing::default()
            },
            start,
        );
        assert_eq!(tracker.slot_of(86), Some(0));
        assert_eq!(slots(&tracker, start), json!([]));
        tracker.observe_effect(
            SpellEffect {
                spell_id: 99,
                caster_level: 1,
                effect_flag: 4,
                instrument_modifier: 10,
                target_id: 7,
                caster_id: 7,
                spell_level: 1,
                push: None,
            },
            Landing::default(),
            start,
        );
        assert_eq!(tracker.slot_of(99), None);
        assert!(tracker.effects().contains_key(&99));
        assert_eq!(slots(&tracker, start), json!([]));
        tracker.apply(
            BuffUpdate {
                spell_id: 86,
                entity_id: 7,
                slot: 0,
                applies: Applies::BySpell,
                record_type: 2,
                buff: Some(buff(86, 10)),
            },
            start + Duration::from_secs(5),
        );
        assert_eq!(slots(&tracker, start + Duration::from_secs(6)), json!([]));
        tracker.apply(
            BuffUpdate {
                spell_id: 86,
                entity_id: 7,
                slot: 0,
                applies: Applies::BySlot,
                record_type: 2,
                buff: Some(buff(86, 10)),
            },
            start + Duration::from_secs(10),
        );
        let report = slots(&tracker, start + Duration::from_secs(15));
        assert_eq!(report[0]["duration_ticks"], 10);
        assert_eq!(report[0]["observation_age_ms"], 5000);
        assert_eq!(report[0]["estimated_remaining_ms"], 55000);
    }
}
