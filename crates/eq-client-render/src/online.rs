//! Session events enter presentation here; this module never decodes wire packets.

use super::{
    Collision, HudText, OrbitCamera, Player, PlayerBody, SceneEntity, SceneInfo, TerrainSurface,
    ViewerSettings, build_collision, character, hud, spawn_player_and_hud, spawn_static_zone,
};
use bevy::prelude::*;
use eq_client_core::{PlayerState, WorldEvent, WorldUpdate, classic_model, render_position};
use std::{
    collections::BTreeMap,
    sync::{Mutex, mpsc::Receiver},
};

#[derive(Resource)]
pub(super) struct Updates(pub Mutex<Option<Receiver<WorldUpdate>>>);

#[derive(Resource)]
pub(super) struct OnlineState {
    pub selection: Option<super::character_select::Selection>,
    pub regions: eq_client_assets::regions::ZoneRegions,
    pub enabled: bool,
    pub zone: String,
    pub connected: bool,
    pending_transfer: Option<eq_client_core::ZoneOffer>,
    pub death: Option<eq_client_core::Death>,
    pub finished: bool,
    pub session_id: Option<u64>,
    pub player: Option<PlayerState>,
    pub spawns: BTreeMap<u16, eq_client_core::SpawnState>,
    pub doors: eq_client_core::doors::Doors,
    pub door_status: String,
    pub revisions: BTreeMap<u16, u64>,
    pub health: BTreeMap<u16, u8>,
    pub postures: BTreeMap<u16, eq_client_core::PostureState>,
    revision: u64,
}

impl OnlineState {
    pub fn new(enabled: bool) -> Self {
        Self {
            selection: None,
            regions: eq_client_assets::regions::ZoneRegions::default(),
            enabled,
            zone: String::new(),
            connected: false,
            pending_transfer: None,
            death: None,
            finished: false,
            session_id: None,
            player: None,
            spawns: BTreeMap::new(),
            doors: eq_client_core::doors::Doors::default(),
            door_status: String::new(),
            revisions: BTreeMap::new(),
            health: BTreeMap::new(),
            postures: BTreeMap::new(),
            revision: 0,
        }
    }
}

type SceneRoots = Or<(With<SceneEntity>, With<HudText>, With<hud::HudRoot>)>;

/// Applies bounded event batches; zone assets stay local and all movement stays disabled.
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
    for update in receiver.try_iter().take(256) {
        match update {
            WorldUpdate::Connection {
                connected,
                terminal,
                label,
            } => {
                state.connected = connected;
                if !connected {
                    hud.casting = None;
                    hud.interrupted = None;
                    hud.pending_cast = None;
                    hud.action_feedback = None;
                    hud.book_action = None;
                    if terminal || state.pending_transfer.is_none() {
                        hud.reset_cooldowns();
                        hud.spell_book = None;
                        hud.buff_state.clear();
                    }
                    inventory.cancel_actions();
                    motion.reset(None);
                }
                state.finished = terminal;
                if terminal {
                    state.selection = None;
                    state.pending_transfer = None;
                    inventory.clear();
                    state.spawns.clear();
                    state.doors = eq_client_core::doors::Doors::default();
                    state.door_status.clear();
                    state.revisions.clear();
                    state.health.clear();
                    state.postures.clear();
                }
                hud.status = if state.death.is_some() {
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
            WorldUpdate::Game(WorldEvent::CharacterSelection {
                selection_id,
                characters,
            }) => {
                state.selection = Some(super::character_select::Selection::new(
                    selection_id,
                    characters,
                ));
            }
            WorldUpdate::Game(WorldEvent::Entered {
                session_id,
                zone,
                player,
            }) => {
                motion.reset(None);
                state.selection = None;
                state.pending_transfer = None;
                hud.spell_book = None;
                hud.buff_state.clear();
                hud.interrupted = None;
                hud.restore_cooldowns(&player, std::time::Instant::now());
                inventory.clear();
                state.death = None;
                state.spawns.clear();
                state.doors = eq_client_core::doors::Doors::default();
                state.door_status.clear();
                state.revisions.clear();
                state.health.clear();
                state.postures.clear();
                state.zone.clone_from(&zone);
                let Some(directory) = &settings.0.eq_directory else {
                    continue;
                };
                let zone = match eq_client_assets::load_zone(directory, &zone) {
                    Ok(zone) => zone,
                    Err(error) => {
                        hud.status = format!("Zone load failed: {error}");
                        continue;
                    }
                };
                let asset = classic_model(player.race, player.gender).and_then(|model| {
                    match eq_client_assets::characters::load_installed_character(
                        directory,
                        &state.zone,
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
                let ground = surface.height_below(position.x, position.z, position.y + 0.5);
                let valid = |ground: f32| {
                    let offset = position.y - ground;
                    (-0.5..=height * 2.0).contains(&offset).then_some(offset)
                };
                // Terrain first, then any solid collision surface (floors of dungeons
                // such as the Gloomingdeep tutorial are objects, not terrain).
                let feet_offset = ground
                    .and_then(valid)
                    .or_else(|| {
                        collision
                            .as_ref()
                            .and_then(|world| world.ground(position, 0.5, height * 2.0))
                            .and_then(valid)
                    })
                    .unwrap_or(height * 0.5);
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
                    false,
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
                hud.hp = None;
                hud.experience = None;
                hud.mana = Some(player.mana);
                hud.endurance = player.endurance;
                hud.hp_percent = player.hp_percent;
                hud.spells = player.memorized_spells;
                state.player = Some(*player);
                state.session_id = Some(session_id);
            }
            WorldUpdate::Game(WorldEvent::MotionState {
                session_id,
                units_per_second,
                backward_units_per_second,
                walk_units_per_second,
                strafe_units_per_second,
            }) => {
                if state.session_id == Some(session_id) {
                    motion.reset(units_per_second);
                    motion.backward_speed = backward_units_per_second;
                    motion.walk_speed = walk_units_per_second;
                    motion.strafe_speed = strafe_units_per_second;
                }
            }
            WorldUpdate::Game(WorldEvent::MotionSent {
                session_id,
                position,
            }) => {
                if state.session_id == Some(session_id) && state.connected && state.death.is_none()
                {
                    motion.accepted();
                    if let Some(player) = &mut state.player {
                        player.position = position;
                    }
                    if let Ok(transform) = players.single_mut() {
                        motion.display_sample(*transform, position);
                    }
                }
            }
            WorldUpdate::Game(WorldEvent::Inventory(update)) => {
                inventory.apply(update);
            }
            WorldUpdate::Game(WorldEvent::ItemUseAction {
                session_id,
                request_id,
                error,
            }) => {
                if state.connected
                    && state.session_id == Some(session_id)
                    && inventory.item_use_result(session_id, request_id, error)
                {
                    hud.action_feedback = Some((
                        std::time::Instant::now(),
                        inventory.action_message().to_owned(),
                    ));
                }
            }
            WorldUpdate::Game(WorldEvent::InventoryAction {
                session_id,
                revision,
                error,
            }) => {
                inventory.action_result(session_id, revision, error);
            }
            WorldUpdate::Game(WorldEvent::ItemDetails(item)) => {
                eprintln!("Item definition received: ID {}", item.id);
                if items.cache.len() >= 128 {
                    items.cache.pop_first();
                }
                items.cache.insert(item.id, item);
            }
            WorldUpdate::Game(WorldEvent::Death(death)) => {
                if let Ok(id) = u16::try_from(death.spawn_id) {
                    state.health.insert(id, 0);
                    // Titanium corpses keep the spawn ID; redraw the entity as a corpse.
                    if let Some(spawn) = state.spawns.get_mut(&id) {
                        spawn.kind = spawn.kind.corpse();
                        state.revision = state.revision.wrapping_add(1);
                        let revision = state.revision;
                        state.revisions.insert(id, revision);
                    }
                }
                if state
                    .player
                    .as_ref()
                    .is_some_and(|p| u32::from(p.spawn_id) == death.spawn_id)
                {
                    motion.reset(None);
                    inventory.cancel_actions();
                    state.death = Some(death.clone());
                    hud.casting = None;
                    hud.book_action = None;
                    *target = super::target::TargetState::default();
                    hud.interrupted = None;
                    hud.reset_cooldowns();
                    hud.hp_percent = Some(0);
                    if let Some((_, maximum)) = hud.hp {
                        hud.hp = Some((0, maximum));
                    }
                    hud.status = "Dead - awaiting server bind destination".into();
                }
            }
            WorldUpdate::Game(WorldEvent::ZoneTransfer(offer)) => {
                state.pending_transfer = Some(offer.clone());
                hud.casting = None;
                hud.interrupted = None;
                hud.pending_cast = None;
                hud.action_feedback = None;
                hud.book_action = None;
                inventory.cancel_actions();
                motion.reset(None);
                state.connected = false;
                *target = super::target::TargetState::default();
                hud.status = if offer.to_bind {
                    "Respawning at bind".into()
                } else {
                    "Zoning".into()
                };
            }
            WorldUpdate::Game(WorldEvent::ZoneTransferRejected { session_id, reason }) => {
                if state.session_id != Some(session_id) {
                    continue;
                }
                state.pending_transfer = None;
                state.connected = state.death.is_none();
                hud.status = reason.to_string();
                chat.history
                    .push(super::chat::system_line(reason.to_string()));
            }
            WorldUpdate::Game(WorldEvent::ZoneLineRejected { session_id, reason }) => {
                if state.session_id == Some(session_id) {
                    motion.accepted();
                    hud.status = format!("Cannot cross zone line: {reason}");
                }
            }
            WorldUpdate::Game(WorldEvent::TargetSent(id)) => {
                if target.selected == id {
                    target.sent = true;
                    eprintln!("Target packet sent: {id:?}");
                }
            }
            WorldUpdate::Game(WorldEvent::TargetRejected {
                session_id,
                spawn_id,
                reason,
            }) => {
                if state.connected && state.session_id == Some(session_id) {
                    target.reject(spawn_id, &reason);
                }
            }
            WorldUpdate::Game(WorldEvent::HealthPercent { spawn_id, percent }) => {
                state.health.insert(spawn_id, percent);
                if target.selected == Some(spawn_id) {
                    eprintln!("Target health received: spawn {spawn_id}, {percent}%");
                }
            }
            WorldUpdate::Game(WorldEvent::Spawns(spawns)) => {
                for spawn in spawns {
                    state.postures.remove(&spawn.spawn_id);
                    state.revision = state.revision.wrapping_add(1);
                    let revision = state.revision;
                    state.revisions.insert(spawn.spawn_id, revision);
                    state.health.remove(&spawn.spawn_id);
                    state.spawns.insert(spawn.spawn_id, spawn);
                }
            }
            WorldUpdate::Game(WorldEvent::Visibility {
                spawn_id,
                invisible,
            }) => {
                if let Some(spawn) = state.spawns.get_mut(&spawn_id) {
                    spawn.invisible = invisible;
                }
            }
            WorldUpdate::Game(WorldEvent::Posture { spawn_id, posture }) => {
                if state.connected
                    && (state.spawns.contains_key(&spawn_id)
                        || state
                            .player
                            .as_ref()
                            .is_some_and(|player| player.spawn_id == spawn_id))
                {
                    if state
                        .player
                        .as_ref()
                        .is_some_and(|player| player.spawn_id == spawn_id)
                    {
                        debug!(?posture, "Own posture update");
                    }
                    state.postures.insert(spawn_id, posture);
                }
            }
            WorldUpdate::Game(WorldEvent::Mana(mana)) => hud.mana = Some(mana),
            WorldUpdate::Game(WorldEvent::CastPending {
                session_id,
                spell_id,
            }) => {
                if state.connected && state.death.is_none() && state.session_id == Some(session_id)
                {
                    hud.pending_cast = spell_id;
                }
            }
            WorldUpdate::Game(WorldEvent::CastRejected {
                session_id,
                spell_id,
                reason,
            }) => {
                if state.connected && state.death.is_none() && state.session_id == Some(session_id)
                {
                    hud.action_feedback = Some((
                        std::time::Instant::now(),
                        format!("Cast rejected (spell {spell_id}): {reason}"),
                    ));
                }
            }
            WorldUpdate::Game(WorldEvent::Spell(update)) => {
                if matches!(update, eq_client_core::SpellUpdate::Slot { mode: 2, .. })
                    && matches!(
                        hud.book_action,
                        Some(eq_client_core::BookActionStatus::Submitted)
                    )
                {
                    hud.book_action = None;
                }
                if let Some(book) = hud.spell_book.as_mut() {
                    book.apply(&update);
                }
                let active = state.connected && state.death.is_none();
                if let Some(player) = state.player.as_mut() {
                    if let eq_client_core::SpellUpdate::Interrupted {
                        caster_id,
                        message_id,
                    } = update
                        && caster_id == u32::from(player.spawn_id)
                    {
                        debug!(message_id, "Own cast interrupted");
                    }
                    update.apply_gems(&mut player.memorized_spells);
                    hud.spells = player.memorized_spells;
                    if active {
                        hud.cast_update(player.spawn_id, &update, std::time::Instant::now());
                    }
                }
            }
            WorldUpdate::Game(WorldEvent::Despawn(id)) => {
                state.spawns.remove(&id);
                state.revisions.remove(&id);
                state.health.remove(&id);
                state.postures.remove(&id);
            }
            WorldUpdate::Game(WorldEvent::Position { spawn_id, position }) => {
                if let Some(spawn) = state.spawns.get_mut(&spawn_id) {
                    spawn.position = position;
                }
                if let Some(player) = &mut state.player
                    && player.spawn_id == spawn_id
                {
                    motion.reset(None);
                    player.position = position;
                    if let Ok(mut transform) = players.single_mut() {
                        transform.translation = Vec3::from_array(render_position(position));
                        transform.rotation =
                            Quat::from_rotation_y(eq_client_core::render_heading(position.heading));
                        for mut camera in &mut cameras {
                            camera.focus = transform.translation;
                        }
                    }
                }
            }
            WorldUpdate::Game(WorldEvent::HitPoints {
                spawn_id,
                current,
                maximum,
            }) => {
                if state
                    .player
                    .as_ref()
                    .is_some_and(|player| player.spawn_id == spawn_id)
                {
                    hud.hp = Some((current, maximum));
                    if maximum != 0 {
                        let percent =
                            u8::try_from((u64::from(current) * 100 / u64::from(maximum)).min(100))
                                .expect("percentage is bounded to 100");
                        hud.hp_percent = Some(percent);
                        state.health.insert(spawn_id, percent);
                        if let Some(player) = state.player.as_mut() {
                            player.hp_percent = Some(percent);
                        }
                    }
                }
            }
            WorldUpdate::Game(WorldEvent::Resources { mana, endurance }) => {
                hud.mana = Some(mana);
                hud.endurance = Some(endurance);
            }
            WorldUpdate::Game(WorldEvent::Experience(value)) => hud.experience = Some(value),
            WorldUpdate::Game(WorldEvent::Doors(update)) => {
                if state.connected {
                    if matches!(update, eq_client_core::doors::DoorUpdate::RemoveAll) {
                        state.door_status.clear();
                    }
                    state.doors.apply(&update);
                }
            }
            WorldUpdate::Game(WorldEvent::DoorAction {
                session_id,
                door_id,
                error,
            }) => {
                if state.session_id == Some(session_id) && state.connected {
                    state.door_status = error.map_or_else(
                        || format!("Door {door_id}: request sent"),
                        |error| format!("Door {door_id}: {error}"),
                    );
                }
            }
            WorldUpdate::Game(WorldEvent::Level {
                current,
                experience,
                ..
            }) => {
                if state.connected
                    && let Some(player) = state.player.as_mut()
                {
                    player.level = current;
                    hud.experience = Some(experience);
                }
            }
            WorldUpdate::Game(WorldEvent::Skill { skill_id, value }) => {
                if state.connected
                    && let Some(player) = state.player.as_mut()
                {
                    player.apply_skill(skill_id, value);
                }
            }
            WorldUpdate::Game(WorldEvent::BuffSnapshot(buffs)) => {
                hud.buff_state.replace_snapshot(
                    buffs
                        .into_iter()
                        .enumerate()
                        .filter_map(|(slot, buff)| {
                            buff.map(|buff| {
                                (u32::try_from(slot).expect("bounded buff table"), buff)
                            })
                        })
                        .collect(),
                );
            }
            WorldUpdate::Game(WorldEvent::Buff(update)) => {
                if state
                    .player
                    .as_ref()
                    .is_some_and(|player| u32::from(player.spawn_id) == update.entity_id)
                {
                    debug!(
                        spell_id = update.spell_id,
                        slot = update.slot,
                        removed = update.buff.is_none(),
                        "Own buff slot update"
                    );
                    hud.buff_update(update);
                }
            }
            WorldUpdate::Game(WorldEvent::SpellEffect(effect)) => {
                if state
                    .player
                    .as_ref()
                    .is_some_and(|player| player.spawn_id == effect.target_id)
                {
                    debug!(
                        spell_id = effect.spell_id,
                        caster_level = effect.caster_level,
                        effect_flag = effect.effect_flag,
                        "Own spell effect"
                    );
                    hud.spell_effect(effect, spell_names.as_deref());
                }
            }
            WorldUpdate::Game(WorldEvent::Camp(status)) => {
                if let Some(actions) = actions.as_mut() {
                    actions.camp = match &status {
                        eq_client_core::CampStatus::Preparing => {
                            Some((std::time::Instant::now(), false))
                        }
                        eq_client_core::CampStatus::LoggingOut => Some(
                            actions
                                .camp
                                .map_or((std::time::Instant::now(), true), |(since, _)| {
                                    (since, true)
                                }),
                        ),
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
                    eq_client_core::CampStatus::Camped => {
                        // Leave the zone; the world server sends a fresh character list.
                        state.session_id = None;
                        state.player = None;
                        state.connected = false;
                        state.pending_transfer = None;
                        state.spawns.clear();
                        state.revisions.clear();
                        state.health.clear();
                        state.postures.clear();
                        hud.spell_book = None;
                        hud.buff_state.clear();
                        hud.casting = None;
                        hud.pending_cast = None;
                        inventory.clear();
                        motion.reset(None);
                        hud.status = "Camped - choose a character".into();
                        None
                    }
                    eq_client_core::CampStatus::Rejected(reason) => Some(reason.clone()),
                };
                if let Some(text) = text {
                    chat.history.push(super::chat::system_line(text));
                }
            }
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
            WorldUpdate::Game(WorldEvent::Consideration(consideration)) => {
                let name = state
                    .spawns
                    .get(&consideration.target_id)
                    .map_or_else(String::new, |spawn| {
                        super::combat::display_name(&spawn.name)
                    });
                if let Some(messages) = messages.as_deref() {
                    chat.history
                        .push(super::chat::system_line(super::combat::consideration_text(
                            messages,
                            &name,
                            &consideration,
                        )));
                }
                combat
                    .considered
                    .insert(consideration.target_id, consideration.color);
            }
            WorldUpdate::Game(WorldEvent::Damage(damage)) => {
                if let (Some(player), Some(messages)) = (state.player.as_ref(), messages.as_deref())
                    && let Some(text) = super::combat::damage_text(
                        messages,
                        player.spawn_id,
                        |id| {
                            state.spawns.get(&id).map_or_else(
                                || "someone".to_owned(),
                                |spawn| super::combat::display_name(&spawn.name),
                            )
                        },
                        &damage,
                    )
                {
                    chat.history.push(super::chat::system_line(text));
                }
            }
            WorldUpdate::Game(WorldEvent::SpellBook(book)) => hud.spell_book = Some(book),
            WorldUpdate::Game(WorldEvent::BookAction(status)) => {
                hud.book_action = (!matches!(status, eq_client_core::BookActionStatus::Confirmed))
                    .then_some(status);
                hud.book_action_revision = hud.book_action_revision.wrapping_add(1);
            }
            WorldUpdate::Game(_) => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered integration scenario and its assertions together"
    )]
    fn only_own_death_disables_targeting_and_zone_entry_clears_death() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut app = App::new();
        let mut state = OnlineState::new(true);
        state.connected = true;
        state.session_id = Some(1);
        state.player = Some(PlayerState {
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
            skills: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
        });
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
            app.world()
                .resource::<hud::HudState>()
                .buff_state
                .slots()
                .as_ref()
                .unwrap()
                .get(&1),
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
        assert_eq!(
            app.world()
                .resource::<hud::HudState>()
                .buff_state
                .slots()
                .as_ref()
                .unwrap()
                .len(),
            1
        );
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
        assert!(
            app.world()
                .resource::<hud::HudState>()
                .buff_state
                .slots()
                .as_ref()
                .unwrap()
                .is_empty()
        );
        let now = std::time::Instant::now();
        let mut timed_player = app
            .world()
            .resource::<OnlineState>()
            .player
            .clone()
            .unwrap();
        timed_player.memorized_spells[0] = Some(42);
        timed_player.spell_refresh_ms = Some([10000; 8]);
        {
            let mut hud = app.world_mut().resource_mut::<hud::HudState>();
            let mut book = eq_client_core::SpellBook::default();
            book.apply(&eq_client_core::SpellUpdate::Slot {
                slot: 0,
                spell_id: 42,
                mode: 0,
            });
            hud.spell_book = Some(book);
            hud.restore_cooldowns(&timed_player, now);
        }
        sender
            .send(WorldUpdate::Game(WorldEvent::ZoneTransfer(
                eq_client_core::ZoneOffer {
                    zone_id: 9,
                    instance_id: 0,
                    position: eq_client_core::WorldPosition::default(),
                    reason: 0,
                    to_bind: false,
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
        assert!(
            app.world()
                .resource::<OnlineState>()
                .pending_transfer
                .is_some()
        );
        assert!(!app.world().resource::<OnlineState>().connected);
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
        assert!(app.world().resource::<OnlineState>().connected);
        let history = &app
            .world()
            .resource::<super::super::chat::ChatState>()
            .history;
        let notices = history.lines(eq_client_core::chat::ChatTab::System);
        assert_eq!(notices.len(), 1);
        assert!(notices[0].1.message.text.contains("server code -7"));
        let hud = app.world().resource::<hud::HudState>();
        assert_eq!(hud.spell_book.as_ref().unwrap().slots()[0], Some(42));
        assert_eq!(
            hud.cooldowns.remaining(42, now),
            std::time::Duration::from_secs(10)
        );
        for (current, maximum, percent) in [(34, 34, 100), (17, 34, 50), (35, 34, 100)] {
            sender
                .send(WorldUpdate::Game(WorldEvent::HitPoints {
                    spawn_id: 7,
                    current,
                    maximum,
                }))
                .unwrap();
            app.update();
            assert_eq!(
                app.world().resource::<OnlineState>().health.get(&7),
                Some(&percent)
            );
            assert_eq!(
                app.world().resource::<hud::HudState>().hp,
                Some((current, maximum))
            );
        }
        for (success, expected) in [(false, Some(42)), (true, None)] {
            app.world_mut().resource_mut::<hud::HudState>().book_action =
                Some(eq_client_core::BookActionStatus::AwaitingReply);
            sender
                .send(WorldUpdate::Game(WorldEvent::Spell(
                    eq_client_core::SpellUpdate::BookDeletion { slot: 0, success },
                )))
                .unwrap();
            app.update();
            assert_eq!(
                app.world().resource::<hud::HudState>().book_action,
                Some(eq_client_core::BookActionStatus::AwaitingReply)
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
            assert_eq!(
                app.world()
                    .resource::<hud::HudState>()
                    .book_action
                    .is_none(),
                success
            );
            assert_eq!(
                app.world()
                    .resource::<hud::HudState>()
                    .spell_book
                    .as_ref()
                    .unwrap()
                    .slots()[0],
                expected
            );
        }
        app.world_mut()
            .resource_mut::<OnlineState>()
            .player
            .as_mut()
            .unwrap()
            .skills = Some(vec![0; 100]);
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
            app.world().resource::<OnlineState>().postures.get(&7),
            Some(&eq_client_core::PostureState::Sitting)
        );
        assert!(
            !app.world()
                .resource::<OnlineState>()
                .postures
                .contains_key(&999)
        );
        sender
            .send(WorldUpdate::Game(WorldEvent::Skill {
                skill_id: 22,
                value: 1,
            }))
            .unwrap();
        app.update();
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .player
                .as_ref()
                .unwrap()
                .skills
                .as_ref()
                .unwrap()[22],
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
            assert_eq!(
                app.world()
                    .resource::<OnlineState>()
                    .player
                    .as_ref()
                    .unwrap()
                    .level,
                level
            );
            assert_eq!(app.world().resource::<hud::HudState>().experience, Some(99));
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
        assert_eq!(
            app.world().resource::<OnlineState>().doors.entries()[&7].action,
            Some(2)
        );
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
        assert_eq!(
            app.world().resource::<hud::HudState>().pending_cast,
            Some(42)
        );
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
        assert_eq!(hud.pending_cast, Some(42));
        sender
            .send(WorldUpdate::Game(WorldEvent::CastPending {
                session_id: 1,
                spell_id: None,
            }))
            .unwrap();
        app.update();
        assert_eq!(app.world().resource::<hud::HudState>().pending_cast, None);
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
            app.world().resource::<hud::HudState>().book_action,
            Some(eq_client_core::BookActionStatus::Preparing)
        );
        assert_eq!(
            app.world().resource::<hud::HudState>().book_action_revision,
            3 // Two deletion replies and one preparation notification.
        );
        sender
            .send(WorldUpdate::Game(WorldEvent::BookAction(
                eq_client_core::BookActionStatus::Preparing,
            )))
            .unwrap();
        app.update();
        assert_eq!(
            app.world().resource::<hud::HudState>().book_action_revision,
            4
        );
        sender
            .send(WorldUpdate::Game(WorldEvent::Death(death.clone())))
            .unwrap();
        app.update();
        assert!(app.world().resource::<OnlineState>().death.is_none());
        app.world_mut()
            .resource_mut::<super::super::target::TargetState>()
            .selected = Some(8);
        death.spawn_id = 7;
        sender
            .send(WorldUpdate::Game(WorldEvent::Death(death)))
            .unwrap();
        app.update();
        assert!(app.world().resource::<OnlineState>().death.is_some());
        assert_eq!(
            app.world()
                .resource::<super::super::target::TargetState>()
                .selected,
            None
        );
        assert_eq!(app.world().resource::<hud::HudState>().hp_percent, Some(0));
        assert_eq!(app.world().resource::<hud::HudState>().pending_cast, None);
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
        assert!(app.world().resource::<hud::HudState>().casting.is_none());
        assert!(
            app.world()
                .resource::<hud::HudState>()
                .book_action
                .is_none()
        );
        sender
            .send(WorldUpdate::Game(WorldEvent::ZoneTransferRejected {
                session_id: 1,
                reason: eq_client_core::ZoneRejection::Cancelled,
            }))
            .unwrap();
        app.update();
        assert!(!app.world().resource::<OnlineState>().connected);
        let player = app
            .world()
            .resource::<OnlineState>()
            .player
            .clone()
            .unwrap();
        sender
            .send(WorldUpdate::Game(WorldEvent::Entered {
                session_id: 2,
                zone: "freportw".into(),
                player: Box::new(player),
            }))
            .unwrap();
        app.update();
        // Asset loading is intentionally absent in this event-level test.
        assert!(app.world().resource::<OnlineState>().death.is_none());
        assert!(app.world().resource::<OnlineState>().health.is_empty());
        assert!(
            app.world()
                .resource::<OnlineState>()
                .doors
                .entries()
                .is_empty()
        );
    }
}
