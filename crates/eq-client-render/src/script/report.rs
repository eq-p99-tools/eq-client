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
    ((hud, lines), ..): &Observed,
    transform: Option<&Transform>,
) {
    let position = transform.map(placement);
    let slots: Vec<u32> = online
        .world()
        .buffs()
        .slots()
        .map(|slots| slots.values().map(|buff| buff.spell_id).collect())
        .unwrap_or_default();
    let effects: Vec<u16> = online.world().buffs().effects().keys().copied().collect();
    let posture = online
        .world()
        .player()
        .and_then(|player| online.world().posture(player.spawn_id));
    let items: Vec<ReportedItem> = online
        .world()
        .inventory()
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
    let book: Vec<(usize, u32)> = online
        .world()
        .spell_book()
        .map(|book| {
            book.slots()
                .iter()
                .enumerate()
                .filter_map(|(slot, spell)| spell.map(|spell| (slot, spell)))
                .collect()
        })
        .unwrap_or_default();
    let gems = online.world().gems();
    // Milliseconds until each memorized gem can be cast again.
    let now = std::time::Instant::now();
    let cooldowns = gems.map(|spell| {
        spell.map(|spell| {
            online
                .world()
                .casting()
                .cooldowns
                .remaining(spell, now)
                .as_millis()
        })
    });
    info!(
        label,
        zone = online.world().zone(),
        capabilities = ?online.world().capabilities(),
        world = ?online.world().world_name(),
        far_clip = ?online.world().far_clip(),
        connected = online.world().connected(),
        ?position,
        ?posture,
        hp = ?online.world().hit_points(),
        mana = ?online.world().vitals().mana,
        endurance = ?online.world().vitals().endurance,
        estimate = ?hud.resource_estimate,
        casting = ?online.world().casting().cast.map(|(spell, _, _)| spell),
        pending = ?online.world().casting().pending,
        interrupted = ?online.world().casting().interrupted.map(|(_, id)| id),
        feedback = lines.feedback.text(std::time::Instant::now()),
        target = ?online.world().target().selected,
        considered = ?online.world().target().selected.and_then(|id| online.world().considered(id)),
        camp = ?online.world().camp().map(|camp| camp.logging_out),
        ?gems,
        ?cooldowns,
        ?book,
        ?slots,
        ?effects,
        ?items,
        inventory_predicted = online.world().inventory().predicted(),
        inventory_stale = online.world().inventory().stale(),
        cursor_queued = online.world().inventory().queued().count(),
        gear = ?online.world().player().map(|player| player.appearance.materials),
        tints = ?online.world().player().map(|player| player.appearance.tints),
        face = ?online.world().player().map(|player| player.appearance.face),
        show_helm = ?online.world().player().map(|player| player.appearance.show_helm),
        "Script report"
    );
}

/// A nearby spawn's gear: its id, materials per texture slot, the chest's
/// tint and whether its helm shows.
type Gear = (u16, [u32; 9], Option<[u8; 3]>, bool);

/// A nearby spawn's model: its id, race, gender and the model they draw.
type NearbyModel = (u16, u32, u32, Option<&'static str>);

/// Spawn id, shown name, kind, class, distance and rounded EQ x, y, z.
/// Door id, open type, latest action, distance and EQ position.
type NearbyDoor = (u8, u8, Option<u8>, i32, [i32; 3]);
type NearbySpawn = (u16, String, String, Option<u8>, i32, [i32; 3]);

/// Logs the nearest visible spawns, coins, open trade windows and auto-attack.
pub(super) fn surroundings(
    online: &crate::online::OnlineState,
    ((_, lines), .., combat, _): &Observed,
) {
    let origin = online
        .world()
        .player()
        .map(|player| Vec3::from_array(eq_client_core::render_position(player.position)));
    let mut nearby: Vec<NearbySpawn> = online
        .world()
        .spawns()
        .iter()
        .map(|(id, spawn)| (id, &spawn.state))
        .filter(|(id, spawn)| {
            !spawn.invisible
                && online
                    .world()
                    .player()
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
                eq_client_core::entities::display_name(&spawn.name),
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
        .world()
        .doors()
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
        .world()
        .objects()
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
    let (models, gear) = looks(online, &nearby);
    info!(
        ?nearby,
        ?creatures,
        ?doors,
        ?ground,
        ?gear,
        ?models,
        door_status = lines.door.text(std::time::Instant::now()),
        coins = ?online.world().coins(),
        trade = crate::trade::summary(online.world()),
        auto_attack = combat.auto_attack,
        "Script surroundings"
    );
}

/// How the nearest spawns look: each one's model by race and gender, and the
/// gear of the nearest six.
fn looks(
    online: &crate::online::OnlineState,
    nearby: &[NearbySpawn],
) -> (Vec<NearbyModel>, Vec<Gear>) {
    let spawns = || {
        nearby.iter().filter_map(|entry| {
            Some((
                entry.0,
                online.world().spawn(entry.0).map(|spawn| &spawn.state)?,
            ))
        })
    };
    let models = spawns()
        .map(|(id, spawn)| {
            let model = eq_client_core::races::model(spawn.race, spawn.gender);
            (id, spawn.race, spawn.gender, model)
        })
        .collect();
    let gear = spawns()
        .take(6)
        .map(|(id, spawn)| {
            let look = spawn.appearance;
            (id, look.materials, look.tints[1], look.show_helm)
        })
        .collect();
    (models, gear)
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
