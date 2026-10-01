use super::*;
use crate::{
    SpawnKind, ZoneRejection,
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
        spell_refresh_ms: None,
        memorized_spells: [None; 8],
        size: 0.0,
        walk_speed: 0.0,
        run_speed: 0.0,
        hp_percent: Some(100),
        appearance: Appearance::default(),
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
        &WorldUpdate::Connection {
            connected,
            terminal,
            label: String::new(),
        },
        Instant::now(),
        &NoSpells,
    )
}

fn entered(session_id: u64) -> WorldEvent {
    WorldEvent::Entered {
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
    };
    assert_eq!(game(&mut world, WorldEvent::Death(rat)).reset, None);
    let corpse = world.spawn(5).unwrap();
    assert_eq!(corpse.state.kind, SpawnKind::NpcCorpse);
    assert_eq!(corpse.health, Some(0));
    assert!(corpse.revision > revision);
    assert!(world.in_world());
    let own = Death {
        spawn_id: 9,
        killer_id: 5,
        corpse_id: 10,
        bind_zone_id: 2,
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
    };
    assert_eq!(spell(&mut world, interrupted(8)).cast, None);
    assert!(world.casting().cast.is_some());
    assert!(world.casting().interrupted.is_none());
    let mana = |spell_id, keep_casting| SpellUpdate::Mana {
        spell_id,
        keep_casting,
    };
    spell(&mut world, mana(42, true));
    assert!(world.casting().cast.is_some());
    assert_eq!(
        spell(&mut world, interrupted(9)).cast,
        Some(CastNews::Interrupted)
    );
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
        },
        icon: 0,
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
