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

const SPELL_DATA_UNAVAILABLE_NOTICE: &str =
    "Spell data unavailable; spell-dependent actions are paused.";

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
    pub deletion: super::character_delete::State,
    /// The current zone's regions, from its assets.
    pub regions: eq_client_assets::regions::ZoneRegions,
    pub enabled: bool,
    /// Established backend policy for the currently attached producer.
    pub(super) chat_link_expansion: Option<eq_client_core::chat::LinkExpansion>,
    /// Changes when a new worker replaces the current connection.
    pub(super) connection_revision: u64,
    server_placement: Option<ServerPlacement>,
    placement_revision: u64,
    loading: super::background::Background<Result<super::zone::PreparedEntry, String>>,
    /// A new zone's admission and the news after it, held back while the
    /// loading screen waits for the player's casting gesture over the zone
    /// they are leaving.
    held: Vec<WorldUpdate>,
}

/// Last server-sourced placement for the current admission, not a move receipt.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct ServerPlacement {
    pub admission_id: u64,
    pub position: eq_client_core::WorldPosition,
    pub source: PlacementSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlacementSource {
    Admission,
    PositionUpdate,
    SameZoneRelocation,
}

impl OnlineState {
    /// The last server placement remains separate from local motion submissions.
    pub(super) fn server_placement(&self) -> Option<ServerPlacement> {
        self.server_placement.filter(|entry| {
            self.world.connected() && self.world.session_id() == Some(entry.admission_id)
        })
    }

    /// A placement or reset breaks continuity for local hazards.
    pub(super) const fn placement_revision(&self) -> u64 {
        self.placement_revision
    }

    fn observe_server_placement(&mut self, changes: &Changes) {
        if changes.reset.is_some()
            || changes.entered
            || changes.placed.is_some()
            || changes.local_pad.is_some()
        {
            self.placement_revision = self.placement_revision.wrapping_add(1);
        }
        if changes.reset.is_some() {
            self.server_placement = None;
        }
        let received = if let Some(position) = changes.placed {
            let source = if changes.same_zone_relocation {
                PlacementSource::SameZoneRelocation
            } else {
                PlacementSource::PositionUpdate
            };
            Some((position, source))
        } else if changes.entered {
            self.world
                .player()
                .map(|p| (p.position, PlacementSource::Admission))
        } else {
            None
        };
        if let Some((admission_id, (position, source))) = self.world.session_id().zip(received) {
            self.server_placement = Some(ServerPlacement {
                admission_id,
                position,
                source,
            });
        }
    }

    /// Whether the admitted zone's scene is still loading in the background.
    /// A zone whose files fail to load is not: its spawns still show.
    pub(crate) fn scene_loading(&self) -> bool {
        self.loading.pending(self.world.session_id())
    }

    /// What the server has told the client.
    pub(super) fn world(&self) -> &ClientWorld {
        &self.world
    }

    /// Captures the exact currently selected identity for the ordinary dialog.
    pub(crate) fn prepare_character_deletion(
        &mut self,
    ) -> Result<eq_client_core::deletion::Request, String> {
        let selection = self
            .selection
            .as_ref()
            .ok_or("Character selection is unavailable")?;
        if selection.entering() {
            return Err("Character entry is already in progress".into());
        }
        let name = selection
            .entry(selection.chosen().ok_or("Select a character first")?)
            .ok_or("Character is not listed")?
            .name
            .clone();
        let list = self
            .world
            .characters()
            .filter(|list| list.scope() == selection.scope())
            .ok_or("Character list is no longer current")?;
        self.deletion.prepare(list, &name)
    }

    /// Shared sender for the ordinary Yes answer and the explicit script step.
    pub(crate) fn submit_character_deletion(
        &mut self,
        request: &eq_client_core::deletion::Request,
        confirmation: &str,
        outbox: &crate::outbox::Outbox,
    ) -> Result<(), String> {
        let selection = self
            .selection
            .as_ref()
            .filter(|selection| !selection.entering())
            .ok_or("Character selection is unavailable")?;
        let list = self
            .world
            .characters()
            .filter(|list| list.scope() == selection.scope())
            .ok_or("Character list is no longer current")?;
        let selected = selection
            .chosen()
            .and_then(|slot| selection.entry(slot))
            .map(|entry| entry.name.as_str());
        self.deletion
            .submit(request, confirmation, list, selected, |request| {
                outbox
                    .send(
                        &self.world,
                        eq_client_core::ClientCommand::DeleteCharacter(request),
                    )
                    .map_err(|error| error.to_string())
            })
    }

    /// An explicitly named script intent chooses by exact identity, then uses
    /// the same confirmation and sender as the ordinary selection screen.
    pub(crate) fn script_character_deletion(
        &mut self,
        name: &str,
        confirmation: &str,
        outbox: &crate::outbox::Outbox,
    ) -> Result<eq_client_core::deletion::Request, String> {
        if self.deletion.blocked() {
            return Err("Character deletion is already awaiting attention".into());
        }
        if name != confirmation {
            return Err("Deletion confirmation must repeat the exact name".into());
        }
        if !self
            .selection
            .as_mut()
            .is_some_and(|selection| selection.choose_exact(name))
        {
            return Err("Deletion requires an exact listed character name".into());
        }
        let request = self.prepare_character_deletion()?;
        self.submit_character_deletion(&request, confirmation, outbox)?;
        Ok(request)
    }

    /// Queues and projects a move through the world's single inventory owner.
    pub(crate) fn queue_inventory_move(
        &mut self,
        request: eq_client_core::inventory::InventoryMove,
        actor: eq_client_core::inventory::InventoryActor,
        sender: &crate::outbox::Outbox,
    ) -> anyhow::Result<()> {
        self.world
            .queue_inventory_move(request, actor, |world, request| {
                Ok(sender.send(
                    world,
                    eq_client_core::ClientCommand::MoveInventory(request.clone()),
                )?)
            })
    }

    /// The player took off in a jump that costs this much endurance, shown
    /// at once ([`eq_client_core::world::ClientWorld::spend_endurance`]).
    pub(super) fn spend_endurance(&mut self, cost: u32) {
        self.world.spend_endurance(cost);
    }

    /// The player asked to camp, ending there.
    pub(super) fn camp_to(&mut self, to: eq_client_core::camp::CampTo) {
        self.world.camp_to(to);
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

    /// Starts over for a new session: the world forgets what the servers
    /// said, keeping the player's own choices, and no character choice is
    /// in progress.
    pub(super) fn restart(&mut self) {
        self.connection_revision = self.connection_revision.wrapping_add(1);
        self.deletion.ended();
        self.chat_link_expansion = None;
        self.world.restart();
        self.selection = None;
        self.loading.clear();
        self.held.clear();
    }

    /// Clears a session the player cancelled. An ended link tells the
    /// script runner to release its input, without implying logout success.
    pub(super) fn disconnect(
        &mut self,
        panels: &mut Panels,
        scene: &mut super::zone::Scene,
        commands: &mut Commands,
    ) {
        let old_gems = super::spellbook::sets::snapshot(&self.world);
        self.restart();
        let changes = self.tell(
            &WorldUpdate::Connection(eq_client_core::world::Link::Ended),
            std::time::Instant::now(),
            &NoSpells,
        );
        if let Some(reason) = changes.reset {
            self.reset_presentation((reason, old_gems), panels, scene, commands);
        }
        scene.leave(commands, &mut self.regions);
    }

    fn reset_presentation(
        &mut self,
        (reason, old_gems): (Reset, Option<super::spellbook::sets::AdmissionGems>),
        panels: &mut Panels,
        scene: &mut super::zone::Scene,
        commands: &mut Commands,
    ) {
        self.loading.clear();
        if let Some(sets) = panels.spell_sets.as_mut() {
            sets.reset(reason, old_gems);
        }
        panels.forget(reason, self);
        scene.forget(reason, commands);
    }

    /// Bind NPC consider presentation to the selected worker's policy.
    pub(super) const fn consider_npcs_by_level(&mut self, enabled: bool) {
        self.world.consider_npcs_by_level(enabled);
    }

    /// Bind host planning to the selected worker's placement policy.
    pub(super) fn plan_inventory_as(
        &mut self,
        rules: Option<eq_client_core::inventory::MoveRules>,
    ) {
        self.world.plan_inventory_as(rules);
    }

    /// The player asked to play on a world of the login server's list.
    pub(super) fn ask_server(&mut self, index: usize) {
        self.world.ask_server(index);
    }

    /// Runs the world's clocks.
    pub(super) fn tick(&mut self, now: std::time::Instant, spells: &dyn SpellCatalog) {
        self.world.tick(now, spells);
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

    /// Takes the oldest guild invitation not yet answered or asked about.
    pub(super) fn take_guild_invitation(
        &mut self,
    ) -> Option<eq_client_core::world::GuildInvitation> {
        self.world.take_guild_invitation()
    }

    /// The front end queued an answer to this exact offer.
    pub(super) fn dismiss_consent(&mut self, id: eq_client_core::confirmations::ConsentId) {
        self.world.dismiss_consent(id);
    }

    /// The player closed the book or note they were reading.
    pub(super) fn close_reading(&mut self) {
        self.world.close_reading();
    }

    pub(super) fn close_player_inspection(&mut self) {
        self.world.close_player_inspection();
    }

    /// The track window closed, by its button or `/stoptracking`.
    pub(super) fn close_track_window(&mut self) {
        self.world.close_track_window();
    }

    /// The player chose an entry of the track window, or none.
    pub(super) fn select_tracked(&mut self, spawn_id: Option<u16>) {
        self.world.select_tracked(spawn_id);
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
            deletion: default(),
            regions: eq_client_assets::regions::ZoneRegions::default(),
            enabled,
            chat_link_expansion: None,
            connection_revision: 0,
            server_placement: None,
            placement_revision: 0,
            loading: default(),
            held: Vec::new(),
        }
    }
}

impl OnlineState {
    /// The news to apply this frame: any held back, then `batch`. The
    /// player's casting gesture, as Gate's, plays to its end over the zone
    /// they are leaving before the loading screen is drawn, even once the
    /// next zone has admitted them (inferred, 6.230): while the screen is
    /// `waiting` for it, the admission and what follows it are held back,
    /// unapplied. Only the screen waits; the session takes the new zone as it
    /// always does.
    fn news(
        &mut self,
        batch: impl IntoIterator<Item = WorldUpdate>,
        waiting: bool,
    ) -> Vec<WorldUpdate> {
        let mut news = std::mem::take(&mut self.held);
        news.extend(batch);
        if waiting && let Some(admission) = news.iter().position(admits) {
            self.held = news.split_off(admission);
        }
        news
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
    movement_sync: Option<ResMut<'w, super::options::MovementSync>>,
    inventory: ResMut<'w, super::inventory::InventoryState>,
    combat: ResMut<'w, super::combat::CombatState>,
    trade: ResMut<'w, super::trade::TradeState>,
    items: ResMut<'w, super::items::ItemState>,
    book: Option<ResMut<'w, super::spellbook::BookSelection>>,
    book_view: Option<ResMut<'w, super::spellbook::BookView>>,
    spell_sets: Option<ResMut<'w, super::spellbook::sets::Sets>>,
    sounds: ResMut<'w, super::sounds::Sounds>,
    marquee: Option<ResMut<'w, super::marquee::Banners>>,
    combat_text: Option<ResMut<'w, super::combat_text::CombatText>>,
    weather: Option<ResMut<'w, super::weather::Field>>,
    projectiles: Option<ResMut<'w, super::projectiles::Flights>>,
    socials: Option<ResMut<'w, super::socials::State>>,
}

impl Panels<'_> {
    /// Reconcile merchant intent only with admitted worker updates.
    fn merchant_news(&mut self, update: &WorldUpdate, changes: &Changes, world: &ClientWorld) {
        if !changes.ignored {
            self.trade.merchant_news(update, world);
        }
    }

    /// Shows what the world's changes mean for the panels: the inventory, the
    /// notices, motion, the trade windows and the answers to the panels' own
    /// requests; and plays the sounds the server names.
    fn show(
        &mut self,
        changes: &Changes,
        state: &mut OnlineState,
        messages: Option<&hud::messages::Messages>,
        chat: &mut super::chat::ChatState,
    ) {
        // Capture received facts here; action/draw systems apply the current
        // character's options after its profile has loaded for this frame.
        self.trade.killed(state.world(), &changes.kills);
        if let Some(text) = self.combat_text.as_mut() {
            text.hear(state.world(), &changes.damage, true);
        }
        if changes.attack_ended {
            self.combat.end_quietly();
        }
        if let Some((flights, flight)) = self.projectiles.as_mut().zip(changes.projectile.as_ref())
        {
            flights.receive(state.world(), flight, std::time::Instant::now());
        }
        if changes.inventory {
            self.inventory.refresh(state.world.inventory().stale());
        }
        for notice in &changes.notices {
            self.tell(notice, messages, chat);
        }
        if state.world.in_world() {
            for cue in &changes.sounds {
                self.sounds.play_cue(cue);
            }
        }
        if let Some((banners, marquee)) = self.marquee.as_mut().zip(changes.marquee.as_ref()) {
            banners.receive(marquee, std::time::Instant::now());
        }
        if changes.characters {
            if let Some(list) = state.world.characters() {
                state.deletion.listed(list);
            }
            state.selection = state.world.characters().map(|list| {
                super::character_select::Selection::new(list.selection_id, list.characters.clone())
                    .with_creation(list.creation)
                    .with_deletion(list.scope(), list.deletion)
            });
        }
        if let Some(selection) = state.selection.as_mut() {
            for notice in &changes.notices {
                selection.hear(notice);
            }
        }
        if changes.motion
            && let Some(grant) = state.world.motion()
        {
            log_grant(grant);
            self.motion.grant(grant, state.world.generation().root());
        }
        if changes.trade {
            self.trade.changed();
        }
        for reply in &changes.replies {
            match reply {
                Reply::CharacterDeletion(update) => {
                    state.deletion.hear(update, state.world.characters());
                }
                Reply::ItemUse {
                    session_id,
                    request_id,
                    error,
                    present_error,
                } => self.inventory.item_use_result(
                    *session_id,
                    *request_id,
                    error.clone(),
                    *present_error,
                ),
                Reply::InventoryMove {
                    session_id,
                    revision,
                    error,
                    present_error,
                } => self.inventory.action_result(
                    *session_id,
                    *revision,
                    error.clone(),
                    *present_error,
                    state.world.inventory(),
                ),
                Reply::LootRequest {
                    session_id,
                    created,
                    request,
                    status,
                } => {
                    if let Some(reason) =
                        self.trade
                            .loot_result(state, *session_id, *created, *request, status)
                    {
                        chat.refuse(reason);
                    }
                }
                Reply::LootTaken { place, accepted } => self.trade.taken(*place, *accepted),
                Reply::LootReceived { place } => self.trade.received(*place),
                // A crossing without a transfer releases the movement hold.
                Reply::ZoneLineRefused | Reply::SameZoneBoundary => self.motion.crossing_resolved(),
                Reply::MotionRefused { created, reason } => {
                    self.motion
                        .move_refused(state.world().session_id(), *created, reason);
                }
            }
        }
    }

    /// The worker's local result for the player's move.
    fn moved(&mut self, admission_id: Option<u64>, moved: &Moved, drawn: Option<Transform>) {
        debug!(
            admission_id = ?admission_id,
            position = ?moved.position,
            outcome = if moved.refused.is_some() { "local_refused" } else { "local_accepted" },
            refused = ?moved.refused,
            "Current-admission movement worker result; no server acknowledgment implied"
        );

        if self.motion.moved(admission_id, moved.created) {
            self.motion.resolve_water_entry(moved.refused.is_some());
            self.motion.refused.clone_from(&moved.refused);
        }
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
                    // The line takes the colour of the type the server gave
                    // it, else the one its own words take.
                    let mut line = super::chat::system_line(said);
                    line.message_type = notice.message_type().or(line.message_type);
                    chat.history.push(line);
                }
                Place::Status => self.lines.status.set(said.words.text),
            }
        }
    }

    /// Forgets what the world's reset made stale on screen and in the panels.
    fn forget(&mut self, reason: Reset, state: &mut OnlineState) {
        // A death keeps the zone, so the killing blow's sound and any banner
        // already queued in this batch still play.
        if !matches!(reason, Reset::Died) {
            self.sounds.reset();
            if let Some(banners) = self.marquee.as_mut() {
                banners.clear();
            }
        }
        if let Some(sync) = &mut self.movement_sync {
            // The world clears the receipt here, even when a refused
            // transfer reconnects the same admission in this receive batch.
            sync.invalidate();
        }
        if let Some(flights) = self.projectiles.as_mut() {
            flights.reset(reason);
        }
        if let Some(socials) = self.socials.as_mut() {
            socials.reset();
        }
        if !matches!(reason, Reset::Died)
            && let Some(weather) = self.weather.as_mut()
        {
            // A rejected transfer can restore the same admission in this batch.
            **weather = super::weather::Field::default();
        }
        if matches!(
            reason,
            Reset::Entered | Reset::Camped | Reset::Lost { ended: true, .. }
        ) {
            state.deletion.ended();
            // Requests and choices made in the old admission are void.
            *self.combat = super::combat::CombatState::default();
            *self.items = super::items::ItemState::default();
            self.trade.reset_opening_shop();
            if let Some(book) = self.book.as_mut() {
                **book = super::spellbook::BookSelection::default();
            }
            if let Some(view) = self.book_view.as_mut() {
                **view = super::spellbook::BookView::default();
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

/// Tells the session the player bled out once the world says so: the
/// server's report, with what the player's items add, is at the threshold of
/// a server type that leaves such a death to the client. The session checks
/// it again; until it takes the report, the world asks again on the
/// server's next one.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn report_bleed_out(mut state: ResMut<OnlineState>, outbox: Res<super::outbox::Outbox>) {
    if state.world.bled_out().is_none() {
        return;
    }
    let Some(stamp) = outbox.peek(&state.world) else {
        return;
    };
    let command = eq_client_core::ClientCommand::BledOut {
        session_id: stamp.session_id,
        created: stamp.created,
    };
    if outbox.tell(&state.world, command) {
        let hp = state.world.take_bled_out();
        debug!(?hp, "Reported that the player bled out");
    }
}

/// Whether news that stopped without an end came from a session that
/// vanished, rather than one that closed it itself, whose last word did not
/// fit in the queue and whose own reason says why it ended.
fn vanished(front: Option<&super::login::FrontEnd>) -> bool {
    !front.is_some_and(super::login::FrontEnd::session_closed)
}

/// Tells the world which generation's program carries spawns on between
/// reports: the installed client's.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn carry_as(settings: Res<ViewerSettings>, mut state: ResMut<OnlineState>) {
    if settings.is_changed() || state.is_added() {
        state
            .world
            .carry_as(super::entities::dialect(settings.0.installed_client));
    }
}

/// Applies the session's news to the world and shows what it changed. The
/// world decides what each update means; this only hands its changes to the
/// panels, the chat and the scene.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
#[allow(
    clippy::too_many_lines,
    reason = "keep ordered world reduction, panel resets and zone loading together"
)]
pub(super) fn receive(
    mut commands: Commands,
    (updates, front, outbox): (
        Res<Updates>,
        Option<Res<super::login::FrontEnd>>,
        Option<Res<super::outbox::Outbox>>,
    ),
    definitions: (
        Res<ViewerSettings>,
        Option<ResMut<super::spellbook::SpellNames>>,
        Option<Res<hud::messages::Messages>>,
    ),
    mut state: ResMut<OnlineState>,
    mut panels: Panels,
    mut chat: ResMut<super::chat::ChatState>,
    (mut scene, loading): (super::zone::Scene, Option<Res<super::loading::Loading>>),
) {
    let (settings, mut spell_names, messages) = definitions;
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
    // Provider facts precede the admission/loading gate. The producer binding
    // is retained verbatim, including for old queued Connecting notifications.
    .filter(|update| {
        if let WorldUpdate::SpellTable(fact) = update {
            if spell_names
                .as_deref_mut()
                .is_some_and(|spells| spells.receive(fact))
            {
                chat.history
                    .push(super::chat::system_line(SPELL_DATA_UNAVAILABLE_NOTICE));
            }
            false
        } else {
            true
        }
    })
    .collect();
    if let Some(outbox) = outbox.as_deref() {
        outbox.set_spell_ready(
            spell_names
                .as_deref()
                .is_none_or(super::spellbook::SpellNames::ready),
        );
    }
    // The installed client's spell data, when there is one.
    let spells: &dyn SpellCatalog = match spell_names.as_deref() {
        Some(names) => names,
        None => &NoSpells,
    };
    // A worker that stopped without saying so (for example after a panic) ends
    // the session here, instead of leaving it looking connected; once any
    // held news has applied, so it is said once.
    let lost = (ended && !state.world.ended() && state.held.is_empty())
        .then_some(WorldUpdate::Connection(eq_client_core::world::Link::Ended));
    if lost.is_some() && vanished(front.as_deref()) {
        warn!("The session stopped without saying why");
    }
    let waiting = loading.is_some_and(|loading| loading.waiting());
    let news = state.news(batch.into_iter().chain(lost), waiting);
    for update in news {
        let (changes, before, old_gems) =
            apply(&mut state.world, &update, spells, outbox.as_deref());
        state.observe_server_placement(&changes);
        trace(&update, &changes, &state.world);
        panels.merchant_news(&update, &changes, &state.world);
        if let Some(reason) = changes.reset {
            chat.socials = super::socials::Inbox::default();
            match reason {
                Reset::Entered => chat.admit_links(&state),
                Reset::Camped | Reset::Lost { ended: true, .. } => chat.clear_link_scope(),
                _ => (),
            }
            state.reset_presentation((reason, old_gems), &mut panels, &mut scene, &mut commands);
        }
        panels.show(&changes, &mut state, messages.as_deref(), &mut chat);
        if let Some(moved) = &changes.moved {
            panels.moved(state.world.session_id(), moved, scene.player());
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
            start_entry(&mut state, &settings.0, directory, &mut chat);
        }
        if let Some(position) = changes.placed {
            let at = |p: eq_client_core::WorldPosition| [p.x, p.y, p.z];
            info!(from = ?before.map(at), to = ?at(position), "Server corrected the player");
            if changes.same_zone_relocation {
                panels.motion.same_zone_landed();
            } else {
                panels.motion.rebase();
            }
            if let Some(sets) = panels.spell_sets.as_mut() {
                sets.interrupt("The server moved the character.");
            }
            scene.place(&mut commands, super::zone::placement(position), None);
        }
        if let Some(pad) = changes.local_pad {
            info!(entry = ?pad.entry, landing = ?pad.landing, "Crossed a local WLD pad");
            panels.motion.same_zone_landed();
            if let Some(sets) = panels.spell_sets.as_mut() {
                sets.interrupt("The character crossed a pad.");
            }
            scene.place(&mut commands, super::zone::placement(pad.landing), None);
        }
        if let WorldUpdate::Chat(line) = update {
            chat.receive(line, &state);
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

/// Ends the program once the player camped to the desktop, as the
/// character list's Quit does where there is no login screen, so what is
/// saved on any exit is saved.
pub(super) fn leave(mut state: ResMut<OnlineState>, mut exit: MessageWriter<AppExit>) {
    // Read before written, so the state is marked changed only then.
    if state.world.leaving() && state.world.take_leaving() {
        info!("Camped to the desktop");
        exit.write(AppExit::Success);
    }
}

/// Applies one update, settles any local book request it answers, and
/// preserves the old position and gems for corrections and resets.
fn apply(
    world: &mut ClientWorld,
    update: &WorldUpdate,
    spells: &dyn SpellCatalog,
    outbox: Option<&super::outbox::Outbox>,
) -> (
    Changes,
    Option<eq_client_core::WorldPosition>,
    Option<super::spellbook::sets::AdmissionGems>,
) {
    let before = world.player().map(|player| player.position);
    let old_gems = super::spellbook::sets::snapshot(world);
    let changes = world.apply(update, std::time::Instant::now(), spells);
    if let Some(outbox) = outbox
        && !changes.ignored
    {
        match changes.reset {
            Some(Reset::Entered | Reset::Camped | Reset::Lost { ended: true, .. }) => {
                outbox.reset_book();
            }
            Some(Reset::Lost { ended: false, .. }) => outbox.interrupt_book(world.session_id()),
            _ => {}
        }
        if let WorldUpdate::Game(event) = update {
            outbox.book_news(event, world.session_id());
        }
        // The program lies the player down for a feign spell's landing
        // (read R21); the session holds no spell data, so it is told. The
        // outbox shows why it did not go.
        if changes.feign_landed
            && let Some(spawn_id) = world.player().map(|player| player.spawn_id)
        {
            let _ = outbox.post(world, |stamp| eq_client_core::ClientCommand::FeignLanded {
                session_id: stamp.session_id,
                spawn_id,
                created: stamp.created,
            });
        }
    }
    (changes, before, old_gems)
}

/// Logs the session's word on how the player may move: a withdrawal (no
/// speed, as after a server correction until it grants again) or a grant.
fn log_grant(grant: eq_client_core::world::MotionGrant) {
    let eq_client_core::world::MotionGrant {
        units_per_second,
        backward_units_per_second,
        walk_units_per_second,
        strafe_units_per_second,
        strafe_walk_units_per_second,
        swim_backward_units_per_second,
        diagonal,
        falls,
        jump_factor,
    } = grant;
    if units_per_second.is_none() {
        info!(falls, "Movement withdrawn");
    } else if units_per_second == Some(0.0) {
        info!(falls, "Movement rooted");
    } else {
        info!(
            ?units_per_second,
            ?backward_units_per_second,
            ?walk_units_per_second,
            ?strafe_units_per_second,
            ?strafe_walk_units_per_second,
            ?swim_backward_units_per_second,
            ?diagonal,
            falls,
            jump_factor,
            "Movement granted"
        );
    }
}

/// Whether the news is a new admission, which brings its zone.
const fn admits(update: &WorldUpdate) -> bool {
    matches!(update, WorldUpdate::Game(WorldEvent::Entered { .. }))
}

/// Logs what diagnosing a session needs from its news.
/// Starts loading the zone the world just admitted the player to, off the
/// frame thread, or says why there is nothing to load.
fn start_entry(
    state: &mut OnlineState,
    settings: &crate::ViewerConfig,
    directory: &std::path::Path,
    chat: &mut super::chat::ChatState,
) {
    if let (Some(id), Some(player)) = (state.world.session_id(), state.world.player().cloned()) {
        let zone = state.world.zone().to_owned();
        let (directory, navmesh) = (directory.to_path_buf(), settings.navmesh);
        let drawn_by = (
            settings.installed_client,
            state.world.classic_model_scale_flag(),
        );
        let titanium_swimming = drawn_by.0 == eq_client_assets::ui::InstalledClient::Titanium
            && state.world.can(eq_client_core::Capability::Swimming);
        state.loading.start(id, move || {
            let loading = std::time::Instant::now();
            let entry = super::zone::Entry::admission(&zone, &player, drawn_by, &directory)
                .map(|entry| entry.prepare(navmesh, titanium_swimming));
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
    } else {
        // Nothing to show the zone around, said as it was before the
        // load moved off the frame.
        let text = "No admitted player".to_owned();
        error!("{text}");
        chat.history.push(super::chat::system_line(text));
    }
}

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
        WorldEvent::TargetSent { state, .. } if !changes.ignored => {
            debug!("Target packet sent: {:?}", state.spawn_id);
        }
        WorldEvent::HealthPercent { spawn_id, percent }
            if world.target().selected == Some(*spawn_id) =>
        {
            debug!("Target health received: spawn {spawn_id}, {percent}%");
        }
        WorldEvent::Posture { spawn_id, posture } if world.is_player(*spawn_id) => {
            debug!(?posture, "Own posture update");
        }
        WorldEvent::Spell(eq_client_core::SpellUpdate::Began {
            caster_id,
            spell_id,
            duration_ms,
        }) if world.is_player(*caster_id) => {
            debug!(spell_id, duration_ms, "Own cast began");
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
        // A zone header, at entry or sent again: its sky, read just before,
        // and its clip, with the far clip the world now draws to.
        WorldEvent::ZoneDescription(description) => {
            let sky = world.sky();
            debug!(
                sky = sky.map(|sky| sky.sky),
                fog_color = ?sky.map(|sky| sky.fog[0].color),
                fog_near = sky.map(|sky| sky.fog[0].near),
                fog_far = sky.map(|sky| sky.fog[0].far),
                min_clip = description.min_clip,
                max_clip = description.max_clip,
                far_clip = world.far_clip(),
                classic_model_scale_flag = description.classic_model_scale_flag,
                "Zone header decoded"
            );
        }
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

    /// Selects the native generation before synthetic admission/door news.
    pub(crate) fn generation(state: &mut OnlineState, generation: eq_client_core::GameDialect) {
        state.world.carry_as(generation);
    }

    pub(crate) fn empty_carried_coins(state: &mut OnlineState) {
        use eq_client_core::Coins;
        news(
            state,
            [
                WorldEvent::Coins(Coins::default()),
                WorldEvent::CoinsElsewhere {
                    cursor: Coins::default(),
                    bank: Coins::default(),
                    given: Coins::default(),
                    offered: Coins::default(),
                },
            ],
        );
    }

    /// Applies one update as the session's receiver hands it over.
    pub(crate) fn apply(state: &mut OnlineState, update: &WorldUpdate) {
        let changes = state.world.apply(update, Instant::now(), &NoSpells);
        state.observe_server_placement(&changes);
    }

    /// Applies session news to the world.
    pub(crate) fn news(state: &mut OnlineState, events: impl IntoIterator<Item = WorldEvent>) {
        news_at(state, events, Instant::now());
    }

    /// Applies session news to the world with this spell data, as the
    /// receiver does with the loaded spell file.
    pub(crate) fn news_with(
        state: &mut OnlineState,
        events: impl IntoIterator<Item = WorldEvent>,
        spells: &dyn eq_client_core::world::SpellCatalog,
    ) {
        for event in events {
            let changes = state
                .world
                .apply(&WorldUpdate::Game(event), Instant::now(), spells);
            state.observe_server_placement(&changes);
        }
    }

    /// Applies session news to the world as if it arrived at `now`.
    pub(crate) fn news_at(
        state: &mut OnlineState,
        events: impl IntoIterator<Item = WorldEvent>,
        now: Instant,
    ) {
        for event in events {
            let changes = state.world.apply(&WorldUpdate::Game(event), now, &NoSpells);
            state.observe_server_placement(&changes);
        }
    }

    /// A level 1 human player with this spawn ID, at the origin.
    pub(crate) fn player(spawn_id: u16) -> PlayerState {
        PlayerState {
            name: "Example".into(),
            titanium_environment: None,
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
            languages: None,
            disciplines: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 6.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
            posture: None,
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
            forward: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            size: 0.0,
            invisible: false,
            appearance: eq_client_core::outfit::Appearance::default(),
            level: 1,
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
            pet_owner: Some(owner),
            hp_percent: Some(100),
            posture: None,
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
        state.plan_inventory_as(Some(eq_client_core::inventory::MoveRules::EqEmu));
        news(
            state,
            [WorldEvent::Entered {
                capabilities: eq_client_core::Capability::ALL.to_vec(),
                choices: Vec::new(),
                session_id,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip,
                spell_landing_flag: None,
                zone_id: 0,
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
                forward: 0.0,
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

    /// Delivers a book result to both the world and the outgoing queue.
    pub(crate) fn book_news(
        state: &mut OnlineState,
        outbox: &crate::outbox::Outbox,
        event: WorldEvent,
    ) {
        outbox.book_news(&event, state.world.session_id());
        news(state, [event]);
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
    use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
    use eq_client_core::PlayerState;

    mod spell_notices;

    /// A feign spell's landing on the player goes to the session as one
    /// `FeignLanded`, and nothing else: no auto attack off (read R21).
    #[test]
    fn a_feign_spell_landing_tells_the_session_once_and_leaves_attack_alone() {
        use crate::outbox::Outbox;
        use eq_client_core::{ClientCommand, SpellEffect, world::SpellTiming};
        use std::sync::mpsc;

        struct FeignFortyTwo;
        impl SpellCatalog for FeignFortyTwo {
            fn timing(&self, _spell: u32) -> Option<SpellTiming> {
                None
            }
            fn instant_effect(&self, _spell: u32) -> bool {
                false
            }
            fn feigns(&self, spell: u32) -> bool {
                spell == 42
            }
        }
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 5, testing::player(7));
        let (sender, sent) = mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(sender));
        let landing = |spell_id| {
            WorldUpdate::Game(WorldEvent::SpellEffect(SpellEffect {
                target_id: 7,
                caster_id: 7,
                caster_level: 20,
                instrument_modifier: 10,
                spell_id,
                spell_level: 20,
                effect_flag: 4,
                push: None,
            }))
        };
        let (changes, ..) = apply(
            &mut state.world,
            &landing(42),
            &FeignFortyTwo,
            Some(&outbox),
        );
        assert!(changes.feign_landed && !changes.attack_ended);
        let commands: Vec<_> = sent.try_iter().collect();
        assert!(
            matches!(
                commands[..],
                [ClientCommand::FeignLanded {
                    session_id: 5,
                    spawn_id: 7,
                    ..
                }]
            ),
            "{commands:?}"
        );
        // Another spell's landing says nothing.
        apply(
            &mut state.world,
            &landing(43),
            &FeignFortyTwo,
            Some(&outbox),
        );
        assert!(sent.try_recv().is_err());
    }

    #[test]
    fn a_resumed_admission_keeps_manual_book_ownership_until_the_worker_result() {
        use crate::outbox::{Outbox, Refusal};
        use eq_client_core::{
            BookActionStatus, ClientCommand, SpellBook, SpellUpdate, world::Link,
        };
        use std::sync::mpsc;

        let mut state = OnlineState::new(true);
        let mut player = testing::player(7);
        player.memorized_spells[1] = Some(42);
        testing::admit(&mut state, 5, player);
        testing::book(&mut state, SpellBook::default());
        let (sender, sent) = mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(sender));
        let forget = |stamp: crate::outbox::Stamp| ClientCommand::ForgetSpell {
            session_id: stamp.session_id,
            gem: 1,
            spell_id: 42,
            created: stamp.created,
        };
        outbox.post(state.world(), forget).unwrap();
        sent.try_recv().unwrap();
        for update in [
            WorldUpdate::Game(WorldEvent::BookAction(BookActionStatus::Submitted)),
            WorldUpdate::Connection(Link::Zoning),
            WorldUpdate::Connection(Link::Zoning),
            WorldUpdate::Connection(Link::Connected),
        ] {
            apply(&mut state.world, &update, &NoSpells, Some(&outbox));
        }
        assert_eq!(state.world().session_id(), Some(5));
        assert!(state.world().in_world());
        assert!(state.world().spell_book().is_some());
        assert_eq!(outbox.take_refused(), [Refusal::BookUnconfirmed]);

        for (slot, spell_id) in [(0, 42), (1, 73), (1, 42)] {
            assert_eq!(
                outbox.post(state.world(), forget),
                Err(Refusal::BookUnconfirmed)
            );
            assert!(matches!(sent.try_recv(), Err(mpsc::TryRecvError::Empty)));
            let update = WorldUpdate::Game(WorldEvent::Spell(SpellUpdate::Slot {
                slot,
                spell_id,
                mode: 2,
            }));
            apply(&mut state.world, &update, &NoSpells, Some(&outbox));
            assert_eq!(state.world().gem(1), (slot != 1).then_some(42));
        }

        assert_eq!(
            outbox.begin_spell_set(state.world(), std::time::Instant::now()),
            Err(Refusal::BookUnconfirmed)
        );
        apply(
            &mut state.world,
            &WorldUpdate::Game(WorldEvent::BookAction(BookActionStatus::Confirmed)),
            &NoSpells,
            Some(&outbox),
        );
        assert_eq!(
            outbox.begin_spell_set(state.world(), std::time::Instant::now()),
            Ok(())
        );
    }

    #[test]
    fn a_camp_to_the_desktop_ends_the_program_once_camped_and_a_plain_one_does_not() {
        use eq_client_core::{CampStatus, camp::CampTo};
        let exits = |to| {
            let mut state = OnlineState::new(true);
            testing::admit(&mut state, 4, testing::player(12));
            state.camp_to(to);
            let mut app = App::new();
            app.add_message::<AppExit>()
                .insert_resource(state)
                .add_systems(Update, leave);
            let mut tell = |status| {
                let mut state = app.world_mut().resource_mut::<OnlineState>();
                testing::news(&mut state, [WorldEvent::Camp(status)]);
            };
            tell(CampStatus::Preparing);
            tell(CampStatus::LoggingOut);
            app.update();
            let before = app.world().resource::<Messages<AppExit>>().len();
            let mut state = app.world_mut().resource_mut::<OnlineState>();
            testing::news(&mut state, [WorldEvent::Camp(CampStatus::Camped)]);
            app.update();
            app.update();
            (before, app.world().resource::<Messages<AppExit>>().len())
        };
        // Nothing before the camp ends; then one exit, said once.
        assert_eq!(exits(CampTo::Desktop), (0, 1));
        assert_eq!(exits(CampTo::CharacterSelect), (0, 0));
    }

    #[test]
    fn news_closed_by_its_own_session_is_not_taken_for_a_vanished_one() {
        use super::super::login::{FrontEnd, Session, testing};
        use std::sync::atomic::Ordering;
        let thread = testing::Thread::default();
        let (_news, updates) = std::sync::mpsc::sync_channel(1);
        let (commands, _queue) = std::sync::mpsc::sync_channel(1);
        let (front, ..) = FrontEnd::launched(
            Box::new(testing::Fake {
                servers: vec![testing::server("Example", "someone")],
                heard: std::sync::Arc::default(),
            }),
            Some(Session {
                classic_npc_consider: false,
                chat_link_expansion: Some(eq_client_core::chat::LinkExpansion::EQEMU_TITANIUM),
                inventory_move_rules: Some(eq_client_core::inventory::MoveRules::EqEmu),
                spell_provider: eq_client_core::spell_tables::Binding::new(
                    eq_client_core::spell_tables::Policy::Installed,
                ),
                updates,
                commands,
                worker: Box::new(thread.clone()),
            }),
        );
        // A worker that ended without closing its news, as on a panic.
        assert!(vanished(Some(&front)));
        // One whose last word did not fit in a full queue closed it itself.
        thread.closed.store(true, Ordering::SeqCst);
        assert!(!vanished(Some(&front)));
        // Without a front end there is no session to ask.
        assert!(vanished(None));
    }

    #[test]
    fn the_session_hears_of_a_bleed_out_the_world_finds_once() {
        use eq_client_core::{ClientCommand, ItemHitPoints, inventory::InventoryUpdate};
        let (sender, queue) = std::sync::mpsc::sync_channel(4);
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::news(
            &mut online,
            [
                WorldEvent::DeathThreshold(-11),
                WorldEvent::Inventory(InventoryUpdate::Snapshot(Vec::new())),
                WorldEvent::HitPoints {
                    spawn_id: 7,
                    current: -11,
                    maximum: 100,
                    items: ItemHitPoints::LeftOutOfCurrent,
                },
            ],
        );
        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(super::super::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, report_bleed_out);
        app.update();
        app.update();
        let sent: Vec<_> = queue.try_iter().collect();
        assert!(
            matches!(sent[..], [ClientCommand::BledOut { session_id: 1, .. }]),
            "{sent:?}"
        );
        assert_eq!(
            app.world().resource::<OnlineState>().world().bled_out(),
            None
        );
    }

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
            .init_resource::<crate::sounds::Sounds>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
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
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered integration scenario and its assertions together"
    )]
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
            .init_resource::<super::super::sounds::Sounds>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, receive);
        let player = PlayerState {
            name: "Example".into(),
            titanium_environment: None,
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
            languages: None,
            disciplines: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
            posture: None,
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
            forward: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            size: 0.0,
            invisible: false,
            appearance: eq_client_core::outfit::Appearance::default(),
            level: 0,
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
            pet_owner: None,
            hp_percent: None,
            posture: None,
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
                spell_landing_flag: None,
                zone_id: 0,
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
            .init_resource::<super::super::sounds::Sounds>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
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
                spell_landing_flag: None,
                zone_id: 0,
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

    fn spell_provider_fixture() -> (crate::spellbook::SpellNames, Vec<WorldUpdate>) {
        use eq_client_core::spell_tables::{
            Assignment, Binding, Context, Event, Fact, Policy, SpellId, Stage, Table,
            Update as SpellUpdate,
        };
        let binding = Binding::new(Policy::Project1999);
        let mut fields = vec!["0"; 20];
        fields[0] = "1";
        fields[1] = "Source";
        fields[19] = "10";
        let source = fields.join("^");
        fields[0] = "2";
        fields[1] = "Destination";
        fields[19] = "20";
        let mut spells =
            crate::spellbook::SpellNames::parse(&format!("{source}\n{}", fields.join("^")));
        spells.bind(binding);
        let source_id = SpellId::new(1).unwrap();
        let destination_id = SpellId::new(2).unwrap();
        let facts = [
            Fact::RunStarted {
                run_id: 7,
                attempt: 1,
            },
            Fact::Table(Event::new(
                Context::new(7, 1, Stage::World),
                SpellUpdate::Table(Table::Registration(vec![source_id])),
            )),
            Fact::ZonePending { run_id: 7 },
            Fact::Table(Event::new(
                Context::new(7, 2, Stage::Zone { session_id: 2 }),
                SpellUpdate::Table(Table::Replay(vec![Assignment::new(
                    destination_id,
                    source_id,
                )])),
            )),
            Fact::ZoneEntered {
                run_id: 7,
                session_id: 2,
            },
        ]
        .into_iter()
        .map(|fact| WorldUpdate::SpellTable(binding.envelope(fact)))
        .collect();
        (spells, facts)
    }

    #[test]
    fn a_new_admission_waits_while_the_players_casting_gesture_plays() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        let missing = std::env::temp_dir().join("no-everquest-installed-here");
        let (spells, facts) = spell_provider_fixture();
        let mut app = App::new();
        app.insert_resource(state)
            .insert_resource(spells)
            .insert_resource(Updates(Mutex::new(Some(receiver))))
            .insert_resource(ViewerSettings(super::super::ViewerConfig {
                eq_directory: Some(missing),
                ..default()
            }))
            .insert_resource(super::super::loading::Loading::waiting_for_gesture(true))
            .init_resource::<hud::HudState>()
            .init_resource::<super::super::motion::Controls>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<crate::notices::Lines>()
            .init_resource::<super::super::combat::CombatState>()
            .init_resource::<super::super::trade::TradeState>()
            .init_resource::<super::super::items::ItemState>()
            .init_resource::<super::super::inventory::InventoryState>()
            .init_resource::<super::super::sounds::Sounds>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, receive);
        // What the zone being left shows, the player's body among it.
        let old = app.world_mut().spawn(super::super::SceneEntity).id();
        let resources = |mana| {
            WorldUpdate::Game(WorldEvent::Resources {
                mana,
                endurance: mana,
            })
        };
        sender.send(resources(5)).unwrap();
        sender
            .send(WorldUpdate::Game(WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 2,
                zone: "example".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }))
            .unwrap();
        sender.send(resources(6)).unwrap();
        // These arrive after the admission in the queue but must reach the
        // provider even while that admission is held by the casting gesture.
        for fact in facts {
            sender.send(fact).unwrap();
        }
        // The news before the admission applies; the admission and what
        // follows it wait, and the zone being left stays drawn.
        app.update();
        sender.send(resources(7)).unwrap();
        app.update();
        assert_eq!(world(&app).session_id(), Some(1));
        assert_eq!(world(&app).vitals().mana, Some(5));
        assert_eq!(
            app.world()
                .resource::<crate::spellbook::SpellNames>()
                .mana(2),
            Some(10)
        );
        assert_eq!(
            app.world()
                .resource::<crate::spellbook::SpellNames>()
                .label(2),
            "Destination"
        );
        assert!(app.world().get_entity(old).is_ok());
        // Once the gesture has played, the held news applies in its order.
        app.insert_resource(super::super::loading::Loading::waiting_for_gesture(false));
        app.update();
        assert_eq!(world(&app).session_id(), Some(2));
        assert_eq!(world(&app).vitals().mana, Some(7));
        assert!(app.world().get_entity(old).is_err());
    }

    #[test]
    fn a_session_that_stops_during_a_held_admission_ends_once_after_it() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        let mut app = App::new();
        app.insert_resource(state)
            .insert_resource(Updates(Mutex::new(Some(receiver))))
            .insert_resource(ViewerSettings(super::super::ViewerConfig::default()))
            .insert_resource(super::super::loading::Loading::waiting_for_gesture(true))
            .init_resource::<hud::HudState>()
            .init_resource::<super::super::motion::Controls>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<crate::notices::Lines>()
            .init_resource::<super::super::combat::CombatState>()
            .init_resource::<super::super::trade::TradeState>()
            .init_resource::<super::super::items::ItemState>()
            .init_resource::<super::super::inventory::InventoryState>()
            .init_resource::<super::super::sounds::Sounds>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, receive);
        sender
            .send(WorldUpdate::Game(WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 2,
                zone: "example".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }))
            .unwrap();
        drop(sender);
        // The worker is gone while the admission waits: its end is said once,
        // held behind the admission, however many frames the wait takes.
        for _ in 0..3 {
            app.update();
        }
        let state = app.world().resource::<OnlineState>();
        assert!(matches!(
            state.held[..],
            [
                WorldUpdate::Game(WorldEvent::Entered { .. }),
                WorldUpdate::Connection(eq_client_core::world::Link::Ended)
            ]
        ));
        assert!(!state.world.ended());
        // After the wait the admission applies, and the session ends.
        app.insert_resource(super::super::loading::Loading::waiting_for_gesture(false));
        app.update();
        assert_eq!(world(&app).session_id(), Some(2));
        assert!(world(&app).ended());
        app.update();
        assert!(app.world().resource::<OnlineState>().held.is_empty());
    }

    fn held_rules_receiver() -> (App, std::sync::mpsc::Sender<WorldUpdate>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut app = crate::testing::app();
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        app.insert_resource(state)
            .insert_resource(Updates(Mutex::new(Some(receiver))))
            .insert_resource(crate::loading::Loading::waiting_for_gesture(true))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, receive);
        (app, sender)
    }

    fn rules_update(pvp: u32) -> WorldUpdate {
        WorldUpdate::Game(WorldEvent::Rules(eq_client_core::rules::WorldRules {
            pvp,
            ..default()
        }))
    }

    fn held_entry(session_id: u64) -> WorldUpdate {
        WorldUpdate::Game(WorldEvent::Entered {
            capabilities: Vec::new(),
            choices: Vec::new(),
            session_id,
            zone: "synthetic".into(),
            player: Box::new(testing::player(7)),
            far_clip: None,
            spell_landing_flag: None,
            zone_id: 0,
        })
    }

    #[test]
    fn within_run_loading_keeps_held_admission_and_rules_in_order() {
        let (mut app, sender) = held_rules_receiver();
        sender.send(rules_update(1)).unwrap();
        sender.send(held_entry(2)).unwrap();
        sender.send(rules_update(4)).unwrap();
        app.update();
        assert_eq!(world(&app).session_id(), Some(1));
        assert_eq!(world(&app).pvp_rule(), 1);
        app.insert_resource(crate::loading::Loading::waiting_for_gesture(false));
        app.update();
        assert_eq!(world(&app).session_id(), Some(2));
        assert_eq!(world(&app).pvp_rule(), 4);
    }

    #[test]
    fn restart_discards_old_held_admission_and_rules_before_new_receiver_news() {
        let (mut app, old_sender) = held_rules_receiver();
        old_sender.send(rules_update(1)).unwrap();
        old_sender.send(held_entry(2)).unwrap();
        old_sender.send(rules_update(4)).unwrap();
        app.update();
        assert_eq!(world(&app).session_id(), Some(1));
        assert_eq!(world(&app).pvp_rule(), 1);
        // FrontEnd::begin restarts the state then replaces the receiver.
        // Exercise receive's actual held batch across that same boundary.
        app.world_mut().resource_mut::<OnlineState>().restart();
        let (new_sender, receiver) = std::sync::mpsc::channel();
        *app.world().resource::<Updates>().0.lock().unwrap() = Some(receiver);
        assert!(old_sender.send(rules_update(8)).is_err());
        app.insert_resource(crate::loading::Loading::waiting_for_gesture(false));
        new_sender
            .send(WorldUpdate::Connection(
                eq_client_core::world::Link::Connected,
            ))
            .unwrap();
        app.update();
        assert_eq!(world(&app).session_id(), None);
        assert!(world(&app).player().is_none());
        assert_eq!(world(&app).pvp_rule(), 0);
        new_sender.send(rules_update(2)).unwrap();
        new_sender.send(held_entry(3)).unwrap();
        app.update();
        assert_eq!(world(&app).session_id(), Some(3));
        assert_eq!(world(&app).pvp_rule(), 2);
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
            .init_resource::<super::super::sounds::Sounds>()
            .init_resource::<super::super::spellbook::BookSelection>()
            .init_resource::<super::super::spellbook::BookView>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
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
                spell_landing_flag: None,
                zone_id: 0,
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
            titanium_environment: None,
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
            languages: None,
            disciplines: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
            posture: None,
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
                spell_landing_flag: None,
                zone_id: 0,
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
            .init_resource::<super::super::sounds::Sounds>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
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
                    applies: eq_client_core::buffs::Applies::Received,
                    record_type: 0,
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
                    applies: eq_client_core::buffs::Applies::Received,
                    record_type: 0,
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
                        spell_landing_flag: None,
                        zone_id: 0,
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
                string_id: None,
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
                string_id: None,
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
        assert!(!world(&app).is_dead());
        app.world_mut()
            .resource_mut::<OnlineState>()
            .world
            .select_target(Some(8));
        death.spawn_id = 7;
        sender
            .send(WorldUpdate::Game(WorldEvent::Death(death)))
            .unwrap();
        app.update();
        assert!(world(&app).is_dead());
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
                spell_landing_flag: None,
                zone_id: 0,
            }))
            .unwrap();
        app.update();
        // Asset loading is intentionally absent in this event-level test.
        assert!(!world(&app).is_dead());
        assert!(world(&app).spawns().is_empty());
        assert!(world(&app).doors().entries().is_empty());
    }
}
