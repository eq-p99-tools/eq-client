//! Session events enter presentation here; this module never decodes wire
//! packets. The world model in `eq_client_core::world` applies each update;
//! this module shows what it changed.

use super::{ViewerSettings, hud};
use bevy::prelude::*;
use eq_client_core::{
    WorldEvent, WorldUpdate,
    world::{Changes, ClientWorld, Moved, NoSpells, Reply, Reset, SpellCatalog},
};
use std::sync::{LazyLock, Mutex, mpsc::Receiver};

#[derive(Resource)]
pub(super) struct Updates(pub Mutex<Option<Receiver<WorldUpdate>>>);

/// What the server has told the client, and what presenting it needs besides.
#[derive(Resource)]
pub(super) struct OnlineState {
    /// What the server has told the client. Only news changes it (the
    /// session's, through `receive`, or the preview's), and the player's
    /// own requests, which the world then waits on; everything else reads it.
    world: ClientWorld,
    /// The character choice in progress, from the world's character list.
    pub selection: Option<super::character_select::Selection>,
    /// The current zone's regions, from its assets.
    pub regions: eq_client_assets::regions::ZoneRegions,
    pub enabled: bool,
    loading: super::background::Background<Result<super::zone::PreparedEntry, String>>,
}

impl OnlineState {
    /// Whether the admitted zone's scene is still loading in the background.
    /// A zone whose files fail to load is not: its spawns still show.
    pub(crate) fn scene_loading(&self) -> bool {
        self.loading.pending(self.world.session_id())
    }

    /// What the server has told the client.
    pub(super) fn world(&self) -> &ClientWorld {
        &self.world
    }

    /// News for the world, as a session would tell it; only the offline
    /// preview's own settled moves come this way besides `receive`'s.
    pub(super) fn tell(
        &mut self,
        update: &WorldUpdate,
        now: std::time::Instant,
        spells: &dyn SpellCatalog,
    ) -> eq_client_core::world::Changes {
        self.world.apply(update, now, spells)
    }

    /// Runs the world's clocks.
    pub(super) fn tick(&mut self, now: std::time::Instant, spells: &dyn SpellCatalog) {
        self.world.tick(now, spells);
    }

    /// The player's choice, from their options, of what a session may leave
    /// to them to turn on.
    pub(super) fn choose(&mut self, chosen: Vec<eq_client_core::Capability>) {
        self.world.choose(chosen);
    }

    /// The character choice in progress, with the world it is made in.
    pub(super) fn choosing(
        &mut self,
    ) -> (
        Option<&mut super::character_select::Selection>,
        &ClientWorld,
    ) {
        (self.selection.as_mut(), &self.world)
    }

    /// The target the server's answer to an assist names, taken once.
    pub(super) fn take_assisted(&mut self) -> Option<u16> {
        self.world.take_assisted()
    }

    /// The player chose a target, or none; the world waits for the server.
    pub(super) fn select_target(&mut self, spawn: Option<u16>) {
        self.world.select_target(spawn);
    }

    /// The player asked to loot this corpse.
    pub(super) fn open_loot(&mut self, corpse_id: u16) {
        self.world.open_loot(corpse_id);
    }

    /// The player closed the loot window.
    pub(super) fn close_loot(&mut self) {
        self.world.close_loot();
    }

    /// The player asked to trade with this merchant.
    pub(super) fn open_shop(&mut self, merchant_id: u16) {
        self.world.open_shop(merchant_id);
    }

    /// The player answered the resurrection offered.
    pub(super) fn answer_resurrection(&mut self) {
        self.world.answer_resurrection();
    }

    /// The player closed the book or note they were reading.
    pub(super) fn close_reading(&mut self) {
        self.world.close_reading();
    }

    /// The player asked to open a world container.
    pub(super) fn ask_container(&mut self, drop_id: u32) {
        self.world.ask_container(drop_id);
    }

    /// The player closed the world container open for them.
    pub(super) fn close_container(&mut self) {
        self.world.close_container();
    }

    /// The player closed the merchant window.
    pub(super) fn close_shop(&mut self) {
        self.world.close_shop();
    }

    /// The player asked a character to trade.
    pub(super) fn offer_trade(&mut self, with: u16) {
        self.world.offer_trade(with);
    }

    /// The player clicked Give.
    pub(super) fn give(&mut self) {
        self.world.give();
    }

    /// The player closed the give window.
    pub(super) fn close_trade(&mut self) {
        self.world.close_trade();
    }

    /// Whether an admitted character can act now: connected, alive and not zoning.
    pub fn in_world(&self) -> bool {
        self.world.in_world()
    }

    pub fn new(enabled: bool) -> Self {
        Self {
            world: ClientWorld::default(),
            selection: None,
            regions: eq_client_assets::regions::ZoneRegions::default(),
            enabled,
            loading: default(),
        }
    }
}

/// The world the session reports, or an empty one where no session runs.
pub(super) fn world(online: Option<&OnlineState>) -> &ClientWorld {
    static OFFLINE: LazyLock<ClientWorld> = LazyLock::new(ClientWorld::default);
    online.map_or(&OFFLINE, |online| &online.world)
}

/// Runs the world's clocks each frame: doors the server leaves open swing
/// shut, and refreshed gems start their timers.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn tick(
    mut state: ResMut<OnlineState>,
    names: Option<Res<super::spellbook::SpellNames>>,
) {
    let spells: &dyn SpellCatalog = match names.as_deref() {
        Some(names) => names,
        None => &NoSpells,
    };
    state.tick(std::time::Instant::now(), spells);
}

/// The panels the session's news reaches besides the world, and the one
/// place a reset of the world reaches them: no panel watches the admission
/// to reset itself.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Panels<'w> {
    lines: ResMut<'w, super::notices::Lines>,
    motion: ResMut<'w, super::motion::Controls>,
    inventory: ResMut<'w, super::inventory::InventoryState>,
    combat: ResMut<'w, super::combat::CombatState>,
    trade: ResMut<'w, super::trade::TradeState>,
    items: ResMut<'w, super::items::ItemState>,
    book: Option<ResMut<'w, super::spellbook::BookSelection>>,
    book_view: Option<ResMut<'w, super::spellbook::BookView>>,
}

impl Panels<'_> {
    /// Shows what the world's changes mean for the panels: the inventory, the
    /// notices, motion, the trade windows and the answers to the panels' own
    /// requests.
    fn show(
        &mut self,
        changes: &Changes,
        state: &mut OnlineState,
        messages: Option<&hud::messages::Messages>,
        chat: &mut super::chat::ChatState,
    ) {
        if changes.inventory {
            self.inventory.refresh(state.world.inventory().stale());
        }
        for notice in &changes.notices {
            self.tell(notice, messages, chat);
        }
        if changes.characters {
            state.selection = state.world.characters().map(|list| {
                super::character_select::Selection::new(list.selection_id, list.characters.clone())
            });
        }
        if changes.motion
            && let Some(grant) = state.world.motion()
        {
            self.motion.grant(grant);
        }
        if changes.trade {
            self.trade.changed();
        }
        for reply in &changes.replies {
            match reply {
                Reply::ItemUse {
                    session_id,
                    request_id,
                    error,
                } => self
                    .inventory
                    .item_use_result(*session_id, *request_id, error.clone()),
                Reply::InventoryMove {
                    session_id,
                    revision,
                    error,
                } => self.inventory.action_result(
                    *session_id,
                    *revision,
                    error.clone(),
                    state.world.inventory(),
                ),
                Reply::LootTaken { place, accepted } => self.trade.taken(*place, *accepted),
                // The refused crossing no longer holds the player's motion.
                Reply::ZoneLineRefused => self.motion.accepted(),
            }
        }
    }

    /// The session sent the player's move, or refused it.
    fn moved(&mut self, moved: &Moved, drawn: Option<Transform>) {
        self.motion.accepted();
        self.motion.refused.clone_from(&moved.refused);
        if let Some(transform) = drawn {
            self.motion.display_sample(transform, moved.position);
        }
    }

    /// Shows what a notice says where it belongs.
    fn tell(
        &mut self,
        notice: &eq_client_core::world::Notice,
        messages: Option<&hud::messages::Messages>,
        chat: &mut super::chat::ChatState,
    ) {
        use super::notices::Place;
        for (place, said) in super::notices::wording(notice, messages) {
            match place {
                Place::Chat => {
                    // The line takes the colour of the type the server gave it.
                    let mut line = super::chat::system_line(said);
                    line.message_type = notice.message_type();
                    chat.history.push(line);
                }
                Place::Status => self.lines.status.set(said.text),
            }
        }
    }

    /// Forgets what the world's reset made stale on screen and in the panels.
    fn forget(&mut self, reason: Reset, state: &mut OnlineState) {
        if matches!(reason, Reset::Entered | Reset::Camped) {
            // Requests and choices made in the old admission are void.
            *self.combat = super::combat::CombatState::default();
            *self.items = super::items::ItemState::default();
            if let Some(book) = self.book.as_mut() {
                **book = super::spellbook::BookSelection::default();
            }
            if let Some(view) = self.book_view.as_mut() {
                view.page = 0;
                view.spread = 0;
            }
        }
        match reason {
            Reset::Lost { ended, .. } => {
                self.inventory.cancel_actions();
                self.motion.reset(None);
                if ended {
                    state.selection = None;
                    self.inventory.forget();
                }
            }
            Reset::Entered => {
                self.motion.reset(None);
                state.selection = None;
                self.inventory.forget();
            }
            Reset::Zoning { to_bind } => {
                self.inventory.cancel_actions();
                self.motion.reset(None);
                self.lines.status.set(if to_bind {
                    "Respawning at bind"
                } else {
                    "Zoning"
                });
            }
            Reset::Died => {
                self.motion.reset(None);
                self.inventory.cancel_actions();
                self.lines
                    .status
                    .set("Dead - awaiting server bind destination");
            }
            Reset::Camped => {
                // Leave the zone; the world server sends a fresh character list.
                self.inventory.forget();
                self.motion.reset(None);
                self.lines.status.set("Camped - choose a character");
            }
        }
    }
}

/// Applies the session's news to the world and shows what it changed. The
/// world decides what each update means; this only hands its changes to the
/// panels, the chat and the scene.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn receive(
    mut commands: Commands,
    updates: Res<Updates>,
    definitions: (
        Res<ViewerSettings>,
        Option<Res<super::spellbook::SpellNames>>,
        Option<Res<hud::messages::Messages>>,
    ),
    mut state: ResMut<OnlineState>,
    mut panels: Panels,
    mut chat: ResMut<super::chat::ChatState>,
    mut scene: super::zone::Scene,
) {
    let (settings, spell_names, messages) = definitions;
    let Ok(receiver) = updates.0.lock() else {
        return;
    };
    let Some(receiver) = receiver.as_ref() else {
        return;
    };
    let mut ended = false;
    let batch: Vec<_> = std::iter::from_fn(|| match receiver.try_recv() {
        Ok(update) => Some(update),
        Err(std::sync::mpsc::TryRecvError::Empty) => None,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            ended = true;
            None
        }
    })
    .take(256)
    .collect();
    // The installed client's spell data, when there is one.
    let spells: &dyn SpellCatalog = match spell_names.as_deref() {
        Some(names) => names,
        None => &NoSpells,
    };
    // A worker that stopped without saying so (for example after a panic) ends
    // the session here, instead of leaving it looking connected.
    let lost = (ended && !state.world.ended())
        .then_some(WorldUpdate::Connection(eq_client_core::world::Link::Ended));
    if lost.is_some() {
        warn!("The session stopped without saying why");
    }
    for update in batch.into_iter().chain(lost) {
        let changes = state
            .world
            .apply(&update, std::time::Instant::now(), spells);
        trace(&update, &changes, &state.world);
        if let Some(reason) = changes.reset {
            state.loading.clear();
            panels.forget(reason, &mut state);
            scene.forget(reason, &mut commands);
        }
        panels.show(&changes, &mut state, messages.as_deref(), &mut chat);
        if let Some(moved) = &changes.moved {
            panels.moved(moved, scene.player());
        }
        // Offline, the zone on screen is the viewer's own, and the preview
        // admits its player into it.
        if changes.entered
            && state.enabled
            && let Some(directory) = &settings.0.eq_directory
        {
            // The session is this zone's even if its assets fail to load, so
            // nothing of the previous zone stays on screen either.
            scene.leave(&mut commands, &mut state.regions);
            if let (Some(id), Some(player)) =
                (state.world.session_id(), state.world.player().cloned())
            {
                let zone = state.world.zone().to_owned();
                let directory = directory.clone();
                state.loading.start(id, move || {
                    let loading = std::time::Instant::now();
                    let entry = super::zone::Entry::admission(&zone, &player, &directory)
                        .map(super::zone::Entry::prepare);
                    if entry.is_ok() {
                        // Read and prepared off the frame thread, behind the
                        // loading screen.
                        info!(
                            zone = zone.as_str(),
                            milliseconds = loading.elapsed().as_millis(),
                            "Zone loaded"
                        );
                    }
                    entry
                });
            }
        }
        if let Some(position) = changes.placed {
            panels.motion.reset(None);
            scene.place(&mut commands, super::zone::placement(position), None);
        }
        if let WorldUpdate::Chat(line) = update {
            chat.history.push(line);
        }
    }
    let admission = state.world.session_id();
    if let Some(result) = state.loading.poll(admission) {
        match result {
            Ok(mut entry) => {
                if let Some(player) = state.world.player() {
                    entry.place(player.position);
                }
                scene.enter(
                    &mut commands,
                    entry,
                    settings.0.terrain_only,
                    &mut state.regions,
                );
            }
            Err(text) => {
                error!("{text}");
                chat.history.push(super::chat::system_line(text));
            }
        }
    }
}

/// Logs what diagnosing a session needs from its news.
fn trace(update: &WorldUpdate, changes: &eq_client_core::world::Changes, world: &ClientWorld) {
    let event = match update {
        WorldUpdate::Game(event) => event,
        // Every change of connection is logged, so a session that ends
        // leaves its reason behind.
        WorldUpdate::Connection(link) => {
            info!(?link, dead = world.ended(), "Session connection changed");
            return;
        }
        _ => return,
    };
    match event {
        WorldEvent::ItemDetails(item) => debug!("Item definition received: ID {}", item.id),
        WorldEvent::TargetSent(id) if !changes.ignored => debug!("Target packet sent: {id:?}"),
        WorldEvent::HealthPercent { spawn_id, percent }
            if world.target().selected == Some(*spawn_id) =>
        {
            debug!("Target health received: spawn {spawn_id}, {percent}%");
        }
        WorldEvent::Posture { spawn_id, posture } if world.is_player(*spawn_id) => {
            debug!(?posture, "Own posture update");
        }
        WorldEvent::Spell(eq_client_core::SpellUpdate::Interrupted {
            caster_id,
            message_id,
            ..
        }) if world
            .player()
            .is_some_and(|player| u32::from(player.spawn_id) == *caster_id) =>
        {
            debug!(message_id, "Own cast interrupted");
        }
        WorldEvent::Buff(update) if !changes.ignored => debug!(
            spell_id = update.spell_id,
            slot = update.slot,
            removed = update.buff.is_none(),
            "Own buff slot update"
        ),
        WorldEvent::SpellEffect(effect) if !changes.ignored => debug!(
            spell_id = effect.spell_id,
            caster_level = effect.caster_level,
            effect_flag = effect.effect_flag,
            "Own spell effect"
        ),
        // Hunger and thirst, and each bite the session takes for them.
        WorldEvent::Nourishment(nourishment) => debug!(
            food = nourishment.food,
            water = nourishment.water,
            "Stamina report"
        ),
        WorldEvent::NothingToEat { food, water } => debug!(?food, ?water, "Nothing to eat"),
        WorldEvent::Inventory(eq_client_core::inventory::InventoryUpdate::Deduct {
            slot,
            quantity,
        }) => {
            debug!(slot = slot.0, quantity, "Inventory deduction");
        }
        _ => (),
    }
}

/// Puts the world in the state a test needs by sending what the session would.
#[cfg(test)]
pub(crate) mod testing {
    use super::OnlineState;
    use eq_client_core::{
        PlayerState, SpawnState, WorldEvent, WorldPosition, WorldUpdate, world::NoSpells,
    };
    use std::time::Instant;

    /// Puts a world a test built in place of the session's.
    pub(crate) fn set_world(state: &mut OnlineState, world: eq_client_core::world::ClientWorld) {
        state.world = world;
    }

    /// Applies session news to the world.
    pub(crate) fn news(state: &mut OnlineState, events: impl IntoIterator<Item = WorldEvent>) {
        news_at(state, events, Instant::now());
    }

    /// Applies session news to the world as if it arrived at `now`.
    pub(crate) fn news_at(
        state: &mut OnlineState,
        events: impl IntoIterator<Item = WorldEvent>,
        now: Instant,
    ) {
        for event in events {
            state.world.apply(&WorldUpdate::Game(event), now, &NoSpells);
        }
    }

    /// A level 1 human player with this spawn ID, at the origin.
    pub(crate) fn player(spawn_id: u16) -> PlayerState {
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
            size: 6.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
        }
    }

    /// The player's pet: a level 1 earth elemental with this spawn ID, at
    /// the origin, owned by this spawn.
    pub(crate) fn pet(spawn_id: u16, owner: u16) -> SpawnState {
        SpawnState {
            class: Some(1),
            spawn_id,
            name: "Gabober000".into(),
            kind: eq_client_core::SpawnKind::Npc,
            race: 75,
            gender: 2,
            position: WorldPosition::default(),
            velocity: [0.0; 3],
            size: 0.0,
            invisible: false,
            appearance: eq_client_core::outfit::Appearance::default(),
            level: 1,
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
            pet_owner: Some(owner),
            hp_percent: Some(100),
        }
    }

    /// Connects or disconnects the session without ending it.
    pub(crate) fn connect(state: &mut OnlineState, connected: bool) {
        link(
            state,
            if connected {
                eq_client_core::world::Link::Connected
            } else {
                eq_client_core::world::Link::Entering
            },
        );
    }

    /// Puts the session's connection where this says, such as ended.
    pub(crate) fn link(state: &mut OnlineState, link: eq_client_core::world::Link) {
        state
            .world
            .apply(&WorldUpdate::Connection(link), Instant::now(), &NoSpells);
    }

    /// Starts loading the admitted zone's scene in the background, as zone
    /// entry does; it fails once the returned sender sends or drops.
    pub(crate) fn hold_scene(state: &mut OnlineState) -> std::sync::mpsc::Sender<()> {
        let (release, wait) = std::sync::mpsc::channel();
        state
            .loading
            .start(state.world.session_id().unwrap_or_default(), move || {
                let _ = wait.recv();
                Err("Synthetic scene".into())
            });
        release
    }

    /// Takes the background load's result if it is done, as receiving the
    /// next batch does; says whether it was.
    pub(crate) fn take_scene(state: &mut OnlineState) -> bool {
        state.loading.poll(state.world.session_id()).is_some()
    }

    /// Admits this player in this session, connected.
    pub(crate) fn admit(state: &mut OnlineState, session_id: u64, player: PlayerState) {
        enter(state, session_id, player, None);
    }

    /// Admits this player in this session, connected, in a zone with this far clip.
    pub(crate) fn enter(
        state: &mut OnlineState,
        session_id: u64,
        player: PlayerState,
        far_clip: Option<f32>,
    ) {
        news(
            state,
            [WorldEvent::Entered {
                capabilities: eq_client_core::Capability::ALL.to_vec(),
                choices: Vec::new(),
                session_id,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip,
            }],
        );
        connect(state, true);
    }

    /// Puts spawns in the zone.
    pub(crate) fn spawns(state: &mut OnlineState, spawns: Vec<SpawnState>) {
        news(state, [WorldEvent::Spawns(spawns)]);
    }

    /// Puts one spawn in the zone under its own spawn ID.
    pub(crate) fn spawn_entry(state: &mut OnlineState, spawn_id: u16, spawn: SpawnState) {
        assert_eq!(spawn.spawn_id, spawn_id, "a spawn is kept under its own ID");
        spawns(state, vec![spawn]);
    }

    /// Applies a door update as if it arrived at `now`.
    pub(crate) fn doors(
        state: &mut OnlineState,
        update: &eq_client_core::doors::DoorUpdate,
        now: Instant,
    ) {
        news_at(state, [WorldEvent::Doors(update.clone())], now);
    }

    /// Applies a ground-object update.
    pub(crate) fn objects(state: &mut OnlineState, update: &eq_client_core::ground::ObjectUpdate) {
        news(state, [WorldEvent::Objects(update.clone())]);
    }

    /// Moves the player as a server correction would, changing what `change` does.
    pub(crate) fn place_axis(state: &mut OnlineState, change: impl FnOnce(&mut WorldPosition)) {
        let mut position = state.world.player().expect("an admitted player").position;
        change(&mut position);
        place(state, position);
    }

    /// Puts the player where the server says.
    pub(crate) fn place(state: &mut OnlineState, position: WorldPosition) {
        let spawn_id = state.world.player().expect("an admitted player").spawn_id;
        news(
            state,
            [WorldEvent::Position {
                spawn_id,
                position,
                velocity: [0.0; 3],
            }],
        );
    }

    /// Sets the player's mana and endurance, as a resource report would.
    pub(crate) fn resources(state: &mut OnlineState, mana: u32, endurance: u32) {
        news(state, [WorldEvent::Resources { mana, endurance }]);
    }

    /// Gives the player these buffs in their server slots.
    pub(crate) fn buffs(
        state: &mut OnlineState,
        slots: std::collections::BTreeMap<u32, eq_client_core::Buff>,
    ) {
        let size = slots.keys().max().map_or(0, |slot| *slot as usize + 1);
        let mut table = vec![None; size];
        for (slot, buff) in slots {
            table[slot as usize] = Some(buff);
        }
        news(state, [WorldEvent::BuffSnapshot(table)]);
    }

    /// Reports a change to the player's inventory.
    pub(crate) fn inventory(
        state: &mut OnlineState,
        update: eq_client_core::inventory::InventoryUpdate,
    ) {
        news(state, [WorldEvent::Inventory(update)]);
    }

    /// Gives the player this spellbook.
    pub(crate) fn book(state: &mut OnlineState, book: eq_client_core::SpellBook) {
        news(state, [WorldEvent::SpellBook(book)]);
    }

    /// Reports how the spellbook change in flight stands.
    pub(crate) fn book_action(state: &mut OnlineState, status: eq_client_core::BookActionStatus) {
        news(state, [WorldEvent::BookAction(status)]);
    }

    /// Reports a spell notice.
    pub(crate) fn spell(state: &mut OnlineState, update: eq_client_core::SpellUpdate) {
        news(state, [WorldEvent::Spell(update)]);
    }

    /// Holds a cast request for this spell until it is answered.
    pub(crate) fn pending_cast(state: &mut OnlineState, spell_id: Option<u32>) {
        let session_id = state.world.session_id().expect("an admission");
        news(
            state,
            [WorldEvent::CastPending {
                session_id,
                spell_id,
            }],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::PlayerState;

    #[test]
    fn a_blocked_asset_load_keeps_applying_ordered_session_events() {
        use eq_client_core::inventory::InventoryUpdate;
        let (sender, receiver) = std::sync::mpsc::sync_channel(1024);
        let (release, wait) = std::sync::mpsc::channel();
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        state.loading.start(1, move || {
            wait.recv().unwrap();
            Err("Synthetic asset read failure".into())
        });
        let mut app = App::new();
        app.insert_resource(state)
            .insert_resource(Updates(Mutex::new(Some(receiver))))
            .insert_resource(ViewerSettings(super::super::ViewerConfig::default()))
            .init_resource::<hud::HudState>()
            .init_resource::<crate::motion::Controls>()
            .init_resource::<crate::chat::ChatState>()
            .init_resource::<crate::notices::Lines>()
            .init_resource::<crate::combat::CombatState>()
            .init_resource::<crate::trade::TradeState>()
            .init_resource::<crate::items::ItemState>()
            .init_resource::<crate::inventory::InventoryState>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, receive);
        // More than the queue's capacity over multiple frames while I/O is blocked.
        // Alternating invalidation and snapshots makes reordering observable.
        for batch in 0..16 {
            for value in 0..64 {
                let mana = batch * 64 + value;
                for event in [
                    WorldEvent::Inventory(InventoryUpdate::Invalidated),
                    WorldEvent::Resources {
                        mana,
                        endurance: mana,
                    },
                    WorldEvent::Spell(eq_client_core::SpellUpdate::BookDeletion {
                        slot: 0,
                        success: true,
                    }),
                    WorldEvent::Inventory(InventoryUpdate::Snapshot(Vec::new())),
                ] {
                    sender.try_send(WorldUpdate::Game(event)).unwrap();
                }
            }
            app.update();
            assert!(world(&app).in_world());
            assert!(!world(&app).inventory().stale());
            assert_eq!(world(&app).vitals().mana, Some(batch * 64 + 63));
        }
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app
            .world()
            .resource::<crate::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::System)
            .is_empty()
        {
            assert!(std::time::Instant::now() < deadline);
            app.update();
            std::thread::yield_now();
        }
        assert!(world(&app).in_world());
    }

    #[test]
    fn a_zone_entry_batch_keeps_doors_postures_and_the_new_session() {
        use eq_client_core::doors::{Door, DoorUpdate};
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut app = App::new();
        // Admission arrives while the session still reports it is zoning.
        let state = OnlineState::new(true);
        app.insert_resource(state)
            .insert_resource(Updates(Mutex::new(Some(receiver))))
            .insert_resource(ViewerSettings(super::super::ViewerConfig::default()))
            .init_resource::<hud::HudState>()
            .init_resource::<super::super::motion::Controls>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<crate::notices::Lines>()
            .init_resource::<super::super::combat::CombatState>()
            .init_resource::<super::super::trade::TradeState>()
            .init_resource::<super::super::items::ItemState>()
            .init_resource::<super::super::inventory::InventoryState>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, receive);
        let player = PlayerState {
            name: "Example".into(),
            base_attributes: None,
            deity: None,
            class: Some(1),
            spawn_id: 9,
            race: 1,
            gender: 0,
            level: 1,
            position: eq_client_core::WorldPosition::default(),
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
            appearance: eq_client_core::outfit::Appearance::default(),
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
        };
        let spawn = eq_client_core::SpawnState {
            class: None,
            spawn_id: 5,
            name: "a_rat".into(),
            kind: eq_client_core::SpawnKind::Npc,
            race: 1,
            gender: 0,
            position: eq_client_core::WorldPosition::default(),
            velocity: [0.0; 3],
            size: 0.0,
            invisible: false,
            appearance: eq_client_core::outfit::Appearance::default(),
            level: 0,
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
            pet_owner: None,
            hp_percent: None,
        };
        let door = Door {
            id: 3,
            model: "DOOR1".into(),
            position: eq_client_core::WorldPosition::default(),
            incline: 0,
            size: 100,
            open_type: 5,
            state_at_spawn: 0,
            invert_state: 0,
            parameter: 0,
            action: None,
        };
        // No installation is configured, so the zone's assets cannot load.
        for update in [
            WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 2,
                zone: "example".into(),
                player: Box::new(player),
                far_clip: None,
            },
            WorldEvent::Spawns(vec![spawn]),
            WorldEvent::Posture {
                spawn_id: 5,
                posture: eq_client_core::PostureState::Sitting,
            },
            WorldEvent::Doors(DoorUpdate::Spawn(vec![door])),
        ] {
            sender.send(WorldUpdate::Game(update)).unwrap();
        }
        app.update();
        let world = &app.world().resource::<OnlineState>().world;
        assert_eq!(world.session_id(), Some(2));
        assert_eq!(world.player().map(|player| player.spawn_id), Some(9));
        assert_eq!(
            world.posture(5),
            Some(eq_client_core::PostureState::Sitting)
        );
        assert!(world.doors().entries().contains_key(&3));
    }

    #[test]
    fn a_zone_that_fails_to_load_leaves_nothing_of_the_last_one() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        let missing = std::env::temp_dir().join("no-everquest-installed-here");
        let mut app = App::new();
        app.insert_resource(state)
            .insert_resource(Updates(Mutex::new(Some(receiver))))
            .insert_resource(ViewerSettings(super::super::ViewerConfig {
                eq_directory: Some(missing),
                ..default()
            }))
            .init_resource::<hud::HudState>()
            .init_resource::<super::super::motion::Controls>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<crate::notices::Lines>()
            .init_resource::<super::super::combat::CombatState>()
            .init_resource::<super::super::trade::TradeState>()
            .init_resource::<super::super::items::ItemState>()
            .init_resource::<super::super::inventory::InventoryState>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, receive);
        // What the last zone left on screen.
        let old = app.world_mut().spawn(super::super::SceneEntity).id();
        app.insert_resource(super::super::TerrainSurface(vec![[Vec3::ZERO; 3]]))
            .insert_resource(super::super::SceneInfo {
                zone_name: "qeynos2".into(),
            });
        sender
            .send(WorldUpdate::Game(WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 2,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
            }))
            .unwrap();
        app.update();
        assert!(app.world().get_entity(old).is_err());
        assert_eq!(
            app.world().resource::<super::super::TerrainSurface>().0,
            Vec::<[Vec3; 3]>::new()
        );
        assert!(
            app.world()
                .resource::<super::super::Collision>()
                .0
                .is_none()
        );
        assert_eq!(
            app.world().resource::<super::super::SceneInfo>().zone_name,
            ""
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            app.update();
            if app
                .world()
                .resource::<super::super::chat::ChatState>()
                .history
                .lines(eq_client_core::chat::ChatTab::System)
                .iter()
                .any(|(_, line)| line.message.text.contains("could not be loaded"))
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "missing load failure");
            std::thread::yield_now();
        }
        // The session is the new zone's all the same.
        assert_eq!(world(&app).session_id(), Some(2));
    }

    #[test]
    fn a_new_admission_starts_the_windows_over() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        let mut app = App::new();
        app.insert_resource(state)
            .insert_resource(Updates(Mutex::new(Some(receiver))))
            .insert_resource(ViewerSettings(super::super::ViewerConfig::default()))
            .init_resource::<hud::HudState>()
            .init_resource::<super::super::motion::Controls>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<crate::notices::Lines>()
            .init_resource::<super::super::combat::CombatState>()
            .init_resource::<super::super::trade::TradeState>()
            .init_resource::<super::super::items::ItemState>()
            .init_resource::<super::super::inventory::InventoryState>()
            .init_resource::<super::super::spellbook::BookSelection>()
            .init_resource::<super::super::spellbook::BookView>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, receive);
        let item = crate::preview::items().remove(0).details;
        app.world_mut()
            .resource_mut::<super::super::items::ItemState>()
            .open_received(item);
        app.world_mut()
            .resource_mut::<super::super::spellbook::BookView>()
            .page = 2;
        // Zoning keeps the item panel and the book's page.
        sender
            .send(WorldUpdate::Game(WorldEvent::ZoneTransfer(
                eq_client_core::ZoneOffer {
                    zone_id: 2,
                    instance_id: 0,
                    position: eq_client_core::WorldPosition::default(),
                    reason: 0,
                    to_bind: false,
                    solicited: true,
                },
            )))
            .unwrap();
        app.update();
        assert!(
            app.world()
                .resource::<super::super::items::ItemState>()
                .selected()
                .is_some()
        );
        sender
            .send(WorldUpdate::Game(WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 2,
                zone: "qeynos2".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
            }))
            .unwrap();
        app.update();
        assert!(
            app.world()
                .resource::<super::super::items::ItemState>()
                .selected()
                .is_none()
        );
        assert_eq!(
            app.world()
                .resource::<super::super::spellbook::BookView>()
                .page,
            0
        );
    }

    fn world(app: &App) -> &ClientWorld {
        &app.world().resource::<OnlineState>().world
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered integration scenario and its assertions together"
    )]
    fn only_own_death_disables_targeting_and_zone_entry_clears_death() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut app = App::new();
        let mut state = OnlineState::new(true);
        let player = PlayerState {
            name: "Example".into(),
            base_attributes: None,
            deity: None,
            class: Some(1),
            spawn_id: 7,
            race: 1,
            gender: 0,
            level: 1,
            position: eq_client_core::WorldPosition::default(),
            mana: 0,
            endurance: Some(0),
            skills: Some(vec![0; 100]),
            practice_points: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
        };
        let admitted = std::time::Instant::now();
        for update in [
            WorldUpdate::Game(WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 1,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip: None,
            }),
            WorldUpdate::Connection(eq_client_core::world::Link::Connected),
        ] {
            state.world.apply(&update, admitted, &NoSpells);
        }
        app.insert_resource(state)
            .insert_resource(Updates(Mutex::new(Some(receiver))))
            .insert_resource(ViewerSettings(super::super::ViewerConfig::default()))
            .init_resource::<hud::HudState>()
            .init_resource::<super::super::motion::Controls>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<crate::notices::Lines>()
            .init_resource::<super::super::combat::CombatState>()
            .init_resource::<super::super::trade::TradeState>()
            .init_resource::<super::super::items::ItemState>()
            .init_resource::<super::super::inventory::InventoryState>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, receive);
        let buff = eq_client_core::Buff {
            spell_id: 42,
            caster_level: 1,
            effect_type: 2,
            bard_modifier: 10,
            duration_ticks: 5,
            counters: 0,
            caster_id: 7,
        };
        sender
            .send(WorldUpdate::Game(WorldEvent::BuffSnapshot(vec![
                None,
                Some(buff.clone()),
            ])))
            .unwrap();
        app.update();
        assert_eq!(
            world(&app).buffs().slots().as_ref().unwrap().get(&1),
            Some(&buff)
        );
        sender
            .send(WorldUpdate::Game(WorldEvent::Buff(
                eq_client_core::BuffUpdate {
                    spell_id: 42,
                    entity_id: 99,
                    slot: 1,
                    buff: None,
                },
            )))
            .unwrap();
        app.update();
        assert_eq!(world(&app).buffs().slots().as_ref().unwrap().len(), 1);
        sender
            .send(WorldUpdate::Game(WorldEvent::Buff(
                eq_client_core::BuffUpdate {
                    spell_id: 42,
                    entity_id: 7,
                    slot: 1,
                    buff: None,
                },
            )))
            .unwrap();
        app.update();
        assert!(world(&app).buffs().slots().as_ref().unwrap().is_empty());
        let now = std::time::Instant::now();
        let mut timed_player = world(&app).player().cloned().unwrap();
        timed_player.memorized_spells[0] = Some(42);
        timed_player.spell_refresh_ms = Some([10000; 8]);
        {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            let mut book = eq_client_core::SpellBook::default();
            book.apply(&eq_client_core::SpellUpdate::Slot {
                slot: 0,
                spell_id: 42,
                mode: 0,
            });
            // Admitted again with the spell recovering, and its book.
            testing::news_at(
                &mut online,
                [
                    WorldEvent::Entered {
                        capabilities: Vec::new(),
                        choices: Vec::new(),
                        session_id: 1,
                        zone: "qeytoqrg".into(),
                        player: Box::new(timed_player),
                        far_clip: None,
                    },
                    WorldEvent::SpellBook(book),
                ],
                now,
            );
        }
        sender
            .send(WorldUpdate::Game(WorldEvent::ZoneTransfer(
                eq_client_core::ZoneOffer {
                    zone_id: 9,
                    instance_id: 0,
                    position: eq_client_core::WorldPosition::default(),
                    reason: 0,
                    to_bind: false,
                    solicited: true,
                },
            )))
            .unwrap();
        sender
            .send(WorldUpdate::Connection(eq_client_core::world::Link::Zoning))
            .unwrap();
        sender
            .send(WorldUpdate::Game(WorldEvent::ZoneTransferRejected {
                session_id: 99,
                reason: eq_client_core::ZoneRejection::Server(-1),
            }))
            .unwrap();
        app.update();
        assert!(world(&app).pending_transfer().is_some());
        assert!(!world(&app).connected());
        sender
            .send(WorldUpdate::Game(WorldEvent::ZoneTransferRejected {
                session_id: 1,
                reason: eq_client_core::ZoneRejection::Server(-7),
            }))
            .unwrap();
        sender
            .send(WorldUpdate::Connection(
                eq_client_core::world::Link::Connected,
            ))
            .unwrap();
        app.update();
        assert!(world(&app).connected());
        let history = &app
            .world()
            .resource::<super::super::chat::ChatState>()
            .history;
        let notices = history.lines(eq_client_core::chat::ChatTab::System);
        assert_eq!(notices.len(), 1);
        assert!(notices[0].1.message.text.contains("server code -7"));
        assert_eq!(world(&app).spell_book().unwrap().slots()[0], Some(42));
        assert_eq!(
            world(&app).casting().cooldowns.remaining(42, now),
            std::time::Duration::from_secs(10)
        );
        for (current, maximum, percent) in [(34, 34, 100), (17, 34, 50), (35, 34, 100)] {
            sender
                .send(WorldUpdate::Game(WorldEvent::HitPoints {
                    spawn_id: 7,
                    current,
                    maximum,
                    items: eq_client_core::ItemHitPoints::LeftOut,
                }))
                .unwrap();
            app.update();
            assert_eq!(world(&app).health(7), Some(percent));
            assert_eq!(
                world(&app).hit_points(),
                Some((current.unsigned_abs(), maximum.unsigned_abs()))
            );
        }
        for (success, expected) in [(false, Some(42)), (true, None)] {
            testing::book_action(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                eq_client_core::BookActionStatus::AwaitingReply,
            );
            sender
                .send(WorldUpdate::Game(WorldEvent::Spell(
                    eq_client_core::SpellUpdate::BookDeletion { slot: 0, success },
                )))
                .unwrap();
            app.update();
            assert_eq!(
                world(&app).book_action(),
                Some(&eq_client_core::BookActionStatus::AwaitingReply)
            );
            // Raw slot changes are independent from the worker's matched result.
            sender
                .send(WorldUpdate::Game(WorldEvent::BookAction(if success {
                    eq_client_core::BookActionStatus::Confirmed
                } else {
                    eq_client_core::BookActionStatus::Rejected(
                        "Server rejected spellbook deletion".into(),
                    )
                })))
                .unwrap();
            app.update();
            assert_eq!(world(&app).book_action().is_none(), success);
            assert_eq!(world(&app).spell_book().unwrap().slots()[0], expected);
        }
        sender
            .send(WorldUpdate::Game(WorldEvent::Posture {
                spawn_id: 7,
                posture: eq_client_core::PostureState::Sitting,
            }))
            .unwrap();
        sender
            .send(WorldUpdate::Game(WorldEvent::Posture {
                spawn_id: 999,
                posture: eq_client_core::PostureState::Ducking,
            }))
            .unwrap();
        app.update();
        assert_eq!(
            world(&app).posture(7),
            Some(eq_client_core::PostureState::Sitting)
        );
        assert_eq!(world(&app).posture(999), None);
        sender
            .send(WorldUpdate::Game(WorldEvent::Skill {
                skill_id: 22,
                value: 1,
            }))
            .unwrap();
        app.update();
        assert_eq!(
            world(&app).player().unwrap().skills.as_ref().unwrap()[22],
            1
        );
        for level in [2, 1] {
            sender
                .send(WorldUpdate::Game(WorldEvent::Level {
                    current: level,
                    previous: 3 - level,
                    experience: 99,
                }))
                .unwrap();
            app.update();
            assert_eq!(world(&app).player().unwrap().level, level);
            assert_eq!(world(&app).vitals().experience, Some(99));
        }
        let mut door_packet = [0u8; 80];
        door_packet[60] = 7;
        sender
            .send(WorldUpdate::Game(WorldEvent::Doors(
                eq_client_core::doors::decode(0x4c24, &door_packet)
                    .unwrap()
                    .unwrap(),
            )))
            .unwrap();
        sender
            .send(WorldUpdate::Game(WorldEvent::Doors(
                eq_client_core::doors::DoorUpdate::Move { id: 7, action: 2 },
            )))
            .unwrap();
        app.update();
        assert_eq!(world(&app).doors().entries()[&7].action, Some(2));
        sender
            .send(WorldUpdate::Game(WorldEvent::CastPending {
                session_id: 1,
                spell_id: Some(42),
            }))
            .unwrap();
        sender
            .send(WorldUpdate::Game(WorldEvent::CastPending {
                session_id: 2,
                spell_id: None,
            }))
            .unwrap();
        app.update();
        assert_eq!(world(&app).casting().pending, Some(42));
        sender
            .send(WorldUpdate::Game(WorldEvent::CastRejected {
                session_id: 2,
                spell_id: 73,
                reason: "Old admission".into(),
            }))
            .unwrap();
        app.update();
        // A refusal from an earlier admission is not said.
        assert!(
            !app.world()
                .resource::<crate::chat::ChatState>()
                .newest()
                .contains("Old admission")
        );
        sender
            .send(WorldUpdate::Game(WorldEvent::CastRejected {
                session_id: 1,
                spell_id: 73,
                reason: "Target unavailable".into(),
            }))
            .unwrap();
        app.update();
        assert!(
            app.world()
                .resource::<crate::chat::ChatState>()
                .newest()
                .contains("Target unavailable")
        );
        assert_eq!(world(&app).casting().pending, Some(42));
        sender
            .send(WorldUpdate::Game(WorldEvent::CastPending {
                session_id: 1,
                spell_id: None,
            }))
            .unwrap();
        app.update();
        assert_eq!(world(&app).casting().pending, None);
        sender
            .send(WorldUpdate::Game(WorldEvent::CastPending {
                session_id: 1,
                spell_id: Some(42),
            }))
            .unwrap();
        let mut death = eq_client_core::Death {
            spawn_id: 8,
            killer_id: 9,
            corpse_id: 8,
            bind_zone_id: 9,
            corpse_name: None,
        };
        sender
            .send(WorldUpdate::Game(WorldEvent::BookAction(
                eq_client_core::BookActionStatus::Preparing,
            )))
            .unwrap();
        app.update();
        assert_eq!(
            world(&app).book_action(),
            Some(&eq_client_core::BookActionStatus::Preparing)
        );
        assert_eq!(
            world(&app).book_action_revision(),
            5 // Two waits, two deletion replies and one preparation notification.
        );
        sender
            .send(WorldUpdate::Game(WorldEvent::BookAction(
                eq_client_core::BookActionStatus::Preparing,
            )))
            .unwrap();
        app.update();
        assert_eq!(world(&app).book_action_revision(), 6);
        sender
            .send(WorldUpdate::Game(WorldEvent::Death(death.clone())))
            .unwrap();
        app.update();
        assert!(world(&app).death().is_none());
        app.world_mut()
            .resource_mut::<OnlineState>()
            .world
            .select_target(Some(8));
        death.spawn_id = 7;
        sender
            .send(WorldUpdate::Game(WorldEvent::Death(death)))
            .unwrap();
        app.update();
        assert!(world(&app).death().is_some());
        assert_eq!(world(&app).target().selected, None);
        assert_eq!(world(&app).health(7), Some(0));
        assert_eq!(world(&app).casting().pending, None);
        sender
            .send(WorldUpdate::Game(WorldEvent::Spell(
                eq_client_core::SpellUpdate::Began {
                    caster_id: 7,
                    spell_id: 42,
                    duration_ms: 3000,
                },
            )))
            .unwrap();
        app.update();
        assert!(world(&app).casting().cast.is_none());
        assert!(world(&app).book_action().is_none());
        sender
            .send(WorldUpdate::Game(WorldEvent::ZoneTransferRejected {
                session_id: 1,
                reason: eq_client_core::ZoneRejection::Cancelled,
            }))
            .unwrap();
        app.update();
        assert!(!world(&app).connected());
        let player = world(&app).player().cloned().unwrap();
        sender
            .send(WorldUpdate::Game(WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 2,
                zone: "freportw".into(),
                player: Box::new(player),
                far_clip: Some(450.0),
            }))
            .unwrap();
        app.update();
        // Asset loading is intentionally absent in this event-level test.
        assert!(world(&app).death().is_none());
        assert!(world(&app).spawns().is_empty());
        assert!(world(&app).doors().entries().is_empty());
    }
}
