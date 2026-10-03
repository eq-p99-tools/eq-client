//! State a script logs for later review.
use bevy::prelude::*;

use super::Observed;

/// Slot, item id, stack count, scroll spell and whether it is NO DROP.
type ReportedItem = (i32, u32, Option<u32>, Option<u32>, bool, Option<u32>);

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
    (hud, ..): &Observed,
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
    let items = items(online);
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
    let pet = pet(online);
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
        exchange = ?online.world().exchange(),
        abilities = ?online.world().abilities(),
        abilities_not_offered = ?online
            .world()
            .abilities()
            .into_iter()
            .filter(|ability| !online.world().ability_offered(*ability))
            .collect::<Vec<_>>(),
        nourishment = ?online.world().nourishment(),
        game_time = ?online.world().game_time(std::time::Instant::now()),
        sky = ?online.world().sky(),
        ?pet,
        training = ?training(online),
        practice_points = ?online.world().player().and_then(|player| player.practice_points),
        resurrection = ?resurrection(online),
        readable = ?readable(online),
        containers = ?containers(online),
        combining = ?online.world().combining(),
        purse = ?online.world().coins(),
        cursor_coins = ?online.world().coins_in(eq_client_core::money::CoinPlace::Cursor),
        bank_coins = ?online.world().coins_in(eq_client_core::money::CoinPlace::Bank),
        gear = ?online.world().player().map(|player| player.appearance.materials),
        tints = ?online.world().player().map(|player| player.appearance.tints),
        face = ?online.world().player().map(|player| player.appearance.face),
        show_helm = ?online.world().player().map(|player| player.appearance.show_helm),
        "Script report"
    );
}

/// Every item the player has, where it is and what it is.
fn items(online: &crate::online::OnlineState) -> Vec<ReportedItem> {
    online
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
                item.details.price,
            )
        })
        .collect()
}

/// The guildmaster training the player, and how many skills they teach.
fn training(online: &crate::online::OnlineState) -> Option<(u16, usize)> {
    let offer = online.world().training()?;
    Some((
        offer.trainer,
        offer.caps.iter().filter(|cap| **cap > 0).count(),
    ))
}

/// The carried books and notes: slot, item, window kind and text name.
fn readable(online: &crate::online::OnlineState) -> Vec<(i32, u32, u8, String)> {
    online
        .world()
        .inventory()
        .items()
        .values()
        .filter_map(|item| {
            let book = item.book.as_ref()?;
            Some((item.slot.0, item.details.id, book.kind, book.file.clone()))
        })
        .collect()
}

/// The carried tradeskill containers: slot, item and bag type.
fn containers(online: &crate::online::OnlineState) -> Vec<(i32, u32, u8)> {
    online
        .world()
        .inventory()
        .items()
        .values()
        .filter(|item| eq_client_core::tradeskills::can_combine_in(item))
        .map(|item| (item.slot.0, item.details.id, item.rules.bag_type))
        .collect()
}

/// The resurrection waiting for an answer: its caster, corpse and spell.
fn resurrection(online: &crate::online::OnlineState) -> Option<(String, String, u32)> {
    let offer = online.world().resurrection()?;
    Some((offer.caster.clone(), offer.corpse.clone(), offer.spell_id))
}

/// The pet's spawn, health, posture and buffs.
fn pet(online: &crate::online::OnlineState) -> Option<impl std::fmt::Debug> {
    let id = online.world().pet()?.state.spawn_id;
    Some((
        id,
        online.world().health(id),
        online.world().posture(id),
        online.world().pet_buffs().map(|buffs| {
            buffs
                .slots
                .iter()
                .flatten()
                .map(|buff| buff.spell_id)
                .collect::<Vec<_>>()
        }),
    ))
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

/// A guildmaster in view: spawn, name, class, distance and position.
type NearbyGuildmaster = (u16, String, Option<u8>, i32, [i32; 3]);

/// An object near the player: id, model, kind or type, distance and
/// position.
type NearbyObject<T> = (u32, String, T, i32, [i32; 3]);

/// The objects on the ground nearest the player, and the zone's world
/// containers (such as forges and ovens) nearest first.
fn objects(
    online: &crate::online::OnlineState,
    origin: Option<Vec3>,
) -> (Vec<NearbyObject<String>>, Vec<NearbyObject<u32>>) {
    // Objects on the ground: id, model, kind, distance and position.
    let mut ground: Vec<NearbyObject<String>> = online
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
    // The zone's world containers, such as forges and ovens, nearest first.
    let mut stations: Vec<NearbyObject<u32>> = online
        .world()
        .objects()
        .entries()
        .values()
        .filter(|object| object.is_tradeskill_container())
        .filter_map(|object| {
            let at = ground.iter().find(|entry| entry.0 == object.drop_id)?;
            Some((
                object.drop_id,
                object.model.clone(),
                object.object_type,
                at.3,
                at.4,
            ))
        })
        .collect();
    stations.sort_by_key(|station| station.3);
    stations.truncate(8);
    ground.truncate(5);
    (ground, stations)
}

/// Logs the nearest visible spawns, coins, open trade windows and auto-attack.
pub(super) fn surroundings(online: &crate::online::OnlineState, (.., combat, _): &Observed) {
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
    // Guildmasters anywhere in reach of the view, for training checks.
    let guildmasters: Vec<NearbyGuildmaster> = nearby
        .iter()
        .filter(|entry| crate::training::is_guildmaster(entry.3))
        .take(10)
        .map(|(id, name, _, class, distance, at)| (*id, name.clone(), *class, *distance, *at))
        .collect();
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
    let (ground, stations) = objects(online, origin);
    let (models, gear) = looks(online, &nearby);
    info!(
        ?nearby,
        ?creatures,
        ?guildmasters,
        ?doors,
        ?ground,
        ?stations,
        container = ?online
            .world()
            .container()
            .map(|view| (view.drop_id, view.name.clone(), view.object_type, view.icon)),
        ?gear,
        ?models,
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
            // The items a line links to, such as a recipe's components.
            let links: Vec<(&str, u32)> = line
                .message
                .item_links
                .iter()
                .map(|link| (link.text.as_str(), link.item_id))
                .collect();
            info!(text = line.message.text, ?links, "Script game message");
        }
        *seen = (*seen).max(id);
    }
}
