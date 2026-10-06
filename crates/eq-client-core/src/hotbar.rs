//! The hotbar's ten slots and what each is bound to. A file keeps them per
//! character, one `slot_<n> = <binding>` line each, so a file from an older
//! client still reads. A character with no file of their own starts from
//! the official client's hotbuttons where this client knows their kinds, or
//! else from the client's defaults.
use crate::{abilities::Ability, inventory::InventorySlot};
use std::fmt::Write as _;

/// The first line of a hotbar file.
pub const HEADER: &str = "# eq-client hotbar v1";

/// How many slots the hotbar has.
pub const SLOTS: usize = 10;

/// What a slot does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    /// Casts the spell in this gem, from 0.
    Gem(u8),
    /// Uses the item in this slot, while it is still this item.
    Item {
        /// Where the item is carried.
        slot: InventorySlot,
        /// The item's ID.
        id: u32,
    },
    /// Sits down.
    Sit,
    /// Stands up.
    Stand,
    /// Uses this ability.
    Ability(Ability),
    /// Turns melee auto-attack on or off, as the Actions window's Melee
    /// Attack does.
    Attack,
    /// Camps, as the Actions window's Camp does.
    Camp,
    /// Invites the target into the player's group.
    Invite,
    /// Joins the group the player was last invited to.
    Follow,
    /// Leaves the group, or disbands it as its leader, which with an
    /// invitation waiting declines it.
    Disband,
}

impl Binding {
    /// The words a file keeps it as.
    fn words(self) -> String {
        match self {
            Self::Gem(gem) => format!("gem {}", u16::from(gem) + 1),
            Self::Item { slot, id } => format!("item {} {id}", slot.0),
            Self::Sit => "sit".to_owned(),
            Self::Stand => "stand".to_owned(),
            Self::Ability(ability) => format!("ability {}", ability.skill()),
            Self::Attack => "attack".to_owned(),
            Self::Camp => "camp".to_owned(),
            Self::Invite => "invite".to_owned(),
            Self::Follow => "follow".to_owned(),
            Self::Disband => "disband".to_owned(),
        }
    }

    /// The binding a file's words say; None for words it cannot read.
    fn parse(words: &str) -> Option<Self> {
        let parts: Vec<&str> = words.split_whitespace().collect();
        Some(match parts[..] {
            ["gem", gem] => {
                Self::Gem(gem.parse::<u8>().ok().filter(|gem| (1..=8).contains(gem))? - 1)
            }
            ["item", slot, id] => Self::Item {
                slot: InventorySlot(slot.parse().ok()?),
                id: id.parse().ok()?,
            },
            ["sit"] => Self::Sit,
            ["stand"] => Self::Stand,
            ["ability", skill] => Self::Ability(Ability::from_skill(skill.parse().ok()?)?),
            ["attack"] => Self::Attack,
            ["camp"] => Self::Camp,
            ["invite"] => Self::Invite,
            ["follow"] => Self::Follow,
            ["disband"] => Self::Disband,
            _ => return None,
        })
    }
}

/// The hotbar's slots, from the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotbar(pub [Option<Binding>; SLOTS]);

impl Default for Hotbar {
    /// The client's defaults: the eight gems, then sit and stand.
    fn default() -> Self {
        Self(std::array::from_fn(|index| {
            Some(match index {
                8 => Binding::Sit,
                9 => Binding::Stand,
                _ => Binding::Gem(u8::try_from(index).unwrap_or_default()),
            })
        }))
    }
}

impl Hotbar {
    /// The slots a file's text holds, over these defaults: `none` empties a
    /// slot, and a line it cannot read keeps the default.
    #[must_use]
    pub fn read(text: &str, defaults: Self) -> Self {
        let mut hotbar = defaults;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let Some(index) = key
                .trim()
                .strip_prefix("slot_")
                .and_then(|number| number.parse::<usize>().ok())
                .and_then(|number| number.checked_sub(1))
                .filter(|index| *index < SLOTS)
            else {
                continue;
            };
            let value = value.trim();
            if value == "none" {
                hotbar.0[index] = None;
            } else if let Some(binding) = Binding::parse(value) {
                hotbar.0[index] = Some(binding);
            }
        }
        hotbar
    }

    /// The text a file keeps: the header, then every slot.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = format!("{HEADER}\n");
        for (index, binding) in self.0.iter().enumerate() {
            let words = binding.map_or_else(|| "none".to_owned(), Binding::words);
            // Writing to a String cannot fail.
            let _ = writeln!(text, "slot_{} = {words}", index + 1);
        }
        text
    }

    /// The official client's first page of hotbuttons, by button from 1,
    /// as far as this client knows their kinds: `H<n>` casts the spell in
    /// gem n (from 0) and `J<n>` uses the ability of skill n. A button of
    /// another kind (a social, a combat button, an item) leaves its slot
    /// empty. None when the page has no buttons, so the defaults stand.
    ///
    /// The kinds are inferred from installed character files (`H` holds 0 to
    /// 7, as gems do; `J` holds the skill IDs of hide and sneak), not from
    /// the client itself.
    #[must_use]
    pub fn official(buttons: &[(u8, String)]) -> Option<Self> {
        if buttons.is_empty() {
            return None;
        }
        let mut hotbar = Self([None; SLOTS]);
        for (button, code) in buttons {
            let Some(index) = usize::from(*button)
                .checked_sub(1)
                .filter(|index| *index < SLOTS)
            else {
                continue;
            };
            // The ini is read lossily, so a code may start with any character.
            hotbar.0[index] = if let Some(gem) = code.strip_prefix('H') {
                gem.parse::<u8>()
                    .ok()
                    .filter(|gem| *gem < 8)
                    .map(Binding::Gem)
            } else if let Some(skill) = code.strip_prefix('J') {
                skill
                    .parse()
                    .ok()
                    .and_then(Ability::from_skill)
                    .map(Binding::Ability)
            } else {
                None
            };
        }
        Some(hotbar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hotbar_keeps_to_its_file_and_old_files_still_read() {
        let mut hotbar = Hotbar::default();
        hotbar.0[2] = None;
        hotbar.0[3] = Some(Binding::Ability(Ability::Kick));
        hotbar.0[4] = Some(Binding::Item {
            slot: InventorySlot(23),
            id: 13027,
        });
        let text = hotbar.text();
        assert!(text.starts_with(HEADER));
        assert!(text.contains("slot_3 = none\n"));
        assert!(text.contains("slot_4 = ability 30\n"));
        assert!(text.contains("slot_5 = item 23 13027\n"));
        assert!(text.contains("slot_1 = gem 1\n") && text.contains("slot_10 = stand\n"));
        assert_eq!(Hotbar::read(&text, Hotbar::default()), hotbar);
        // Unknown slots, kinds and numbers keep the defaults.
        let odd =
            "slot_0 = sit\nslot_11 = sit\nslot_2 = gem 9\nslot_6 = ability 999\nslot_7 = dance\n";
        assert_eq!(Hotbar::read(odd, Hotbar::default()), Hotbar::default());
    }

    #[test]
    fn the_actions_windows_buttons_keep_to_a_file_by_name() {
        let mut hotbar = Hotbar([None; SLOTS]);
        let kinds = [
            Binding::Attack,
            Binding::Camp,
            Binding::Invite,
            Binding::Follow,
            Binding::Disband,
        ];
        for (slot, binding) in hotbar.0.iter_mut().zip(kinds) {
            *slot = Some(binding);
        }
        let text = hotbar.text();
        for line in [
            "slot_1 = attack\n",
            "slot_2 = camp\n",
            "slot_3 = invite\n",
            "slot_4 = follow\n",
            "slot_5 = disband\n",
            "slot_6 = none\n",
        ] {
            assert!(text.contains(line), "{text}");
        }
        assert_eq!(Hotbar::read(&text, Hotbar::default()), hotbar);
    }

    #[test]
    fn the_official_hotbuttons_seed_the_kinds_this_client_knows() {
        let buttons = [
            (1, "H0".to_owned()),
            (2, "H7".to_owned()),
            (3, "J29".to_owned()),
            (4, "E12".to_owned()),
            (5, "H8".to_owned()),
            (11, "H1".to_owned()),
        ];
        let hotbar = Hotbar::official(&buttons).unwrap();
        assert_eq!(hotbar.0[0], Some(Binding::Gem(0)));
        assert_eq!(hotbar.0[1], Some(Binding::Gem(7)));
        assert_eq!(hotbar.0[2], Some(Binding::Ability(Ability::Hide)));
        // A social, a ninth gem and an empty button leave their slots empty.
        assert_eq!(hotbar.0[3..], [None; 7]);
        assert_eq!(Hotbar::official(&[]), None);
    }

    #[test]
    fn a_corrupt_hotbutton_leaves_its_slot_empty() {
        // A byte the ini reader replaced with U+FFFD, other characters that
        // are more than one byte long, and an empty code.
        let buttons = [
            (1, "\u{FFFD}1".to_owned()),
            (2, "é2".to_owned()),
            (3, "H\u{FFFD}".to_owned()),
            (4, "J€".to_owned()),
            (5, String::new()),
            (6, "H3".to_owned()),
        ];
        let hotbar = Hotbar::official(&buttons).unwrap();
        assert_eq!(hotbar.0[..5], [None; 5]);
        assert_eq!(hotbar.0[5], Some(Binding::Gem(3)));
    }
}
