//! A Bard's melody list, `/melody` (Titanium eqgame.exe 0x472156, read
//! R49-33 c): the gems it plays in turn, one step at each of the player's
//! own gem refreshes. The host makes the presses; this keeps the list.

use super::ClientWorld;

/// The highest gem `/melody` takes. The program reads 1 to 9; this client
/// and the session have 8 gems, so a 9 is passed over (labelled).
const LAST_GEM: u8 = 8;

/// The gems a melody plays, in order, and where it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Melody {
    /// Gem indexes (0 to 7), in the order typed.
    gems: Vec<u8>,
    /// The index into `gems` the next step presses.
    next: usize,
    /// A refresh came since the last step: the host owes one.
    due: bool,
}

impl Melody {
    /// The gems the melody plays, as indexes from 0, in order.
    #[must_use]
    pub fn gems(&self) -> &[u8] {
        &self.gems
    }

    /// Whether a step is owed.
    #[must_use]
    pub const fn due(&self) -> bool {
        self.due
    }

    /// A refresh of the player's own gem (`OP_MemorizeSpell` kind 3): with
    /// a list, the next step is owed. Nothing else moves the melody (no
    /// timer, no retry), so without a refresh it stalls (reviewer Y1, [I]).
    pub(super) fn refreshed(&mut self) {
        self.due = !self.gems.is_empty();
    }
}

/// The gems a `/melody` line names, as indexes from 0: every number from 1
/// to 8 in the text, in order (0x472156 takes gems 1 to 9). How the program
/// splits the text into numbers is not in the read; any run of digits is
/// one here (inferred).
#[must_use]
pub fn melody_gems(words: &str) -> Vec<u8> {
    words
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|number| number.parse::<u8>().ok())
        .filter(|gem| (1..=LAST_GEM).contains(gem))
        .map(|gem| gem - 1)
        .collect()
}

impl ClientWorld {
    /// Starts a melody of `gems` (indexes from 0) in place of any before,
    /// and gives the first gem for the host to press at once; none for an
    /// empty list, which only clears.
    pub fn start_melody(&mut self, gems: Vec<u8>) -> Option<u8> {
        let first = gems.first().copied();
        self.casting.melody = Melody {
            next: usize::from(first.is_some()),
            gems,
            due: false,
        };
        first
    }

    /// Clears the melody list, as `/melody` first does and a gem press
    /// while singing does (0x47e8e0).
    pub fn clear_melody(&mut self) {
        self.casting.melody = Melody::default();
    }

    /// The gem the owed melody step presses, taken once; the list then
    /// moves on, cycling. A press the host refuses keeps no place: the
    /// next refresh plays the gem after it (inferred, read R49-48 f).
    pub fn take_melody_step(&mut self) -> Option<u8> {
        let melody = &mut self.casting.melody;
        if !std::mem::take(&mut melody.due) {
            return None;
        }
        let gem = *melody.gems.get(melody.next)?;
        melody.next = (melody.next + 1) % melody.gems.len();
        Some(gem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_melody_line_names_gems_one_to_eight_in_order() {
        assert_eq!(melody_gems("1 3 2"), [0, 2, 1]);
        assert_eq!(melody_gems("1,3, 2"), [0, 2, 1]);
        // 0, 9 and 13 aren't this client's gems.
        assert_eq!(melody_gems("0 9 13 8"), [7]);
        assert_eq!(melody_gems(""), Vec::<u8>::new());
    }

    #[test]
    fn each_refresh_owes_one_step_and_the_list_cycles() {
        let mut world = ClientWorld::default();
        assert_eq!(world.start_melody(vec![0, 2, 1]), Some(0));
        assert_eq!(world.take_melody_step(), None);
        let mut played = Vec::new();
        for _ in 0..4 {
            world.casting.melody.refreshed();
            played.push(world.take_melody_step().unwrap());
            assert_eq!(world.take_melody_step(), None);
        }
        assert_eq!(played, [2, 1, 0, 2]);
        world.clear_melody();
        world.casting.melody.refreshed();
        assert_eq!(world.take_melody_step(), None);
        assert_eq!(world.start_melody(Vec::new()), None);
        world.casting.melody.refreshed();
        assert!(!world.casting().melody.due());
    }
}
