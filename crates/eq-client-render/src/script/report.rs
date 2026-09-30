//! State a script logs for later review.
use bevy::prelude::*;

use super::Observed;

/// Slot, item id, stack count, scroll spell and whether it is NO DROP.
type ReportedItem = (i32, u32, Option<u32>, Option<u32>, bool);

/// EQ coordinates and heading of the movement root.
pub(super) fn placement(transform: &Transform) -> (f32, f32, f32, f32) {
    let world = crate::world_position(transform.translation.to_array(), 0.0);
    let facing = transform.rotation * Vec3::Z;
    let heading = eq_client_core::world_heading(facing.x.atan2(facing.z));
    (world.x, world.y, world.z, heading)
}

/// Logs player, resource, cast, buff, book and inventory state under a label.
pub(super) fn state(
    label: &str,
    online: &crate::online::OnlineState,
    (hud, inventory, target, ..): &Observed,
    transform: Option<&Transform>,
) {
    let position = transform.map(placement);
    let slots: Vec<u32> = hud
        .buff_state
        .slots()
        .map(|slots| slots.values().map(|buff| buff.spell_id).collect())
        .unwrap_or_default();
    let effects: Vec<u16> = hud.buff_state.effects().keys().copied().collect();
    let posture = online
        .player
        .as_ref()
        .and_then(|player| online.postures.get(&player.spawn_id));
    let items: Vec<ReportedItem> = inventory
        .data
        .items()
        .values()
        .map(|item| {
            (
                item.slot.0,
                item.details.id,
                item.stack_count,
                item.scroll_spell,
                item.details.flags.iter().any(|flag| flag == "NO DROP"),
            )
        })
        .collect();
    let book: Vec<(usize, u32)> = hud
        .spell_book
        .as_ref()
        .map(|book| {
            book.slots()
                .iter()
                .enumerate()
                .filter_map(|(slot, spell)| spell.map(|spell| (slot, spell)))
                .collect()
        })
        .unwrap_or_default();
    info!(
        label,
        zone = online.zone,
        world = ?online.world,
        far_clip = ?online.far_clip,
        connected = online.connected,
        ?position,
        ?posture,
        hp = ?hud.hp,
        mana = ?hud.mana,
        endurance = ?hud.endurance,
        estimate = ?hud.resource_estimate,
        casting = ?hud.casting.map(|(spell, _, _)| spell),
        pending = ?hud.pending_cast,
        interrupted = ?hud.interrupted.map(|(_, id)| id),
        feedback = ?hud.action_feedback.as_ref().map(|(_, text)| text),
        target = ?target.selected,
        gems = ?hud.spells,
        ?book,
        ?slots,
        ?effects,
        ?items,
        inventory_predicted = inventory.data.predicted(),
        inventory_stale = inventory.data.stale(),
        cursor_queued = inventory.data.queued().count(),
        "Script report"
    );
}

/// Spawn id, shown name, kind, class, distance and rounded EQ x, y, z.
/// Door id, open type, latest action, distance and EQ position.
type NearbyDoor = (u8, u8, Option<u8>, i32, [i32; 3]);
type NearbySpawn = (u16, String, String, Option<u8>, i32, [i32; 3]);

/// Logs the nearest visible spawns, coins, open trade windows and auto-attack.
pub(super) fn surroundings(online: &crate::online::OnlineState, (.., trade, combat, _): &Observed) {
    let origin = online
        .player
        .as_ref()
        .map(|player| Vec3::from_array(eq_client_core::render_position(player.position)));
    let mut nearby: Vec<NearbySpawn> = online
        .spawns
        .iter()
        .filter(|(id, spawn)| {
            !spawn.invisible
                && online
                    .player
                    .as_ref()
                    .is_none_or(|player| player.spawn_id != **id)
        })
        .map(|(id, spawn)| {
            let position = Vec3::from_array(eq_client_core::render_position(spawn.position));
            #[allow(clippy::cast_possible_truncation)] // Rounded report distances and coordinates.
            let (distance, at) = (
                origin.map_or(-1, |origin| position.distance(origin).round() as i32),
                [spawn.position.x, spawn.position.y, spawn.position.z].map(|v| v.round() as i32),
            );
            (
                *id,
                crate::combat::display_name(&spawn.name),
                format!("{:?}", spawn.kind),
                spawn.class,
                distance,
                at,
            )
        })
        .collect();
    nearby.sort_by_key(|entry| entry.4);
    let creatures: Vec<(u16, String, String, i32, [i32; 3])> = nearby
        .iter()
        .filter(|entry| entry.1.starts_with(|c: char| c.is_ascii_lowercase()))
        .take(10)
        .map(|(id, name, kind, _, distance, at)| (*id, name.clone(), kind.clone(), *distance, *at))
        .collect();
    nearby.truncate(12);
    // Doors near the player: id, open type, latest action, distance and position.
    let mut doors: Vec<NearbyDoor> = online
        .doors
        .entries()
        .values()
        .map(|door| {
            let position = Vec3::from_array(eq_client_core::render_position(door.position));
            #[allow(clippy::cast_possible_truncation)] // Rounded report values.
            let (distance, at) = (
                origin.map_or(-1, |origin| position.distance(origin).round() as i32),
                [door.position.x, door.position.y, door.position.z].map(|v| v.round() as i32),
            );
            (door.id, door.open_type, door.action, distance, at)
        })
        .collect();
    doors.sort_by_key(|door| door.3);
    doors.truncate(3);
    // Objects on the ground: id, model, kind, distance and position.
    let mut ground: Vec<(u32, String, String, i32, [i32; 3])> = online
        .objects
        .entries()
        .values()
        .map(|object| {
            let position = Vec3::from_array(eq_client_core::render_position(object.position));
            #[allow(clippy::cast_possible_truncation)] // Rounded report values.
            let (distance, at) = (
                origin.map_or(-1, |origin| position.distance(origin).round() as i32),
                [object.position.x, object.position.y, object.position.z].map(|v| v.round() as i32),
            );
            let kind = format!("{:?}", object.kind());
            (object.drop_id, object.model.clone(), kind, distance, at)
        })
        .collect();
    ground.sort_by_key(|object| object.3);
    ground.truncate(5);
    info!(
        ?nearby,
        ?creatures,
        ?doors,
        ?ground,
        door_status = online.door_status,
        coins = ?trade.coins,
        trade = trade.summary(),
        auto_attack = combat.auto_attack,
        "Script surroundings"
    );
}

/// Logs game messages (lines without a speaker) received since the last report.
pub(super) fn game_messages(seen: &mut u64, chat: &crate::chat::ChatState) {
    for (id, line) in chat.history.lines(eq_client_core::chat::ChatTab::All) {
        if id > *seen && line.sender.as_deref().is_none_or(str::is_empty) {
            info!(text = line.message.text, "Script game message");
        }
        *seen = (*seen).max(id);
    }
}
