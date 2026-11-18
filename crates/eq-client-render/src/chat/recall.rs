//! The lines the player sent from each chat window, stepped through with
//! Shift+Up and Shift+Down while composing, as the Titanium program does
//! (read R34): 40 lines per window (0x4f68b3), kept at submit before any
//! command handling (0x4f6ea1), never the same line twice in a row, never
//! an empty line or the installed client's password command, Up stopping at
//! the oldest (0x4f6d57) and Down past the newest clearing the line
//! (0x4f6da3). Lines hold tells and names, so they live in memory only and
//! go with their window or the character: never saved, reported or logged.
use super::draft::Draft;
use crate::windows::WindowId;
use std::collections::BTreeMap;

/// How many lines each window keeps (read R34 (a)).
const KEPT: usize = 40;

/// What the line shows after a step.
pub(super) enum Step {
    /// A kept line.
    Line(Draft),
    /// An empty line.
    Clear,
}

/// One window's ring, as the program keeps it: the slot the next line goes
/// in and the slot the line shows.
struct Ring {
    slots: Vec<Option<Draft>>,
    write: usize,
    cursor: usize,
}

impl Default for Ring {
    fn default() -> Self {
        Self {
            slots: vec![None; KEPT],
            write: 0,
            cursor: 0,
        }
    }
}

impl Ring {
    fn newest(&self) -> Option<&Draft> {
        self.slots[(self.write + KEPT - 1) % KEPT].as_ref()
    }
}

#[derive(Default)]
pub(super) struct Recall(BTreeMap<WindowId, Ring>);

impl Recall {
    /// Keeps a line submitted from `window`, with its links. An empty line
    /// changes nothing; the newest line again, or a line that starts as
    /// `withheld` does (its first four characters), is not kept but ends any
    /// recall.
    pub(super) fn remember(&mut self, window: WindowId, line: &Draft, withheld: Option<&str>) {
        let text = line.text().trim();
        if text.is_empty() {
            return;
        }
        let ring = self.0.entry(window).or_default();
        let repeated = ring
            .newest()
            .is_some_and(|newest| newest.text() == line.text());
        if !repeated && !withholds(withheld, text) {
            ring.slots[ring.write] = Some(line.clone());
            ring.write = (ring.write + 1) % KEPT;
        }
        ring.cursor = ring.write;
    }

    /// Shift+Up: the next older line, or nothing at the oldest.
    pub(super) fn older(&mut self, window: WindowId) -> Option<Draft> {
        let ring = self.0.get_mut(&window)?;
        let previous = (ring.cursor + KEPT - 1) % KEPT;
        if previous == ring.write {
            return None;
        }
        let line = ring.slots[previous].clone()?;
        ring.cursor = previous;
        Some(line)
    }

    /// Shift+Down: the next newer line; past the newest, or while not
    /// recalling, an empty line.
    pub(super) fn newer(&mut self, window: WindowId) -> Option<Step> {
        let Some(ring) = self.0.get_mut(&window) else {
            return Some(Step::Clear);
        };
        if ring.cursor == ring.write {
            return Some(Step::Clear);
        }
        let next = (ring.cursor + 1) % KEPT;
        if next == ring.write {
            ring.cursor = ring.write;
            return Some(Step::Clear);
        }
        let line = ring.slots[next].clone()?;
        ring.cursor = next;
        Some(Step::Line(line))
    }

    /// Keeps each line's words but not its links, as the draft does when its
    /// links lose their scope.
    pub(super) fn demote_links(&mut self) {
        for line in self
            .0
            .values_mut()
            .flat_map(|ring| ring.slots.iter_mut().flatten())
        {
            line.demote_links();
        }
    }

    /// Forgets the lines of windows that no longer exist.
    pub(super) fn retain(&mut self, exists: impl Fn(WindowId) -> bool) {
        self.0.retain(|window, _| exists(*window));
    }

    /// Forgets every line, as at a camp.
    pub(super) fn clear(&mut self) {
        self.0.clear();
    }
}

/// Whether the line starts as the withheld command does. The program
/// compares four characters (0x573f00); the case rule is inferred, and
/// ignoring case keeps more out (checklist 6.467).
fn withholds(withheld: Option<&str>, text: &str) -> bool {
    let Some(withheld) = withheld else {
        return false;
    };
    let prefix: String = withheld.chars().take(4).collect();
    !prefix.is_empty()
        && text
            .chars()
            .take(4)
            .collect::<String>()
            .eq_ignore_ascii_case(&prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: WindowId = WindowId::Chat;

    fn line(text: &str) -> Draft {
        let mut draft = Draft::default();
        draft.insert_text(text);
        draft
    }

    fn steps(recall: &mut Recall, keys: &str) -> Vec<String> {
        keys.chars()
            .map(|key| {
                let step = if key == 'u' {
                    recall.older(MAIN).map(Step::Line)
                } else {
                    recall.newer(MAIN)
                };
                match step {
                    Some(Step::Line(draft)) => draft.text().to_owned(),
                    Some(Step::Clear) => "<clear>".into(),
                    None => "<stay>".into(),
                }
            })
            .collect()
    }

    #[test]
    fn up_steps_back_to_the_oldest_and_down_past_the_newest_clears() {
        let mut recall = Recall::default();
        assert!(recall.older(MAIN).is_none());
        assert!(matches!(recall.newer(MAIN), Some(Step::Clear)));
        for text in ["/say a", "/say b", "/em c"] {
            recall.remember(MAIN, &line(text), None);
        }
        assert_eq!(
            steps(&mut recall, "uuuuddd"),
            [
                "/em c", "/say b", "/say a", "<stay>", "/say b", "/em c", "<clear>"
            ]
        );
        // Not recalling, Down clears the line as well.
        assert_eq!(steps(&mut recall, "d"), ["<clear>"]);
    }

    #[test]
    fn only_the_newest_repeat_empty_lines_and_the_withheld_command_are_not_kept() {
        let mut recall = Recall::default();
        let withheld = Some("/password");
        for text in ["a", "b", "b", "   ", "/PASSword secret", "a"] {
            recall.remember(MAIN, &line(text), withheld);
        }
        assert_eq!(steps(&mut recall, "uuuu"), ["a", "b", "a", "<stay>"]);
        // A line that is not kept still ends the recall.
        recall.remember(MAIN, &line("a"), withheld);
        assert_eq!(steps(&mut recall, "u"), ["a"]);
    }

    #[test]
    fn forty_lines_are_kept_per_window_and_go_with_it() {
        let mut recall = Recall::default();
        for index in 0..45 {
            recall.remember(MAIN, &line(&format!("line {index}")), None);
        }
        let other = WindowId::ChatGroup(1);
        recall.remember(other, &line("elsewhere"), None);
        let back = steps(&mut recall, &"u".repeat(41));
        assert_eq!(back[0], "line 44");
        // Once full, Up stops one short of the slot it would write next.
        assert_eq!(back[38], "line 6");
        assert_eq!(back[39], "<stay>");
        assert_eq!(recall.older(other).unwrap().text(), "elsewhere");
        recall.retain(|window| window == MAIN);
        assert!(recall.older(other).is_none());
        recall.clear();
        assert!(recall.older(MAIN).is_none());
    }
}
