//! Admission-scoped received AA selections and definitions, without activation state.
pub use eq_network_game::alternate_advancement::{Effect, Rank, Selection, Update};
use std::collections::BTreeMap;

/// Local retention bound; exceeding it withdraws definitions, never supplies zeros.
const MAX_DEFINITIONS: usize = 4096;

/// Received facts for one admission. A partial table never means a complete table.
#[derive(Default)]
pub struct State {
    selections: Option<Vec<Selection>>,
    definitions: BTreeMap<u32, Rank>,
}

impl State {
    /// Applies the ordered fact stream belonging to this admission.
    pub fn apply(&mut self, update: &Update) {
        match update {
            Update::Unknown => *self = Self::default(),
            Update::Selections(selections) => {
                self.selections = selections
                    .as_ref()
                    .filter(|selected| {
                        selected.len() <= eq_network_game::alternate_advancement::SELECTION_SLOTS
                    })
                    .cloned();
            }
            Update::Definition { id, rank } => {
                self.definitions.remove(id);
                if let Some(rank) = rank.as_ref().filter(|rank| {
                    rank.id == *id
                        && *id != 0
                        && rank.effects.len() <= eq_network_game::alternate_advancement::MAX_EFFECTS
                }) {
                    if self.definitions.len() == MAX_DEFINITIONS {
                        self.definitions.clear();
                    } else {
                        self.definitions.insert(*id, rank.clone());
                    }
                }
            }
            Update::DefinitionsUnknown => self.definitions.clear(),
        }
    }

    /// Selected eligible effects, only after every selected definition resolves.
    /// Native 0x40feaa uses exact selected IDs, not the value as a multiplier,
    /// and tests class mask bit `1 << class` and required character level.
    pub fn effects(&self, class: u32, level: u8) -> Option<Vec<&Effect>> {
        if !(1..=16).contains(&class) || level == 0 {
            return None;
        }
        let mut effects = Vec::new();
        for selected in self.selections.as_ref()? {
            if selected.id == 0 {
                return None;
            }
            let definition = self.definitions.get(&selected.id)?;
            if definition.classes & (1 << class) != 0
                && definition.required_level <= u32::from(level)
            {
                effects.extend(&definition.effects);
            }
        }
        Some(effects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_need_exact_definitions_and_recheck_native_eligibility() {
        let mut state = State::default();
        assert!(state.effects(2, 28).is_none());
        state.apply(&Update::Selections(Some(vec![Selection {
            id: 42,
            value: 99,
        }])));
        assert!(state.effects(2, 28).is_none());
        let rank = Rank {
            id: 42,
            required_level: 28,
            classes: 1 << 2,
            sequence: 900,
            rank: 3,
            effects: vec![Effect {
                id: 14,
                value: -2,
                limit: 0,
                slot: 1,
            }],
        };
        state.apply(&Update::Definition {
            id: 42,
            rank: Some(rank.clone()),
        });
        assert_eq!(state.effects(2, 28).unwrap(), [&rank.effects[0]]);
        assert_eq!(state.effects(2, 27).unwrap(), [] as [&Effect; 0]);
        assert_eq!(state.effects(1, 28).unwrap(), [] as [&Effect; 0]);
        state.apply(&Update::Definition { id: 42, rank: None });
        assert!(state.effects(2, 28).is_none());
        state.apply(&Update::Selections(Some(Vec::new())));
        assert_eq!(state.effects(2, 28).unwrap(), [] as [&Effect; 0]);
        state.apply(&Update::Selections(None));
        assert!(state.effects(2, 28).is_none());
    }
}
