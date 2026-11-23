//! Read-only fields from the current client projection, with its existing evidence bounds.

use eq_client_core::world::ClientWorld;

use super::{Kind, Value};

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
            _ => None,
        }
    }

    pub(super) const fn kind(self) -> Kind {
        match self {
            Self::Ready
            | Self::TargetSelected
            | Self::Casting
            | Self::Stunned
            | Self::PetPresent => Kind::Bool,
            Self::Zone
            | Self::TargetName
            | Self::Posture
            | Self::TargetCon
            | Self::TargetKind
            | Self::PetName => Kind::Text,
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
            | Self::PetHpPercent => Kind::Int,
        }
    }

    pub(super) fn read(self, world: &ClientWorld) -> Value {
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
        }
    }

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
    use super::{Field, Value};
    use crate::online::{OnlineState, testing as session};
    use eq_client_core::{
        Coins, SpawnKind, WorldEvent,
        combat::{ConColor, Consideration},
        targets::TargetState,
    };

    const PLAYER: u16 = 7;
    const TARGET: u16 = 42;

    fn read(online: &OnlineState, name: &str) -> Value {
        Field::parse(name)
            .unwrap_or_else(|| panic!("{name} is a field"))
            .read(online.world())
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
        ] {
            assert_eq!(read(&online, name), Value::Stale, "{name}");
        }
    }
}
