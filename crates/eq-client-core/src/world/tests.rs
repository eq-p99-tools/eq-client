use super::*;
use crate::{
    SpawnKind, WorldPosition, ZoneRejection,
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
    game(&mut world, loot(LootUpdate::Item(Box::new(item))));
    game(&mut world, loot(LootUpdate::Listed { corpse_id: 8 }));
    assert!(!world.loot().unwrap().listed, "another corpse's listing");
    game(&mut world, loot(LootUpdate::Listed { corpse_id: 9 }));
    assert!(world.loot().unwrap().listed);
    let taken = |accepted| loot(LootUpdate::Taken { slot: 22, accepted });
    game(&mut world, taken(false));
    assert!(world.loot().unwrap().items.contains_key(&22));
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
fn loot_coins_and_purchases_adjust_the_carried_total() {
    use crate::{
        loot::{LootResponse, LootUpdate},
        merchant::MerchantUpdate,
    };
    let mut world = admitted();
    game(&mut world, WorldEvent::Coins(coins(1, 0, 0, 0)));
    world.open_loot(9);
    world.open_shop(8);
    game(
        &mut world,
        WorldEvent::Loot(LootUpdate::Opened {
            response: LootResponse::Normal,
            coins: coins(0, 0, 1, 5),
        }),
    );
    assert_eq!(world.coins().unwrap().total_copper(), 1015);
    game(
        &mut world,
        WorldEvent::Merchant(MerchantUpdate::Bought {
            slot: 2,
            quantity: 1,
            price: 20,
        }),
    );
    assert_eq!(world.coins(), Some(&coins(0, 9, 9, 5)));
    // The next money update restores the server's denominations.
    game(&mut world, WorldEvent::Coins(coins(0, 0, 99, 5)));
    assert_eq!(world.coins(), Some(&coins(0, 0, 99, 5)));
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
    assert!(notices(&mut world, refused(2)).is_empty());
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
        },
        Instant::now(),
        &NoSpells,
    );
    assert_eq!(
        message.notices,
        [Notice::ServerString {
            id: 12293,
            arguments: vec!["x".into()],
        }]
    );
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
    assert!(
        game(&mut world, opened(LootResponse::Normal, Coins::default()))
            .notices
            .is_empty()
    );
    assert_eq!(
        game(
            &mut world,
            WorldEvent::Loot(LootUpdate::Taken {
                slot: 22,
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
    assert!(
        game(&mut world, opened(LootResponse::TooFar, Coins::default()))
            .notices
            .is_empty()
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
    assert!(game(&mut world, used(2)).replies.is_empty());
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
            label: String::new(),
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
    assert!(world.capabilities().is_empty());
}
