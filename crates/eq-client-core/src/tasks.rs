//! The player's task journal and the tasks an NPC offers, as the session
//! tells them (checklist 6.560).
//!
//! The journal follows the session's own rule: a new admission starts it
//! empty, since `EQEmu` sends every entry again on each zone's entry, and a
//! description of another task in a slot drops that slot's steps. An offer
//! stands until the player answers it, the next offer replaces it, or the
//! admission ends; one acceptance ends it, as the session's does.
//!
//! The task's words (titles, descriptions, step text) are the server's, and
//! only a window shows them: nothing here logs them. The words this client
//! builds for a step without its own and for a duration are ours, inferred
//! until the program's are read (R49-38), and so is every line the client
//! would add when a step or task ends: none is shown until that read.

pub use eq_network_game::tasks::{
    CompletedTask, Objective, Outcome, Step, TaskActivity, TaskCompletion, TaskDescription,
    TaskKind, TaskSlot,
    offers::{DoneStep, OfferedStep, OfferedTask, TaskHistory, TaskOffers},
};

use crate::WorldEvent;
use std::collections::BTreeMap;

/// One journal entry: its description and its steps by number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskEntry {
    /// The entry's description, as the server last sent it.
    pub description: TaskDescription,
    /// Its steps, by their number in the task.
    pub steps: BTreeMap<u32, TaskActivity>,
}

/// The journal of one admission.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskJournal {
    session_id: Option<u64>,
    entries: Vec<TaskEntry>,
    completed: Option<Vec<CompletedTask>>,
    histories: BTreeMap<u32, TaskHistory>,
    opened: u64,
}

const fn slot_order(at: TaskSlot) -> (u8, u32) {
    let kind = match at.kind {
        TaskKind::Task => 0,
        TaskKind::Quest => 1,
    };
    (kind, at.slot)
}

impl TaskJournal {
    /// The entries, the task first and then the quests by slot.
    #[must_use]
    pub fn entries(&self) -> &[TaskEntry] {
        &self.entries
    }

    /// The entry in this slot.
    #[must_use]
    pub fn entry(&self, at: TaskSlot) -> Option<&TaskEntry> {
        self.entries.iter().find(|entry| entry.description.at == at)
    }

    /// Whether the journal holds this task in any slot.
    #[must_use]
    pub fn holds(&self, task_id: u32) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.description.task_id == task_id)
    }

    /// The finished tasks the server last listed, oldest first.
    #[must_use]
    pub fn completed(&self) -> Option<&[CompletedTask]> {
        self.completed.as_deref()
    }

    /// A finished task's done steps, by its index in that list, once asked
    /// for and answered.
    #[must_use]
    pub fn history(&self, index: u32) -> Option<&TaskHistory> {
        self.histories.get(&index)
    }

    /// Counts the descriptions in this admission that asked for the journal
    /// to open, as `EQEmu` asks when a task is taken.
    #[must_use]
    pub const fn open_asked(&self) -> u64 {
        self.opened
    }

    /// Takes one task event of `session_id`, the current admission. A new
    /// admission starts the journal empty. Says whether the event was one
    /// the journal holds; an offer, a conclusion and a refusal are not.
    pub fn observe(&mut self, session_id: u64, event: &WorldEvent) -> bool {
        if self.session_id != Some(session_id) {
            *self = Self {
                session_id: Some(session_id),
                ..Self::default()
            };
        }
        match event {
            WorldEvent::TaskDescribed { description, .. } => {
                self.describe(description);
                true
            }
            WorldEvent::TaskStepUpdated { activity, .. } => self.step(activity),
            WorldEvent::TaskRemoved { at, .. } => {
                let before = self.entries.len();
                self.entries.retain(|entry| entry.description.at != *at);
                self.entries.len() != before
            }
            WorldEvent::CompletedTasks { list, .. } => {
                // A history is asked for by its index in the last list.
                self.histories.clear();
                self.completed = Some(list.clone());
                true
            }
            WorldEvent::TaskHistory { history, .. } => {
                let listed = self.completed.as_ref().map_or(0, Vec::len);
                if usize::try_from(history.index).is_ok_and(|index| index < listed) {
                    self.histories.insert(history.index, history.clone());
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn describe(&mut self, description: &TaskDescription) {
        if description.open_journal {
            self.opened = self.opened.wrapping_add(1);
        }
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.description.at == description.at)
        {
            if entry.description.task_id != description.task_id {
                entry.steps.clear();
            }
            entry.description = description.clone();
            return;
        }
        let place = self
            .entries
            .partition_point(|entry| slot_order(entry.description.at) < slot_order(description.at));
        self.entries.insert(
            place,
            TaskEntry {
                description: description.clone(),
                steps: BTreeMap::new(),
            },
        );
    }

    fn step(&mut self, activity: &TaskActivity) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| {
            entry.description.at == activity.at && entry.description.task_id == activity.task_id
        }) else {
            return false;
        };
        entry.steps.insert(activity.activity_id, activity.clone());
        true
    }
}

/// The tasks an NPC offers, until the player answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskOffer {
    /// The admission it was heard in.
    pub session_id: u64,
    /// Counts the offers, so that two alike still differ.
    pub serial: u64,
    /// The offer, as sent.
    pub offers: TaskOffers,
}

/// What a duration reads as: hours and minutes when the server gives
/// seconds, else its code's word. The words are ours (inferred, R49-38).
#[must_use]
pub fn duration_words(seconds: u32, code: u32) -> String {
    if seconds == 0 {
        return match code {
            1 => "Short",
            2 => "Medium",
            3 => "Long",
            _ => "Unlimited",
        }
        .to_owned();
    }
    let hours = seconds / 3600;
    let minutes = seconds % 3600 / 60;
    match (hours, minutes) {
        (0, 0) => format!("{seconds} s"),
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

/// The words of a step: its own when the task gives them, else built from
/// its type, target, items and goal in our words (inferred, R49-38).
#[must_use]
pub fn objective_words(
    activity_type: i32,
    target: &str,
    items: &str,
    goal: i32,
    description: &str,
) -> String {
    let own = description.trim();
    if !own.is_empty() {
        return own.to_owned();
    }
    let target = target.trim();
    let items = items.trim();
    let count = |what: &str| {
        if goal > 1 {
            format!("{goal} {what}")
        } else {
            what.to_owned()
        }
    };
    let joined = |verb: &str, what: String, link: &str, whom: &str| {
        let mut words = verb.to_owned();
        if !what.is_empty() {
            words.push(' ');
            words.push_str(&what);
        }
        if !whom.is_empty() {
            words.push(' ');
            words.push_str(link);
            words.push(' ');
            words.push_str(whom);
        }
        words
    };
    match activity_type {
        1 => joined("Deliver", count(items), "to", target),
        2 => joined("Kill", count(target), "", ""),
        3 => joined("Loot", count(items), "from", target),
        4 => joined("Speak with", target.to_owned(), "", ""),
        5 => joined("Explore", target.to_owned(), "", ""),
        6 => joined("Create", count(items), "", ""),
        7 => joined("Fish for", count(items), "", ""),
        8 => joined("Forage", count(items), "", ""),
        9 => joined("Cast", times(goal), "on", target),
        10 => joined("Use a skill", times(goal), "on", target),
        11 => joined("Touch", target.to_owned(), "", ""),
        13 => joined("Collect", count(items), "", ""),
        100 => joined("Give coin", String::new(), "to", target),
        _ => joined("Complete", target.to_owned(), "", ""),
    }
}

fn times(goal: i32) -> String {
    if goal > 1 {
        format!("{goal} times")
    } else {
        String::new()
    }
}

/// An offered step's words, as [`objective_words`].
#[must_use]
pub fn offered_step_words(step: &OfferedStep) -> String {
    objective_words(
        step.activity_type,
        &step.target_name,
        &step.item_list,
        step.goal,
        &step.description,
    )
}

#[cfg(test)]
mod tests;
