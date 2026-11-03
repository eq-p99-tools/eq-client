use super::*;
use eq_client_core::spell_tables::{Assignment, Context};

fn id(value: u16) -> SpellId {
    SpellId::new(value).unwrap()
}

fn definitions() -> Definitions {
    let text = (1..=4)
        .map(|id| {
            let mut fields = vec!["0".to_owned(); 183];
            fields[0] = id.to_string();
            fields[1] = format!("Destination {id}");
            fields[8] = format!("Fade {id}");
            fields[19] = (id * 10).to_string();
            fields[83] = (id % 2).to_string();
            fields[144] = (id * 3).to_string();
            fields.join("^")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Definitions::parse(&text, &eq_client_assets::spells::Layout::TITANIUM)
}

fn provider() -> Provider {
    let mut provider = Provider::new(Binding::new(Policy::Project1999), definitions());
    fact(
        &mut provider,
        Fact::RunStarted {
            run_id: 7,
            attempt: 1,
        },
    );
    provider
}

fn fact(provider: &mut Provider, fact: Fact) {
    provider.receive(&provider.binding.envelope(fact));
}

fn table(provider: &mut Provider, stage: Stage, table: Table) {
    let event = Event::new(
        Context::new(provider.run_id.unwrap(), provider.revision + 1, stage),
        Update::Table(table),
    );
    fact(provider, Fact::Table(event));
}

fn register(provider: &mut Provider, ids: &[u16]) {
    table(
        provider,
        Stage::World,
        Table::Registration(ids.iter().copied().map(id).collect()),
    );
}

fn pending(provider: &mut Provider) {
    let run_id = provider.run_id.unwrap();
    fact(provider, Fact::ZonePending { run_id });
}

fn replay(provider: &mut Provider, zone: u64, pairs: &[(u16, u16)]) {
    table(
        provider,
        Stage::Zone { session_id: zone },
        Table::Replay(
            pairs
                .iter()
                .map(|(destination, source)| Assignment::new(id(*destination), id(*source)))
                .collect(),
        ),
    );
}

#[test]
fn readiness_requires_registration_and_current_zone_replay_even_when_empty() {
    let mut provider = provider();
    assert_eq!(provider.readiness(), &Readiness::AwaitingRegistration);
    assert!(provider.get(1).is_none());
    assert_eq!(
        provider.text(1).unwrap().name.as_deref(),
        Some("Destination 1")
    );
    register(&mut provider, &[]);
    assert_eq!(provider.readiness(), &Readiness::AwaitingReplay);
    pending(&mut provider);
    replay(&mut provider, 11, &[]);
    assert_eq!(provider.readiness(), &Readiness::Ready { session_id: 11 });
    assert_eq!(provider.get(1).unwrap().mana, Some(10));
    pending(&mut provider);
    assert_eq!(provider.readiness(), &Readiness::AwaitingReplay);
    fact(
        &mut provider,
        Fact::ZoneEntered {
            run_id: 7,
            session_id: 12,
        },
    );
    assert!(!provider.ready());
    replay(&mut provider, 12, &[]);
    assert!(provider.ready());
}

#[test]
fn replay_uses_immutable_sources_in_order_and_preserves_destination_strings() {
    let mut provider = provider();
    register(&mut provider, &[1, 2, 1]);
    pending(&mut provider);
    replay(&mut provider, 11, &[(1, 2), (2, 1), (3, 1), (3, 2)]);
    assert_eq!(provider.snapshots.len(), 2);
    assert_eq!(provider.get(1).unwrap().mana, Some(20));
    assert_eq!(provider.get(2).unwrap().mana, Some(10));
    let destination = provider.get(3).unwrap();
    assert_eq!(destination.mana, Some(20));
    assert_eq!(destination.icon, Some(6));
    assert_eq!(destination.beneficial, Some(false));
    assert_eq!(destination.name.as_deref(), Some("Destination 3"));
    assert_eq!(destination.fades.as_deref(), Some("Fade 3"));
    assert_eq!(provider.base.get(3).unwrap().mana, Some(30));
    replay(&mut provider, 11, &[]);
    assert_eq!(provider.get(3).unwrap().mana, Some(20));
    replay(&mut provider, 11, &[(4, 1)]);
    assert_eq!(provider.get(3).unwrap().mana, Some(20));
}

#[test]
fn first_registration_snapshots_current_effective_values_and_never_refreshes() {
    let mut provider = provider();
    register(&mut provider, &[1, 4]);
    pending(&mut provider);
    replay(&mut provider, 11, &[(2, 1), (1, 4)]);
    // First registration of 2 snapshots its CURRENT replayed value (10).
    // A duplicate 1 still refers to its original immutable snapshot (10).
    register(&mut provider, &[2, 1, 2]);
    replay(&mut provider, 11, &[(3, 2), (4, 1)]);
    assert_eq!(provider.get(3).unwrap().mana, Some(10));
    assert_eq!(provider.get(4).unwrap().mana, Some(10));
    assert_eq!(provider.get(1).unwrap().mana, Some(40));
}

#[test]
fn missing_suffix_rolls_back_registration_and_replay_whole_batches() {
    let mut provider = provider();
    register(&mut provider, &[1, 9]);
    assert!(provider.snapshots.is_empty());
    assert_eq!(
        provider.readiness(),
        &Readiness::Unavailable(Failure::SourceMissing(id(9)))
    );
    let mut provider = self::provider();
    register(&mut provider, &[1]);
    pending(&mut provider);
    replay(&mut provider, 11, &[(2, 1), (9, 1)]);
    assert!(provider.effective.is_empty());
    assert!(provider.get(2).is_none());
    assert_eq!(
        provider.readiness(),
        &Readiness::Unavailable(Failure::DestinationMissing(id(9)))
    );
    let mut provider = self::provider();
    register(&mut provider, &[1]);
    pending(&mut provider);
    replay(&mut provider, 11, &[(2, 1), (3, 4)]);
    assert!(provider.effective.is_empty());
    assert_eq!(
        provider.readiness(),
        &Readiness::Unavailable(Failure::SourceMissing(id(4)))
    );
}

#[test]
fn malformed_failure_is_sticky_across_later_empty_facts_and_successor_runs() {
    let mut provider = provider();
    register(&mut provider, &[1]);
    pending(&mut provider);
    replay(&mut provider, 11, &[(2, 1)]);
    fact(
        &mut provider,
        Fact::Table(Event::new(
            Context::new(7, 3, Stage::Zone { session_id: 11 }),
            Update::Unavailable {
                kind: eq_network_game::spell_tables::Kind::Replay,
                reason: eq_network_game::spell_tables::Unavailable::Malformed(
                    eq_network_game::spell_tables::DecodeError::Length {
                        expected: 12,
                        actual: 10,
                    },
                ),
            },
        )),
    );
    register(&mut provider, &[]);
    replay(&mut provider, 11, &[]);
    fact(
        &mut provider,
        Fact::RunStarted {
            run_id: 8,
            attempt: 2,
        },
    );
    register(&mut provider, &[1]);
    pending(&mut provider);
    replay(&mut provider, 12, &[]);
    assert_eq!(
        provider.readiness(),
        &Readiness::Unavailable(Failure::Network)
    );
    assert_eq!(provider.effective.get(&2).unwrap().mana, Some(10));
    assert!(provider.get(2).is_none());
}

#[test]
fn same_provider_reconnect_preserves_cache_but_requires_a_real_successor_start() {
    let mut provider = provider();
    register(&mut provider, &[1]);
    pending(&mut provider);
    replay(&mut provider, 11, &[(2, 1)]);
    let old_start = provider.binding.envelope(Fact::RunStarted {
        run_id: 7,
        attempt: 1,
    });
    fact(
        &mut provider,
        Fact::RunStarted {
            run_id: 8,
            attempt: 2,
        },
    );
    provider.receive(&old_start);
    assert_eq!(provider.run_id, Some(8));
    assert_eq!(provider.revision, 0);
    assert_eq!(provider.snapshots.len(), 1);
    assert_eq!(provider.readiness(), &Readiness::AwaitingReplay);
    // An unannounced run cannot reset revision ordering or apply a table.
    fact(
        &mut provider,
        Fact::Table(Event::new(
            Context::new(9, 1, Stage::World),
            Update::Table(Table::Registration(vec![id(4)])),
        )),
    );
    assert_eq!(provider.run_id, Some(8));
    assert_eq!(provider.revision, 0);
    pending(&mut provider);
    replay(&mut provider, 12, &[(3, 1)]);
    assert_eq!(provider.get(3).unwrap().mana, Some(10));
    // Stale events/Connecting from the former run cannot undo current readiness.
    provider.receive(&old_start);
    fact(
        &mut provider,
        Fact::Table(Event::new(
            Context::new(7, 99, Stage::World),
            Update::Table(Table::Registration(vec![id(4)])),
        )),
    );
    assert_eq!(provider.readiness(), &Readiness::Ready { session_id: 12 });
    assert_eq!(provider.snapshots.len(), 1);
}

#[test]
fn provider_replacement_rejects_stale_connecting_and_table_envelopes() {
    let mut provider = provider();
    let old = provider.binding;
    register(&mut provider, &[1]);
    pending(&mut provider);
    replay(&mut provider, 11, &[(2, 1)]);
    let replacement = Binding::new(Policy::Project1999);
    provider.bind(replacement);
    provider.receive(&old.envelope(Fact::RunStarted {
        run_id: 99,
        attempt: 99,
    }));
    provider.receive(&old.envelope(Fact::Table(Event::new(
        Context::new(7, 3, Stage::World),
        Update::Table(Table::Registration(vec![id(1)])),
    ))));
    assert!(provider.run_id.is_none());
    assert!(provider.snapshots.is_empty());
    assert!(provider.effective.is_empty());
    assert_eq!(provider.readiness(), &Readiness::AwaitingRegistration);
    fact(
        &mut provider,
        Fact::RunStarted {
            run_id: 8,
            attempt: 1,
        },
    );
    register(&mut provider, &[]);
    pending(&mut provider);
    replay(&mut provider, 12, &[]);
    assert_eq!(provider.get(2).unwrap().mana, Some(20));
    provider.bind(Binding::new(Policy::Installed));
    provider.receive(&replacement.envelope(Fact::RunStarted {
        run_id: 8,
        attempt: 2,
    }));
    assert_eq!(provider.readiness(), &Readiness::Installed);
    assert_eq!(provider.get(2).unwrap().mana, Some(20));
}

#[test]
fn ordering_and_zone_freshness_cannot_be_reset_by_table_context() {
    let mut provider = provider();
    register(&mut provider, &[1]);
    pending(&mut provider);
    replay(&mut provider, 11, &[]);
    // A duplicate is ignored, while a missing revision makes readiness unavailable.
    fact(
        &mut provider,
        Fact::Table(Event::new(
            Context::new(7, 2, Stage::World),
            Update::Table(Table::Registration(vec![id(2)])),
        )),
    );
    assert_eq!(provider.snapshots.len(), 1);
    fact(
        &mut provider,
        Fact::Table(Event::new(
            Context::new(7, 4, Stage::World),
            Update::Table(Table::Registration(vec![id(2)])),
        )),
    );
    assert_eq!(
        provider.readiness(),
        &Readiness::Unavailable(Failure::Order)
    );
    let mut provider = self::provider();
    register(&mut provider, &[1]);
    pending(&mut provider);
    replay(&mut provider, 11, &[]);
    pending(&mut provider);
    replay(&mut provider, 11, &[]);
    assert_eq!(
        provider.readiness(),
        &Readiness::Unavailable(Failure::Stage)
    );
}

#[test]
fn replay_before_any_registration_is_unavailable_even_if_it_is_empty() {
    let mut provider = provider();
    pending(&mut provider);
    replay(&mut provider, 11, &[]);
    assert_eq!(
        provider.readiness(),
        &Readiness::Unavailable(Failure::RegistrationMissing)
    );
}

#[test]
fn unknown_run_identity_withdraws_ready_mechanics_without_clearing_snapshots() {
    let mut provider = provider();
    register(&mut provider, &[1]);
    pending(&mut provider);
    replay(&mut provider, 11, &[]);
    let binding = provider.binding;
    let missing = binding.envelope(Fact::RunUnavailable { attempt: 2 });
    assert!(provider.receive(&missing));
    assert!(!provider.receive(&missing));
    assert_eq!(provider.run_id, None);
    assert_eq!(provider.snapshots.len(), 1);
    assert_eq!(provider.get(1), None);
    assert_eq!(
        provider.readiness(),
        &Readiness::Unavailable(Failure::RunIdentity)
    );
    fact(
        &mut provider,
        Fact::RunStarted {
            run_id: 8,
            attempt: 3,
        },
    );
    assert_eq!(
        provider.readiness(),
        &Readiness::Unavailable(Failure::RunIdentity)
    );
}
