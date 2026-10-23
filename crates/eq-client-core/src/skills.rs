//! The servers' skills by number, as the installed client names them: each
//! skill's string in the client's table (`eqstr_us.txt`), and the same name
//! for a viewer without one. The numbers and strings follow `EQEmu`'s
//! `common/skills.h`; Titanium's table has no string for the last three.

/// The official client's line as a skill rises, which names the skill and
/// its new value.
pub const BETTER_AT: u32 = 12091;

/// Safe Fall's number: the skill that lessens a fall's damage.
pub const SAFE_FALL: u32 = 39;

/// Each skill's string and name, by number.
const SKILLS: [(Option<u32>, &str); 78] = [
    (Some(13855), "1H Blunt"),
    (Some(13856), "1H Slashing"),
    (Some(13857), "2H Blunt"),
    (Some(13858), "2H Slashing"),
    (Some(13859), "Abjuration"),
    (Some(13861), "Alteration"),
    (Some(13862), "Apply Poison"),
    (Some(13863), "Archery"),
    (Some(13864), "Backstab"),
    (Some(13866), "Bind Wound"),
    (Some(13867), "Bash"),
    (Some(13871), "Block"),
    (Some(13872), "Brass Instruments"),
    (Some(13874), "Channeling"),
    (Some(13875), "Conjuration"),
    (Some(13876), "Defense"),
    (Some(13877), "Disarm"),
    (Some(13878), "Disarm Traps"),
    (Some(13879), "Divination"),
    (Some(13880), "Dodge"),
    (Some(13881), "Double Attack"),
    (Some(13882), "Dragon Punch"),
    (Some(13883), "Dual Wield"),
    (Some(13884), "Eagle Strike"),
    (Some(13885), "Evocation"),
    (Some(13886), "Feign Death"),
    (Some(13888), "Flying Kick"),
    (Some(13889), "Forage"),
    (Some(13890), "Hand to Hand"),
    (Some(13891), "Hide"),
    (Some(13893), "Kick"),
    (Some(13894), "Meditate"),
    (Some(13895), "Mend"),
    (Some(13896), "Offense"),
    (Some(13897), "Parry"),
    (Some(13899), "Pick Lock"),
    (Some(13900), "Piercing"),
    (Some(13903), "Riposte"),
    (Some(13904), "Round Kick"),
    (Some(13905), "Safe Fall"),
    (Some(13906), "Sense Heading"),
    (Some(13908), "Singing"),
    (Some(13909), "Sneak"),
    (Some(13910), "Specialize Abjure"),
    (Some(13911), "Specialize Alteration"),
    (Some(13912), "Specialize Conjuration"),
    (Some(13913), "Specialize Divination"),
    (Some(13914), "Specialize Evocation"),
    (Some(13915), "Pick Pockets"),
    (Some(13916), "Stringed Instruments"),
    (Some(13917), "Swimming"),
    (Some(13919), "Throwing"),
    (Some(13920), "Tiger Claw"),
    (Some(13921), "Tracking"),
    (Some(13923), "Wind Instruments"),
    (Some(13854), "Fishing"),
    (Some(13853), "Make Poison"),
    (Some(13852), "Tinkering"),
    (Some(13851), "Research"),
    (Some(13850), "Alchemy"),
    (Some(13865), "Baking"),
    (Some(13918), "Tailoring"),
    (Some(13907), "Sense Traps"),
    (Some(13870), "Blacksmithing"),
    (Some(13887), "Fletching"),
    (Some(13873), "Brewing"),
    (Some(13860), "Alcohol Tolerance"),
    (Some(13868), "Begging"),
    (Some(13892), "Jewelry Making"),
    (Some(13901), "Pottery"),
    (Some(13898), "Percussion Instruments"),
    (Some(13922), "Intimidation"),
    (Some(13869), "Berserking"),
    (Some(13902), "Taunt"),
    (Some(5837), "Frenzy"),
    (None, "Remove Traps"),
    (None, "Triple Attack"),
    (None, "2H Piercing"),
];

/// A skill's entry, if the number is a skill's.
fn entry(skill: u32) -> Option<(Option<u32>, &'static str)> {
    SKILLS.get(usize::try_from(skill).ok()?).copied()
}

/// The client's string for a skill's name, where its table has one.
#[must_use]
pub fn name_string(skill: u32) -> Option<u32> {
    entry(skill)?.0
}

/// A skill's name, as the client's table spells it.
#[must_use]
pub fn name(skill: u32) -> Option<&'static str> {
    Some(entry(skill)?.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_are_named_by_number_with_the_clients_strings() {
        assert_eq!((name(0), name_string(0)), (Some("1H Blunt"), Some(13855)));
        assert_eq!((name(30), name_string(30)), (Some("Kick"), Some(13893)));
        assert_eq!((name(74), name_string(74)), (Some("Frenzy"), Some(5837)));
        assert_eq!((name(77), name_string(77)), (Some("2H Piercing"), None));
        assert_eq!(name(SAFE_FALL), Some("Safe Fall"));
        assert_eq!((name(78), name_string(78)), (None, None));
        // The abilities the session knows have the same names here.
        for ability in crate::abilities::Ability::ALL {
            assert_eq!(name(ability.skill()), Some(ability.name()), "{ability:?}");
        }
    }
}
