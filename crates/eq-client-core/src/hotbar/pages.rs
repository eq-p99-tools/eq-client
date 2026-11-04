//! Ten pages of the first hotbar (Titanium 0x513b6b, 0x4b0812, 0x4b023b).
use super::{Binding, Hotbar, SLOTS};
use std::fmt::Write as _;

/// Pages in one hotbar.
pub const PAGES: usize = 10;
/// File header for a hotbar containing all ten pages.
const HEADER: &str = "# eq-client hotbar v2";

fn slot_key(key: &str) -> Option<(usize, usize)> {
    let index = |number: &str, limit| {
        number
            .parse::<usize>()
            .ok()?
            .checked_sub(1)
            .filter(|index| *index < limit)
    };
    if let Some(slot) = key.strip_prefix("slot_") {
        return Some((0, index(slot, SLOTS)?));
    }
    let (page, slot) = key.strip_prefix("page_")?.split_once("_slot_")?;
    Some((index(page, PAGES)?, index(slot, SLOTS)?))
}

/// Independent ten-slot pages of one hotbar. Page selection is runtime state,
/// separate from the bindings saved by the native client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pages(pub [Hotbar; PAGES]);

impl Default for Pages {
    fn default() -> Self {
        let mut pages = Self([Hotbar([None; SLOTS]); PAGES]);
        pages.0[0] = Hotbar::default();
        pages
    }
}

impl Pages {
    /// Reads old single-page files and all-page files over the supplied
    /// defaults. Unknown, malformed and out-of-range keys preserve defaults.
    #[must_use]
    pub fn read(text: &str, mut defaults: Self) -> Self {
        for line in text.lines() {
            let Some((key, words)) = line.split_once('=') else {
                continue;
            };
            let Some((page, slot)) = slot_key(key.trim()) else {
                continue;
            };
            let words = words.trim();
            if words == "none" {
                defaults.0[page].0[slot] = None;
            } else if let Some(binding) = Binding::parse(words) {
                defaults.0[page].0[slot] = Some(binding);
            }
        }
        defaults
    }

    /// Saves every binding. First-page keys retain their v1 spelling, so an
    /// older reader still sees the same first page. Other pages use explicit
    /// page/slot numbers; no current-page alias can overwrite another page.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = format!("{HEADER}\n");
        for (page, hotbar) in self.0.iter().enumerate() {
            for (slot, binding) in hotbar.0.iter().enumerate() {
                let words = binding.map_or_else(|| "none".into(), Binding::words);
                let prefix = if page == 0 {
                    String::new()
                } else {
                    format!("page_{}_", page + 1)
                };
                let _ = writeln!(text, "{prefix}slot_{} = {words}", slot + 1);
            }
        }
        text
    }

    /// Imports the first hotbar's native (page, button, code) entries, both
    /// numbers from 1. Explicit empty and unsupported codes leave that slot
    /// empty. No entries means no native configuration was found.
    #[must_use]
    pub fn official(buttons: &[(u8, u8, String)]) -> Option<Self> {
        let mut pages = Self([Hotbar([None; SLOTS]); PAGES]);
        let mut found = false;
        for (page, button, code) in buttons {
            let Some(page) = usize::from(*page)
                .checked_sub(1)
                .filter(|page| *page < PAGES)
            else {
                continue;
            };
            let Some(slot) = usize::from(*button)
                .checked_sub(1)
                .filter(|slot| *slot < SLOTS)
            else {
                continue;
            };
            found = true;
            pages.0[page].0[slot] = Binding::official(code);
        }
        found.then_some(pages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_files_migrate_to_the_first_page_and_all_pages_roundtrip() {
        let mut first = Hotbar::default();
        first.0[0] = Some(Binding::Camp);
        first.0[1] = None;
        let mut pages = Pages::read(&first.text(), Pages::default());
        assert_eq!(pages.0[0], first);
        assert!(pages.0[1..].iter().all(|page| page.0 == [None; SLOTS]));
        for page in 1..PAGES {
            pages.0[page].0[page] = Some(Binding::Gem(u8::try_from(page % 8).unwrap()));
        }
        let text = pages.text();
        assert_eq!(Pages::read(&text, Pages::default()), pages);
        assert_eq!(Hotbar::read(&text, Hotbar::default()), first);
        assert!(text.contains("page_10_slot_10 = gem 2\n"));
    }

    #[test]
    fn bad_page_keys_and_values_cannot_alias_valid_pages() {
        let defaults = Pages::default();
        let text = "page_0_slot_1 = sit\npage_11_slot_1 = stand\npage_2_slot_0 = sit\npage_2_slot_11 = stand\npage_2_slot_1 = gem 99\npage_x_slot_1 = camp\n";
        assert_eq!(Pages::read(text, defaults), defaults);
        let pages = Pages::read("page_10_slot_1 = camp\nslot_1 = none\n", defaults);
        assert_eq!(pages.0[0].0[0], None);
        assert_eq!(pages.0[9].0[0], Some(Binding::Camp));
        let aliases = Pages::read("page_1_slot_1 = stand\nslot_1 = camp\n", defaults);
        assert_eq!(aliases.0[0].0[0], Some(Binding::Camp));
    }

    #[test]
    fn native_pages_are_independent_and_explicit_empties_do_not_seed_defaults() {
        let pages = Pages::official(&[
            (1, 1, "H0".into()),
            (10, 1, "H7".into()),
            (2, 10, "J29".into()),
            (1, 2, String::new()),
            (3, 1, "unsupported".into()),
        ])
        .unwrap();
        assert_eq!(pages.0[0].0[0], Some(Binding::Gem(0)));
        assert_eq!(pages.0[9].0[0], Some(Binding::Gem(7)));
        assert_eq!(
            pages.0[1].0[9],
            Some(Binding::Ability(crate::abilities::Ability::Hide))
        );
        assert_eq!(pages.0[0].0[1], None);
        assert_eq!(pages.0[2].0[0], None);
        assert_eq!(
            Pages::official(&[(1, 1, String::new())]),
            Some(Pages([Hotbar([None; SLOTS]); PAGES]))
        );
        assert_eq!(
            Pages::official(&[(0, 1, "H0".into()), (11, 1, "H1".into())]),
            None
        );
    }
}
