//! The offline preview: a stand-in session for screenshots and visual
//! checks without a server. It tells the client its news through the door a
//! session uses, so the windows show it as they would a server's, and it
//! never runs alongside a session. Everything it shows is synthetic and
//! labelled so.
use super::{Player, SceneInfo, Stage, TerrainSurface, setup_scene, world_position};
use bevy::prelude::*;
use eq_client_core::{
    CharacterChoice, Coins, SpawnKind, WorldEvent, WorldPosition, WorldUpdate,
    exchange::ExchangeUpdate,
    inventory::{InventoryItem, InventorySlot, InventoryUpdate},
    loot::{LootResponse, LootUpdate},
    merchant::{MerchantItem, MerchantUpdate},
    render_position,
};
use std::sync::mpsc::{Receiver, SyncSender};

/// What the offline preview shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent preview switches.
pub struct Preview {
    /// Synthetic moving entities, with synthetic chat.
    pub entities: bool,
    /// A synthetic inventory.
    pub inventory: bool,
    /// Bank storage in the inventory preview.
    pub bank: bool,
    /// A synthetic spellbook, with gems and effects.
    pub spellbook: bool,
    /// Character selection with synthetic names.
    pub character_select: bool,
    /// Loot and merchant windows with synthetic items.
    pub trade: bool,
}

impl Preview {
    /// Whether the preview admits its player into the zone on screen.
    const fn admits(&self) -> bool {
        self.entities || self.inventory || self.spellbook || self.trade
    }

    const fn any(&self) -> bool {
        self.admits() || self.character_select
    }
}

/// The preview's news on its way to the world.
#[derive(Resource)]
struct News(SyncSender<WorldUpdate>);

impl News {
    fn tell(&self, update: WorldUpdate) {
        // The viewer reads every frame and holds far more than a frame's news.
        let _ = self.0.try_send(update);
    }

    fn game(&self, event: WorldEvent) {
        self.tell(WorldUpdate::Game(event));
    }
}

/// The preview's switches, for its systems.
#[derive(Resource)]
struct Switches(Preview);

/// What the preview shows once its player is admitted: an admission
/// starts the windows over, as a session's does, so they fill after it.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Shows;

/// Installs what the preview shows, and returns the news it sends for the
/// viewer to read as a session's; nothing if it shows nothing.
pub(crate) fn install(app: &mut App, preview: Preview) -> Option<Receiver<WorldUpdate>> {
    if !preview.any() {
        return None;
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(4096);
    app.insert_resource(News(sender))
        .configure_sets(Update, Shows.run_if(admitted).in_set(Stage::Scene));
    if preview.admits() {
        app.add_systems(Startup, admit.after(setup_scene));
    }
    if preview.character_select {
        app.add_systems(Startup, characters);
    }
    if preview.inventory {
        app.add_systems(Update, inventory.in_set(Shows))
            .add_systems(
                Update,
                super::inventory::settle
                    .after(super::inventory::input)
                    .in_set(Stage::Input),
            );
    }
    if preview.spellbook {
        app.add_systems(Update, spellbook.in_set(Shows));
    }
    if preview.trade {
        app.add_systems(Update, trade.in_set(Shows));
    }
    if preview.entities {
        app.add_systems(Update, chat.in_set(Shows))
            .add_systems(Update, entities.run_if(in_world).in_set(Stage::Scene));
    }
    app.insert_resource(Switches(preview));
    Some(receiver)
}

/// True on one frame: the first on which the world has the preview's player.
fn admitted(online: Res<super::online::OnlineState>, mut done: Local<bool>) -> bool {
    let now = !*done && in_world(online);
    *done |= now;
    now
}

/// Whether the world has the preview's player.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn in_world(online: Res<super::online::OnlineState>) -> bool {
    online.world().session_id().is_some()
}

/// The preview's player, standing here with these spells memorized.
pub(crate) fn player(
    position: WorldPosition,
    gems: [Option<u32>; 8],
) -> eq_client_core::PlayerState {
    eq_client_core::PlayerState {
        name: "Preview".into(),
        base_attributes: None,
        deity: None,
        class: Some(1),
        spawn_id: 1,
        race: 1,
        gender: 0,
        level: 1,
        position,
        mana: 0,
        endurance: Some(0),
        skills: None,
        spell_refresh_ms: None,
        memorized_spells: gems,
        size: 0.0,
        walk_speed: 0.0,
        run_speed: 0.0,
        hp_percent: None,
        appearance: eq_client_core::outfit::Appearance::default(),
    }
}

/// Admits the preview's player where the viewer stands, in the zone on
/// screen, as a session would; every part of the preview fills that world.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn admit(news: Res<News>, scene: Res<SceneInfo>, players: Query<&Transform, With<Player>>) {
    let Ok(standing) = players.single() else {
        return;
    };
    let origin = world_position(standing.translation.to_array(), 0.0);
    news.game(WorldEvent::Entered {
        // The preview lets the player do everything, jumping included, so
        // it never lags what eq-network adds.
        capabilities: eq_client_core::Capability::ALL.to_vec(),
        session_id: 1,
        zone: scene.zone_name.clone(),
        player: Box::new(player(origin, [None; 8])),
        far_clip: None,
    });
    news.tell(WorldUpdate::Connection(
        eq_client_core::world::Link::Connected,
    ));
}

/// Offers two synthetic characters to choose from.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn characters(news: Res<News>) {
    news.game(WorldEvent::CharacterSelection {
        selection_id: 1,
        characters: vec![
            CharacterChoice {
                slot: 0,
                name: "Examplewarrior".into(),
                level: Some(12),
                zone_id: Some(22),
            },
            CharacterChoice {
                slot: 3,
                name: "Examplecleric".into(),
                level: Some(5),
                zone_id: Some(9),
            },
        ],
    });
}

/// Fills the inventory, and the bank if asked, and opens their windows.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn inventory(
    news: Res<News>,
    switches: Res<Switches>,
    mut state: ResMut<super::inventory::InventoryState>,
    mut shown: ResMut<super::windows::Shown>,
) {
    news.game(WorldEvent::Inventory(InventoryUpdate::Snapshot(items())));
    state.preview(switches.0.bank);
    shown.open(super::windows::WindowId::Inventory);
    if switches.0.bank {
        shown.open(super::windows::WindowId::Bank);
    }
}

/// Scribes fourteen spells, memorizes eight and starts two effects, then
/// opens the book.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn spellbook(news: Res<News>, mut shown: ResMut<super::windows::Shown>) {
    let mut book = eq_client_core::SpellBook::default();
    for slot in 0..14 {
        book.apply(&eq_client_core::SpellUpdate::Slot {
            slot,
            spell_id: slot + 1,
            mode: 0,
        });
    }
    let buff = |spell_id| eq_client_core::Buff {
        spell_id,
        caster_level: 1,
        effect_type: 2,
        bard_modifier: 10,
        duration_ticks: 5,
        counters: 0,
        caster_id: 0,
    };
    news.game(WorldEvent::SpellBook(book));
    news.game(WorldEvent::BuffSnapshot(vec![
        Some(buff(202)),
        None,
        Some(buff(200)),
    ]));
    for slot in 0..8 {
        news.game(WorldEvent::Spell(eq_client_core::SpellUpdate::Slot {
            slot,
            spell_id: slot + 1,
            mode: 1,
        }));
    }
    shown.open(super::windows::WindowId::Spellbook);
}

/// Opens a corpse to loot and a merchant to trade with, as if the player
/// had asked for both, and another player's trade, with two of their items
/// and some coins in it and their Trade clicked.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn trade(
    news: Res<News>,
    mut online: ResMut<super::online::OnlineState>,
    mut trade: ResMut<super::trade::TradeState>,
) {
    online.open_loot(1);
    online.open_shop(2);
    trade.preview("a preview rat", "Preview merchant");
    let items = items();
    news.game(WorldEvent::Coins(Coins {
        platinum: 3,
        gold: 1,
        silver: 4,
        copper: 7,
    }));
    news.game(WorldEvent::Loot(LootUpdate::Opened {
        response: LootResponse::Normal,
        coins: Coins::default(),
    }));
    news.game(WorldEvent::Merchant(MerchantUpdate::Opened {
        merchant_id: 2,
        accepted: true,
        rate: 1.0,
    }));
    for (item, slot) in items.iter().take(3).zip(22..) {
        let mut item = item.clone();
        item.slot = InventorySlot(slot);
        news.game(WorldEvent::Loot(LootUpdate::Item(Box::new(item))));
    }
    news.game(WorldEvent::Loot(LootUpdate::Listed { corpse_id: 1 }));
    for (item, slot) in items.iter().skip(3).cloned().zip(1u32..) {
        news.game(WorldEvent::Merchant(MerchantUpdate::Item(Box::new(
            MerchantItem {
                slot,
                price: slot * 137,
                quantity: 0,
                item,
            },
        ))));
    }
    let trader = online
        .world()
        .player()
        .map(|player| player.position)
        .unwrap_or_default();
    news.game(WorldEvent::Spawns(vec![eq_client_core::SpawnState {
        name: "Preview_Trader000".into(),
        kind: SpawnKind::Player,
        ..synthetic(5, 1, 0.0, trader)
    }]));
    news.game(WorldEvent::Exchange(ExchangeUpdate::Taken { from: 5 }));
    for (index, item) in [0, 2].into_iter().zip(items) {
        news.game(WorldEvent::Exchange(ExchangeUpdate::Offered {
            index,
            item: Box::new(item),
        }));
    }
    news.game(WorldEvent::CoinsElsewhere {
        cursor: Coins::default(),
        bank: Coins::default(),
        given: Coins::default(),
        offered: Coins {
            gold: 3,
            silver: 5,
            ..Coins::default()
        },
    });
    news.game(WorldEvent::Exchange(ExchangeUpdate::Accepted { by: 5 }));
}

/// Says a line on each channel, one of them with an item link.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn chat(news: Res<News>) {
    for line in chat_lines() {
        news.tell(WorldUpdate::Chat(line));
    }
}

/// Moves three synthetic spawns around the player: a player who sits,
/// ducks and stands, and two creatures circling; the player stands wherever
/// the viewer moved them.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn entities(
    news: Res<News>,
    time: Res<Time>,
    online: Res<super::online::OnlineState>,
    players: Query<&Transform, With<Player>>,
    surface: Res<TerrainSurface>,
) {
    let Ok(standing) = players.single() else {
        return;
    };
    let origin = world_position(standing.translation.to_array(), 0.0);
    news.game(WorldEvent::Position {
        spawn_id: 1,
        position: origin,
        velocity: [0.0; 3],
    });
    for (id, race, offset, size) in [(2u16, 1, 0.0, 0.0), (3, 42, 2.1, 2.5), (4, 54, 4.2, 6.0)] {
        let phase = time.elapsed_secs() % 18.0;
        let angle = if id == 2 && phase < 12.0 {
            0.0
        } else {
            time.elapsed_secs() * 0.25 + offset
        };
        let mut p = origin;
        let radius = if id == 2 { 6.0 } else { 14.0 };
        p.x += angle.cos() * radius;
        p.y += angle.sin() * radius;
        let height = if size > 0.0 { size } else { 6.0 };
        let [x, _, z] = render_position(p);
        p.z = surface
            .height_below(x, z, origin.z + 10.0)
            .unwrap_or(origin.z - height * 0.5)
            + height * 0.5;
        p.heading = (-angle).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 512.0;
        // A spawn appears once, then only moves, so it is not drawn again.
        news.game(if online.world().spawn(id).is_none() {
            WorldEvent::Spawns(vec![synthetic(id, race, size, p)])
        } else {
            WorldEvent::Position {
                spawn_id: id,
                position: p,
                velocity: [0.0; 3],
            }
        });
        if id == 2 {
            let posture = if phase < 6.0 {
                eq_client_core::PostureState::Sitting
            } else if phase < 12.0 {
                eq_client_core::PostureState::Ducking
            } else {
                eq_client_core::PostureState::Standing
            };
            news.game(WorldEvent::Posture {
                spawn_id: id,
                posture,
            });
        }
    }
}

/// One of the synthetic spawns: spawn 2 a player, the rest creatures.
fn synthetic(id: u16, race: u32, size: f32, position: WorldPosition) -> eq_client_core::SpawnState {
    eq_client_core::SpawnState {
        class: None,
        spawn_id: id,
        name: format!("Synthetic {id}"),
        kind: if id == 2 {
            SpawnKind::Player
        } else {
            SpawnKind::Npc
        },
        race,
        gender: 0,
        position,
        velocity: [0.0; 3],
        size,
        invisible: false,
        appearance: eq_client_core::outfit::Appearance::default(),
    }
}

/// The preview's chat: a synthetic line on each channel; the auction names
/// an item by link.
pub(crate) fn chat_lines() -> Vec<eq_client_core::chat::ChatLine> {
    use eq_client_core::chat::{ChannelName, ChatLine, Message};
    [
        (
            ChannelName::System,
            "Offline preview - these are synthetic messages.",
        ),
        (
            ChannelName::Guild,
            "Meet by the tunnel when everyone is ready.",
        ),
        (ChannelName::Group, "Ready when you are."),
        (
            ChannelName::Auction,
            "WTS Fine Steel Long Sword - send a tell.",
        ),
        (ChannelName::Ooc, "Anyone heading toward the inn?"),
        (ChannelName::Tell, "I will wait here."),
        (ChannelName::Emote, "waves hello."),
    ]
    .into_iter()
    .map(|(channel, text)| ChatLine {
        channel,
        sender: Some("Preview".into()),
        target: None,
        message: Message {
            message: None,
            message_hex: None,
            text: text.into(),
            item_links: if channel == ChannelName::Auction {
                let label = "Fine Steel Long Sword";
                let start = text.find(label).unwrap_or_default();
                vec![eq_client_core::ItemLink {
                    body: format!("00002A{}1234ABCD", "0".repeat(31)),
                    text: label.into(),
                    item_id: 42,
                    start: 0,
                    end: 0,
                    text_start: start,
                    text_end: start + label.len(),
                }]
            } else {
                vec![]
            },
        },
    })
    .collect()
}

/// The preview's items: a sword, two bags, arrows, rations, bandages, a
/// lantern and a bank item. Tests use them as ordinary items too.
pub(crate) fn items() -> Vec<InventoryItem> {
    use eq_client_core::ItemDetails;
    let mut items = Vec::new();
    for (slot, id, name, count, bag) in [
        (13, 1, "Preview sword", None, 0),
        (22, 2, "Preview backpack", None, 8),
        (23, 7, "Preview satchel", None, 10),
        (261, 8, "Preview arrows", Some(50), 0),
        (251, 3, "Preview rations", Some(20), 0),
        (252, 4, "Preview bandages", Some(7), 0),
        (30, 5, "Preview lantern", None, 0),
        (2000, 6, "Preview bank item", None, 0),
    ] {
        items.push(InventoryItem {
            activation: eq_client_core::inventory::ItemActivation::default(),
            scroll_spell: None,
            rules: eq_client_core::inventory::ItemPlacement {
                stack_size: if id == 8 { 100 } else { 20 },
                size: 1,
                bag_size: 4,
                item_type: if id == 8 { 27 } else { 0 },
                ..default()
            },
            slot: InventorySlot(slot),
            icon: match id {
                2 => 557,
                7 => 539,
                3 => 537,
                4 => 538,
                8 => 598,
                _ => 519,
            },
            stack_count: count,
            charges: 0,
            bag_slots: bag,
            details: ItemDetails {
                equipment: None,
                bonuses: None,
                id,
                name: name.into(),
                lore: String::new(),
                weight_tenths: 10,
                slots: if id == 1 {
                    1 << 13
                } else if id == 8 {
                    1 << 21
                } else {
                    0
                },
                classes: u32::MAX,
                races: u32::MAX,
                flags: Vec::new(),
                stats: Vec::new(),
            },
        });
    }
    items
}
