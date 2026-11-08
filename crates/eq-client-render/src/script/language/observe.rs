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
            _ => None,
        }
    }

    pub(super) const fn kind(self) -> Kind {
        match self {
            Self::Ready | Self::TargetSelected => Kind::Bool,
            Self::Zone | Self::TargetName => Kind::Text,
            Self::Level | Self::ReportedHp | Self::ReportedHpMax | Self::Mana | Self::Endurance => {
                Kind::Int
            }
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
            Self::TargetSelected | Self::TargetName => self.target(world),
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
        Value::Text(eq_client_core::entities::display_name(&spawn.state.name).into())
    }
}
