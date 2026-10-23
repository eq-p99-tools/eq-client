//! Reads authoritative world state into skin gauges, labels and conditional visibility.
use super::{Shows, Step, gem_name};
use crate::theme;
use bevy::prelude::*;

/// Whether a piece that shows only at times shows now; None for one that
/// always shows.
pub(super) fn shown_now(
    shows: Shows,
    world: &eq_client_core::world::ClientWorld,
    combat: &crate::combat::CombatState,
    trade: &crate::trade::TradeState,
) -> Option<bool> {
    Some(match shows {
        Shows::Attacking => combat.auto_attack,
        Shows::WhilePetSits(sits) => {
            let pet_sits = world
                .pet()
                .and_then(|pet| world.posture(pet.state.spawn_id))
                == Some(eq_client_core::PostureState::Sitting);
            sits == pet_sits
        }
        Shows::WhileSelling(selling) => {
            matches!(trade.chosen(), Some(crate::trade::Chosen::Carried(_))) == selling
        }
        Shows::PetBuff(slot) => world
            .pet_buffs()
            .and_then(|buffs| buffs.slots.get(slot).copied().flatten())
            .is_some(),
        Shows::Buff(window, index) => world.buffs().in_window(window, index).is_some(),
        Shows::Pet => world.pet().is_some(),
        Shows::Member(place) => world.group_place(place).is_some(),
        Shows::MemberPet(place) => world.group_pet(place).is_some(),
        Shows::WhileInvited(invited) => world.group_invitation().is_some() == invited,
        Shows::WhileRaidInvited(invited) => world.raid_invitation().is_some() == invited,
        Shows::WhileRaidLocked(locked) => world.raid().is_some_and(|raid| raid.locked) == locked,
        _ => return None,
    })
}

/// Fills a gauge's bar as far as this fraction, from 0 to 1, showing a
/// step's fill only once the level passes its box. A node changes only
/// when what it shows does.
pub(super) fn fill_to(node: &mut Mut<Node>, step: Option<&Step>, fraction: f32) {
    let width = percent(fraction * 100.0);
    if node.width != width {
        node.width = width;
    }
    let display = if step.is_none_or(|step| step.shows(fraction)) {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
}

/// Shows the world in the skinned windows' gauges and labels.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    online: Res<crate::online::OnlineState>,
    (hud, inventory, names): (
        Res<crate::hud::HudState>,
        Res<crate::inventory::InventoryState>,
        Res<crate::spellbook::SpellNames>,
    ),
    (combat, trade, book, requests): (
        Res<crate::combat::CombatState>,
        Res<crate::trade::TradeState>,
        Res<crate::spellbook::BookView>,
        Res<crate::hud::action_bar::ActionRequests>,
    ),
    mut fills: Query<(&Shows, &mut Node, Option<&Step>), Without<Text>>,
    mut texts: Query<(&Shows, &mut Text, &mut TextColor)>,
    mut boxes: Query<(&Shows, &mut Visibility)>,
) {
    let world = online.world();
    for (shows, mut visibility) in &mut boxes {
        let Some(shown) = shown_now(*shows, world, &combat, &trade) else {
            continue;
        };
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (shows, mut node, step) in &mut fills {
        if let Shows::Fill(kind) = *shows {
            let fraction = filled(world, (hud.resource_estimate, &requests), kind);
            fill_to(&mut node, step, fraction.clamp(0.0, 1.0));
        }
    }
    for (shows, mut text, mut color) in &mut texts {
        let (wanted, tint) = match *shows {
            Shows::GaugeText(kind) => gauge_text(world, kind),
            // The spell the player casts.
            Shows::Label(134) => (
                crate::hud::action_bar::cast_spell(world)
                    .map_or_else(String::new, |spell| names.label(spell)),
                None,
            ),
            Shows::Label(kind) => (label_text(world, hud.resource_estimate, kind), None),
            Shows::BuffName(window, index) => (
                world
                    .buffs()
                    .in_window(window, index)
                    .map_or_else(String::new, |shown| names.label(shown.spell_id())),
                None,
            ),
            Shows::GemName(gem) => (gem_name(world, &names, gem), None),
            Shows::Coins(place, coin) => (crate::coins::shown(world, place, coin), None),
            Shows::Offered(coin) => (
                world
                    .offered_coins()
                    .map_or_else(String::new, |coins| coins.of(coin).to_string()),
                None,
            ),
            Shows::Banker => (inventory.banker().to_owned(), None),
            Shows::PracticePoints => (crate::training::practice_points(world), None),
            Shows::Corpse | Shows::Merchant | Shows::ChosenName | Shows::ChosenPrice => {
                (trading_text(*shows, &trade, world), None)
            }
            Shows::BookName(place) => (
                book.spell(world.spell_book(), place)
                    .map_or_else(String::new, |spell| names.label(spell)),
                None,
            ),
            // Two pages to a spread, counted from one.
            Shows::BookPage(right) => {
                ((book.spread * 2 + 1 + usize::from(right)).to_string(), None)
            }
            // The sign shows with the number before it.
            Shows::MemberPercent(place) => (
                if world.group_health(place).is_some() {
                    "%".to_owned()
                } else {
                    String::new()
                },
                None,
            ),
            Shows::RaidCount | Shows::RaidLevel => (raid_text(*shows, world), None),
            Shows::Partner => (
                crate::give::partner(world),
                Some(crate::give::ink(world, crate::give::Side::Theirs)),
            ),
            Shows::Trader => (
                world
                    .player()
                    .map_or_else(String::new, |player| player.name.clone()),
                Some(crate::give::ink(world, crate::give::Side::Mine)),
            ),
            Shows::Fill(_)
            | Shows::Attacking
            | Shows::WhilePetSits(_)
            | Shows::PetBuff(_)
            | Shows::Buff(..)
            | Shows::WhileSelling(_)
            | Shows::Pet
            | Shows::Member(_)
            | Shows::MemberPet(_)
            | Shows::WhileInvited(_)
            | Shows::WhileRaidInvited(_)
            | Shows::WhileRaidLocked(_) => {
                continue;
            }
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
        if let Some(tint) = tint
            && color.0 != tint
        {
            color.0 = tint;
        }
    }
}

/// What the Raid window's count of members and their average level say:
/// nought outside a raid, as the skin's own text says.
pub(super) fn raid_text(shows: Shows, world: &eq_client_core::world::ClientWorld) -> String {
    let raid = world.raid();
    match shows {
        Shows::RaidCount => raid.map_or(0, |raid| raid.members.len()).to_string(),
        _ => raid
            .and_then(eq_client_core::world::Raid::level_average)
            .unwrap_or(0)
            .to_string(),
    }
}

/// What a loot or merchant window's label says. What the merchant pays for
/// a carried item is not known yet, so its price shows only for a ware.
pub(super) fn trading_text(
    shows: Shows,
    trade: &crate::trade::TradeState,
    world: &eq_client_core::world::ClientWorld,
) -> String {
    let chosen = || crate::trade::chosen(trade, world);
    match shows {
        Shows::Corpse => trade.corpse().unwrap_or_default().to_owned(),
        Shows::Merchant => trade.merchant().unwrap_or_default().to_owned(),
        Shows::ChosenName => {
            chosen().map_or_else(String::new, |(item, _)| item.details.name.clone())
        }
        Shows::ChosenPrice => chosen()
            .and_then(|(_, price)| price)
            .map_or_else(String::new, |price| {
                crate::trade::coin_text(u64::from(price))
            }),
        _ => String::new(),
    }
}

/// How full a gauge is drawn: as `fraction` says, or the spellbook's
/// memorization (9) and scribe (10), as their gauges' names say; empty
/// where nothing says.
pub(super) fn filled(
    world: &eq_client_core::world::ClientWorld,
    (estimate, requests): (Option<(u32, u32)>, &crate::hud::action_bar::ActionRequests),
    kind: u32,
) -> f32 {
    use crate::hud::action_bar::{BookChange, book_progress};
    let now = std::time::Instant::now();
    match kind {
        9 => book_progress(world, requests, BookChange::Memorize, now),
        10 => book_progress(world, requests, BookChange::Scribe, now),
        _ => fraction(world, estimate, kind),
    }
    .unwrap_or(0.0)
}

/// How full a gauge is, by the official client's numbering.
#[allow(clippy::cast_possible_truncation, reason = "a fraction for drawing")]
pub(super) fn fraction(
    world: &eq_client_core::world::ClientWorld,
    estimate: Option<(u32, u32)>,
    kind: u32,
) -> Option<f32> {
    use crate::hud::Stat;
    let stat = |stat: Stat| stat.ratio(world, estimate).map(|ratio| ratio as f32);
    match kind {
        1 => stat(Stat::Hp),
        2 => stat(Stat::Mana),
        3 => stat(Stat::Stamina),
        4 => stat(Stat::Experience),
        6 => target_health(world).map(|percent| f32::from(percent) / 100.0),
        // The player's cast; empty until the server begins it.
        7 => Some(
            crate::hud::action_bar::cast_progress(world, std::time::Instant::now()).unwrap_or(0.0),
        ),
        16 => world
            .pet()
            .and_then(|pet| world.health(pet.state.spawn_id))
            .map(|percent| f32::from(percent) / 100.0),
        // The group's other members, and their pets, in their places.
        11..=15 => world
            .group_health((kind - 11) as usize)
            .map(|percent| f32::from(percent) / 100.0),
        17..=21 => world
            .group_pet((kind - 17) as usize)
            .and_then(|pet| world.health(pet.state.spawn_id))
            .map(|percent| f32::from(percent) / 100.0),
        _ => None,
    }
}

/// A gauge's text: the player's name over their hit points, the target's,
/// in its consider colour, over theirs, the pet's over its own, and each
/// group member's over theirs.
pub(super) fn gauge_text(
    world: &eq_client_core::world::ClientWorld,
    kind: u32,
) -> (String, Option<Color>) {
    match kind {
        1 => (
            world
                .player()
                .map_or_else(String::new, |player| player.name.clone()),
            None,
        ),
        6 => {
            let target = world.target().selected;
            let color = theme::con(target.and_then(|id| world.considered(id)));
            (target_name(world), Some(color))
        }
        // A group member's name over their health (inferred).
        11..=15 => (
            world
                .group_place((kind - 11) as usize)
                .unwrap_or_default()
                .to_owned(),
            None,
        ),
        // The pet's name; without a pet, the gauge hides.
        16 => (
            world
                .pet()
                .map(|pet| eq_client_core::entities::display_name(&pet.state.name))
                .unwrap_or_default(),
            None,
        ),
        _ => (String::new(), None),
    }
}

/// The target's name: a spawn's as players see it, or the player's own
/// while they target themselves; nothing without a target.
pub(super) fn target_name(world: &eq_client_core::world::ClientWorld) -> String {
    let target = world.target().selected;
    target
        .and_then(|id| world.spawn(id))
        .map(|spawn| eq_client_core::entities::display_name(&spawn.state.name))
        .or_else(|| {
            world
                .player()
                .filter(|player| Some(player.spawn_id) == target)
                .map(|player| player.name.clone())
        })
        .unwrap_or_default()
}

/// A label's text, by the official client's numbering: percentages of the
/// player's hit points, mana and stamina, the target's name and hit points,
/// each group member's, and the members' names.
pub(super) fn label_text(
    world: &eq_client_core::world::ClientWorld,
    estimate: Option<(u32, u32)>,
    kind: u32,
) -> String {
    let percent = |fraction: Option<f32>| {
        fraction.map_or_else(String::new, |fraction| {
            format!("{:.0}", (fraction * 100.0).clamp(0.0, 100.0))
        })
    };
    let player = world.player();
    let number = |value: Option<u32>| value.map_or_else(String::new, |value| value.to_string());
    let attribute = |pick: fn(&eq_client_core::BaseAttributes) -> i32| {
        player
            .and_then(|player| player.base_attributes.as_ref())
            .map_or_else(String::new, |attributes| pick(attributes).to_string())
    };
    let vitals = world.vitals();
    match kind {
        1 => player.map_or_else(String::new, |player| player.name.clone()),
        2 => player.map_or_else(String::new, |player| player.level.to_string()),
        3 => player
            .and_then(|player| player.class)
            .and_then(eq_client_core::classes::class_name)
            .unwrap_or_default()
            .to_owned(),
        4 => player
            .and_then(|player| player.deity)
            .and_then(eq_client_core::classes::deity_name)
            .unwrap_or_default()
            .to_owned(),
        // The profile's base attributes; what items and spells add is not
        // reported to the client.
        5 => attribute(|attributes| attributes.strength),
        6 => attribute(|attributes| attributes.stamina),
        7 => attribute(|attributes| attributes.dexterity),
        8 => attribute(|attributes| attributes.agility),
        9 => attribute(|attributes| attributes.wisdom),
        10 => attribute(|attributes| attributes.intelligence),
        11 => attribute(|attributes| attributes.charisma),
        17 => number(world.hit_points().map(|(current, _)| current)),
        18 => number(world.hit_points().map(|(_, maximum)| maximum)),
        19 => percent(fraction(world, estimate, 1)),
        20 => percent(fraction(world, estimate, 2)),
        21 => percent(fraction(world, estimate, 3)),
        // The target's name, in the skin's colour; whether the official
        // client tints it by consider colour, as the target gauge's text is
        // tinted, is inferred.
        28 => target_name(world),
        29 => target_health(world).map_or_else(String::new, |health| health.to_string()),
        // The group's other members' names and health, in their places.
        30..=34 => world
            .group_place((kind - 30) as usize)
            .unwrap_or_default()
            .to_owned(),
        35..=39 => world
            .group_health((kind - 35) as usize)
            .map_or_else(String::new, |health| health.to_string()),
        124 => number(vitals.mana),
        125 => number(estimate.map(|(mana, _)| mana)),
        126 => number(vitals.endurance),
        127 => number(estimate.map(|(_, endurance)| endurance)),
        _ => String::new(),
    }
}

/// The target's health in percent, the player's own when they target
/// themselves.
pub(super) fn target_health(world: &eq_client_core::world::ClientWorld) -> Option<u8> {
    let target = world.target().selected?;
    world.health(target).or_else(|| {
        world
            .player()
            .filter(|player| player.spawn_id == target)
            .and_then(|player| player.hp_percent)
    })
}
