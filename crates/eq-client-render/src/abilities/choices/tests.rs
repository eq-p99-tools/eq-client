use super::*;
use crate::online::{OnlineState, testing};

/// An admitted rogue of this class with these skills above zero.
fn character(class: u32, level: u8, skills: &[u32]) -> OnlineState {
    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.class = Some(class);
    player.level = level;
    let mut values = vec![0; 100];
    for &skill in skills {
        values[usize::try_from(skill).unwrap()] = 5;
    }
    player.skills = Some(values);
    testing::admit(&mut online, 1, player);
    online
}

const ROGUE: u32 = 9;
const WARRIOR: u32 = 1;
const PALADIN: u32 = 3;

/// Backstab, Bind Wound, Disarm Traps, Hide, Pick Lock, Sense Heading,
/// Sneak, Pick Pockets, Fishing, Sense Traps and Begging.
const ROGUE_SKILLS: [u32; 11] = [8, 9, 17, 29, 35, 40, 42, 48, 55, 62, 67];

fn button(page: Page, index: usize) -> AbilityButton {
    AbilityButton { page, index }
}

fn names(offers: &[Offer]) -> Vec<&'static str> {
    offers.iter().map(|offer| offer.name).collect()
}

#[test]
fn the_pickers_list_the_programs_sets_in_its_order_and_the_combat_one_its_combat_skills() {
    let online = character(ROGUE, 21, &ROGUE_SKILLS);
    let world = online.world();
    assert_eq!(
        names(&offered(world, Page::Abilities)),
        [
            "Disarm Traps",
            "Hide",
            "Pick Lock",
            "Sense Heading",
            "Sneak",
            "Pick Pockets",
            "Fishing",
            "Sense Traps",
            "Begging",
            "Backstab",
            "Bind Wound",
        ]
    );
    // Fishing, an ability skill, is never on the Combat page's list.
    assert_eq!(
        names(&offered(world, Page::Combat)),
        ["Backstab", "Bind Wound"]
    );
    assert_eq!(offered(world, Page::Abilities)[6].value, Some(5));
    // Only skills the player has: a fresh character has no Sense Traps.
    let fresh = character(ROGUE, 1, &[55]);
    assert!(!names(&offered(fresh.world(), Page::Abilities)).contains(&"Sense Traps"));
}

#[test]
fn a_knights_ability_and_the_disciplines_come_after_the_programs_list() {
    let online = character(PALADIN, 1, &[9]);
    let offers = offered(online.world(), Page::Combat);
    assert_eq!(names(&offers), ["Bind Wound", "Lay on Hands"]);
    // The program's special number for it (R17b).
    assert_eq!(offers[1].slot, Slot::Skill(107));
    let offers = offered(online.world(), Page::Abilities);
    assert_eq!(names(&offers).last(), Some(&"Lay on Hands"));
}

#[test]
fn before_the_first_choice_the_buttons_follow_the_character_then_all_ten_are_kept() {
    let mut online = character(WARRIOR, 1, &[30]);
    let mut choices = Choices::default();
    let combat = |online: &OnlineState, choices: &Choices, index| {
        assigned(online.world(), choices, button(Page::Combat, index))
    };
    assert_eq!(
        combat(&online, &choices, 0),
        Some(Binding::Ability(Ability::Kick))
    );
    assert_eq!(combat(&online, &choices, 1), None);
    assert!(!choices.kept());
    // A skill gained later shows as it would without the picker.
    online = character(WARRIOR, 5, &[30, 73]);
    assert_eq!(
        combat(&online, &choices, 1),
        Some(Binding::Ability(Ability::Taunt))
    );
    choices.choose(
        online.world(),
        button(Page::Abilities, 0),
        Slot::Skill(Ability::Fishing.skill()),
    );
    assert!(choices.kept());
    assert_eq!(
        choices.slot(button(Page::Combat, 1)),
        Slot::Skill(Ability::Taunt.skill())
    );
    assert_eq!(choices.slot(button(Page::Combat, 2)), Slot::Empty);
    // Kept now: another skill gained no longer moves the buttons.
    online = character(WARRIOR, 10, &[30, 73, 74]);
    assert_eq!(
        combat(&online, &choices, 1),
        Some(Binding::Ability(Ability::Taunt))
    );
    assert_eq!(combat(&online, &choices, 2), None);
    assert_eq!(
        assigned(online.world(), &choices, button(Page::Abilities, 0)),
        Some(Binding::Ability(Ability::Fishing))
    );
}

#[test]
fn the_program_keys_carry_only_skill_numbers_and_the_rest_goes_in_the_clients_own_section() {
    let online = character(PALADIN, 1, &[9]);
    let mut choices = Choices::default();
    choices.choose(online.world(), button(Page::Combat, 1), Slot::Skill(107));
    choices.choose(
        online.world(),
        button(Page::Abilities, 5),
        Slot::Discipline(Discipline::Evasive),
    );
    choices.choose(online.world(), button(Page::Abilities, 4), Slot::Skill(71));
    let text = choices.text();
    // Bind Wound is not at a target, so this client's order has it first
    // on the Abilities page; the knight's ability first on the Combat page,
    // each knight's ability under its special number.
    assert!(text.contains("[Abilities]\nAbility1=9\n"));
    assert!(text.contains(
        "Ability5=71\nAbility6=-1\n[CombatSkills]\nCombatSkill1=107\nCombatSkill2=107\n"
    ));
    assert!(text.contains("[ClientAbilities]\nAbility6=discipline:evasive\n"));
    assert!(!text.contains("knight:"));
    let read = Choices::read(&text);
    assert_eq!(read, choices);
    assert_eq!(read.text(), text);
}

#[test]
fn a_file_read_back_keeps_empty_unknown_and_own_buttons() {
    let read = Choices::read(
        "[Abilities]\nAbility1=55\nAbility2=-1\nAbility3=72\nAbility9=8\n[CombatSkills]\nCombatSkill1=105\n[ClientAbilities]\nAbility4=discipline:unknown\n",
    );
    assert!(read.kept());
    assert_eq!(read.slot(button(Page::Abilities, 0)), Slot::Skill(55));
    assert_eq!(read.slot(button(Page::Abilities, 1)), Slot::Empty);
    assert_eq!(read.slot(button(Page::Abilities, 2)), Slot::Skill(72));
    assert_eq!(read.slot(button(Page::Abilities, 3)), Slot::Empty);
    // A special number reads back as itself.
    assert_eq!(read.slot(button(Page::Combat, 0)), Slot::Skill(105));
    // A missing key is empty once the file is the player's own.
    assert_eq!(read.slot(button(Page::Combat, 3)), Slot::Empty);
}

#[test]
fn the_installed_clients_choices_seed_the_buttons_but_are_not_the_players_own() {
    let installed = Choices::installed(&eq_client_assets::ui::AbilityChoices {
        abilities: [Some(55), Some(-1), None, None, None, None],
        combat: [Some(105), None, None, None],
    });
    assert!(!installed.kept());
    assert_eq!(installed.slot(button(Page::Abilities, 0)), Slot::Skill(55));
    assert_eq!(installed.slot(button(Page::Abilities, 1)), Slot::Empty);
    assert_eq!(installed.slot(button(Page::Abilities, 2)), Slot::Follows);
    assert_eq!(installed.slot(button(Page::Combat, 0)), Slot::Skill(105));
}

#[test]
fn a_choice_the_client_cannot_use_is_not_empty_and_says_so() {
    let online = character(ROGUE, 21, &ROGUE_SKILLS);
    let world = online.world();
    let mut choices = Choices::default();
    let place = button(Page::Abilities, 2);
    choices.choose(world, place, Slot::Skill(105));
    assert_eq!(assigned(world, &choices, place), None);
    assert!(!empty(world, &choices, place));
    assert_eq!(caption(world, &choices, place), UNUSABLE);
    choices.choose(world, place, Slot::Empty);
    assert!(empty(world, &choices, place));
    assert_eq!(caption(world, &choices, place), "");
    // A skill no longer held keeps its button; the session refuses a press.
    let fresh = character(ROGUE, 1, &[]);
    choices.choose(fresh.world(), place, Slot::Skill(Ability::PickLock.skill()));
    assert_eq!(
        assigned(fresh.world(), &choices, place),
        Some(Binding::Ability(Ability::PickLock))
    );
}

#[test]
fn a_special_number_survives_a_save_and_another_classs_is_kept_unusable() {
    let online = character(PALADIN, 1, &[9]);
    let place = button(Page::Combat, 0);
    let mut choices = Choices::default();
    choices.choose(online.world(), place, Slot::Skill(107));
    assert_eq!(
        assigned(online.world(), &choices, place),
        Some(Binding::KnightAbility(KnightAbility::LayOnHands))
    );
    let text = choices.text();
    assert!(text.contains("CombatSkill1=107\n"));
    assert_eq!(Choices::read(&text), choices);
    // Another class's special, or one the client has no use for, is kept
    // and unusable.
    let rogue = character(ROGUE, 21, &ROGUE_SKILLS);
    assert_eq!(assigned(rogue.world(), &choices, place), None);
    assert_eq!(caption(rogue.world(), &choices, place), UNUSABLE);
    for special in [111, 114, 115] {
        let mut kept = choices;
        kept.choose(rogue.world(), place, Slot::Skill(special));
        assert_eq!(
            Choices::read(&kept.text()).slot(place),
            Slot::Skill(special)
        );
        assert_eq!(assigned(rogue.world(), &kept, place), None);
    }
}
