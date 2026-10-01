//! Session events enter presentation here; this module never decodes wire
//! packets. The world model in `eq_client_core::world` applies each update;
//! this module shows what it changed.

use super::{
    Collision, HudText, OrbitCamera, Player, PlayerBody, SceneEntity, SceneInfo, TerrainSurface,
    ViewerSettings, build_collision, character, hud, spawn_player_and_hud, spawn_static_zone,
};
use bevy::prelude::*;
use eq_client_core::{
    WorldEvent, WorldUpdate, races, render_position,
    world::{CastNews, ClientWorld, NoSpells, Reset, SpellCatalog},
};
use std::sync::{LazyLock, Mutex, mpsc::Receiver};

#[derive(Resource)]
pub(super) struct Updates(pub Mutex<Option<Receiver<WorldUpdate>>>);

/// What the server has told the client, and what presenting it needs besides.
#[derive(Resource)]
pub(super) struct OnlineState {
    /// What the server has told the client; only `receive` changes it.
    pub world: ClientWorld,
    /// The character choice in progress, from the world's character list.
    pub selection: Option<super::character_select::Selection>,
    /// The current zone's regions, from its assets.
    pub regions: eq_client_assets::regions::ZoneRegions,
    pub enabled: bool,
    /// What became of the last door the player used.
    pub door_status: String,
}

impl OnlineState {
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
            door_status: String::new(),
        }
    }
}

/// The world the session reports, or an empty one where no session runs.
pub(super) fn world(online: Option<&OnlineState>) -> &ClientWorld {
    static OFFLINE: LazyLock<ClientWorld> = LazyLock::new(ClientWorld::default);
    online.map_or(&OFFLINE, |online| &online.world)
}

type SceneRoots = Or<(With<SceneEntity>, With<HudText>, With<hud::HudRoot>)>;

/// The offline demos' player, standing here with these spells memorized.
pub(super) fn preview_player(
    position: eq_client_core::WorldPosition,
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
    state.world.tick(std::time::Instant::now(), spells);
}

/// The panels a reset reaches besides the world itself.
struct Panels<'a> {
    hud: &'a mut hud::HudState,
    motion: &'a mut super::motion::Controls,
    inventory: &'a mut super::inventory::InventoryState,
    target: &'a mut super::target::TargetState,
    combat: &'a mut super::combat::CombatState,
    actions: Option<&'a mut hud::action_bar::ActionRequests>,
}

impl Panels<'_> {
    /// Forgets what the world's reset made stale on screen and in the panels.
    fn forget(&mut self, reason: Reset, state: &mut OnlineState) {
        // Whatever was in flight, its feedback is stale, and the world forgot
        // the target.
        self.hud.action_feedback = None;
        self.target.status.clear();
        if !matches!(reason, Reset::Died)
            && let Some(actions) = self.actions.as_mut()
        {
            actions.camp = None;
        }
        match reason {
            Reset::Lost { ended, .. } => {
                self.inventory.cancel_actions();
                self.motion.reset(None);
                if ended {
                    state.selection = None;
                    state.door_status.clear();
                    self.inventory.forget();
                }
            }
            Reset::Entered => {
                // Requests made in the old admission are void.
                *self.combat = super::combat::CombatState::default();
                self.motion.reset(None);
                state.selection = None;
                state.door_status.clear();
                self.inventory.forget();
            }
            Reset::Zoning { to_bind } => {
                self.inventory.cancel_actions();
                self.motion.reset(None);
                self.hud.status = if to_bind {
                    "Respawning at bind".into()
                } else {
                    "Zoning".into()
                };
            }
            Reset::Died => {
                self.motion.reset(None);
                self.inventory.cancel_actions();
                self.hud.status = "Dead - awaiting server bind destination".into();
            }
            Reset::Camped => {
                // Leave the zone; the world server sends a fresh character list.
                *self.combat = super::combat::CombatState::default();
                state.door_status.clear();
                self.inventory.forget();
                self.motion.reset(None);
                self.hud.status = "Camped - choose a character".into();
            }
        }
    }
}

/// Applies bounded event batches to the world and shows what they changed.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)] // Bevy schedules these disjoint resources.
pub(super) fn receive(
    mut commands: Commands,
    updates: Res<Updates>,
    definitions: (
        Res<ViewerSettings>,
        Option<Res<super::spellbook::SpellNames>>,
        Option<Res<hud::messages::Messages>>,
    ),
    mut state: ResMut<OnlineState>,
    mut hud: ResMut<hud::HudState>,
    mut motion: ResMut<super::motion::Controls>,
    mut chat: ResMut<super::chat::ChatState>,
    (mut target, mut combat, mut trade, mut actions): (
        ResMut<super::target::TargetState>,
        ResMut<super::combat::CombatState>,
        ResMut<super::trade::TradeState>,
        Option<ResMut<hud::action_bar::ActionRequests>>,
    ),
    mut items: ResMut<super::items::ItemState>,
    mut inventory: ResMut<super::inventory::InventoryState>,
    entities: Query<Entity, SceneRoots>,
    mut players: Query<&mut Transform, With<Player>>,
    mut cameras: Query<&mut OrbitCamera>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (settings, spell_names, messages) = definitions;
    let Ok(receiver) = updates.0.lock() else {
        return;
    };
    let Some(receiver) = receiver.as_ref() else {
        return;
    };
    // The player entity spawned by zone entry in this batch, not yet in the world.
    let mut entered_player = None;
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
    let lost = (ended && !state.world.ended()).then(|| WorldUpdate::Connection {
        connected: false,
        terminal: true,
        label: "Disconnected".into(),
    });
    for update in batch.into_iter().chain(lost) {
        let now = std::time::Instant::now();
        let changes = state.world.apply(&update, now, spells);
        if matches!(
            changes.cast,
            Some(CastNews::Began | CastNews::Refreshed | CastNews::Interrupted)
        ) {
            hud.action_feedback = None;
        }
        if let Some(reason) = changes.reset {
            Panels {
                hud: &mut hud,
                motion: &mut motion,
                inventory: &mut inventory,
                target: &mut target,
                combat: &mut combat,
                actions: actions.as_deref_mut(),
            }
            .forget(reason, &mut state);
        }
        if changes.inventory {
            inventory.refresh(state.world.inventory().stale());
        }
        if changes.characters {
            state.selection = state.world.characters().map(|list| {
                super::character_select::Selection::new(list.selection_id, list.characters.clone())
            });
        }
        if let Some(position) = changes.placed {
            motion.reset(None);
            let placed = Transform::from_translation(Vec3::from_array(render_position(position)))
                .with_rotation(Quat::from_rotation_y(eq_client_core::render_heading(
                    position.heading,
                )));
            // A correction right after zone entry belongs to the new player,
            // which only exists once this batch's commands apply.
            if let Some(entity) = entered_player {
                commands.entity(entity).insert(placed);
            } else if let Ok(mut transform) = players.single_mut() {
                *transform = placed;
            }
            for mut camera in &mut cameras {
                camera.focus = placed.translation;
            }
        }
        match update {
            WorldUpdate::Connection { label, .. } => {
                hud.status = if state.world.death().is_some() {
                    "Dead - awaiting respawn".into()
                } else {
                    label
                };
            }
            WorldUpdate::Chat(message) => {
                chat.history.push(message);
            }
            WorldUpdate::ServerMessage {
                string_id,
                arguments,
            } => {
                let text = messages.as_deref().map_or_else(
                    || format!("Server message {string_id}"),
                    |messages| messages.format(string_id, &arguments),
                );
                chat.history.push(super::chat::system_line(text));
            }
            WorldUpdate::Game(WorldEvent::Entered { player, .. }) => {
                // The session is this zone's even if its assets fail to load, so
                // commands and later events never follow the previous zone's.
                let Some(directory) = &settings.0.eq_directory else {
                    continue;
                };
                let zone = match eq_client_assets::load_zone(directory, state.world.zone()) {
                    Ok(zone) => zone,
                    Err(error) => {
                        for entity in &entities {
                            commands.entity(entity).despawn();
                        }
                        let text =
                            format!("Zone {} could not be loaded: {error}", state.world.zone());
                        error!("{text}");
                        chat.history.push(super::chat::system_line(text));
                        continue;
                    }
                };
                let asset = races::model(player.race, player.gender).and_then(|model| {
                    match eq_client_assets::characters::load_installed_character(
                        directory,
                        state.world.zone(),
                        model,
                    ) {
                        Ok(asset) => Some(asset),
                        Err(error) => {
                            warn!("Character model: {error}");
                            None
                        }
                    }
                });
                for entity in &entities {
                    commands.entity(entity).despawn();
                }
                let collision = build_collision(&zone);
                state.regions = zone.regions.clone();
                let surface = TerrainSurface::from_primitives(&zone.primitives);
                let position = Vec3::from_array(render_position(player.position));
                let height = asset
                    .as_ref()
                    .map_or(6.0, eq_client_assets::characters::CharacterAsset::height);
                // EQ's rule for the model keeps every later server-placed position
                // (zoning, teleports) at the same height above the feet.
                let feet_offset = eq_client_core::z_offset(player.race, player.size);
                // Lets a logged session be replayed offline with the same feet height.
                debug!(
                    "Admission feet offset {feet_offset} for size {} and model height {height}",
                    player.size
                );
                commands.insert_resource(Collision(collision));
                let body = PlayerBody {
                    feet_offset,
                    height,
                };
                commands.insert_resource(surface);
                commands.insert_resource(SceneInfo {
                    zone_name: zone.short_name.clone(),
                });
                spawn_static_zone(
                    &mut commands,
                    zone,
                    settings.0.terrain_only,
                    &mut images,
                    &mut meshes,
                    &mut materials,
                );
                let entity = spawn_player_and_hud(
                    &mut commands,
                    position,
                    asset.is_none(),
                    body,
                    &mut meshes,
                    &mut materials,
                );
                if let Some(asset) = asset {
                    character::spawn(
                        &mut commands,
                        entity,
                        asset,
                        body.feet_offset,
                        &mut images,
                        &mut meshes,
                        &mut materials,
                    );
                }
                commands.entity(entity).insert(
                    Transform::from_translation(position).with_rotation(Quat::from_rotation_y(
                        eq_client_core::render_heading(player.position.heading),
                    )),
                );
                for mut camera in &mut cameras {
                    camera.focus = position;
                }
                entered_player = Some(entity);
            }
            WorldUpdate::Game(WorldEvent::MotionState {
                session_id,
                units_per_second,
                backward_units_per_second,
                walk_units_per_second,
                strafe_units_per_second,
                falls,
            }) => {
                if state.world.session_id() == Some(session_id) {
                    motion.reset(units_per_second);
                    motion.backward_speed = backward_units_per_second;
                    motion.walk_speed = walk_units_per_second;
                    motion.strafe_speed = strafe_units_per_second;
                    motion.airborne =
                        falls.then(eq_client_core::movement::AirborneController::default);
                }
            }
            WorldUpdate::Game(WorldEvent::MotionSent {
                position, refused, ..
            }) => {
                if !changes.ignored {
                    motion.accepted();
                    motion.refused = refused;
                    if let Ok(transform) = players.single_mut() {
                        motion.display_sample(*transform, position);
                    }
                }
            }
            WorldUpdate::Game(WorldEvent::ItemUseAction {
                session_id,
                request_id,
                error,
            }) => {
                if state.world.accepts_reply(session_id)
                    && inventory.item_use_result(session_id, request_id, error)
                {
                    hud.action_feedback = Some((now, inventory.action_message().to_owned()));
                }
            }
            WorldUpdate::Game(WorldEvent::InventoryAction {
                session_id,
                revision,
                error,
            }) => {
                inventory.action_result(session_id, revision, error, state.world.inventory());
            }
            WorldUpdate::Game(WorldEvent::ItemDetails(item)) => {
                debug!("Item definition received: ID {}", item.id);
                items.received(item);
            }
            WorldUpdate::Game(WorldEvent::ZoneTransferRejected { reason, .. }) => {
                if !changes.ignored {
                    hud.status = reason.to_string();
                    chat.history
                        .push(super::chat::system_line(reason.to_string()));
                }
            }
            WorldUpdate::Game(WorldEvent::ZoneLineRejected { session_id, reason }) => {
                if state.world.session_id() == Some(session_id) {
                    motion.accepted();
                    hud.status = format!("Cannot cross zone line: {reason}");
                }
            }
            WorldUpdate::Game(WorldEvent::TargetSent(id)) => {
                if !changes.ignored {
                    debug!("Target packet sent: {id:?}");
                }
            }
            WorldUpdate::Game(WorldEvent::TargetRejected { reason, .. }) => {
                if !changes.ignored {
                    target.status = format!("Target rejected: {reason}");
                }
            }
            WorldUpdate::Game(WorldEvent::HealthPercent { spawn_id, percent }) => {
                if state.world.target().selected == Some(spawn_id) {
                    debug!("Target health received: spawn {spawn_id}, {percent}%");
                }
            }
            WorldUpdate::Game(WorldEvent::Posture { spawn_id, posture }) => {
                if state.world.is_player(spawn_id) {
                    debug!(?posture, "Own posture update");
                }
            }
            WorldUpdate::Game(WorldEvent::CastRejected {
                spell_id, reason, ..
            }) => {
                if !changes.ignored {
                    hud.action_feedback =
                        Some((now, format!("Cast rejected (spell {spell_id}): {reason}")));
                }
            }
            WorldUpdate::Game(WorldEvent::Spell(eq_client_core::SpellUpdate::Interrupted {
                caster_id,
                message_id,
            })) => {
                if state
                    .world
                    .player()
                    .is_some_and(|player| u32::from(player.spawn_id) == caster_id)
                {
                    debug!(message_id, "Own cast interrupted");
                }
            }
            WorldUpdate::Game(WorldEvent::Doors(update)) => {
                if matches!(update, eq_client_core::doors::DoorUpdate::RemoveAll) {
                    state.door_status.clear();
                }
            }
            WorldUpdate::Game(WorldEvent::DoorAction {
                session_id,
                door_id,
                error,
            }) => {
                if state.world.accepts_reply(session_id) {
                    state.door_status = error.map_or_else(
                        || format!("Door {door_id}: request sent"),
                        |error| format!("Door {door_id}: {error}"),
                    );
                }
            }
            WorldUpdate::Game(WorldEvent::ObjectAction {
                session_id,
                error: Some(error),
                ..
            }) => {
                if state.world.accepts_reply(session_id) {
                    let line = super::ground::refusal(&error);
                    chat.history.push(super::chat::system_line(line));
                }
            }
            WorldUpdate::Game(WorldEvent::Buff(update)) => {
                if !changes.ignored {
                    debug!(
                        spell_id = update.spell_id,
                        slot = update.slot,
                        removed = update.buff.is_none(),
                        "Own buff slot update"
                    );
                }
            }
            WorldUpdate::Game(WorldEvent::SpellEffect(effect)) => {
                if !changes.ignored {
                    debug!(
                        spell_id = effect.spell_id,
                        caster_level = effect.caster_level,
                        effect_flag = effect.effect_flag,
                        "Own spell effect"
                    );
                }
            }
            WorldUpdate::Game(WorldEvent::Camp(status)) => {
                if let Some(actions) = actions.as_mut() {
                    actions.camp = match &status {
                        eq_client_core::CampStatus::Preparing => Some((now, false)),
                        eq_client_core::CampStatus::LoggingOut => {
                            Some(actions.camp.map_or((now, true), |(since, _)| (since, true)))
                        }
                        _ => None,
                    };
                }
                let text = match &status {
                    eq_client_core::CampStatus::Preparing => messages
                        .as_deref()
                        .map(|messages| messages.format(12293, &[])),
                    eq_client_core::CampStatus::Abandoned => messages
                        .as_deref()
                        .map(|messages| messages.format(12290, &[])),
                    eq_client_core::CampStatus::LoggingOut => Some("Logging out...".into()),
                    eq_client_core::CampStatus::Camped => None,
                    eq_client_core::CampStatus::Rejected(reason) => Some(reason.clone()),
                };
                if let Some(text) = text {
                    chat.history.push(super::chat::system_line(text));
                }
            }
            // The shop window still keeps its own copy, adjusted between
            // money updates, until loot and merchant state join the world.
            WorldUpdate::Game(WorldEvent::Coins(coins)) => trade.coins = Some(coins),
            WorldUpdate::Game(WorldEvent::Loot(update)) => {
                if let Some(text) = trade.apply_loot(update) {
                    chat.history.push(super::chat::system_line(text));
                }
            }
            WorldUpdate::Game(WorldEvent::Merchant(update)) => {
                if let Some(text) = trade.apply_merchant(update) {
                    chat.history.push(super::chat::system_line(text));
                }
            }
            WorldUpdate::Game(WorldEvent::MerchantRefused { session_id, reason }) => {
                if state.world.session_id() == Some(session_id) {
                    chat.history.push(super::chat::system_line(reason));
                }
            }
            WorldUpdate::Game(WorldEvent::Consideration(consideration)) => {
                let name = state
                    .world
                    .spawn(consideration.target_id)
                    .map_or_else(String::new, |spawn| {
                        super::combat::display_name(&spawn.state.name)
                    });
                if let Some(messages) = messages.as_deref() {
                    chat.history
                        .push(super::chat::system_line(super::combat::consideration_text(
                            messages,
                            &name,
                            &consideration,
                        )));
                }
            }
            WorldUpdate::Game(WorldEvent::Damage(damage)) => {
                if let (Some(player), Some(messages)) = (state.world.player(), messages.as_deref())
                    && let Some(text) = super::combat::damage_text(
                        messages,
                        player.spawn_id,
                        |id| {
                            state.world.spawn(id).map_or_else(
                                || "someone".to_owned(),
                                |spawn| super::combat::display_name(&spawn.state.name),
                            )
                        },
                        &damage,
                    )
                {
                    chat.history.push(super::chat::system_line(text));
                }
            }
            WorldUpdate::Game(_) => (),
        }
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
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 6.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
        }
    }

    /// Connects or disconnects the session without ending it.
    pub(crate) fn connect(state: &mut OnlineState, connected: bool) {
        state.world.apply(
            &WorldUpdate::Connection {
                connected,
                terminal: false,
                label: String::new(),
            },
            Instant::now(),
            &NoSpells,
        );
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
            .init_resource::<super::super::target::TargetState>()
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
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
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
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
        };
        let admitted = std::time::Instant::now();
        for update in [
            WorldUpdate::Game(WorldEvent::Entered {
                session_id: 1,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip: None,
            }),
            WorldUpdate::Connection {
                connected: true,
                terminal: false,
                label: "Connected".into(),
            },
        ] {
            state.world.apply(&update, admitted, &NoSpells);
        }
        app.insert_resource(state)
            .insert_resource(Updates(Mutex::new(Some(receiver))))
            .insert_resource(ViewerSettings(super::super::ViewerConfig::default()))
            .init_resource::<hud::HudState>()
            .init_resource::<super::super::motion::Controls>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<super::super::target::TargetState>()
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
            .send(WorldUpdate::Connection {
                connected: false,
                terminal: false,
                label: "Zoning".into(),
            })
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
            .send(WorldUpdate::Connection {
                connected: true,
                terminal: false,
                label: "Connected".into(),
            })
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
                    without_items: false,
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
        assert!(
            app.world()
                .resource::<hud::HudState>()
                .action_feedback
                .is_none()
        );
        sender
            .send(WorldUpdate::Game(WorldEvent::CastRejected {
                session_id: 1,
                spell_id: 73,
                reason: "Target unavailable".into(),
            }))
            .unwrap();
        app.update();
        let hud = app.world().resource::<hud::HudState>();
        assert!(
            hud.action_feedback
                .as_ref()
                .unwrap()
                .1
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
