use super::*;
use crate::{
    CampStatus, Coins, SpawnKind, SpellUpdate, WorldPosition, ZoneRejection,
    combat::ConColor,
    doors::{Door, DoorUpdate},
    ground::ObjectUpdate,
    outfit::Appearance,
};

fn player(spawn_id: u16) -> PlayerState {
    PlayerState {
        name: "Example".into(),
        base_attributes: None,
        deity: None,
        class: Some(1),
        spawn_id,
        race: 1,
        gender: 0,
        level: 1,
        position: WorldPosition::default(),
        mana: 0,
        endurance: Some(0),
        skills: None,
        practice_points: None,
        spell_refresh_ms: None,
        memorized_spells: [None; 8],
        size: 0.0,
        walk_speed: 0.0,
        run_speed: 0.0,
        hp_percent: Some(100),
        appearance: Appearance::default(),
        listing: crate::listing::Listing::default(),
        name_parts: crate::names::NameParts::default(),
    }
}

fn spawn(spawn_id: u16) -> SpawnState {
    SpawnState {
        class: None,
        spawn_id,
        name: "a_rat".into(),
        kind: SpawnKind::Npc,
        race: 1,
        gender: 0,
        position: WorldPosition::default(),
        velocity: [0.0; 3],
        size: 0.0,
        invisible: false,
        appearance: Appearance::default(),
        level: 0,
        listing: crate::listing::Listing::default(),
        name_parts: crate::names::NameParts::default(),
        pet_owner: None,
        hp_percent: None,
    }
}

fn door(id: u8) -> Door {
    Door {
        id,
        model: "DOOR1".into(),
        position: WorldPosition::default(),
        incline: 0,
        size: 100,
        open_type: 5,
        state_at_spawn: 0,
        invert_state: 0,
        parameter: 0,
        action: None,
    }
}

fn game(world: &mut ClientWorld, event: WorldEvent) -> Changes {
    world.apply(&WorldUpdate::Game(event), Instant::now(), &NoSpells)
}

fn connection(world: &mut ClientWorld, connected: bool, terminal: bool) -> Changes {
    world.apply(
        &WorldUpdate::Connection(match (connected, terminal) {
            (true, _) => Link::Connected,
            (false, true) => Link::Ended,
            (false, false) => Link::Entering,
        }),
        Instant::now(),
        &NoSpells,
    )
}

fn entered(session_id: u64) -> WorldEvent {
    WorldEvent::Entered {
        capabilities: Vec::new(),
        session_id,
        zone: "qeytoqrg".into(),
        player: Box::new(player(9)),
        far_clip: None,
    }
}

/// Player 9 admitted in session 1, with a rat (5), a door (3) and a ground
/// item in the zone.
fn admitted() -> ClientWorld {
    let mut world = ClientWorld::default();
    game(&mut world, entered(1));
    connection(&mut world, true, false);
    game(&mut world, WorldEvent::Spawns(vec![spawn(5)]));
    game(
        &mut world,
        WorldEvent::Doors(DoorUpdate::Spawn(vec![door(3)])),
    );
    game(
        &mut world,
        WorldEvent::Objects(ObjectUpdate::Spawn(crate::ground::GroundObject {
            drop_id: 7,
            model: "IT63_ACTORDEF".into(),
            position: WorldPosition::default(),
            object_type: 0,
        })),
    );
    world
}

fn zone_is_empty(world: &ClientWorld) -> bool {
    world.spawns().is_empty()
        && world.doors().entries().is_empty()
        && world.objects().entries().is_empty()
}

#[test]
fn a_zone_entry_keeps_what_arrives_with_it_before_the_connection() {
    let mut world = ClientWorld::default();
    let changes = game(&mut world, entered(2));
    assert!(changes.entered);
    assert_eq!(changes.reset, Some(Reset::Entered));
    game(&mut world, WorldEvent::Spawns(vec![spawn(5)]));
    game(
        &mut world,
        WorldEvent::Posture {
            spawn_id: 5,
            posture: PostureState::Sitting,
        },
    );
    game(
        &mut world,
        WorldEvent::Doors(DoorUpdate::Spawn(vec![door(3)])),
    );
    assert_eq!(world.session_id(), Some(2));
    assert_eq!(world.player().map(|player| player.spawn_id), Some(9));
    assert_eq!(world.posture(5), Some(PostureState::Sitting));
    assert!(world.doors().entries().contains_key(&3));
    assert!(!world.in_world());
    connection(&mut world, true, false);
    assert!(world.in_world());
}

#[test]
fn a_new_zone_entry_forgets_the_old_zone_and_death() {
    let mut world = admitted();
    game(
        &mut world,
        WorldEvent::Death(Death {
            spawn_id: 9,
            killer_id: 0,
            corpse_id: 10,
            bind_zone_id: 2,
            corpse_name: None,
        }),
    );
    game(&mut world, entered(2));
    assert!(world.death().is_none());
    assert!(zone_is_empty(&world));
    assert_eq!(world.session_id(), Some(2));
}

#[test]
fn camping_forgets_the_admission_and_its_zone() {
    let mut world = admitted();
    game(&mut world, WorldEvent::Mana(25));
    game(&mut world, WorldEvent::Coins(Coins::default()));
    game(
        &mut world,
        WorldEvent::Spell(SpellUpdate::Began {
            caster_id: 9,
            spell_id: 42,
            duration_ms: 3000,
        }),
    );
    assert!(world.casting().cast.is_some());
    let changes = game(&mut world, WorldEvent::Camp(CampStatus::Camped));
    assert_eq!(changes.reset, Some(Reset::Camped));
    assert!(zone_is_empty(&world));
    assert!(world.session_id().is_none());
    assert!(world.player().is_none());
    assert!(!world.connected());
    // Nothing about the departed character lingers.
    assert_eq!(world.vitals().mana, None);
    assert!(world.casting().cast.is_none());
    assert!(world.coins().is_none());
}

#[test]
fn only_the_player_dying_holds_them_and_every_death_leaves_a_corpse() {
    let mut world = admitted();
    let revision = world.spawn(5).unwrap().revision;
    let rat = Death {
        spawn_id: 5,
        killer_id: 9,
        corpse_id: 5,
        bind_zone_id: 0,
        corpse_name: Some("a_rat`s_corpse5".into()),
    };
    assert_eq!(game(&mut world, WorldEvent::Death(rat.clone())).reset, None);
    let corpse = world.spawn(5).unwrap();
    assert_eq!(corpse.state.kind, SpawnKind::NpcCorpse);
    assert_eq!(corpse.health, Some(0));
    assert!(corpse.revision > revision);
    // It takes the name the death gives its corpse, once.
    assert_eq!(corpse.state.name, "a_rat`s_corpse5");
    game(
        &mut world,
        WorldEvent::Death(Death {
            corpse_name: Some("another".into()),
            ..rat
        }),
    );
    assert_eq!(world.spawn(5).unwrap().state.name, "a_rat`s_corpse5");
    assert!(world.in_world());
    let own = Death {
        spawn_id: 9,
        killer_id: 5,
        corpse_id: 10,
        bind_zone_id: 2,
        corpse_name: None,
    };
    assert_eq!(
        game(&mut world, WorldEvent::Death(own)).reset,
        Some(Reset::Died)
    );
    assert!(!world.in_world());
    assert_eq!(world.health(9), Some(0));
    // The zone stays drawn around the corpse.
    assert!(world.spawn(5).is_some());
}

#[test]
fn a_death_in_view_names_who_died_and_who_killed_them() {
    use crate::world::Party;
    let mut world = admitted();
    let named = |spawn_id, name: &str| SpawnState {
        name: name.into(),
        ..spawn(spawn_id)
    };
    game(
        &mut world,
        WorldEvent::Spawns(vec![
            named(6, "Guard_Example01"),
            named(7, "a_gnoll02"),
            named(8, "a_bear03"),
        ]),
    );
    let death = |spawn_id, killer_id| {
        WorldEvent::Death(Death {
            spawn_id,
            killer_id,
            corpse_id: spawn_id,
            bind_zone_id: 0,
            corpse_name: None,
        })
    };
    let slain = |victim, killer| [Notice::Slain { victim, killer }];
    // Whose line it is goes by spawn ID: the player (9) slew the rat.
    assert_eq!(
        game(&mut world, death(5, 9)).notices,
        slain(Party::Named("a rat".into()), Party::Player)
    );
    // Someone slew someone else, each named as players see them.
    assert_eq!(
        game(&mut world, death(7, 6)).notices,
        slain(
            Party::Named("a gnoll".into()),
            Party::Named("Guard Example".into())
        )
    );
    // No killer (0) is no one the player can name.
    assert_eq!(
        game(&mut world, death(6, 0)).notices,
        slain(Party::Named("Guard Example".into()), Party::Unseen)
    );
    // A death of no one the player knows says nothing.
    assert_eq!(game(&mut world, death(77, 9)).notices, []);
    // The player's own death names their killer, first of what it says.
    let own = game(&mut world, death(9, 8));
    assert_eq!(
        own.notices.first(),
        Some(&Notice::Slain {
            victim: Party::Player,
            killer: Party::Named("a bear".into()),
        })
    );
}

#[test]
fn a_corpse_seen_dying_is_named_as_one_first_seen_dead() {
    // A Titanium spawn record as the server sends a corpse it names.
    let fresh = |name: &[u8], kind: u8, id: u32| {
        let mut record = [0u8; 385];
        record[7..7 + name.len()].copy_from_slice(name);
        record[83] = kind;
        record[340..344].copy_from_slice(&id.to_le_bytes());
        eq_network_game::world::titanium_spawns(&record)
            .unwrap()
            .remove(0)
    };
    let mut world = admitted();
    let mut gnoll = spawn(12);
    gnoll.name = "a_gnoll00".into();
    let mut examplar = spawn(4);
    examplar.name = "Examplar".into();
    examplar.kind = SpawnKind::Player;
    game(
        &mut world,
        WorldEvent::Spawns(vec![gnoll.clone(), examplar.clone()]),
    );
    for (living, server_name, kind) in [
        (gnoll, b"a_gnoll`s_corpse12".as_slice(), 3),
        (examplar, b"Examplar's corpse4".as_slice(), 2),
    ] {
        let id = living.spawn_id;
        // The session names the corpse by the Titanium rule.
        let corpse_name = eq_network_game::world::corpse_name(
            eq_network_game::GameDialect::Titanium,
            &living.name,
            living.kind,
            id,
        );
        game(
            &mut world,
            WorldEvent::Death(Death {
                spawn_id: u32::from(id),
                killer_id: 9,
                corpse_id: u32::from(id),
                bind_zone_id: 0,
                corpse_name,
            }),
        );
        let seen = fresh(server_name, kind, u32::from(id));
        let corpse = &world.spawn(id).unwrap().state;
        assert_eq!((&corpse.name, corpse.kind), (&seen.name, seen.kind));
    }
    // A death that names no corpse (an EqMac session's, until TAKP is
    // checked) leaves the living name.
    let mut world = admitted();
    game(&mut world, WorldEvent::Spawns(vec![spawn(5)]));
    game(
        &mut world,
        WorldEvent::Death(Death {
            spawn_id: 5,
            killer_id: 9,
            corpse_id: 5,
            bind_zone_id: 0,
            corpse_name: None,
        }),
    );
    assert_eq!(world.spawn(5).unwrap().state.name, "a_rat");
}

#[test]
fn a_transfer_holds_the_player_and_a_refusal_lets_them_go() {
    let mut world = admitted();
    let offer = ZoneOffer {
        zone_id: 2,
        instance_id: 0,
        position: WorldPosition::default(),
        reason: 0,
        to_bind: false,
        solicited: true,
    };
    let changes = game(&mut world, WorldEvent::ZoneTransfer(offer));
    assert_eq!(changes.reset, Some(Reset::Zoning { to_bind: false }));
    assert!(!world.in_world());
    let refusal = |session_id| WorldEvent::ZoneTransferRejected {
        session_id,
        reason: ZoneRejection::Cancelled,
    };
    assert!(game(&mut world, refusal(7)).ignored);
    assert!(world.pending_transfer().is_some());
    assert!(!game(&mut world, refusal(1)).ignored);
    assert!(world.in_world());
    assert!(world.spawn(5).is_some());
}

#[test]
fn a_dropped_connection_keeps_the_zone_until_the_session_ends() {
    let mut world = admitted();
    let changes = connection(&mut world, false, false);
    assert_eq!(
        changes.reset,
        Some(Reset::Lost {
            ended: false,
            transferring: false
        })
    );
    assert!(world.spawn(5).is_some());
    let changes = connection(&mut world, false, true);
    assert_eq!(
        changes.reset,
        Some(Reset::Lost {
            ended: true,
            transferring: false
        })
    );
    assert!(world.ended());
    assert!(zone_is_empty(&world));
}

#[test]
fn replies_for_an_earlier_admission_change_nothing() {
    let mut world = admitted();
    let moved = WorldPosition {
        x: 5.0,
        ..WorldPosition::default()
    };
    let sent = |session_id| WorldEvent::MotionSent {
        session_id,
        position: moved,
        refused: None,
    };
    assert!(game(&mut world, sent(2)).ignored);
    assert_eq!(world.player().unwrap().position, WorldPosition::default());
    assert!(!game(&mut world, sent(1)).ignored);
    assert_eq!(world.player().unwrap().position, moved);
    assert!(world.accepts_reply(1));
    assert!(!world.accepts_reply(2));
}

#[test]
fn the_server_placing_the_player_says_where() {
    let mut world = admitted();
    let there = WorldPosition {
        x: 1.0,
        y: 2.0,
        z: 3.0,
        heading: 0.0,
    };
    let changes = game(
        &mut world,
        WorldEvent::Position {
            spawn_id: 9,
            position: there,
            velocity: [0.0; 3],
        },
    );
    assert_eq!(changes.placed, Some(there));
    assert_eq!(world.player().unwrap().position, there);
    // Another spawn moving is not the player being placed.
    let changes = game(
        &mut world,
        WorldEvent::Position {
            spawn_id: 5,
            position: there,
            velocity: [1.0, 0.0, 0.0],
        },
    );
    assert_eq!(changes.placed, None);
    assert_eq!(
        world.spawn(5).unwrap().state.velocity.map(f32::to_bits),
        [1.0_f32, 0.0, 0.0].map(f32::to_bits)
    );
}

#[test]
fn a_spawn_sent_again_starts_over_with_a_new_revision() {
    let mut world = admitted();
    game(
        &mut world,
        WorldEvent::HealthPercent {
            spawn_id: 5,
            percent: 40,
        },
    );
    game(
        &mut world,
        WorldEvent::Posture {
            spawn_id: 5,
            posture: PostureState::Sitting,
        },
    );
    let before = world.spawn(5).unwrap().revision;
    assert_eq!(world.health(5), Some(40));
    game(&mut world, WorldEvent::Spawns(vec![spawn(5)]));
    let again = world.spawn(5).unwrap();
    assert!(again.revision > before);
    assert_eq!((again.health, again.posture), (None, None));
    game(&mut world, WorldEvent::Despawn(5));
    assert!(world.spawn(5).is_none());
}

/// Knows spell 42 as instant, and nothing else.
struct InstantFortyTwo;

impl SpellCatalog for InstantFortyTwo {
    fn timing(&self, _spell: u32) -> Option<SpellTiming> {
        None
    }

    fn instant_effect(&self, spell: u32) -> bool {
        spell == 42
    }
}

fn buff(duration_ticks: i32) -> crate::Buff {
    crate::Buff {
        spell_id: 42,
        caster_level: 1,
        effect_type: 2,
        bard_modifier: 10,
        duration_ticks,
        counters: 0,
        caster_id: 9,
    }
}

fn effect(target_id: u16, effect_flag: u8) -> crate::SpellEffect {
    crate::SpellEffect {
        target_id,
        caster_id: 9,
        caster_level: 1,
        instrument_modifier: 10,
        spell_id: 42,
        spell_level: 1,
        effect_flag,
    }
}

#[test]
fn an_instant_effect_never_removes_a_buff_the_server_slotted() {
    let mut world = admitted();
    game(&mut world, WorldEvent::BuffSnapshot(Vec::new()));
    game(
        &mut world,
        WorldEvent::Buff(crate::BuffUpdate {
            entity_id: 9,
            slot: 2,
            spell_id: 42,
            buff: Some(buff(10)),
        }),
    );
    world.apply(
        &WorldUpdate::Game(WorldEvent::SpellEffect(effect(9, 4))),
        Instant::now(),
        &InstantFortyTwo,
    );
    assert_eq!(world.buffs().slots().unwrap()[&2].duration_ticks, 10);
    assert!(world.buffs().effects().is_empty());
}

#[test]
fn only_the_players_lasting_effects_wait_for_a_slot_and_a_fade_clears_them() {
    let mut world = admitted();
    game(&mut world, WorldEvent::BuffSnapshot(Vec::new()));
    // Another spawn's effect, or one without the lasting flag, is not the player's buff.
    assert!(game(&mut world, WorldEvent::SpellEffect(effect(5, 4))).ignored);
    game(&mut world, WorldEvent::SpellEffect(effect(9, 0)));
    assert!(world.buffs().effects().is_empty());
    game(&mut world, WorldEvent::SpellEffect(effect(9, 4)));
    game(&mut world, WorldEvent::SpellEffect(effect(9, 4)));
    assert_eq!(world.buffs().effects().len(), 1);
    assert!(world.buffs().slots().unwrap().is_empty());
    game(
        &mut world,
        WorldEvent::Buff(crate::BuffUpdate {
            entity_id: 9,
            slot: 3,
            spell_id: 42,
            buff: None,
        }),
    );
    assert!(world.buffs().effects().is_empty());
}

#[test]
fn an_interruption_is_the_players_own_and_mana_cannot_undo_it() {
    let mut world = admitted();
    let now = Instant::now();
    let spell = |world: &mut ClientWorld, update| {
        world.apply(
            &WorldUpdate::Game(WorldEvent::Spell(update)),
            now,
            &NoSpells,
        )
    };
    let began = SpellUpdate::Began {
        caster_id: 9,
        spell_id: 42,
        duration_ms: 3000,
    };
    assert_eq!(spell(&mut world, began.clone()).cast, Some(CastNews::Began));
    let interrupted = |caster_id| SpellUpdate::Interrupted {
        caster_id,
        message_id: 439,
        caster_name: None,
    };
    let others = spell(&mut world, interrupted(8));
    assert_eq!((others.cast, others.notices), (None, Vec::new()));
    assert!(world.casting().cast.is_some());
    assert!(world.casting().interrupted.is_none());
    // Another caster's interruption, which the server names, says whose it
    // was, and leaves the player's cast alone.
    let named = |caster_id, caster_name: &str| SpellUpdate::Interrupted {
        caster_id,
        message_id: 444,
        caster_name: Some(caster_name.into()),
    };
    assert_eq!(
        spell(&mut world, named(8, "Examplar")).notices,
        [Notice::OtherCastInterrupted {
            string_id: 444,
            caster: "Examplar".into(),
        }]
    );
    assert!(world.casting().cast.is_some());
    // Whose it was goes by spawn ID: a name never makes it the player's,
    // and an empty one says nothing.
    assert_eq!(
        spell(&mut world, named(9, "Examplar")).notices,
        [Notice::CastInterrupted { string_id: 444 }]
    );
    assert_eq!(spell(&mut world, began.clone()).cast, Some(CastNews::Began));
    assert_eq!(spell(&mut world, named(8, "")).notices, []);
    let mana = |spell_id, keep_casting| SpellUpdate::Mana {
        spell_id,
        keep_casting,
    };
    spell(&mut world, mana(42, true));
    assert!(world.casting().cast.is_some());
    // The player's own interruption also tells them why.
    let own = spell(&mut world, interrupted(9));
    assert_eq!(own.cast, Some(CastNews::Interrupted));
    assert_eq!(own.notices, [Notice::CastInterrupted { string_id: 439 }]);
    assert!(world.casting().cast.is_none());
    assert_eq!(world.casting().interrupted, Some((now, 439)));
    spell(&mut world, mana(42, false));
    assert_eq!(world.casting().interrupted, Some((now, 439)));
    spell(&mut world, began);
    assert!(world.casting().interrupted.is_none());
    spell(&mut world, mana(99, false));
    assert!(world.casting().cast.is_some());
    assert_eq!(
        spell(&mut world, mana(42, false)).cast,
        Some(CastNews::Ended)
    );
    assert!(world.casting().cast.is_none());
}

#[test]
fn a_dead_or_disconnected_player_casts_nothing() {
    let mut world = admitted();
    connection(&mut world, false, false);
    let began = SpellUpdate::Began {
        caster_id: 9,
        spell_id: 42,
        duration_ms: 3000,
    };
    assert_eq!(game(&mut world, WorldEvent::Spell(began)).cast, None);
    assert!(world.casting().cast.is_none());
}

#[test]
fn the_profile_restores_gem_timers_and_vitals_at_admission() {
    let mut world = ClientWorld::default();
    let mut caster = player(9);
    caster.memorized_spells[0] = Some(42);
    caster.spell_refresh_ms = Some([5000, 0, 0, 0, 0, 0, 0, 0]);
    caster.mana = 30;
    let now = Instant::now();
    world.apply(
        &WorldUpdate::Game(WorldEvent::Entered {
            capabilities: Vec::new(),
            session_id: 1,
            zone: "qeytoqrg".into(),
            player: Box::new(caster),
            far_clip: None,
        }),
        now,
        &NoSpells,
    );
    assert_eq!(
        world.casting().cooldowns.remaining(42, now),
        std::time::Duration::from_secs(5)
    );
    assert_eq!(world.vitals().mana, Some(30));
    assert_eq!(world.vitals().experience, None);
}

#[test]
fn a_book_change_holds_until_confirmed_and_every_reply_counts() {
    let mut world = admitted();
    game(
        &mut world,
        WorldEvent::BookAction(crate::BookActionStatus::Preparing),
    );
    game(
        &mut world,
        WorldEvent::BookAction(crate::BookActionStatus::Preparing),
    );
    assert_eq!(world.book_action_revision(), 2);
    assert_eq!(
        world.book_action(),
        Some(&crate::BookActionStatus::Preparing)
    );
    game(
        &mut world,
        WorldEvent::BookAction(crate::BookActionStatus::Confirmed),
    );
    assert!(world.book_action().is_none());
    // A zone transfer drops a change still in flight.
    game(
        &mut world,
        WorldEvent::BookAction(crate::BookActionStatus::AwaitingReply),
    );
    game(
        &mut world,
        WorldEvent::ZoneTransfer(ZoneOffer {
            zone_id: 2,
            instance_id: 0,
            position: WorldPosition::default(),
            reason: 0,
            to_bind: false,
            solicited: true,
        }),
    );
    assert!(world.book_action().is_none());
}

/// A worn chest that gives 100 HP.
fn chest() -> crate::inventory::InventoryItem {
    use crate::inventory::{InventoryItem, InventorySlot, ItemActivation, ItemPlacement};
    InventoryItem {
        activation: ItemActivation::default(),
        scroll_spell: None,
        book: None,
        rules: ItemPlacement {
            item_type: 10,
            ..ItemPlacement::default()
        },
        slot: InventorySlot(17),
        details: crate::ItemDetails {
            equipment: Some(crate::EquipmentRules::default()),
            bonuses: Some(crate::ItemBonuses {
                hit_points: 100,
                ..crate::ItemBonuses::default()
            }),
            id: 1,
            name: "Chest".into(),
            lore: String::new(),
            weight_tenths: 10,
            slots: 1 << 17,
            classes: u32::MAX,
            races: u32::MAX,
            flags: Vec::new(),
            stats: Vec::new(),
            price: None,
            icon: None,
        },
        stack_count: None,
        charges: 0,
        bag_slots: 0,
    }
}

fn hit_points(spawn_id: u16, current: i32, maximum: i32) -> WorldEvent {
    WorldEvent::HitPoints {
        spawn_id,
        current,
        maximum,
        without_items: true,
    }
}

fn inventory(items: Vec<crate::inventory::InventoryItem>) -> WorldEvent {
    WorldEvent::Inventory(crate::inventory::InventoryUpdate::Snapshot(items))
}

#[test]
fn shown_hp_adds_back_what_equipped_items_give() {
    let mut world = admitted();
    assert!(game(&mut world, inventory(vec![chest()])).inventory);
    // Alive at 80 of 250 with a +100 HP chest, the server reports -20 of 150.
    game(&mut world, hit_points(9, -20, 150));
    assert_eq!(world.hit_points(), Some((80, 250)));
    assert_eq!(world.health(9), Some(32));
    // Taking the chest off leaves the last report short of its bonus until the
    // server reports again, as in the official client.
    game(&mut world, inventory(Vec::new()));
    assert_eq!(world.hit_points(), Some((0, 150)));
    assert_eq!(world.health(9), Some(0));
    // Another spawn's report is not the player's.
    assert!(game(&mut world, hit_points(5, 1, 2)).ignored);
    assert_eq!(world.hit_points(), Some((0, 150)));
}

#[test]
fn the_dead_show_no_hp_and_a_new_admission_forgets_the_report() {
    let mut world = admitted();
    game(&mut world, hit_points(9, 75, 150));
    game(
        &mut world,
        WorldEvent::Death(Death {
            spawn_id: 9,
            killer_id: 0,
            corpse_id: 10,
            bind_zone_id: 2,
            corpse_name: None,
        }),
    );
    assert_eq!(world.hit_points(), Some((0, 150)));
    assert_eq!(world.health(9), Some(0));
    game(&mut world, entered(2));
    assert_eq!(world.hit_points(), None);
    assert_eq!(world.vitals().reported_hp, None);
}

#[test]
fn the_inventory_lasts_until_the_admission_or_session_ends() {
    let held = |world: &ClientWorld| !world.inventory().items().is_empty();
    let mut world = admitted();
    game(&mut world, inventory(vec![chest()]));
    // Zoning, dying and a dropped connection that will come back keep it.
    game(
        &mut world,
        WorldEvent::ZoneTransfer(ZoneOffer {
            zone_id: 2,
            instance_id: 0,
            position: WorldPosition::default(),
            reason: 0,
            to_bind: false,
            solicited: true,
        }),
    );
    connection(&mut world, false, false);
    assert!(held(&world));
    // A new admission waits for the server to send it again.
    game(&mut world, entered(2));
    assert!(!held(&world));
    game(&mut world, inventory(vec![chest()]));
    connection(&mut world, false, true);
    assert!(!held(&world));
    let mut world = admitted();
    game(&mut world, inventory(vec![chest()]));
    game(&mut world, WorldEvent::Camp(CampStatus::Camped));
    assert!(!held(&world));
}

#[test]
fn the_target_follows_the_choice_and_the_sessions_word_on_it() {
    let mut world = admitted();
    world.select_target(Some(5));
    assert_eq!(world.target().selected, Some(5));
    assert!(!world.target().sent);
    // News of another choice is not this one's.
    assert!(game(&mut world, WorldEvent::TargetSent(Some(4))).ignored);
    assert!(!game(&mut world, WorldEvent::TargetSent(Some(5))).ignored);
    assert!(world.target().sent);
    let refusal = |session_id, spawn_id| WorldEvent::TargetRejected {
        session_id,
        spawn_id,
        reason: "Too far away".into(),
    };
    assert!(game(&mut world, refusal(2, Some(5))).ignored);
    assert!(game(&mut world, refusal(1, Some(4))).ignored);
    assert!(!game(&mut world, refusal(1, Some(5))).ignored);
    assert_eq!(world.target().selected, None);
}

#[test]
fn a_target_goes_stale_when_its_spawn_leaves_changes_or_hides() {
    let mut world = admitted();
    world.select_target(Some(9));
    assert!(!world.target_stale(), "the player choosing themselves");
    world.select_target(Some(5));
    assert!(!world.target_stale());
    game(
        &mut world,
        WorldEvent::Visibility {
            spawn_id: 5,
            invisible: true,
        },
    );
    assert!(world.target_stale());
    world.select_target(Some(5));
    game(&mut world, WorldEvent::Spawns(vec![spawn(5)]));
    assert!(world.target_stale(), "replaced under the same ID");
    world.select_target(Some(5));
    assert!(!world.target_stale());
    game(&mut world, WorldEvent::Despawn(5));
    assert!(world.target_stale());
}

#[test]
fn every_reset_forgets_the_target() {
    let mut world = admitted();
    world.select_target(Some(5));
    connection(&mut world, false, false);
    assert_eq!(world.target().selected, None);
    let mut world = admitted();
    world.select_target(Some(5));
    game(
        &mut world,
        WorldEvent::Death(Death {
            spawn_id: 9,
            killer_id: 0,
            corpse_id: 10,
            bind_zone_id: 2,
            corpse_name: None,
        }),
    );
    assert_eq!(world.target().selected, None);
}

#[test]
fn consider_colors_last_while_the_spawn_does() {
    let consider = |target_id| {
        WorldEvent::Consideration(crate::combat::Consideration {
            target_id,
            faction: 5,
            color: ConColor::Red,
            hit_points: None,
        })
    };
    let mut world = admitted();
    game(&mut world, consider(5));
    assert_eq!(world.considered(5), Some(ConColor::Red));
    // A spawn replacing it has not been considered.
    game(&mut world, WorldEvent::Spawns(vec![spawn(5)]));
    assert_eq!(world.considered(5), None);
    game(&mut world, consider(5));
    game(&mut world, WorldEvent::Despawn(5));
    assert_eq!(world.considered(5), None);
    game(&mut world, consider(5));
    game(&mut world, entered(2));
    assert_eq!(world.considered(5), None);
}

fn coins(platinum: u32, gold: u32, silver: u32, copper: u32) -> Coins {
    Coins {
        platinum,
        gold,
        silver,
        copper,
    }
}

#[test]
fn a_corpse_fills_while_open_and_closes_when_the_server_says() {
    use crate::loot::{LootResponse, LootUpdate};
    let mut world = admitted();
    let loot = |update| WorldEvent::Loot(update);
    // News about a corpse the player did not open is no one's.
    assert!(game(&mut world, loot(LootUpdate::Closed)).ignored);
    world.open_loot(9);
    let mut item = chest();
    item.slot = crate::inventory::InventorySlot(22);
    game(
        &mut world,
        loot(LootUpdate::Item {
            place: 0,
            item: Box::new(item),
        }),
    );
    game(&mut world, loot(LootUpdate::Listed { corpse_id: 8 }));
    assert!(!world.loot().unwrap().listed, "another corpse's listing");
    game(&mut world, loot(LootUpdate::Listed { corpse_id: 9 }));
    assert!(world.loot().unwrap().listed);
    let taken = |accepted| loot(LootUpdate::Taken { place: 0, accepted });
    game(&mut world, taken(false));
    assert!(world.loot().unwrap().items.contains_key(&0));
    game(&mut world, taken(true));
    assert!(world.loot().unwrap().items.is_empty());
    game(&mut world, loot(LootUpdate::Closed));
    assert!(world.loot().is_none());
    // A refusal closes it too.
    world.open_loot(9);
    game(
        &mut world,
        loot(LootUpdate::Opened {
            response: LootResponse::TooFar,
            coins: Coins::default(),
        }),
    );
    assert!(world.loot().is_none());
    // And the zone's end.
    world.open_loot(9);
    game(&mut world, entered(2));
    assert!(world.loot().is_none());
}

#[test]
fn the_purse_is_what_the_session_says_whatever_loot_and_purchases_suggest() {
    use crate::{
        loot::{LootResponse, LootUpdate},
        merchant::MerchantUpdate,
    };
    let mut world = admitted();
    game(&mut world, WorldEvent::Coins(coins(1, 0, 0, 0)));
    world.open_loot(9);
    world.open_shop(8);
    // The session adds loot coins and takes a price, and says so; the
    // client does no sums of its own.
    game(
        &mut world,
        WorldEvent::Loot(LootUpdate::Opened {
            response: LootResponse::Normal,
            coins: coins(0, 0, 1, 5),
        }),
    );
    game(
        &mut world,
        WorldEvent::Merchant(MerchantUpdate::Bought {
            slot: 2,
            quantity: 1,
            price: 20,
        }),
    );
    assert_eq!(world.coins(), Some(&coins(1, 0, 0, 0)));
    let changes = game(&mut world, WorldEvent::Coins(coins(0, 9, 9, 5)));
    assert!(changes.trade);
    assert_eq!(world.coins(), Some(&coins(0, 9, 9, 5)));
}

#[test]
fn a_merchant_lists_stock_until_closed_or_refusing() {
    use crate::merchant::{MerchantItem, MerchantUpdate};
    let mut world = admitted();
    let merchant = |update| WorldEvent::Merchant(update);
    assert!(game(&mut world, merchant(MerchantUpdate::Closed)).ignored);
    world.open_shop(8);
    game(
        &mut world,
        merchant(MerchantUpdate::Item(Box::new(MerchantItem {
            slot: 3,
            price: 10,
            quantity: 0,
            item: chest(),
        }))),
    );
    assert!(world.merchant().unwrap().stock.contains_key(&3));
    game(&mut world, merchant(MerchantUpdate::Removed { slot: 3 }));
    assert!(world.merchant().unwrap().stock.is_empty());
    game(
        &mut world,
        merchant(MerchantUpdate::Opened {
            merchant_id: 8,
            accepted: false,
            rate: 1.0,
        }),
    );
    assert!(world.merchant().is_none());
    world.open_shop(8);
    world.close_shop();
    assert!(world.merchant().is_none());
    // A merchant that opens keeps the server's rate.
    world.open_shop(8);
    assert_eq!(world.merchant().unwrap().rate, None);
    game(
        &mut world,
        merchant(MerchantUpdate::Opened {
            merchant_id: 8,
            accepted: true,
            rate: 1.25,
        }),
    );
    assert_eq!(world.merchant().unwrap().rate, Some(1.25));
}

#[test]
fn a_merchant_offers_what_eqemu_paid_for_each_sale() {
    use crate::world::trade::Merchant;
    // The rate a merchant opens with: 1 / (0.95 x its modifier), in f32.
    let rate = |modifier: f32| 1.0 / (0.95 * modifier);
    // What a neutral Qeynos Hills merchant paid on EQEmu: a dagger, a stack
    // of eleven bandages, a stack of seven, and a potion with charges,
    // which counts as one.
    let merchant = Merchant {
        merchant_id: 8,
        stock: std::collections::BTreeMap::new(),
        rate: Some(rate(1.0)),
    };
    assert_eq!(merchant.rate, Some(1.052_631_6));
    let item = |price, stack_count| {
        let mut item = chest();
        item.details.price = Some(price);
        item.stack_count = stack_count;
        item
    };
    // A half copper goes to the merchant: 28.5 pays 28 and 522.5 pays 522.
    assert_eq!(merchant.offer(&item(30, None)), Some(28));
    assert_eq!(merchant.offer(&item(50, Some(11))), Some(522));
    assert_eq!(merchant.offer(&item(50, Some(7))), Some(332));
    assert_eq!(merchant.offer(&item(3500, None)), Some(3325));
    // Each product is cut on its own: a merchant whose modifier is 0.96
    // pays 26 for a 30c item, where one factor of 0.912 would give 27.
    let wary = Merchant {
        rate: Some(rate(0.96)),
        ..merchant.clone()
    };
    assert_eq!(wary.offer(&item(30, None)), Some(26));
    // Without the item's price or the merchant's rate, there is no offer.
    let mut unpriced = item(30, None);
    unpriced.details.price = None;
    assert_eq!(merchant.offer(&unpriced), None);
    let unrated = Merchant {
        rate: None,
        ..merchant.clone()
    };
    assert_eq!(unrated.offer(&item(30, None)), None);
    let broken = Merchant {
        rate: Some(0.0),
        ..merchant
    };
    assert_eq!(broken.offer(&item(30, None)), None);
}

#[test]
fn another_players_trade_shows_their_side_and_both_clicks_until_anything_goes_in() {
    use crate::{exchange::ExchangeUpdate, inventory::InventorySlot};
    let mut world = admitted();
    let exchange = |update| WorldEvent::Exchange(update);
    let clicks = |world: &ClientWorld| {
        let open = world.exchange().unwrap();
        (open.given, open.partner_accepted)
    };
    let coins = |given, offered| WorldEvent::CoinsElsewhere {
        cursor: Coins::default(),
        bank: Coins::default(),
        given,
        offered,
    };
    let gold = |gold| Coins {
        gold,
        ..Coins::default()
    };
    // The session took another player's request: the window is open, and
    // nothing of the player's goes in by itself.
    let changes = game(&mut world, exchange(ExchangeUpdate::Taken { from: 9 }));
    assert!(changes.trade);
    let open = world.exchange().unwrap();
    assert_eq!(
        (open.with, open.trade_slots(), open.asker),
        (9, 8, Asker::Partner)
    );
    // Their items show by trade slot, and undo both clicks.
    game(&mut world, exchange(ExchangeUpdate::Accepted { by: 9 }));
    world.give();
    assert_eq!(clicks(&world), (true, true));
    game(
        &mut world,
        exchange(ExchangeUpdate::Offered {
            index: 2,
            item: Box::new(chest()),
        }),
    );
    assert_eq!(world.exchange().unwrap().theirs[&2].details.name, "Chest");
    assert_eq!(clicks(&world), (false, false));
    // So do the player's own coins and items going in, and theirs.
    for event in [
        coins(gold(1), Coins::default()),
        exchange(ExchangeUpdate::Coins {
            coin: crate::money::Coin::Gold,
            amount: 2,
        }),
        inventory(vec![crate::inventory::InventoryItem {
            slot: InventorySlot(3000),
            ..chest()
        }]),
    ] {
        game(&mut world, exchange(ExchangeUpdate::Accepted { by: 9 }));
        world.give();
        game(&mut world, event);
        assert_eq!(clicks(&world), (false, false));
    }
    // Their coins are what the session counts, while the window is open.
    game(&mut world, coins(gold(1), gold(2)));
    assert_eq!(world.offered_coins(), Some(gold(2)));
    // Anyone else's click is not theirs, and nothing else changes them.
    game(&mut world, exchange(ExchangeUpdate::Accepted { by: 1 }));
    game(&mut world, coins(gold(1), gold(2)));
    assert_eq!(clicks(&world), (false, false));
    game(&mut world, exchange(ExchangeUpdate::Accepted { by: 9 }));
    game(&mut world, coins(gold(1), gold(2)));
    assert_eq!(clicks(&world), (false, true));
    // The other player closing the window ends it.
    game(&mut world, exchange(ExchangeUpdate::Cancelled { by: 1 }));
    assert!(world.exchange().is_none() && world.offered_coins().is_none());
}

#[test]
fn abilities_are_the_skills_the_player_has_and_wait_on_the_sessions_timers() {
    use crate::abilities::Ability;
    use std::time::Duration;
    let mut world = admitted();
    // Without skills, only binding wounds and fishing, which anyone can try.
    assert_eq!(world.abilities(), [Ability::BindWound, Ability::Fishing]);
    let mut skills = vec![0; 100];
    skills[30] = 12;
    skills[29] = 3;
    let mut warrior = player(9);
    warrior.skills = Some(skills);
    warrior.race = 10;
    world.apply(
        &WorldUpdate::Game(WorldEvent::Entered {
            capabilities: Vec::new(),
            session_id: 1,
            zone: "qeytoqrg".into(),
            player: Box::new(warrior),
            far_clip: None,
        }),
        Instant::now(),
        &NoSpells,
    );
    // An Ogre slams without the bash skill.
    assert_eq!(
        world.abilities(),
        [
            Ability::Kick,
            Ability::Bash,
            Ability::Hide,
            Ability::BindWound,
            Ability::Fishing
        ]
    );
    let now = Instant::now();
    let used = |session_id| WorldEvent::AbilityUsed {
        session_id,
        ability: Ability::Kick,
        ready_in: Duration::from_secs(4),
    };
    assert!(game(&mut world, used(2)).ignored, "another admission's");
    world.apply(&WorldUpdate::Game(used(1)), now, &NoSpells);
    // Strikes share the timer; other abilities keep their own.
    assert_eq!(
        world.ability_wait(Ability::Bash, now + Duration::from_secs(1)),
        Some(Duration::from_secs(3))
    );
    assert_eq!(world.ability_wait(Ability::Hide, now), None);
    assert_eq!(
        world.ability_wait(Ability::Kick, now + Duration::from_secs(4)),
        None
    );
    // Sense Heading says where the player faces: 128 is west.
    let mut facing = world.player().unwrap().position;
    facing.heading = 128.0;
    game(
        &mut world,
        WorldEvent::Position {
            spawn_id: 9,
            position: facing,
            velocity: [0.0; 3],
        },
    );
    let changes = game(
        &mut world,
        WorldEvent::AbilityUsed {
            session_id: 1,
            ability: Ability::SenseHeading,
            ready_in: Duration::ZERO,
        },
    );
    assert_eq!(changes.notices, [Notice::Heading(6)]);
    let changes = game(
        &mut world,
        WorldEvent::AbilityRefused {
            session_id: 1,
            reason: "Out of reach in this test".into(),
            string_id: None,
            arguments: Vec::new(),
        },
    );
    assert_eq!(
        changes.notices,
        [Notice::AbilityRefused {
            reason: "Out of reach in this test".into(),
            string_id: None,
            arguments: Vec::new(),
        }]
    );
}

#[test]
fn a_bandaging_that_ends_frees_bind_wound_and_the_player_hears_how_it_went() {
    use crate::{
        abilities::Ability,
        bind_wound::{BindWoundEnd, BindWoundUpdate},
    };
    use std::time::Duration;
    let mut world = admitted();
    let now = Instant::now();
    game(
        &mut world,
        WorldEvent::AbilityUsed {
            session_id: 1,
            ability: Ability::BindWound,
            ready_in: Duration::from_secs(10),
        },
    );
    assert!(world.ability_wait(Ability::BindWound, now).is_some());
    let started = BindWoundUpdate::Started { target: None };
    let changes = game(&mut world, WorldEvent::BindWound(started.clone()));
    assert_eq!(changes.notices, [Notice::BindWound(started)]);
    // The server's unlock as it starts says nothing.
    let changes = game(&mut world, WorldEvent::BindWound(BindWoundUpdate::Unlocked));
    assert_eq!(changes.notices, []);
    let ended = BindWoundUpdate::Ended(BindWoundEnd::Complete);
    let changes = game(&mut world, WorldEvent::BindWound(ended.clone()));
    assert_eq!(changes.notices, [Notice::BindWound(ended)]);
    assert_eq!(world.ability_wait(Ability::BindWound, now), None);
}

#[test]
fn the_player_hears_what_they_could_not_eat_or_drink() {
    use crate::food::{Nourishment, Shortage};
    let mut world = admitted();
    let fed = Nourishment {
        food: 2500,
        water: 6000,
    };
    game(&mut world, WorldEvent::Nourishment(fed));
    assert_eq!(world.nourishment(), Some(fed));
    assert_eq!(
        game(
            &mut world,
            WorldEvent::NothingToEat {
                food: Some(Shortage::OnlyModified),
                water: None
            }
        )
        .notices,
        [Notice::NothingToEat {
            food: Some(Shortage::OnlyModified),
            water: None
        }]
    );
    let refused = |session_id| WorldEvent::ConsumeRefused {
        session_id,
        reason: "You cannot eat or drink that".into(),
        string_id: None,
    };
    assert!(game(&mut world, refused(2)).ignored, "another admission's");
    assert_eq!(
        game(&mut world, refused(1)).notices,
        [Notice::ConsumeRefused {
            reason: "You cannot eat or drink that".into(),
            string_id: None,
        }]
    );
}

#[test]
fn the_give_window_opens_on_the_npcs_answer_and_closes_on_the_servers_word() {
    use crate::exchange::ExchangeUpdate;
    let mut world = admitted();
    let exchange = |update| WorldEvent::Exchange(update);
    // News of a window nobody asked for is not the player's.
    assert!(game(&mut world, exchange(ExchangeUpdate::Opened { with: 8 })).ignored);
    world.offer_trade(8);
    assert_eq!(world.exchange().unwrap().trade_slots(), 0);
    // Another character's answer opens nothing.
    assert!(game(&mut world, exchange(ExchangeUpdate::Opened { with: 9 })).ignored);
    let changes = game(&mut world, exchange(ExchangeUpdate::Opened { with: 8 }));
    assert!(changes.trade);
    assert_eq!(world.exchange().unwrap().trade_slots(), 4);
    world.give();
    assert!(world.exchange().unwrap().given);
    game(&mut world, exchange(ExchangeUpdate::Finished));
    assert!(world.exchange().is_none());
    // A refusal for this admission closes the request and says why; one
    // for an earlier admission is not this one's.
    world.offer_trade(8);
    let refused = |session_id| WorldEvent::ExchangeRefused {
        session_id,
        reason: "You are too far away to trade".into(),
    };
    assert!(game(&mut world, refused(2)).ignored);
    let changes = game(&mut world, refused(1));
    assert_eq!(
        changes.notices,
        [Notice::GiveRefused("You are too far away to trade".into())]
    );
    assert!(world.exchange().is_none());
    // A busy partner, the player's own closing and death all end it.
    world.offer_trade(8);
    let changes = game(&mut world, exchange(ExchangeUpdate::Busy { by: 8 }));
    assert_eq!(
        changes.notices,
        [Notice::GiveRefused("They are busy".into())]
    );
    assert!(world.exchange().is_none());
    world.offer_trade(8);
    world.close_trade();
    assert!(world.exchange().is_none());
    world.offer_trade(8);
    game(
        &mut world,
        WorldEvent::Death(Death {
            spawn_id: 9,
            killer_id: 0,
            corpse_id: 0,
            bind_zone_id: 0,
            corpse_name: None,
        }),
    );
    assert!(world.exchange().is_none());
}

#[test]
fn coins_outside_the_purse_are_where_the_session_says() {
    use crate::exchange::ExchangeUpdate;
    use crate::money::CoinPlace;
    let mut world = admitted();
    let coins = |platinum, gold, silver, copper| Coins {
        platinum,
        gold,
        silver,
        copper,
    };
    let elsewhere = |cursor, given| WorldEvent::CoinsElsewhere {
        cursor,
        bank: coins(0, 0, 5, 0),
        given,
        offered: Coins::default(),
    };
    game(&mut world, elsewhere(coins(0, 11, 0, 0), Coins::default()));
    assert_eq!(world.coins_in(CoinPlace::Cursor), Some(coins(0, 11, 0, 0)));
    assert_eq!(world.coins_in(CoinPlace::Bank), Some(coins(0, 0, 5, 0)));
    // A window's coins show only while it is open.
    game(&mut world, elsewhere(Coins::default(), coins(0, 1, 0, 0)));
    assert_eq!(world.coins_in(CoinPlace::Trade), None);
    world.offer_trade(8);
    game(
        &mut world,
        WorldEvent::Exchange(ExchangeUpdate::Opened { with: 8 }),
    );
    assert_eq!(world.coins_in(CoinPlace::Trade), Some(coins(0, 1, 0, 0)));
    // A refused move says why, and moves nothing.
    let changes = game(
        &mut world,
        WorldEvent::CoinsRefused {
            session_id: 1,
            reason: "You do not have that many coins there".into(),
        },
    );
    assert_eq!(
        changes.notices,
        [Notice::TradeRefused(
            "You do not have that many coins there".into()
        )]
    );
    assert_eq!(world.coins_in(CoinPlace::Trade), Some(coins(0, 1, 0, 0)));
    // Camping forgets the coins.
    world.apply(
        &WorldUpdate::Game(WorldEvent::Camp(CampStatus::Camped)),
        Instant::now(),
        &NoSpells,
    );
    assert_eq!(world.coins_in(CoinPlace::Cursor), Some(Coins::default()));
    assert_eq!(world.coins_in(CoinPlace::Bank), None);
}

#[test]
fn camping_progress_follows_the_session_and_only_death_leaves_it() {
    let mut world = admitted();
    let start = Instant::now();
    let camp = |world: &mut ClientWorld, status, at| {
        world.apply(&WorldUpdate::Game(WorldEvent::Camp(status)), at, &NoSpells)
    };
    camp(&mut world, CampStatus::Preparing, start);
    assert_eq!(
        world.camp(),
        Some(Camp {
            since: start,
            logging_out: false
        })
    );
    // The logout keeps the time camping began.
    camp(
        &mut world,
        CampStatus::LoggingOut,
        start + std::time::Duration::from_secs(30),
    );
    assert_eq!(
        world.camp(),
        Some(Camp {
            since: start,
            logging_out: true
        })
    );
    camp(&mut world, CampStatus::Abandoned, start);
    assert_eq!(world.camp(), None);
    camp(&mut world, CampStatus::Preparing, start);
    game(
        &mut world,
        WorldEvent::Death(Death {
            spawn_id: 9,
            killer_id: 0,
            corpse_id: 10,
            bind_zone_id: 2,
            corpse_name: None,
        }),
    );
    assert!(world.camp().is_some());
    connection(&mut world, false, false);
    assert_eq!(world.camp(), None);
}

#[test]
fn item_definitions_last_for_the_admission() {
    let mut world = admitted();
    game(&mut world, WorldEvent::ItemDetails(chest().details));
    assert_eq!(world.item(1).map(|item| item.name.as_str()), Some("Chest"));
    game(&mut world, entered(2));
    assert!(world.item(1).is_none());
}

#[test]
fn news_for_the_player_comes_with_notices_and_others_news_without() {
    use crate::combat::{ConColor, Consideration, DamageOutcome};
    let mut world = admitted();
    let notices = |world: &mut ClientWorld, event| game(world, event).notices;
    // A consideration names the spawn as the server does.
    let considered = Consideration {
        target_id: 5,
        faction: 5,
        color: ConColor::Blue,
        hit_points: None,
    };
    assert_eq!(
        notices(&mut world, WorldEvent::Consideration(considered)),
        [Notice::Consideration {
            consideration: considered,
            name: Some("a_rat".into()),
        }]
    );
    // Damage between two others is not the player's to read.
    let mut damage = crate::combat::Damage {
        target_id: 5,
        source_id: 6,
        kind: 1,
        spell_id: None,
        outcome: DamageOutcome::Miss,
    };
    assert!(game(&mut world, WorldEvent::Damage(damage)).ignored);
    damage.source_id = 9;
    assert_eq!(
        notices(&mut world, WorldEvent::Damage(damage)),
        [Notice::Damage {
            damage,
            own_id: 9,
            source: None,
            target: Some("a_rat".into()),
        }]
    );
    // A refusal for an earlier admission says nothing.
    let refused = |session_id| WorldEvent::MerchantRefused {
        session_id,
        reason: "Too far away".into(),
    };
    assert_eq!(notices(&mut world, refused(2)), []);
    assert_eq!(
        notices(&mut world, refused(1)),
        [Notice::TradeRefused("Too far away".into())]
    );
    assert_eq!(
        notices(&mut world, WorldEvent::Camp(CampStatus::Preparing)),
        [Notice::Camp(CampStatus::Preparing)]
    );
    let message = world.apply(
        &WorldUpdate::ServerMessage {
            string_id: 12293,
            arguments: vec!["x".into()],
            message_type: Some(289),
        },
        Instant::now(),
        &NoSpells,
    );
    assert_eq!(
        message.notices,
        [Notice::ServerString {
            id: 12293,
            arguments: vec!["x".into()],
            message_type: Some(289),
        }]
    );
    assert_eq!(message.notices[0].message_type(), Some(289));
}

#[test]
fn loot_and_shop_replies_say_what_the_player_got_or_was_refused() {
    use crate::{
        loot::{LootResponse, LootUpdate},
        merchant::MerchantUpdate,
    };
    let mut world = admitted();
    world.open_loot(9);
    let opened = |response, coins| WorldEvent::Loot(LootUpdate::Opened { response, coins });
    assert_eq!(
        game(&mut world, opened(LootResponse::Normal, coins(0, 1, 0, 2))).notices,
        [Notice::LootCoins(coins(0, 1, 0, 2))]
    );
    assert_eq!(
        game(&mut world, opened(LootResponse::Normal, Coins::default())).notices,
        []
    );
    assert_eq!(
        game(
            &mut world,
            WorldEvent::Loot(LootUpdate::Taken {
                place: 0,
                accepted: false
            })
        )
        .notices,
        [Notice::ItemRefused]
    );
    assert_eq!(
        game(&mut world, opened(LootResponse::TooFar, Coins::default())).notices,
        [Notice::LootRefused(LootResponse::TooFar)]
    );
    // Closed, the corpse's news says nothing more.
    assert_eq!(
        game(&mut world, opened(LootResponse::TooFar, Coins::default())).notices,
        []
    );
    world.open_shop(8);
    assert_eq!(
        game(
            &mut world,
            WorldEvent::Merchant(MerchantUpdate::Opened {
                merchant_id: 8,
                accepted: false,
                rate: 1.0,
            })
        )
        .notices,
        [Notice::ShopRefused]
    );
}

#[test]
fn the_session_grants_motion_until_any_reset() {
    let grant = |session_id| WorldEvent::MotionState {
        session_id,
        units_per_second: Some(20.0),
        backward_units_per_second: None,
        walk_units_per_second: None,
        strafe_units_per_second: None,
        falls: true,
    };
    let mut world = admitted();
    assert!(game(&mut world, grant(2)).ignored);
    assert!(game(&mut world, grant(1)).motion);
    assert_eq!(world.motion().map(|grant| grant.falls), Some(true));
    connection(&mut world, false, false);
    assert!(world.motion().is_none());
}

#[test]
fn moves_and_replies_reach_the_front_end_for_the_current_admission() {
    let mut world = admitted();
    let sent = game(
        &mut world,
        WorldEvent::MotionSent {
            session_id: 1,
            position: WorldPosition::default(),
            refused: Some("Too fast".into()),
        },
    );
    assert_eq!(
        sent.moved.map(|moved| moved.refused),
        Some(Some("Too fast".into()))
    );
    let used = |session_id| WorldEvent::ItemUseAction {
        session_id,
        request_id: 3,
        error: None,
    };
    assert_eq!(game(&mut world, used(2)).replies, []);
    assert_eq!(
        game(&mut world, used(1)).replies,
        [Reply::ItemUse {
            session_id: 1,
            request_id: 3,
            error: None
        }]
    );
    // The connection's word names death when the player is dead.
    let changes = connection(&mut world, true, false);
    assert_eq!(
        changes.notices,
        [Notice::Connection {
            link: Link::Connected,
            dead: false
        }]
    );
}

#[test]
fn the_admission_says_what_the_player_can_do_until_they_camp() {
    use crate::Capability;
    let mut world = ClientWorld::default();
    let mut entry = entered(1);
    if let WorldEvent::Entered { capabilities, .. } = &mut entry {
        *capabilities = vec![Capability::Moving, Capability::Talking];
    }
    game(&mut world, entry);
    assert!(world.can(Capability::Talking));
    assert!(!world.can(Capability::Falling));
    game(&mut world, WorldEvent::Camp(CampStatus::Camped));
    assert_eq!(world.capabilities(), []);
}

#[test]
fn gems_read_the_admitted_players_memorized_spells() {
    let mut world = ClientWorld::default();
    assert_eq!(world.gems(), [None; 8]);
    let mut caster = player(9);
    caster.memorized_spells[2] = Some(73);
    game(
        &mut world,
        WorldEvent::Entered {
            capabilities: Vec::new(),
            session_id: 1,
            zone: "qeytoqrg".into(),
            player: Box::new(caster),
            far_clip: None,
        },
    );
    assert_eq!(world.gem(2), Some(73));
    assert_eq!(world.gem(0), None);
    assert_eq!(world.gem(8), None);
}

#[test]
fn the_zone_who_lists_the_zone_players_as_they_are_listed() {
    use crate::listing::{Anonymity, ListingChange};
    use crate::who::WhoFilter;
    let mut world = admitted();
    let other = |spawn_id, name: &str, class, level| SpawnState {
        class: Some(class),
        name: name.into(),
        kind: SpawnKind::Player,
        level,
        ..spawn(spawn_id)
    };
    game(
        &mut world,
        WorldEvent::Spawns(vec![
            other(6, "Zed", 12, 50),
            other(7, "Ann", 2, 20),
            SpawnState {
                kind: SpawnKind::PlayerCorpse,
                ..other(8, "Gone", 1, 5)
            },
        ]),
    );
    game(
        &mut world,
        WorldEvent::GuildNames(vec![(3, "Seekers".into())]),
    );
    for (spawn_id, change) in [
        (6, ListingChange::Guild(Some(3))),
        (6, ListingChange::Level(51)),
        (7, ListingChange::Anonymity(Anonymity::Anonymous)),
        (9, ListingChange::Away(true)),
    ] {
        game(&mut world, WorldEvent::Listing { spawn_id, change });
    }
    let names = |filter: &WhoFilter| -> Vec<String> {
        world
            .zone_who(filter)
            .into_iter()
            .map(|player| player.name)
            .collect()
    };
    // The player and the zone's other players, by name; not corpses or NPCs.
    assert_eq!(names(&WhoFilter::default()), ["Ann", "Example", "Zed"]);
    let everyone = world.zone_who(&WhoFilter::default());
    assert_eq!(everyone[2].guild.as_deref(), Some("Seekers"));
    assert_eq!(everyone[2].level, 51);
    assert!(everyone[1].listing.away);
    // An anonymous player hides from class, race and level filters.
    let clerics = WhoFilter {
        class: Some(2),
        ..WhoFilter::default()
    };
    assert_eq!(names(&clerics), Vec::<String>::new());
    let levels = WhoFilter {
        levels: Some((50, 60)),
        ..WhoFilter::default()
    };
    assert_eq!(names(&levels), ["Zed"]);
    // A name, guild or zone start matches any player.
    for (text, expected) in [
        ("se", vec!["Zed"]),
        ("an", vec!["Ann"]),
        ("QEY", vec!["Ann", "Example", "Zed"]),
    ] {
        let filter = WhoFilter {
            text: text.into(),
            ..WhoFilter::default()
        };
        assert_eq!(names(&filter), expected, "{text}");
    }
}

#[test]
fn the_clock_runs_on_from_the_time_the_server_gave() {
    use crate::clock::{Fog, GameTime, ZoneSky};
    let mut world = admitted();
    let now = Instant::now();
    assert_eq!(world.game_time(now), None);
    let noon = GameTime {
        hour: 12,
        minute: 0,
        day: 1,
        month: 1,
        year: 3100,
    };
    world.apply(
        &WorldUpdate::Game(WorldEvent::TimeOfDay(noon)),
        now,
        &NoSpells,
    );
    let later = world
        .game_time(now + std::time::Duration::from_secs(180))
        .unwrap();
    assert_eq!((later.hour, later.minute), (13, 0));
    let sky = ZoneSky {
        sky: 1,
        time_type: 2,
        fog: [Fog {
            color: [10, 20, 30],
            near: 10.0,
            far: 500.0,
        }; 4],
    };
    game(&mut world, WorldEvent::Sky(sky));
    assert_eq!(world.sky(), Some(sky));
}

#[test]
fn the_player_has_the_pet_they_own_and_its_buffs() {
    use crate::pets::{PetBuff, PetBuffs};
    let mut world = admitted();
    assert!(world.pet().is_none());
    // A pet the player summoned, and one a charm hands over.
    game(
        &mut world,
        WorldEvent::Spawns(vec![SpawnState {
            pet_owner: Some(9),
            hp_percent: Some(100),
            ..spawn(6)
        }]),
    );
    assert_eq!(world.pet().map(|pet| pet.state.spawn_id), Some(6));
    // Its health is its spawn record's until the server reports a change.
    assert_eq!(world.health(6), Some(100));
    let buffs = PetBuffs {
        pet: 6,
        slots: vec![
            Some(PetBuff {
                spell_id: 312,
                ticks: 10,
            }),
            None,
        ],
    };
    game(&mut world, WorldEvent::PetBuffs(buffs.clone()));
    assert_eq!(world.pet_buffs(), Some(&buffs));
    game(
        &mut world,
        WorldEvent::PetOwner {
            spawn_id: 6,
            owner: None,
        },
    );
    assert!(world.pet().is_none());
    assert_eq!(world.pet_buffs(), None);
    game(
        &mut world,
        WorldEvent::PetOwner {
            spawn_id: 5,
            owner: Some(9),
        },
    );
    assert_eq!(world.pet().map(|pet| pet.state.spawn_id), Some(5));
}

#[test]
fn last_names_change_by_the_name_spawned_with() {
    let mut world = admitted();
    game(
        &mut world,
        WorldEvent::Spawns(vec![
            SpawnState {
                name: "Examplar".into(),
                kind: SpawnKind::Player,
                ..spawn(6)
            },
            spawn(7),
        ]),
    );
    for (name, last_name) in [("Examplar", "Exemplum"), ("Example", "Sample")] {
        game(
            &mut world,
            WorldEvent::LastName {
                name: name.into(),
                last_name: last_name.into(),
            },
        );
    }
    let last_name = |id| world.spawn(id).unwrap().state.name_parts.last_name.clone();
    assert_eq!(last_name(6), "Exemplum");
    assert_eq!(last_name(7), "");
    assert_eq!(world.player().unwrap().name_parts.last_name, "Sample");
    // A last name taken away leaves none.
    game(
        &mut world,
        WorldEvent::LastName {
            name: "Examplar".into(),
            last_name: String::new(),
        },
    );
    assert_eq!(world.spawn(6).unwrap().state.name_parts.last_name, "");
}

#[test]
fn training_opens_raises_skills_with_a_line_and_closes() {
    use crate::training::{TrainingOffer, TrainingUpdate};
    let mut world = ClientWorld::default();
    let mut trainee = player(9);
    trainee.skills = Some(vec![0; 100]);
    trainee.practice_points = Some(5);
    game(
        &mut world,
        WorldEvent::Entered {
            capabilities: Vec::new(),
            session_id: 1,
            zone: "qeynos".into(),
            player: Box::new(trainee),
            far_clip: None,
        },
    );
    connection(&mut world, true, false);
    let mut caps = vec![0; 100];
    caps[30] = 200;
    game(
        &mut world,
        WorldEvent::Training(TrainingUpdate::Offered(TrainingOffer { trainer: 42, caps })),
    );
    assert_eq!(world.training().map(|offer| offer.trainer), Some(42));
    let changes = game(
        &mut world,
        WorldEvent::Training(TrainingUpdate::Trained {
            skill: 30,
            value: 1,
            cost: 0,
        }),
    );
    assert_eq!(
        changes.notices,
        [Notice::SkillUp {
            skill: 30,
            value: 1
        }]
    );
    game(&mut world, WorldEvent::PracticePoints(4));
    assert_eq!(
        world.player().and_then(|player| player.practice_points),
        Some(4)
    );
    // A value that does not rise says nothing.
    let changes = game(
        &mut world,
        WorldEvent::Skill {
            skill_id: 30,
            value: 1,
        },
    );
    assert_eq!(changes.notices.len(), 0);
    game(&mut world, WorldEvent::Training(TrainingUpdate::Ended));
    assert!(world.training().is_none());
    let changes = game(
        &mut world,
        WorldEvent::TrainingRefused {
            session_id: 1,
            reason: "No practice points".into(),
        },
    );
    assert_eq!(
        changes.notices,
        [Notice::TrainingRefused("No practice points".into())]
    );
}

#[test]
fn a_resurrection_offer_is_forgotten_when_the_player_enters_a_zone() {
    let mut world = admitted();
    // An offer from Tester to Example's corpse: the caster's name at 92 and
    // the corpse's at 160 of the 228 bytes.
    let mut body = vec![0; 228];
    body[92..98].copy_from_slice(b"Tester");
    body[160..177].copy_from_slice(b"Example's corpse0");
    let offer = crate::resurrection::titanium_offer(&body).unwrap();
    game(&mut world, WorldEvent::Resurrection(offer));
    assert!(world.resurrection().is_some());
    // The session that held the offer ends with the zone; the next zone's
    // knows nothing of it, so neither does the world.
    game(&mut world, entered(2));
    assert!(world.resurrection().is_none());
}

#[test]
fn a_text_is_read_until_put_away_or_the_player_enters_a_zone() {
    let mut world = admitted();
    let book = || {
        WorldEvent::BookText(crate::books::BookText {
            kind: 1,
            text: "Chapter one".into(),
        })
    };
    game(&mut world, book());
    assert_eq!(world.reading().map(|text| text.kind), Some(1));
    world.close_reading();
    assert!(world.reading().is_none());
    game(&mut world, book());
    game(&mut world, entered(2));
    assert!(world.reading().is_none());
    // A request this session would not send says why; another session's
    // refusal is not this player's news.
    let reason = "That is not something you can read.";
    let changes = game(
        &mut world,
        WorldEvent::ReadRefused {
            session_id: 2,
            reason: reason.into(),
        },
    );
    assert_eq!(changes.notices, [Notice::ReadRefused(reason.into())]);
    let changes = game(
        &mut world,
        WorldEvent::ReadRefused {
            session_id: 1,
            reason: reason.into(),
        },
    );
    assert_eq!(changes.notices.len(), 0);
}

#[test]
fn a_combine_is_under_way_until_the_server_answers() {
    use crate::{inventory::InventorySlot, tradeskills::CombineUpdate};
    let mut world = admitted();
    game(
        &mut world,
        WorldEvent::Combine(CombineUpdate::Started(InventorySlot(25))),
    );
    assert_eq!(world.combining(), Some(InventorySlot(25)));
    game(&mut world, WorldEvent::Combine(CombineUpdate::Answered));
    assert_eq!(world.combining(), None);
    // Zoning forgets a combine the old zone never answered.
    game(
        &mut world,
        WorldEvent::Combine(CombineUpdate::Started(InventorySlot(25))),
    );
    game(&mut world, entered(2));
    assert_eq!(world.combining(), None);
    let reason = "Your cursor must be empty to combine.";
    let changes = game(
        &mut world,
        WorldEvent::CombineRefused {
            session_id: 2,
            reason: reason.into(),
            string_id: Some(crate::tradeskills::HANDS_FULL),
        },
    );
    assert_eq!(
        changes.notices,
        [Notice::CombineRefused {
            reason: reason.into(),
            string_id: Some(crate::tradeskills::HANDS_FULL),
        }]
    );
}

#[test]
fn a_world_container_opens_only_when_asked_and_closes_with_the_player() {
    use crate::ground::{ContainerView, ObjectUpdate};
    let mut world = admitted();
    let view = |open| ContainerView {
        player_id: 9,
        drop_id: 40,
        open,
        object_type: 15,
        icon: 0,
        name: String::new(),
    };
    // A container the player did not ask for never shows.
    game(
        &mut world,
        WorldEvent::Objects(ObjectUpdate::Container(view(true))),
    );
    assert!(world.container().is_none());
    world.ask_container(40);
    let changes = game(
        &mut world,
        WorldEvent::Objects(ObjectUpdate::Container(view(false))),
    );
    assert_eq!(changes.notices, [Notice::ContainerInUse]);
    assert!(world.container().is_none());
    world.ask_container(40);
    game(
        &mut world,
        WorldEvent::Objects(ObjectUpdate::Container(view(true))),
    );
    assert_eq!(world.container().map(|view| view.drop_id), Some(40));
    world.close_container();
    assert!(world.container().is_none());
    // Zoning forgets an open container.
    world.ask_container(40);
    game(
        &mut world,
        WorldEvent::Objects(ObjectUpdate::Container(view(true))),
    );
    game(&mut world, entered(2));
    assert!(world.container().is_none());
}

#[test]
fn abilities_the_server_does_not_offer_are_told_apart() {
    use crate::abilities::Ability;
    let mut world = admitted();
    // Before the session says, every ability counts as offered.
    assert!(world.ability_offered(Ability::Fishing));
    game(
        &mut world,
        WorldEvent::AbilitiesOffered(vec![Ability::Kick, Ability::Bash]),
    );
    assert!(world.ability_offered(Ability::Kick));
    assert!(!world.ability_offered(Ability::Fishing));
    // The player still has fishing; it shows greyed rather than missing.
    assert!(world.abilities().contains(&Ability::Fishing));
}

#[test]
fn a_group_follows_the_servers_word_and_says_what_happened() {
    use crate::group::GroupUpdate;
    let mut world = admitted();
    let news = |world: &mut ClientWorld, update| game(world, WorldEvent::Group(update)).notices;
    let said = |notice| [Notice::Group(notice)];
    let friend = || Party::Named("Friend".into());
    // An invitation waits until the player answers it.
    assert_eq!(
        news(
            &mut world,
            GroupUpdate::Invited {
                inviter: "Leader".into()
            }
        ),
        said(GroupNotice::Invited("Leader".into()))
    );
    assert_eq!(world.group_invitation(), Some("Leader"));
    assert_eq!(
        news(
            &mut world,
            GroupUpdate::Following {
                inviter: "Leader".into()
            }
        ),
        said(GroupNotice::Following("Leader".into()))
    );
    assert_eq!(world.group_invitation(), None);
    // The list that follows joining says the player joined; the leader is
    // among the others.
    let list = || GroupUpdate::Members {
        leader: "Leader".into(),
        members: vec!["Leader".into(), "Friend".into()],
    };
    assert_eq!(
        news(&mut world, list()),
        said(GroupNotice::Joined(Party::Player))
    );
    let group = world.group().cloned().unwrap();
    assert_eq!(
        (group.leader.as_deref(), group.members.len()),
        (Some("Leader"), 2)
    );
    assert!(!world.leads_group());
    // Another list, as after zoning, says nothing.
    assert_eq!(news(&mut world, list()), []);
    // Others leave, and the player is made the leader.
    assert_eq!(
        news(
            &mut world,
            GroupUpdate::Left {
                member: "Friend".into()
            }
        ),
        said(GroupNotice::Left(friend()))
    );
    assert_eq!(
        news(
            &mut world,
            GroupUpdate::Leader {
                name: "Example".into()
            }
        ),
        said(GroupNotice::Leader(Party::Player))
    );
    assert!(world.leads_group());
    assert_eq!(
        world.group().map(|group| group.members.clone()),
        Some(vec!["Leader".to_owned()])
    );
    // The player leaving ends their group.
    assert_eq!(
        news(
            &mut world,
            GroupUpdate::Left {
                member: "Example".into()
            }
        ),
        said(GroupNotice::Left(Party::Player))
    );
    assert!(world.group().is_none());
}

#[test]
fn a_group_the_player_formed_is_theirs_to_lead_until_it_ends() {
    use crate::group::GroupUpdate;
    let mut world = admitted();
    let news = |world: &mut ClientWorld, update| game(world, WorldEvent::Group(update)).notices;
    let said = |notice| [Notice::Group(notice)];
    let friend = || Party::Named("Friend".into());
    // Forming one makes the player its leader; others join it.
    assert_eq!(
        news(&mut world, GroupUpdate::Formed),
        said(GroupNotice::Formed)
    );
    assert_eq!(
        news(
            &mut world,
            GroupUpdate::Joined {
                member: "Friend".into()
            }
        ),
        said(GroupNotice::Joined(friend()))
    );
    // The invitee's acceptance, which follows their join, says nothing.
    assert_eq!(
        news(
            &mut world,
            GroupUpdate::Accepted {
                member: "Friend".into()
            }
        ),
        []
    );
    assert!(world.leads_group());
    assert_eq!(
        news(&mut world, GroupUpdate::Disbanded),
        said(GroupNotice::Disbanded)
    );
    assert!(world.group().is_none());
    // A new admission forgets the group, which the server lists again.
    news(&mut world, GroupUpdate::Formed);
    game(&mut world, entered(2));
    assert!(world.group().is_none());
}

#[test]
fn a_group_request_refused_is_said_for_its_admission_alone() {
    let mut world = admitted();
    let refused = |session_id| WorldEvent::GroupRefused {
        session_id,
        reason: "No".into(),
        string_id: Some(12267),
    };
    assert_eq!(
        game(&mut world, refused(1)).notices,
        [Notice::GroupRefused {
            reason: "No".into(),
            string_id: Some(12267),
        }]
    );
    assert!(game(&mut world, refused(9)).ignored);
}

#[test]
fn the_players_own_listing_changes_and_says_which() {
    use crate::listing::{Anonymity, ListingChange};
    let mut world = admitted();
    let set = |world: &mut ClientWorld, session_id, change| {
        game(world, WorldEvent::ListingSet { session_id, change })
    };
    assert_eq!(
        set(&mut world, 1, ListingChange::Away(true)).notices,
        [Notice::Listing(ListingNotice::Away(true))]
    );
    assert!(world.player().unwrap().listing.away);
    for (anonymity, said) in [
        (Anonymity::Anonymous, ListingNotice::Anonymous(true)),
        (Anonymity::Open, ListingNotice::Anonymous(false)),
        (Anonymity::Roleplaying, ListingNotice::Roleplaying(true)),
        (Anonymity::Open, ListingNotice::Roleplaying(false)),
    ] {
        assert_eq!(
            set(&mut world, 1, ListingChange::Anonymity(anonymity)).notices,
            [Notice::Listing(said)]
        );
        assert_eq!(world.player().unwrap().listing.anonymity, anonymity);
    }
    // A change made for an earlier admission says nothing.
    assert!(set(&mut world, 9, ListingChange::Away(false)).ignored);
    assert!(world.player().unwrap().listing.away);
}

#[test]
fn a_roll_is_said_and_an_assist_answer_is_taken_once() {
    use crate::socials::{Assisted, Roll};
    let mut world = admitted();
    let roll = Roll {
        name: "Friend".into(),
        low: 1,
        high: 6,
        result: 4,
    };
    assert_eq!(
        game(&mut world, WorldEvent::Roll(roll.clone())).notices,
        [Notice::Roll(roll)]
    );
    game(
        &mut world,
        WorldEvent::Assisted(Assisted { target: Some(5) }),
    );
    assert_eq!(world.take_assisted(), Some(5));
    assert_eq!(world.take_assisted(), None);
    // An answer naming no one leaves the target as it was.
    game(&mut world, WorldEvent::Assisted(Assisted { target: None }));
    assert_eq!(world.take_assisted(), None);
}

#[test]
fn a_raid_formed_joined_and_left_says_what_happened() {
    use crate::raid::{RaidMember, RaidUpdate};
    let raid_news = |world: &mut ClientWorld, update| game(world, WorldEvent::Raid(update)).notices;
    let said = |notice| vec![Notice::Raid(notice)];
    let member = |name: &str| {
        RaidUpdate::Added(RaidMember {
            name: name.into(),
            group: None,
            class: 1,
            level: 1,
            group_leader: false,
        })
    };
    let created = |leader: &str| RaidUpdate::Created {
        leader: leader.into(),
    };
    // The player forms a raid with the one they invited.
    let mut leader = admitted();
    raid_news(
        &mut leader,
        RaidUpdate::Inviting {
            player: "Friend".into(),
        },
    );
    assert_eq!(
        raid_news(&mut leader, created("Example")),
        said(RaidNotice::Formed)
    );
    assert_eq!(raid_news(&mut leader, member("Example")), []);
    assert_eq!(
        raid_news(&mut leader, member("Friend")),
        said(RaidNotice::Joined(Party::Named("Friend".into())))
    );
    assert!(leader.leads_raid());
    assert_eq!(leader.raid().map(|raid| raid.members.len()), Some(2));
    // The one invited joins: the list that follows says nothing.
    let mut joiner = admitted();
    raid_news(
        &mut joiner,
        RaidUpdate::Invited {
            inviter: "Leader".into(),
        },
    );
    assert_eq!(joiner.raid_invitation(), Some("Leader"));
    raid_news(
        &mut joiner,
        RaidUpdate::Accepting {
            inviter: "Leader".into(),
        },
    );
    assert_eq!(
        raid_news(&mut joiner, created("Leader")),
        said(RaidNotice::Joined(Party::Player))
    );
    assert_eq!(raid_news(&mut joiner, member("Leader")), []);
    assert_eq!(raid_news(&mut joiner, member("Example")), []);
    assert_eq!(
        raid_news(
            &mut joiner,
            RaidUpdate::Leader {
                name: "Leader".into()
            }
        ),
        []
    );
    // Later joins and leaves say so.
    assert_eq!(
        raid_news(&mut joiner, member("Third")),
        said(RaidNotice::Joined(Party::Named("Third".into())))
    );
    assert_eq!(
        raid_news(
            &mut joiner,
            RaidUpdate::Removed {
                member: "Third".into()
            }
        ),
        said(RaidNotice::Left(Party::Named("Third".into())))
    );
    // Leaving: removed, then the raid's end, which says nothing more.
    assert_eq!(
        raid_news(
            &mut joiner,
            RaidUpdate::Removed {
                member: "Example".into()
            }
        ),
        said(RaidNotice::Left(Party::Player))
    );
    assert_eq!(raid_news(&mut joiner, RaidUpdate::Disbanded), []);
    assert!(joiner.raid().is_none());
    // A group the player is not in, ended as they leave a raid, says
    // nothing.
    assert_eq!(
        game(
            &mut joiner,
            WorldEvent::Group(crate::group::GroupUpdate::Disbanded)
        )
        .notices,
        []
    );
    // Entering a zone lists the raid again without a word.
    raid_news(&mut leader, RaidUpdate::Leaving);
    game(&mut leader, entered(2));
    assert!(leader.raid().is_none());
    assert_eq!(raid_news(&mut leader, created("Example")), []);
    assert_eq!(raid_news(&mut leader, member("Friend")), []);
}

#[test]
fn entering_a_zone_lists_the_player_in_their_raid_group_and_ranks_everyone() {
    use crate::raid::{RaidMember, RaidUpdate};
    let member = |name: &str, group, level, group_leader| RaidMember {
        name: name.into(),
        group,
        class: 3,
        level,
        group_leader,
    };
    let mut world = admitted();
    assert_eq!(
        game(
            &mut world,
            WorldEvent::Raid(RaidUpdate::Created {
                leader: "Leader".into()
            })
        )
        .notices,
        []
    );
    assert_eq!(world.raid().and_then(Raid::level_average), None);
    // As EQEmu lists the raid at a zone-in: the player's own entry, with
    // their raid group, then everyone else, then the raid's leader.
    for update in [
        RaidUpdate::Added(member("Example", Some(2), 10, false)),
        RaidUpdate::Added(member("Leader", Some(0), 20, true)),
        RaidUpdate::Added(member("Other", Some(2), 12, true)),
        RaidUpdate::Added(member("Loner", None, 31, false)),
        RaidUpdate::Leader {
            name: "Leader".into(),
        },
    ] {
        assert_eq!(game(&mut world, WorldEvent::Raid(update)).notices, []);
    }
    let raid = world.raid().expect("listed in a raid");
    let names = |members: Vec<&RaidMember>| {
        members
            .into_iter()
            .map(|member| member.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(raid.grouped()), ["Leader", "Example", "Other"]);
    assert_eq!(names(raid.ungrouped().collect()), ["Loner"]);
    assert_eq!(raid.members[0], member("Example", Some(2), 10, false));
    // 73 levels over four members.
    assert_eq!(raid.level_average(), Some(18));
    let ranks: Vec<RaidRank> = raid.members.iter().map(|known| raid.rank(known)).collect();
    assert_eq!(
        ranks,
        [
            RaidRank::Member,
            RaidRank::Leader,
            RaidRank::GroupLeader,
            RaidRank::Member
        ]
    );
    assert!(!world.leads_raid());
}
