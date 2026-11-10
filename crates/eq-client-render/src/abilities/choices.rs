//! What each of the Actions window's chosen ability buttons holds, as the
//! player picks it from the skills picker (read R17): the Abilities page's
//! six and the Combat page's four after its fixed ones.
//!
//! Until the player first chooses one in this client, nothing is saved and
//! each button follows the character: the installed client's choice where
//! its settings file has one, else this client's own order, worked out
//! again on every entry, so a skill gained later shows as it always has.
//! The first choice keeps every button as it then is and saves all ten
//! (recording checklist 6.448). The installed client's file is only read.
//!
//! Saved as the program saves them, `Ability1` to `Ability6` in
//! `[Abilities]` and `CombatSkill1` to `CombatSkill4` in `[CombatSkills]`,
//! each a skill number or -1 for empty. Those keys carry only a number the
//! program itself would write: a skill's, or one of its specials (read
//! R17b), 105 for Harm Touch and 107 for Lay on Hands, named from the spells
//! they cast (88 and 87), since the program has only the numbers. A
//! discipline, which has none (6.450), is saved under `[ClientAbilities]`
//! with its button's own key at -1; an earlier file's knight ability there
//! is read back as its special number.
use super::{AbilityButton, Page, knight, knows_discipline};
use bevy::prelude::*;
use eq_client_core::{
    Capability, abilities::Ability, disciplines::Discipline, hotbar::Binding,
    knights::KnightAbility, world::ClientWorld,
};

/// How many buttons the Abilities page has.
pub(crate) const ABILITY_BUTTONS: usize = 6;
/// How many buttons the Combat page has for abilities.
pub(crate) const COMBAT_BUTTONS: usize = 4;

/// The program's ability skills, in the order its picker lists them (R17,
/// 0x553f90 mode 3).
const ABILITY_SET: [u32; 11] = [17, 27, 29, 35, 40, 42, 48, 53, 55, 62, 67];
/// Its combat skills, which both pages' pickers list (modes 2 and 3).
const COMBAT_SET: [u32; 16] = [8, 9, 10, 16, 21, 23, 25, 26, 30, 32, 38, 52, 71, 72, 73, 74];

/// The program's special number for a knight's class ability (read R17b,
/// 0x41f4a0): 105 casts spell 88, Harm Touch, and 107 spell 87, Lay on
/// Hands. The names are inferred from the spell numbers.
const fn special(ability: KnightAbility) -> u32 {
    match ability {
        KnightAbility::HarmTouch => 105,
        KnightAbility::LayOnHands => 107,
    }
}

/// The knight's class ability a special number names, if it is one.
const fn knight_special(skill: u32) -> Option<KnightAbility> {
    match skill {
        105 => Some(KnightAbility::HarmTouch),
        107 => Some(KnightAbility::LayOnHands),
        _ => None,
    }
}

/// What one button holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Not chosen yet: the button follows this client's own order.
    #[default]
    Follows,
    /// Chosen empty, the program's -1.
    Empty,
    /// A skill or one of the program's specials, by its number, kept as
    /// saved even where this client cannot use it (6.449).
    Skill(u32),
    /// A discipline, which the program keeps in a window of its own (6.450).
    Discipline(Discipline),
}

impl Slot {
    /// What a button holding this binding holds, kept as a choice.
    fn of(binding: Option<Binding>) -> Self {
        match binding {
            Some(Binding::Ability(ability)) => Self::Skill(ability.skill()),
            Some(Binding::KnightAbility(ability)) => Self::Skill(special(ability)),
            Some(Binding::Discipline(discipline)) => Self::Discipline(discipline),
            _ => Self::Empty,
        }
    }

    /// The number the program's own key saves: a skill's, or -1.
    fn number(self) -> i64 {
        match self {
            Self::Skill(skill) => i64::from(skill),
            _ => -1,
        }
    }

    /// What `[ClientAbilities]` saves for it, if anything.
    fn own(self) -> Option<String> {
        match self {
            Self::Discipline(discipline) => Some(format!("discipline:{}", discipline.key())),
            _ => None,
        }
    }

    /// A `[ClientAbilities]` value read back.
    fn read_own(value: &str) -> Option<Self> {
        let (kind, key) = value.trim().split_once(':')?;
        match kind {
            // An earlier file kept a knight's ability here, before R17b.
            "knight" => KnightAbility::named(key).map(|ability| Self::Skill(special(ability))),
            "discipline" => Discipline::named(key).map(Self::Discipline),
            _ => None,
        }
    }
}

/// Every chosen button's choice.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Choices {
    abilities: [Slot; ABILITY_BUTTONS],
    combat: [Slot; COMBAT_BUTTONS],
    /// Whether these are the player's own, chosen in this client or read
    /// back from its file, and so saved; the installed client's, or none,
    /// are not.
    kept: bool,
}

/// A section's keys, in order, by page.
fn keys(page: Page) -> (&'static str, &'static str, usize) {
    match page {
        Page::Abilities => ("Abilities", "Ability", ABILITY_BUTTONS),
        Page::Combat => ("CombatSkills", "CombatSkill", COMBAT_BUTTONS),
    }
}

/// This client's own section, for what the program's keys cannot carry.
const OWN: &str = "ClientAbilities";

impl Choices {
    /// The choice for a button; one past the page's end follows.
    pub(crate) fn slot(&self, button: AbilityButton) -> Slot {
        self.page(button.page)
            .get(button.index)
            .copied()
            .unwrap_or_default()
    }

    fn page(&self, page: Page) -> &[Slot] {
        match page {
            Page::Abilities => &self.abilities,
            Page::Combat => &self.combat,
        }
    }

    fn page_mut(&mut self, page: Page) -> &mut [Slot] {
        match page {
            Page::Abilities => &mut self.abilities,
            Page::Combat => &mut self.combat,
        }
    }

    /// Chooses what a button holds. Every button still following this
    /// client's order keeps what it holds now, so the first choice saves
    /// all ten as they are.
    pub(crate) fn choose(&mut self, world: &ClientWorld, button: AbilityButton, slot: Slot) {
        for page in [Page::Abilities, Page::Combat] {
            for index in 0..self.page(page).len() {
                let place = AbilityButton { page, index };
                if self.slot(place) == Slot::Follows {
                    self.page_mut(page)[index] = Slot::of(ordered(world, place));
                }
            }
        }
        if let Some(chosen) = self.page_mut(button.page).get_mut(button.index) {
            *chosen = slot;
        }
        self.kept = true;
    }

    /// Whether these are the player's own, to be saved.
    pub(crate) const fn kept(&self) -> bool {
        self.kept
    }

    /// The choices the installed client saved, each button it leaves as it
    /// was following this client's order.
    pub(crate) fn installed(saved: &eq_client_assets::ui::AbilityChoices) -> Self {
        let slot = |value: Option<i32>| match value {
            None => Slot::Follows,
            Some(skill) => u32::try_from(skill).map_or(Slot::Empty, Slot::Skill),
        };
        Self {
            abilities: saved.abilities.map(slot),
            combat: saved.combat.map(slot),
            kept: false,
        }
    }

    /// The file this client saves, once every button is chosen.
    pub(crate) fn text(&self) -> String {
        use std::fmt::Write as _;
        let mut text = String::new();
        let mut own = String::new();
        for page in [Page::Abilities, Page::Combat] {
            let (section, prefix, _) = keys(page);
            let _ = writeln!(text, "[{section}]");
            for (index, slot) in self.page(page).iter().enumerate() {
                let key = format!("{prefix}{}", index + 1);
                let _ = writeln!(text, "{key}={}", slot.number());
                if let Some(value) = slot.own() {
                    let _ = writeln!(own, "{key}={value}");
                }
            }
        }
        if !own.is_empty() {
            let _ = writeln!(text, "[{OWN}]\n{own}");
        }
        text
    }

    /// Reads a file [`Choices::text`] wrote. A button with no number the
    /// program would read stays empty.
    pub(crate) fn read(text: &str) -> Self {
        let mut choices = Self {
            abilities: [Slot::Empty; ABILITY_BUTTONS],
            combat: [Slot::Empty; COMBAT_BUTTONS],
            kept: true,
        };
        let mut section = String::new();
        for line in text.lines().map(str::trim) {
            if let Some(name) = line
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
            {
                name.trim().clone_into(&mut section);
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            for page in [Page::Abilities, Page::Combat] {
                let (own_section, prefix, count) = keys(page);
                let Some(index) = key
                    .strip_prefix(prefix)
                    .and_then(|number| number.parse::<usize>().ok())
                    .filter(|number| (1..=count).contains(number))
                    .map(|number| number - 1)
                else {
                    continue;
                };
                let slot = if section.eq_ignore_ascii_case(OWN) {
                    Slot::read_own(value)
                } else if section.eq_ignore_ascii_case(own_section) {
                    value
                        .trim()
                        .parse::<i64>()
                        .ok()
                        .map(|number| u32::try_from(number).map_or(Slot::Empty, Slot::Skill))
                } else {
                    None
                };
                if let Some(slot) = slot
                    && (section.eq_ignore_ascii_case(own_section)
                        || choices.page(page)[index] == Slot::Empty)
                {
                    choices.page_mut(page)[index] = slot;
                }
            }
        }
        choices
    }
}

/// What a button holds now, if anything the client can use.
pub(crate) fn assigned(
    world: &ClientWorld,
    choices: &Choices,
    button: AbilityButton,
) -> Option<Binding> {
    match choices.slot(button) {
        Slot::Follows => ordered(world, button),
        Slot::Empty => None,
        Slot::Skill(skill) => match knight_special(skill) {
            Some(ability) => (world.can(Capability::KnightAbilities)
                && knight(world) == Some(ability))
            .then_some(Binding::KnightAbility(ability)),
            None => Ability::from_skill(skill).map(Binding::Ability),
        },
        Slot::Discipline(discipline) => (world.can(Capability::UsingDisciplines)
            && knows_discipline(world, discipline))
        .then_some(Binding::Discipline(discipline)),
    }
}

/// Whether a button holds nothing at all, so a click opens the picker as
/// the program's does on an empty button. One holding a choice the client
/// cannot use is not empty (6.449).
pub(crate) fn empty(world: &ClientWorld, choices: &Choices, button: AbilityButton) -> bool {
    match choices.slot(button) {
        Slot::Follows => ordered(world, button).is_none(),
        Slot::Empty => true,
        _ => false,
    }
}

/// This client's own order: knight abilities, disciplines and the
/// at-target abilities on the Combat page, the rest of those then the
/// others on the Abilities page.
fn ordered(world: &ClientWorld, button: AbilityButton) -> Option<Binding> {
    let (targeted, other): (Vec<Ability>, Vec<Ability>) = world
        .abilities()
        .into_iter()
        .partition(|ability| ability.at_target());
    // Only a session that offers them gives these their own buttons, so the
    // ordinary abilities keep their places everywhere else.
    let targeted = knight(world)
        .filter(|_| world.can(Capability::KnightAbilities))
        .map(Binding::KnightAbility)
        .into_iter()
        .chain(
            Discipline::SUPPORTED
                .into_iter()
                .filter(|_| world.can(Capability::UsingDisciplines))
                .filter(|&discipline| knows_discipline(world, discipline))
                .map(Binding::Discipline),
        )
        .chain(targeted.into_iter().map(Binding::Ability));
    match button.page {
        Page::Combat => targeted.take(COMBAT_BUTTONS).nth(button.index),
        Page::Abilities => targeted
            .skip(COMBAT_BUTTONS)
            .chain(other.into_iter().map(Binding::Ability))
            .nth(button.index),
    }
}

/// A row of the skills picker: what it puts on the button, its name, and
/// the player's value in the skill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    pub(crate) slot: Slot,
    pub(crate) name: &'static str,
    pub(crate) value: Option<u32>,
}

/// What a page's picker lists, in the program's order: on the Abilities
/// page its ability skills then its combat skills, on the Combat page its
/// combat skills only, each that the player has; then on both a knight's
/// class ability as the program's special number (R17b), offered by the
/// player's class and level, this client's reading of the profile byte the
/// program tests (6.450); then the disciplines the player knows (6.450).
/// A skill this client has no use for is left out, as are the specials
/// 111, 114 and 115, whose profile bytes or uses the client does not have
/// (6.449, 6.450).
pub(crate) fn offered(world: &ClientWorld, page: Page) -> Vec<Offer> {
    let has = world.abilities();
    let skills = world
        .player()
        .and_then(|player| player.skills.as_deref())
        .unwrap_or_default();
    let set: &[u32] = &COMBAT_SET;
    let first: &[u32] = match page {
        Page::Abilities => &ABILITY_SET,
        Page::Combat => &[],
    };
    let skills_offered = first.iter().chain(set).filter_map(|&skill| {
        let ability = Ability::from_skill(skill).filter(|ability| has.contains(ability))?;
        Some(Offer {
            slot: Slot::Skill(skill),
            name: ability.name(),
            value: usize::try_from(skill)
                .ok()
                .and_then(|index| skills.get(index))
                .copied(),
        })
    });
    let knights = knight(world)
        .filter(|_| world.can(Capability::KnightAbilities))
        .map(|ability| Offer {
            slot: Slot::Skill(special(ability)),
            name: ability.name(),
            value: None,
        });
    let disciplines = Discipline::SUPPORTED
        .into_iter()
        .filter(|_| world.can(Capability::UsingDisciplines))
        .filter(|&discipline| knows_discipline(world, discipline))
        .map(|discipline| Offer {
            slot: Slot::Discipline(discipline),
            name: discipline.name(),
            value: None,
        });
    skills_offered.chain(knights).chain(disciplines).collect()
}

/// The caption of a button that holds a choice the client cannot use:
/// this client's own words (6.449).
pub(crate) const UNUSABLE: &str = "Not available";

/// What a button says: its ability's name, nothing when empty, or
/// [`UNUSABLE`].
pub(crate) fn caption(
    world: &ClientWorld,
    choices: &Choices,
    button: AbilityButton,
) -> &'static str {
    match assigned(world, choices, button) {
        Some(Binding::Ability(ability)) => ability.name(),
        Some(Binding::KnightAbility(ability)) => ability.name(),
        Some(Binding::Discipline(discipline)) => discipline.name(),
        _ if empty(world, choices, button) => "",
        _ => UNUSABLE,
    }
}

/// A button as the script report names it: its page, its place from 1,
/// what it holds and its caption.
pub(crate) type ReportedButton = (&'static str, usize, Slot, &'static str);

/// What the script report says of the Actions window's chosen buttons:
/// each button's page, place from 1, what it holds and its caption; and
/// while the picker is open, its rows' names.
pub(crate) fn report(
    world: &ClientWorld,
    choices: &Choices,
    picker: &super::picker::Picker,
) -> (Vec<ReportedButton>, Vec<&'static str>) {
    let buttons = [Page::Combat, Page::Abilities]
        .into_iter()
        .flat_map(|page| {
            let (count, name) = match page {
                Page::Combat => (COMBAT_BUTTONS, "combat"),
                Page::Abilities => (ABILITY_BUTTONS, "abilities"),
            };
            (0..count).map(move |index| (page, name, index))
        })
        .map(|(page, name, index)| {
            let button = AbilityButton { page, index };
            let slot = match choices.slot(button) {
                Slot::Follows => Slot::of(ordered(world, button)),
                slot => slot,
            };
            (name, index + 1, slot, caption(world, choices, button))
        })
        .collect();
    let rows = picker
        .button()
        .map(|button| {
            offered(world, button.page)
                .into_iter()
                .map(|offer| offer.name)
                .collect()
        })
        .unwrap_or_default();
    (buttons, rows)
}

/// One file owner for the choices.
#[derive(Resource, Default)]
pub(crate) struct Files(crate::profile_files::Store);

/// The file prefix the choices are saved under.
const FILE: &str = "abilities";

/// Loads the choices of whoever is playing, and saves each change once
/// they are the player's own. Before the first choice nothing is written,
/// and the installed client's file is only read.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn persist(
    settings: Res<crate::ViewerSettings>,
    profile: Res<crate::profile_files::Profile>,
    mut choices: ResMut<Choices>,
    mut files: ResMut<Files>,
) {
    let store = &mut files.0;
    let now = std::time::Instant::now();
    store.retry(now);
    if store.loaded && store.profile == profile.key() {
        if choices.is_changed() {
            save(store, &choices, now);
        }
        return;
    }
    if store.loaded {
        save(store, &choices, now);
    } else {
        store.open(settings.0.settings_directory.as_ref());
    }
    store.profile = profile.key();
    let saved = store.profile.as_ref().and_then(|profile| {
        match store.read_result(&crate::profile_files::exact_name(FILE, profile)) {
            Ok(text) => text,
            Err(error) => {
                warn!("Could not read the character's ability choices: {error}");
                None
            }
        }
    });
    *choices = match saved {
        Some(text) => Choices::read(&text),
        None => store
            .profile
            .as_ref()
            .and_then(|profile| {
                settings
                    .0
                    .official_settings()?
                    .ability_choices(&profile.character, &profile.world)
            })
            .map(|saved| Choices::installed(&saved))
            .unwrap_or_default(),
    };
    store.loaded_text(if choices.kept() {
        choices.text()
    } else {
        String::new()
    });
}

/// Writes the player's own choices to the profile's file.
fn save(store: &mut crate::profile_files::Store, choices: &Choices, now: std::time::Instant) {
    if !choices.kept() {
        return;
    }
    let Some(profile) = &store.profile else {
        return;
    };
    let name = crate::profile_files::exact_name(FILE, profile);
    store.save(name, choices.text(), now);
}

#[cfg(test)]
mod tests;
