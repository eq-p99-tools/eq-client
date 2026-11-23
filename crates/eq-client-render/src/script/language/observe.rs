//! Read-only fields from the current client projection, with its existing evidence bounds.

use eq_client_core::world::ClientWorld;

use super::{Kind, Value};

/// The spell gems a player has, on every client the project supports.
const GEMS: u8 = 8;
/// The others a group holds besides the player.
const GROUP_PLACES: u8 = 5;
/// The most effects a script can name by place, more than any effects
/// window shows.
const BUFF_PLACES: u8 = 50;

/// What the player's windows draw on: the world, the installation's spell
/// names and linked timers, the installed client's clock and the moment.
pub(in crate::script) struct Sight<'a> {
    pub world: &'a ClientWorld,
    pub spells: Option<&'a crate::spellbook::SpellNames>,
    pub clock: eq_client_assets::clock::ClientClock,
    pub now: std::time::Instant,
    /// The chat window's history, for the tells it shows.
    pub chat: Option<&'a crate::chat::ChatState>,
    /// The Options window's Filters page, for the mode each row shows.
    pub filters: Option<&'a eq_client_core::chat_filters::ChatFilters>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Ready,
    Zone,
    Level,
    ReportedHp,
    ReportedHpMax,
    Mana,
    Endurance,
    TargetSelected,
    TargetName,
    // The player as the HUD, the cast bar, `/loc`, the experience bar and
    // the inventory's purse show them (6.592).
    HpPercent,
    Posture,
    Casting,
    Stunned,
    LocX,
    LocY,
    LocZ,
    ExpPercent,
    Coin(Coin),
    // The target as its window and a consider show it.
    TargetHpPercent,
    TargetCon,
    TargetKind,
    // The pet as the pet window shows it.
    PetPresent,
    PetName,
    PetHpPercent,
    // A spell gem as the gem bar shows it, counted from 0 (6.592, O1b).
    GemSpell(u8),
    GemReady(u8),
    // The time in Norrath as `/time` tells it.
    Time(TimePart),
    // The group window, its others counted from 0 (6.593, O2).
    GroupPresent,
    GroupCount,
    GroupName(u8),
    GroupHpPercent(u8),
    // The effects window, its icons counted from 0 in the order it draws them.
    BuffCount,
    BuffSpell(u8),
    // The last tell the chat window shows. Never logged or reported (O-b).
    LastTell,
    LastTellFrom,
    // A Filters page row as the Options window shows it (6.494, K3).
    Filter(eq_client_core::filters::FilterCategory),
}

/// One part of the time `/time` tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TimePart {
    Hour,
    Minute,
    Day,
    Month,
    Year,
}

/// A coin in the purse, as the inventory window shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Coin {
    Platinum,
    Gold,
    Silver,
    Copper,
}

impl Field {
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "client.ready" => Some(Self::Ready),
            "zone.name" => Some(Self::Zone),
            "player.level" => Some(Self::Level),
            "player.reported_hp" => Some(Self::ReportedHp),
            "player.reported_hp_max" => Some(Self::ReportedHpMax),
            "player.mana" => Some(Self::Mana),
            "player.endurance" => Some(Self::Endurance),
            "target.selected" => Some(Self::TargetSelected),
            "target.name" => Some(Self::TargetName),
            "player.hp_percent" => Some(Self::HpPercent),
            "player.posture" => Some(Self::Posture),
            "player.casting" => Some(Self::Casting),
            "player.stunned" => Some(Self::Stunned),
            "player.loc_x" => Some(Self::LocX),
            "player.loc_y" => Some(Self::LocY),
            "player.loc_z" => Some(Self::LocZ),
            "player.exp_percent" => Some(Self::ExpPercent),
            "player.platinum" => Some(Self::Coin(Coin::Platinum)),
            "player.gold" => Some(Self::Coin(Coin::Gold)),
            "player.silver" => Some(Self::Coin(Coin::Silver)),
            "player.copper" => Some(Self::Coin(Coin::Copper)),
            "target.hp_percent" => Some(Self::TargetHpPercent),
            "target.con" => Some(Self::TargetCon),
            "target.kind" => Some(Self::TargetKind),
            "pet.present" => Some(Self::PetPresent),
            "pet.name" => Some(Self::PetName),
            "pet.hp_percent" => Some(Self::PetHpPercent),
            "time.hour" => Some(Self::Time(TimePart::Hour)),
            "time.minute" => Some(Self::Time(TimePart::Minute)),
            "time.day" => Some(Self::Time(TimePart::Day)),
            "time.month" => Some(Self::Time(TimePart::Month)),
            "time.year" => Some(Self::Time(TimePart::Year)),
            "group.present" => Some(Self::GroupPresent),
            "group.count" => Some(Self::GroupCount),
            "buffs.count" => Some(Self::BuffCount),
            "chat.last_tell" => Some(Self::LastTell),
            "chat.last_tell_from" => Some(Self::LastTellFrom),
            _ => Self::filter(name)
                .or_else(|| Self::gem(name))
                .or_else(|| Self::member(name))
                .or_else(|| Self::buff(name)),
        }
    }

    /// `group.member1.name` to `group.member5.hp_percent`, the group
    /// window's others from the top.
    fn member(name: &str) -> Option<Self> {
        let (place, part) = name.strip_prefix("group.member")?.split_once('.')?;
        let place = numbered(place, GROUP_PLACES)?;
        match part {
            "name" => Some(Self::GroupName(place)),
            "hp_percent" => Some(Self::GroupHpPercent(place)),
            _ => None,
        }
    }

    /// `buff1.spell` to `buff50.spell`, the effects window's icons in the
    /// order it draws them.
    fn buff(name: &str) -> Option<Self> {
        let place = name.strip_prefix("buff")?.strip_suffix(".spell")?;
        numbered(place, BUFF_PLACES).map(Self::BuffSpell)
    }

    /// Whether the field reads words someone sent the player. Its value
    /// stays on the player's own screen: no log, report, Runbook export or
    /// gap record carries it.
    #[cfg(test)]
    pub(super) const fn private(self) -> bool {
        matches!(self, Self::LastTell | Self::LastTellFrom)
    }

    /// `gem1.spell` to `gem8.ready`, numbered as the gem bar's keys are.
    fn gem(name: &str) -> Option<Self> {
        let (gem, part) = name.strip_prefix("gem")?.split_once('.')?;
        if gem.len() != 1 {
            return None;
        }
        let gem = gem
            .parse::<u8>()
            .ok()
            .filter(|gem| (1..=GEMS).contains(gem))?
            - 1;
        match part {
            "spell" => Some(Self::GemSpell(gem)),
            "ready" => Some(Self::GemReady(gem)),
            _ => None,
        }
    }

    pub(super) const fn kind(self) -> Kind {
        match self {
            Self::Ready
            | Self::TargetSelected
            | Self::Casting
            | Self::Stunned
            | Self::PetPresent
            | Self::GemReady(_)
            | Self::GroupPresent => Kind::Bool,
            Self::Zone
            | Self::TargetName
            | Self::Posture
            | Self::TargetCon
            | Self::TargetKind
            | Self::PetName
            | Self::GemSpell(_)
            | Self::GroupName(_)
            | Self::BuffSpell(_)
            | Self::LastTell
            | Self::LastTellFrom
            | Self::Filter(_) => Kind::Text,
            Self::Level
            | Self::ReportedHp
            | Self::ReportedHpMax
            | Self::Mana
            | Self::Endurance
            | Self::HpPercent
            | Self::LocX
            | Self::LocY
            | Self::LocZ
            | Self::ExpPercent
            | Self::Coin(_)
            | Self::TargetHpPercent
            | Self::PetHpPercent
            | Self::Time(_)
            | Self::GroupCount
            | Self::GroupHpPercent(_)
            | Self::BuffCount => Kind::Int,
        }
    }

    pub(super) fn read(self, sight: &Sight) -> Value {
        let world = sight.world;
        if self != Self::Ready && !world.in_world() {
            return if world.session_id().is_some() {
                Value::Stale
            } else {
                Value::Unknown
            };
        }
        let number = |value: Option<i64>| value.map_or(Value::Unknown, Value::Int);
        let vitals = world.vitals();
        match self {
            Self::Ready => Value::Bool(world.in_world()),
            Self::Zone if world.zone().is_empty() => Value::Unknown,
            Self::Zone => Value::Text(world.zone().into()),
            Self::Level => number(world.player().map(|player| i64::from(player.level))),
            Self::ReportedHp => number(vitals.reported_hp.map(|hp| i64::from(hp.current))),
            Self::ReportedHpMax => number(vitals.reported_hp.map(|hp| i64::from(hp.maximum))),
            Self::Mana => number(vitals.mana.map(i64::from)),
            Self::Endurance => number(vitals.endurance.map(i64::from)),
            Self::TargetSelected
            | Self::TargetName
            | Self::TargetHpPercent
            | Self::TargetCon
            | Self::TargetKind => self.target(world),
            Self::PetPresent | Self::PetName | Self::PetHpPercent => self.pet(world),
            Self::HpPercent
            | Self::Posture
            | Self::Casting
            | Self::Stunned
            | Self::LocX
            | Self::LocY
            | Self::LocZ
            | Self::ExpPercent
            | Self::Coin(_) => self.player(world),
            Self::GemSpell(_) | Self::GemReady(_) => self.gem_of(sight),
            Self::Time(part) => time(part, sight),
            Self::GroupPresent
            | Self::GroupCount
            | Self::GroupName(_)
            | Self::GroupHpPercent(_) => self.group(world),
            Self::BuffCount | Self::BuffSpell(_) => self.buff_of(sight),
            Self::LastTell | Self::LastTellFrom => self.tell(sight),
            Self::Filter(category) => filter_mode(category, sight),
        }
    }

    /// `filter.ooc` and the rest: a Filters page row by its kind's stable
    /// name. A kind with no row on the page is no field.
    fn filter(name: &str) -> Option<Self> {
        let category =
            eq_client_core::filters::FilterCategory::from_key(name.strip_prefix("filter.")?)?;
        eq_client_core::chat_filters::has_row(category).then_some(Self::Filter(category))
    }

    /// A gem as the gem bar shows it: its spell's name, unknown when empty,
    /// and whether it is lit, which is the gem bar's own rule: nothing being
    /// cast or awaiting the server, and the spell's reuse and any linked
    /// timer run out. An empty gem is never ready.
    fn gem_of(self, sight: &Sight) -> Value {
        let (Self::GemSpell(gem) | Self::GemReady(gem)) = self else {
            return Value::Unknown;
        };
        let spell = sight.world.gems().get(usize::from(gem)).copied().flatten();
        if matches!(self, Self::GemSpell(_)) {
            return match (spell, sight.spells) {
                (Some(spell), Some(names)) => Value::Text(names.label(spell).into()),
                _ => Value::Unknown,
            };
        }
        let casting = sight.world.casting();
        Value::Bool(spell.is_some_and(|spell| {
            let linked = sight.spells.and_then(|names| names.linked_timer(spell));
            casting.cast.is_none()
                && casting.pending.is_none()
                && casting.spell_wait(spell, linked, sight.now).is_zero()
        }))
    }
}

/// The time in Norrath as `/time` tells it: the hour as the installed
/// client names it, from 0 for 12 AM to 23 for 11 PM, then the minute, day,
/// month and year. Unknown before the server gives the time, and in a zone
/// where the client says the time can't be told.
fn time(part: TimePart, sight: &Sight) -> Value {
    let timeless = eq_client_core::zones::number(sight.world.zone())
        .and_then(|zone| sight.clock.timeless(zone))
        .is_some();
    let Some(time) = sight.world.game_time(sight.now).filter(|_| !timeless) else {
        return Value::Unknown;
    };
    Value::Int(match part {
        TimePart::Hour => i64::from(sight.clock.shown_hour(time.hour)),
        TimePart::Minute => i64::from(time.minute),
        TimePart::Day => i64::from(time.day),
        TimePart::Month => i64::from(time.month),
        TimePart::Year => i64::from(time.year),
    })
}

/// A place number as a field names it, from 1 to `places`; counted from 0.
fn numbered(place: &str, places: u8) -> Option<u8> {
    if place.starts_with('0') || place.len() > 2 {
        return None;
    }
    place
        .parse::<u8>()
        .ok()
        .filter(|place| (1..=places).contains(place))
        .map(|place| place - 1)
}

impl Field {
    /// The group window: whether the player is in a group, how many others
    /// it holds, and each other's name and health, unknown while they are
    /// out of view.
    fn group(self, world: &ClientWorld) -> Value {
        let group = world.group();
        match self {
            Self::GroupPresent => Value::Bool(group.is_some()),
            Self::GroupCount => Value::Int(group.map_or(0, |group| {
                i64::try_from(group.members.len()).unwrap_or(i64::MAX)
            })),
            Self::GroupName(place) => world
                .group_place(usize::from(place))
                .map_or(Value::Unknown, |name| Value::Text(name.into())),
            Self::GroupHpPercent(place) => world
                .group_health(usize::from(place))
                .map_or(Value::Unknown, |percent| Value::Int(i64::from(percent))),
            _ => Value::Unknown,
        }
    }

    /// The effects window: how many icons it draws, and each one's spell in
    /// the order it draws them. Unknown before the server lists them.
    fn buff_of(self, sight: &Sight) -> Value {
        let tracker = sight.world.buffs();
        let Some(slots) = tracker.slots() else {
            return Value::Unknown;
        };
        let (slotted, unslotted) = crate::buffs::drawn_order(slots, tracker);
        let spells = slotted
            .iter()
            .filter_map(|(slot, modelled)| {
                modelled
                    .map(u32::from)
                    .or_else(|| slots.get(slot).map(|buff| buff.spell_id))
            })
            .chain(unslotted.iter().map(|spell| u32::from(*spell)));
        match self {
            Self::BuffSpell(place) => {
                match (spells.clone().nth(usize::from(place)), sight.spells) {
                    (Some(spell), Some(names)) => Value::Text(names.label(spell).into()),
                    _ => Value::Unknown,
                }
            }
            _ => Value::Int(i64::try_from(spells.count()).unwrap_or(i64::MAX)),
        }
    }

    /// The last tell the chat window shows, and who sent it; unknown before
    /// one arrives.
    fn tell(self, sight: &Sight) -> Value {
        let Some(chat) = sight.chat else {
            return Value::Unknown;
        };
        let lines = chat.history.lines(eq_client_core::chat::ChatTab::All);
        let last = lines.iter().rev().find_map(|(_, line)| {
            (line.channel == eq_client_core::chat::ChannelName::Tell
                && line.source == eq_client_core::chat::Source::Server)
                .then_some(line)
                .zip(line.sender.as_deref().filter(|sender| !sender.is_empty()))
        });
        match (self, last) {
            (Self::LastTell, Some((line, _))) => Value::Text(line.message.text.as_str().into()),
            (_, Some((_, sender))) => Value::Text(sender.into()),
            _ => Value::Unknown,
        }
    }
}

impl Field {
    /// The player's own fields, as the HUD and windows show them.
    fn player(self, world: &ClientWorld) -> Value {
        use eq_client_core::money::CoinPlace;
        let number = |value: Option<i64>| value.map_or(Value::Unknown, Value::Int);
        let Some(player) = world.player() else {
            return Value::Unknown;
        };
        let id = player.spawn_id;
        // `/loc` shows each to two places; a script reads the nearest whole
        // number, never more than the line gives.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a zone's coordinates are far inside i64"
        )]
        let loc = |value: f32| Value::Int(value.round() as i64);
        match self {
            Self::HpPercent => number(world.health(id).map(i64::from)),
            Self::Posture => world.posture(id).map_or(Value::Unknown, |posture| {
                posture_word(posture).map_or(Value::Unknown, |word| Value::Text(word.into()))
            }),
            Self::Casting => Value::Bool(world.casting_of(id).is_some()),
            Self::Stunned => Value::Bool(world.is_stunned()),
            Self::LocX => loc(player.position.x),
            Self::LocY => loc(player.position.y),
            Self::LocZ => loc(player.position.z),
            // The experience bar's fill, in whole percent, rounded down.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a ratio from 0 to 1 times 100"
            )]
            Self::ExpPercent => number(
                world
                    .vitals()
                    .experience_ratio()
                    .map(|ratio| (ratio.clamp(0.0, 1.0) * 100.0).floor() as i64),
            ),
            Self::Coin(coin) => number(world.coins_in(CoinPlace::Purse).map(|coins| {
                i64::from(match coin {
                    Coin::Platinum => coins.platinum,
                    Coin::Gold => coins.gold,
                    Coin::Silver => coins.silver,
                    Coin::Copper => coins.copper,
                })
            })),
            _ => Value::Unknown,
        }
    }

    /// The pet window's fields. No pet is `pet.present` false; its name and
    /// health are then unknown.
    fn pet(self, world: &ClientWorld) -> Value {
        let pet = world.pet();
        match self {
            Self::PetPresent => Value::Bool(pet.is_some()),
            Self::PetName => pet.map_or(Value::Unknown, |pet| {
                Value::Text(eq_client_core::entities::display_name(&pet.state.name).into())
            }),
            Self::PetHpPercent => pet
                .and_then(|pet| world.health(pet.state.spawn_id))
                .map_or(Value::Unknown, |percent| Value::Int(i64::from(percent))),
            _ => Value::Unknown,
        }
    }

    fn target(self, world: &ClientWorld) -> Value {
        if world.target_stale() {
            return Value::Stale;
        }
        let Some(intent) = world.target_intent() else {
            return Value::Unknown;
        };
        if self == Self::TargetSelected {
            return Value::Bool(intent.spawn_id.is_some());
        }
        let Some(id) = intent.spawn_id else {
            return Value::Unknown;
        };
        let Some(spawn) = world.spawn(id) else {
            return Value::Unknown;
        };
        match self {
            Self::TargetHpPercent => world
                .health(id)
                .map_or(Value::Unknown, |percent| Value::Int(i64::from(percent))),
            // The color a consider gave, as the target's name shows it; none
            // before the player considers it.
            Self::TargetCon => world
                .consider_color(id)
                .and_then(con_word)
                .map_or(Value::Unknown, |word| Value::Text(word.into())),
            Self::TargetKind => {
                kind_word(spawn.state.kind).map_or(Value::Unknown, |word| Value::Text(word.into()))
            }
            _ => Value::Text(eq_client_core::entities::display_name(&spawn.state.name).into()),
        }
    }
}

/// A posture as a script reads it; none for one the client can't name.
/// A Filters page row's mode, as its label says it: `show`, `hide`, `mine`
/// or `group`; unknown where the playing generation's page has no such row.
fn filter_mode(category: eq_client_core::filters::FilterCategory, sight: &Sight) -> Value {
    let Some(filters) = sight.filters else {
        return Value::Unknown;
    };
    if category.modes(sight.world.generation()).is_empty() {
        return Value::Unknown;
    }
    Value::Text(filters.get(category).key().into())
}

const fn posture_word(posture: eq_client_core::PostureState) -> Option<&'static str> {
    use eq_client_core::PostureState;
    Some(match posture {
        PostureState::Standing => "standing",
        PostureState::Frozen => "frozen",
        PostureState::Looting => "looting",
        PostureState::Sitting => "sitting",
        PostureState::Ducking => "ducking",
        PostureState::Lying => "lying",
        PostureState::Unknown(_) => return None,
    })
}

/// A consider's color as a script reads it.
const fn con_word(color: eq_client_core::combat::ConColor) -> Option<&'static str> {
    use eq_client_core::combat::ConColor;
    Some(match color {
        ConColor::Gray => "gray",
        ConColor::Green => "green",
        ConColor::LightBlue => "light_blue",
        ConColor::Blue => "blue",
        ConColor::White => "white",
        ConColor::Yellow => "yellow",
        ConColor::Red => "red",
        ConColor::Other(_) => return None,
    })
}

/// What a spawn is, as its name's color and the target window tell it.
const fn kind_word(kind: eq_client_core::SpawnKind) -> Option<&'static str> {
    use eq_client_core::SpawnKind;
    Some(match kind {
        SpawnKind::Player => "player",
        SpawnKind::Npc => "npc",
        SpawnKind::PlayerCorpse | SpawnKind::NpcCorpse => "corpse",
        SpawnKind::Unknown(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{Field, Sight, Value};
    use crate::online::{OnlineState, testing as session};
    use eq_client_assets::clock::{ClientClock, EQMAC, TITANIUM};
    use eq_client_core::{
        Coins, SpawnKind, WorldEvent,
        combat::{ConColor, Consideration},
        targets::TargetState,
    };

    const PLAYER: u16 = 7;
    const TARGET: u16 = 42;

    fn read(online: &OnlineState, name: &str) -> Value {
        read_with(online, name, None, TITANIUM)
    }

    fn read_with(
        online: &OnlineState,
        name: &str,
        spells: Option<&crate::spellbook::SpellNames>,
        clock: ClientClock,
    ) -> Value {
        Field::parse(name)
            .unwrap_or_else(|| panic!("{name} is a field"))
            .read(&Sight {
                world: online.world(),
                spells,
                clock,
                now: std::time::Instant::now(),
                chat: None,
                filters: None,
            })
    }

    fn read_chat(online: &OnlineState, name: &str, chat: &crate::chat::ChatState) -> Value {
        Field::parse(name)
            .unwrap_or_else(|| panic!("{name} is a field"))
            .read(&Sight {
                world: online.world(),
                spells: None,
                clock: TITANIUM,
                now: std::time::Instant::now(),
                chat: Some(chat),
                filters: None,
            })
    }

    fn text(word: &str) -> Value {
        Value::Text(word.into())
    }

    #[test]
    fn filter_fields_read_what_the_filters_page_shows() {
        use eq_client_core::filters::{FilterCategory, FilterMode};
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(PLAYER));
        let mut filters = eq_client_core::chat_filters::ChatFilters::default();
        filters.set(FilterCategory::Ooc, FilterMode::Hide);
        filters.set(FilterCategory::BardSongs, FilterMode::GroupOnly);
        let read = |name: &str, filters| {
            Field::parse(name)
                .unwrap_or_else(|| panic!("{name} is a field"))
                .read(&Sight {
                    world: online.world(),
                    spells: None,
                    clock: TITANIUM,
                    now: std::time::Instant::now(),
                    chat: None,
                    filters,
                })
        };
        assert_eq!(read("filter.ooc", Some(&filters)), text("hide"));
        assert_eq!(read("filter.bard_songs", Some(&filters)), text("group"));
        assert_eq!(read("filter.shouts", Some(&filters)), text("show"));
        // The bad-word filter starts on, as the original's does.
        assert_eq!(read("filter.bad_words", Some(&filters)), text("hide"));
        assert_eq!(read("filter.ooc", None), Value::Unknown);
        assert_eq!(
            Field::parse("filter.ooc").unwrap().kind(),
            super::Kind::Text
        );
        // Swarm pets' deaths have no row on the page, so no field.
        assert_eq!(Field::parse("filter.swarm_pet_death"), None);
        assert_eq!(Field::parse("filter.loud"), None);
    }

    #[test]
    fn player_fields_read_what_the_hud_loc_and_purse_show() {
        let mut online = OnlineState::new(true);
        // Before admission each is unknown, as the existing fields are.
        for name in ["player.hp_percent", "player.posture", "player.platinum"] {
            assert_eq!(read(&online, name), Value::Unknown, "{name}");
        }
        let mut player = session::player(PLAYER);
        player.hp_percent = Some(64);
        player.position.x = 12.4;
        player.position.y = -300.6;
        player.position.z = 3.5;
        session::admit(&mut online, 1, player);
        assert_eq!(read(&online, "player.hp_percent"), Value::Int(64));
        assert_eq!(read(&online, "player.loc_x"), Value::Int(12));
        assert_eq!(read(&online, "player.loc_y"), Value::Int(-301));
        assert_eq!(read(&online, "player.loc_z"), Value::Int(4));
        assert_eq!(read(&online, "player.casting"), Value::Bool(false));
        assert_eq!(read(&online, "player.stunned"), Value::Bool(false));
        // No purse or experience reported yet: unknown, never zero.
        assert_eq!(read(&online, "player.gold"), Value::Unknown);
        assert_eq!(read(&online, "player.exp_percent"), Value::Unknown);
        session::news(
            &mut online,
            [
                WorldEvent::Posture {
                    spawn_id: PLAYER,
                    posture: eq_client_core::PostureState::Sitting,
                },
                WorldEvent::Coins(Coins {
                    platinum: 3,
                    gold: 2,
                    silver: 1,
                    copper: 9,
                }),
                WorldEvent::Experience(165),
            ],
        );
        assert_eq!(read(&online, "player.posture"), text("sitting"));
        assert_eq!(read(&online, "player.platinum"), Value::Int(3));
        assert_eq!(read(&online, "player.gold"), Value::Int(2));
        assert_eq!(read(&online, "player.silver"), Value::Int(1));
        assert_eq!(read(&online, "player.copper"), Value::Int(9));
        // 165 of 330 is half the bar.
        assert_eq!(read(&online, "player.exp_percent"), Value::Int(50));
    }

    #[test]
    fn target_fields_read_its_window_and_a_consider() {
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(PLAYER));
        let mut spawn = session::pet(TARGET, 99);
        spawn.kind = SpawnKind::Npc;
        spawn.pet_owner = None;
        session::spawn_entry(&mut online, TARGET, spawn);
        // No target: unknown.
        assert_eq!(read(&online, "target.hp_percent"), Value::Unknown);
        // The server's answer to a selection, as the target window takes it.
        session::news(
            &mut online,
            [WorldEvent::TargetChanged {
                session_id: 1,
                request_id: 0,
                state: TargetState {
                    revision: 1,
                    spawn_id: Some(TARGET),
                    available: true,
                },
            }],
        );
        session::news(
            &mut online,
            [WorldEvent::HealthPercent {
                spawn_id: TARGET,
                percent: 30,
            }],
        );
        assert_eq!(read(&online, "target.hp_percent"), Value::Int(30));
        assert_eq!(read(&online, "target.kind"), text("npc"));
        // Its con is unknown until the player considers it.
        assert_eq!(read(&online, "target.con"), Value::Unknown);
        session::news(
            &mut online,
            [WorldEvent::Consideration(Consideration {
                target_id: TARGET,
                faction: 5,
                color: ConColor::Red,
                hit_points: None,
            })],
        );
        assert!(matches!(read(&online, "target.con"), Value::Text(_)));
    }

    #[test]
    fn pet_fields_read_the_pet_window() {
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(PLAYER));
        assert_eq!(read(&online, "pet.present"), Value::Bool(false));
        assert_eq!(read(&online, "pet.name"), Value::Unknown);
        assert_eq!(read(&online, "pet.hp_percent"), Value::Unknown);
        session::spawn_entry(&mut online, TARGET, session::pet(TARGET, PLAYER));
        session::news(
            &mut online,
            [WorldEvent::HealthPercent {
                spawn_id: TARGET,
                percent: 80,
            }],
        );
        assert_eq!(read(&online, "pet.present"), Value::Bool(true));
        assert!(matches!(read(&online, "pet.name"), Value::Text(_)));
        assert_eq!(read(&online, "pet.hp_percent"), Value::Int(80));
    }

    #[test]
    fn only_shown_fields_parse_and_each_is_stale_in_a_retained_admission() {
        // Fields that only a window or typed command already shows (O-a):
        // no distance, weight or target level.
        for name in [
            "target.distance",
            "player.weight",
            "target.level",
            "player.heading",
        ] {
            assert!(Field::parse(name).is_none(), "{name}");
        }
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(PLAYER));
        // A dropped connection keeps the admission, so its fields are stale.
        session::connect(&mut online, false);
        for name in [
            "player.hp_percent",
            "player.posture",
            "player.casting",
            "player.loc_x",
            "player.exp_percent",
            "player.copper",
            "target.con",
            "pet.present",
            "gem1.spell",
            "gem1.ready",
            "time.hour",
        ] {
            assert_eq!(read(&online, name), Value::Stale, "{name}");
        }
    }

    #[test]
    fn gem_fields_read_the_gem_bar() {
        let mut online = OnlineState::new(true);
        let mut player = session::player(PLAYER);
        player.memorized_spells[0] = Some(202);
        session::admit(&mut online, 1, player);
        let names = crate::spellbook::SpellNames::default();
        assert_eq!(
            read_with(&online, "gem1.spell", Some(&names), TITANIUM),
            text("Spell 202")
        );
        // Without the installation's spell names the gem's name is unknown.
        assert_eq!(read(&online, "gem1.spell"), Value::Unknown);
        assert_eq!(read(&online, "gem1.ready"), Value::Bool(true));
        // An empty gem has no spell and is never lit.
        assert_eq!(read(&online, "gem8.spell"), Value::Unknown);
        assert_eq!(read(&online, "gem8.ready"), Value::Bool(false));
        for name in [
            "gem0.spell",
            "gem9.ready",
            "gem10.spell",
            "gem1.name",
            "gem.ready",
        ] {
            assert_eq!(Field::parse(name), None, "{name}");
        }
    }

    #[test]
    fn time_fields_read_what_time_tells() {
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(PLAYER));
        assert_eq!(read(&online, "time.hour"), Value::Unknown);
        session::news(
            &mut online,
            [WorldEvent::TimeOfDay(eq_client_core::clock::GameTime {
                hour: 12,
                minute: 30,
                day: 3,
                month: 2,
                year: 3200,
            })],
        );
        // The Mac client names the calendar's hour 12 as 1 PM, Titanium as 12.
        assert_eq!(read(&online, "time.hour"), Value::Int(12));
        assert_eq!(read_with(&online, "time.hour", None, EQMAC), Value::Int(13));
        assert_eq!(read(&online, "time.day"), Value::Int(3));
        assert_eq!(read(&online, "time.month"), Value::Int(2));
        assert_eq!(read(&online, "time.year"), Value::Int(3200));
        // Where `/time` says the time can't be told, a script can't read it.
        session::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: eq_client_core::Capability::ALL.to_vec(),
                choices: Vec::new(),
                session_id: 1,
                zone: "potimea".into(),
                player: Box::new(session::player(PLAYER)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        assert_eq!(read(&online, "time.hour"), Value::Unknown);
        assert_eq!(read(&online, "time.minute"), Value::Unknown);
    }

    fn tell(sender: &str, text: &str) -> eq_client_core::chat::ChatLine {
        use eq_client_core::chat::{ChannelName, ChatLine, Message, Source};
        ChatLine {
            channel: ChannelName::Tell,
            message_type: None,
            tongue: None,
            sender: Some(sender.into()),
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: text.into(),
                item_links: Vec::new(),
            },
            source: Source::Server,
        }
    }

    #[test]
    fn group_fields_read_the_group_window() {
        use eq_client_core::group::GroupUpdate;
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(PLAYER));
        assert_eq!(read(&online, "group.present"), Value::Bool(false));
        assert_eq!(read(&online, "group.count"), Value::Int(0));
        assert_eq!(read(&online, "group.member1.name"), Value::Unknown);
        let mut friend = session::pet(TARGET, 99);
        friend.kind = SpawnKind::Player;
        friend.pet_owner = None;
        friend.name = "Friend".into();
        session::spawn_entry(&mut online, TARGET, friend);
        session::news(
            &mut online,
            [
                WorldEvent::Group(GroupUpdate::Members {
                    leader: "Friend".into(),
                    members: vec!["Friend".into(), "Faraway".into()],
                }),
                WorldEvent::HealthPercent {
                    spawn_id: TARGET,
                    percent: 64,
                },
            ],
        );
        assert_eq!(read(&online, "group.present"), Value::Bool(true));
        assert_eq!(read(&online, "group.count"), Value::Int(2));
        assert_eq!(read(&online, "group.member1.name"), text("Friend"));
        assert_eq!(read(&online, "group.member1.hp_percent"), Value::Int(64));
        // A member out of view has a name but no health bar.
        assert_eq!(read(&online, "group.member2.name"), text("Faraway"));
        assert_eq!(read(&online, "group.member2.hp_percent"), Value::Unknown);
        assert_eq!(read(&online, "group.member3.name"), Value::Unknown);
        for name in [
            "group.member0.name",
            "group.member6.name",
            "group.member01.name",
            "group.member1.level",
        ] {
            assert_eq!(Field::parse(name), None, "{name}");
        }
    }

    #[test]
    fn buff_fields_read_the_effects_window_in_its_order() {
        use eq_client_core::Buff;
        let buff = |spell_id| Buff {
            spell_id,
            caster_level: 1,
            effect_type: 2,
            bard_modifier: 10,
            duration_ticks: 30,
            counters: 0,
            caster_id: u32::from(PLAYER),
        };
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(PLAYER));
        // Before the server lists the player's buffs, there is no count.
        let names = crate::spellbook::SpellNames::default();
        assert_eq!(read(&online, "buffs.count"), Value::Unknown);
        session::buffs(
            &mut online,
            std::collections::BTreeMap::from([(0, buff(202)), (3, buff(278))]),
        );
        assert_eq!(read(&online, "buffs.count"), Value::Int(2));
        // The window's icons in order: a hole in the slots is not a buff.
        assert_eq!(
            read_with(&online, "buff1.spell", Some(&names), TITANIUM),
            text("Spell 202")
        );
        assert_eq!(
            read_with(&online, "buff2.spell", Some(&names), TITANIUM),
            text("Spell 278")
        );
        assert_eq!(
            read_with(&online, "buff3.spell", Some(&names), TITANIUM),
            Value::Unknown
        );
        assert_eq!(read(&online, "buff1.spell"), Value::Unknown);
        for name in ["buff0.spell", "buff51.spell", "buff1.ticks", "buffs.spell"] {
            assert_eq!(Field::parse(name), None, "{name}");
        }
    }

    #[test]
    fn tell_fields_read_the_last_tell_the_chat_window_shows() {
        use eq_client_core::chat::ChannelName;
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(PLAYER));
        let mut chat = crate::chat::ChatState::default();
        assert_eq!(read_chat(&online, "chat.last_tell", &chat), Value::Unknown);
        chat.history.push(tell("Friend", "first words"));
        chat.history.push(tell("Other", "later words"));
        // A say is no tell, and the echo of the player's own tell is not one
        // sent to them.
        let mut say = tell("Third", "said aloud");
        say.channel = ChannelName::Say;
        chat.history.push(say);
        let mut echo = tell("Third", "sent away");
        echo.channel = ChannelName::TellEcho;
        chat.history.push(echo);
        assert_eq!(
            read_chat(&online, "chat.last_tell", &chat),
            text("later words")
        );
        assert_eq!(
            read_chat(&online, "chat.last_tell_from", &chat),
            text("Other")
        );
        assert!(Field::LastTell.private() && Field::LastTellFrom.private());
        assert!(!Field::GroupName(0).private());
    }
}
