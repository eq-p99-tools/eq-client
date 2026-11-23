//! Admission-scoped received AA facts: Titanium's selections, definitions,
//! points, experience and reuse timers, and `EQMac`'s ranks, points,
//! experience and timers (checklist 6.565). Every value is kept as sent.
//! What the player owns or may train is for display only: the session
//! alone decides what is sent (eq-network's AA gates), and nothing here
//! sends.
pub use eq_network_game::alternate_advancement::{
    Effect, MacPoints, MacProgress, MacRank, MacRanks, MacTimer, Points, Progress, Rank, Selection,
    Timer, Update,
};
use std::collections::BTreeMap;

/// Local retention bound; exceeding it withdraws definitions, never supplies zeros.
const MAX_DEFINITIONS: usize = 4096;
/// The most reuse timers kept, by timer id; a new one past it is dropped.
const MAX_TIMERS: usize = 4096;

/// Received facts for one admission. A partial table never means a complete table.
#[derive(Default)]
pub struct State {
    selections: Option<Vec<Selection>>,
    definitions: BTreeMap<u32, Rank>,
    points: Option<Points>,
    progress: Option<Progress>,
    /// The latest unspent points, from the profile's points or the latest
    /// experience update, whichever came last.
    unspent: Option<u32>,
    timers: BTreeMap<u32, Timer>,
    mac_ranks: Option<MacRanks>,
    mac_selections: Option<Vec<MacRank>>,
    mac_points: Option<MacPoints>,
    mac_progress: Option<MacProgress>,
    mac_timers: BTreeMap<i16, MacTimer>,
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
            // Points, experience and timers change none of the effects.
            Update::Points(points) => {
                self.unspent = points.as_ref().map(|points| points.aapoints);
                self.points.clone_from(points);
            }
            Update::Progress(progress) => {
                self.unspent = progress
                    .as_ref()
                    .map(|progress| u32::from(progress.unspent));
                self.progress.clone_from(progress);
            }
            Update::Timer(timer) => {
                if self.timers.len() < MAX_TIMERS || self.timers.contains_key(&timer.ability) {
                    self.timers.insert(timer.ability, *timer);
                }
            }
            Update::MacRanks(ranks) => self.mac_ranks.clone_from(ranks),
            Update::MacSelections(selections) => self.mac_selections.clone_from(selections),
            Update::MacPoints(points) => self.mac_points.clone_from(points),
            Update::MacProgress(progress) => self.mac_progress.clone_from(progress),
            Update::MacTimer(timer) => {
                if self.mac_timers.len() < MAX_TIMERS
                    || self.mac_timers.contains_key(&timer.ability)
                {
                    self.mac_timers.insert(timer.ability, *timer);
                }
            }
        }
    }

    /// A received Titanium definition.
    #[must_use]
    pub fn definition(&self, id: u32) -> Option<&Rank> {
        self.definitions.get(&id)
    }

    /// Whether the player owns this rank, as far as the facts tell, for
    /// display: the server's table offers each ability's owned rank and the
    /// next, or its first, so a rank another offered rank follows is owned;
    /// otherwise the selections name it by exact id, as `OP_RespondAA` does
    /// and the profile does for a first rank. None while no selections have
    /// come. A maxed ability above rank one has no follower and the profile
    /// names only its first rank, so it reads as unowned until
    /// `OP_RespondAA` names it, as the session's gate reads it.
    #[must_use]
    pub fn owned(&self, rank: &Rank) -> Option<bool> {
        let id = i64::from(rank.id);
        if self
            .definitions
            .values()
            .any(|other| i64::from(other.last_id) == id)
        {
            return Some(true);
        }
        self.selections
            .as_ref()
            .map(|selections| selections.iter().any(|selected| selected.id == rank.id))
    }

    /// The offered ranks the player doesn't own and whose rank before is
    /// offered, for display; the session judges points, level and class
    /// again before anything is sent.
    pub fn trainable(&self) -> impl Iterator<Item = &Rank> {
        self.definitions.values().filter(|rank| {
            self.owned(rank) == Some(false)
                && u32::try_from(rank.last_id)
                    .map_or(true, |previous| self.definitions.contains_key(&previous))
        })
    }

    /// The profile's points as sent.
    #[must_use]
    pub const fn points(&self) -> Option<&Points> {
        self.points.as_ref()
    }

    /// The latest AA experience update as sent.
    #[must_use]
    pub const fn progress(&self) -> Option<&Progress> {
        self.progress.as_ref()
    }

    /// The latest unspent points, from the profile or the latest experience
    /// update, whichever came last.
    #[must_use]
    pub const fn unspent(&self) -> Option<u32> {
        self.unspent
    }

    /// The latest reuse timer of each timer id, as sent; no arithmetic.
    pub fn timers(&self) -> impl Iterator<Item = &Timer> {
        self.timers.values()
    }

    /// `EQMac`'s latest `OP_RespondAA`, by Mac AA id.
    #[must_use]
    pub const fn mac_ranks(&self) -> Option<&MacRanks> {
        self.mac_ranks.as_ref()
    }

    /// The `EQMac` profile's AA array, nonzero ids only, values as sent.
    #[must_use]
    pub fn mac_selections(&self) -> Option<&[MacRank]> {
        self.mac_selections.as_deref()
    }

    /// The `EQMac` profile's points as sent.
    #[must_use]
    pub const fn mac_points(&self) -> Option<&MacPoints> {
        self.mac_points.as_ref()
    }

    /// `EQMac`'s latest AA experience update as sent.
    #[must_use]
    pub const fn mac_progress(&self) -> Option<&MacProgress> {
        self.mac_progress.as_ref()
    }

    /// `EQMac`'s latest reuse timer of each Mac AA id, as sent.
    pub fn mac_timers(&self) -> impl Iterator<Item = &MacTimer> {
        self.mac_timers.values()
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
            hotkey_sid: 0,
            hotkey_sid2: 0,
            title_sid: 0,
            desc_sid: 0,
            cost: 0,
            prereq_skill: 0,
            prereq_minpoints: 0,
            kind: 0,
            spellid: 0,
            spell_type: 0,
            spell_refresh: 0,
            max_level: 0,
            last_id: 0,
            next_id: 0,
            cost2: 0,
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

    fn rank(id: u32, last_id: i32) -> Rank {
        Rank {
            id,
            required_level: 51,
            classes: 1 << 1,
            sequence: id,
            rank: 1,
            hotkey_sid: 0,
            hotkey_sid2: 0,
            title_sid: id,
            desc_sid: id,
            cost: 1,
            prereq_skill: 0,
            prereq_minpoints: 0,
            kind: 0,
            spellid: -1,
            spell_type: 0,
            spell_refresh: 0,
            max_level: 0,
            last_id,
            next_id: -1,
            cost2: 0,
            effects: Vec::new(),
        }
    }

    fn offer(state: &mut State, rank: Rank) {
        state.apply(&Update::Definition {
            id: rank.id,
            rank: Some(rank),
        });
    }

    fn selected(state: &mut State, ids: &[u32]) {
        state.apply(&Update::Selections(Some(
            ids.iter().map(|&id| Selection { id, value: 1 }).collect(),
        )));
    }

    fn trainable(state: &State) -> Vec<u32> {
        state.trainable().map(|rank| rank.id).collect()
    }

    /// The table shapes eq-network's train gate reads (checklist 6.556).
    fn table() -> State {
        let mut state = State::default();
        // The profile's form: an ability's first rank, by its id.
        selected(&mut state, &[10]);
        offer(&mut state, rank(10, -1));
        offer(&mut state, rank(11, 10));
        offer(&mut state, rank(20, -1));
        // Its rank before is not in the table.
        offer(&mut state, rank(31, 30));
        state
    }

    #[test]
    fn ownership_is_read_from_the_tables_shape_for_display() {
        let state = table();
        let owned = |id| state.owned(state.definition(id).unwrap());
        // A rank another offered rank follows is owned.
        assert_eq!(owned(10), Some(true));
        assert_eq!(owned(11), Some(false));
        assert_eq!(owned(20), Some(false));
        assert_eq!(owned(31), Some(false));
        // Only the next rank and an unowned first rank may be trained; a
        // rank whose rank before is not offered may not.
        assert_eq!(trainable(&state), [11, 20]);
    }

    #[test]
    fn nothing_is_owned_or_trainable_before_the_selections_come() {
        let mut state = State::default();
        offer(&mut state, rank(20, -1));
        assert_eq!(state.owned(state.definition(20).unwrap()), None);
        assert_eq!(trainable(&state), [] as [u32; 0]);
        // A follower still tells ownership without selections.
        offer(&mut state, rank(21, 20));
        assert_eq!(state.owned(state.definition(20).unwrap()), Some(true));
        assert_eq!(trainable(&state), [] as [u32; 0]);
    }

    #[test]
    fn a_maxed_ability_above_rank_one_is_unowned_until_the_exact_ids_come() {
        // Rank 72 is the last of an ability whose first rank is 70: the
        // table offers it alone and the profile names only the first rank.
        let mut state = State::default();
        selected(&mut state, &[70]);
        offer(&mut state, rank(72, 71));
        assert_eq!(state.owned(state.definition(72).unwrap()), Some(false));
        assert_eq!(trainable(&state), [] as [u32; 0]);
        // OP_RespondAA names the exact owned rank.
        selected(&mut state, &[72]);
        assert_eq!(state.owned(state.definition(72).unwrap()), Some(true));
        assert_eq!(trainable(&state), [] as [u32; 0]);
    }

    #[test]
    fn a_withdrawn_definition_or_table_leaves_nothing_offered() {
        let mut state = table();
        state.apply(&Update::Definition { id: 11, rank: None });
        assert!(state.definition(11).is_none());
        // Rank 10's follower is gone, but the profile still names it.
        assert_eq!(state.owned(state.definition(10).unwrap()), Some(true));
        assert_eq!(trainable(&state), [20]);
        // A definition naming another id is withdrawn, not kept.
        state.apply(&Update::Definition {
            id: 20,
            rank: Some(rank(21, -1)),
        });
        assert!(state.definition(20).is_none());
        assert!(state.definition(21).is_none());
        state.apply(&Update::DefinitionsUnknown);
        assert!(state.definition(10).is_none());
        assert_eq!(trainable(&state), [] as [u32; 0]);
    }

    #[test]
    fn points_experience_and_timers_are_kept_as_sent() {
        let mut state = State::default();
        assert_eq!(state.unspent(), None);
        let points = Points {
            aapoints_spent: 7,
            aapoints: 3,
            exp_aa: 1_000,
        };
        state.apply(&Update::Points(Some(points)));
        assert_eq!(state.points(), Some(&points));
        assert_eq!(state.unspent(), Some(3));
        // The latest experience update says the unspent points after.
        let progress = Progress {
            experience: 165,
            unspent: 2,
            percentage: 100,
        };
        state.apply(&Update::Progress(Some(progress)));
        assert_eq!(state.progress(), Some(&progress));
        assert_eq!(state.unspent(), Some(2));
        assert_eq!(state.points(), Some(&points));
        state.apply(&Update::Progress(None));
        assert_eq!(state.progress(), None);
        assert_eq!(state.unspent(), None);
        // Each timer id keeps its latest timer.
        for (ability, begin) in [(5, 10), (6, 20), (5, 30)] {
            state.apply(&Update::Timer(Timer {
                begin,
                ability,
                end: begin,
            }));
        }
        let timers: Vec<_> = state
            .timers()
            .map(|timer| (timer.ability, timer.begin))
            .collect();
        assert_eq!(timers, [(5, 30), (6, 20)]);
        state.apply(&Update::Unknown);
        assert_eq!(state.timers().count(), 0);
        assert_eq!(state.points(), None);
        assert_eq!(state.unspent(), None);
    }

    #[test]
    fn the_timer_bound_keeps_known_ids_and_drops_new_ones() {
        let mut state = State::default();
        let timer = |ability, begin| Timer {
            begin,
            ability,
            end: 0,
        };
        for ability in 0..u32::try_from(MAX_TIMERS).unwrap() {
            state.apply(&Update::Timer(timer(ability, 1)));
        }
        state.apply(&Update::Timer(timer(u32::MAX, 1)));
        assert_eq!(state.timers().count(), MAX_TIMERS);
        assert!(state.timers().all(|timer| timer.ability != u32::MAX));
        state.apply(&Update::Timer(timer(0, 2)));
        assert_eq!(state.timers().next().map(|timer| timer.begin), Some(2));
    }

    #[test]
    fn eqmac_facts_are_kept_as_sent_beside_titaniums() {
        let mut state = State::default();
        let ranks = MacRanks {
            spent: 4,
            ranks: vec![MacRank { id: 211, value: 3 }],
        };
        state.apply(&Update::MacRanks(Some(ranks.clone())));
        state.apply(&Update::MacSelections(Some(vec![MacRank {
            id: 12,
            value: 48,
        }])));
        let points = MacPoints {
            aapoints: 2,
            exp_aa: 9,
            per_aa: 50,
        };
        state.apply(&Update::MacPoints(Some(points)));
        let progress = MacProgress {
            experience: 100,
            unspent: 2,
            percentage: 50,
        };
        state.apply(&Update::MacProgress(Some(progress)));
        for (ability, begin) in [(12, 1), (12, 5)] {
            state.apply(&Update::MacTimer(MacTimer {
                begin,
                ability,
                end: 0,
            }));
        }
        assert_eq!(state.mac_ranks(), Some(&ranks));
        assert_eq!(
            state.mac_selections(),
            Some(&[MacRank { id: 12, value: 48 }][..])
        );
        assert_eq!(state.mac_points(), Some(&points));
        assert_eq!(state.mac_progress(), Some(&progress));
        let timers: Vec<_> = state.mac_timers().map(|timer| timer.begin).collect();
        assert_eq!(timers, [5]);
        // EQMac facts tell no Titanium effects.
        assert!(state.effects(2, 60).is_none());
        assert_eq!(state.unspent(), None);
        state.apply(&Update::MacRanks(None));
        assert_eq!(state.mac_ranks(), None);
        state.apply(&Update::Unknown);
        assert_eq!(state.mac_selections(), None);
        assert_eq!(state.mac_timers().count(), 0);
    }
}
