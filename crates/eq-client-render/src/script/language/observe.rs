//! Read-only fields from the current client projection, with its existing evidence bounds.

use eq_client_core::world::ClientWorld;

use super::{Kind, Value};

/// The spell gems a player has, on every client the project supports.
const GEMS: u8 = 8;

/// What the player's windows draw on: the world, the installation's spell
/// names and linked timers, the installed client's clock and the moment.
pub(in crate::script) struct Sight<'a> {
    pub world: &'a ClientWorld,
    pub spells: Option<&'a crate::spellbook::SpellNames>,
    pub clock: eq_client_assets::clock::ClientClock,
    pub now: std::time::Instant,
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
            _ => Self::gem(name),
        }
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
            | Self::GemReady(_) => Kind::Bool,
            Self::Zone
            | Self::TargetName
            | Self::Posture
            | Self::TargetCon
            | Self::TargetKind
            | Self::PetName
            | Self::GemSpell(_) => Kind::Text,
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
            | Self::Time(_) => Kind::Int,
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
        }
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
            })
    }

    fn text(word: &str) -> Value {
        Value::Text(word.into())
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
}
