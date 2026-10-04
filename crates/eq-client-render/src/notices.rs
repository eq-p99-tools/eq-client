//! Words the world's notices: what each says to the player and where it
//! shows. The server's strings come from the installed client's table.
use super::chat::Said;
use super::{
    combat::{consideration_text, damage_text},
    hud::messages::Messages,
    trade::coin_text,
};
use eq_client_core::{
    CampStatus,
    bind_wound::{BindWoundUpdate, strings},
    entities::display_name,
    food::Shortage,
    loot::LootResponse,
    world::{GroupNotice, Link, ListingNotice, Notice, Party, RaidNotice},
};

/// One line on screen besides the chat, which notices set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Line {
    text: String,
}

impl Line {
    /// Says this until something else is said.
    pub(crate) fn set(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    /// What the line says.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

/// The client's line besides the chat: the status line, which says what
/// state the session is in. Refusals are said in the chat instead, as the
/// official client says them, since a skinned window has no line of its own
/// for them.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct Lines {
    /// The status line: the connection, death and camping.
    pub status: Line,
}

/// Where a notice shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Place {
    /// A system line in the chat window, where every refusal is said.
    Chat,
    /// The HUD's status line.
    Status,
}

/// Sense Heading in the official client's own words, with the point's name.
fn heading(point: u8, messages: &Messages) -> Said {
    let point = messages.format(12427 + u32::from(point), &[]);
    messages.said(12435, &[point])
}

/// What a hungry or thirsty player hears: the official client's words for
/// food or drink it could not find, and what was left for them to eat or
/// drink by hand because it has modifiers.
fn nothing_to_eat(
    food: Option<Shortage>,
    water: Option<Shortage>,
    messages: Option<&Messages>,
) -> Vec<Said> {
    let lacks = |meal: Option<Shortage>, shortage| meal == Some(shortage);
    let official = match (
        lacks(food, Shortage::Nothing),
        lacks(water, Shortage::Nothing),
    ) {
        (true, true) => Some(12491),
        (true, false) => Some(12488),
        (false, true) => Some(12490),
        (false, false) => None,
    };
    let kept = match (
        lacks(food, Shortage::OnlyModified),
        lacks(water, Shortage::OnlyModified),
    ) {
        (true, true) => Some(
            "You need food and drink, but yours have modifiers, so they are \
             only eaten and drunk when you right-click them.",
        ),
        (true, false) => Some(
            "You need food, but yours has modifiers, so it is only eaten when \
             you right-click it.",
        ),
        (false, true) => Some(
            "You need a drink, but yours has modifiers, so it is only drunk \
             when you right-click it.",
        ),
        (false, false) => None,
    };
    official
        .zip(messages)
        .map(|(id, messages)| messages.said(id, &[]))
        .into_iter()
        .chain(kept.map(Said::own))
        .collect()
}

/// A `/who all` answer's lines; without the installed strings, each still
/// says which string it is.
fn who_lines(
    list: &eq_client_core::who::WhoList,
    messages: Option<&Messages>,
) -> Vec<(Place, Said)> {
    super::who::lines(list, messages.unwrap_or(&Messages::default()))
        .into_iter()
        .map(|line| (Place::Chat, line))
        .collect()
}

/// A consent given or taken back, in the official client's words: to the
/// owner, who was consented and where; to the one consented, whose corpses
/// and where.
fn consent_line(
    consent: &eq_client_core::corpses::Consent,
    own: bool,
    messages: Option<&Messages>,
) -> Said {
    let (id, who) = match (own, consent.given) {
        (true, true) => (1427, &consent.granted),
        (true, false) => (1428, &consent.granted),
        (false, true) => (2080, &consent.owner),
        (false, false) => (2103, &consent.owner),
    };
    messages
        .unwrap_or(&Messages::default())
        .said(id, &[who.clone(), consent.zone.clone()])
}

/// The status line for the connection: dead and waiting, or how the link
/// stands.
fn connection_text(link: Link, dead: bool) -> String {
    if dead {
        "Dead - awaiting respawn".into()
    } else {
        link_text(link).into()
    }
}

/// Why a corpse could not be looted: this client's words, and the official
/// client's string for the server's answer where it has its own: 12073
/// for someone else looting it, 12070 for a corpse not to be looted yet,
/// 5154 for a hostile nearby and 12390 for a corpse too far away. That the
/// official client shows these strings for those answers is inferred from
/// what each says; a recording will check it.
const fn loot_refusal(response: LootResponse) -> (&'static str, Option<u32>) {
    match response {
        LootResponse::SomeoneElse => ("Someone else is looting that corpse.", Some(12073)),
        LootResponse::NotAtThisTime => ("You cannot loot that corpse at this time.", Some(12070)),
        LootResponse::Hostiles => ("You cannot loot while a hostile is nearby.", Some(5154)),
        LootResponse::TooFar => ("That corpse is out of reach.", Some(12390)),
        LootResponse::Normal | LootResponse::Other(_) => ("You cannot loot that corpse.", None),
    }
}

/// The official client's line for coins taken from a corpse, which names
/// the coins (eqstr 12072).
const LOOT_COINS: u32 = 12072;

/// Coins taken from a corpse, in the official client's words where the
/// installation has them.
fn loot_coins(coins: eq_client_core::Coins, messages: Option<&Messages>) -> Said {
    let coins = coin_text(coins.total_copper());
    official(
        Some(LOOT_COINS),
        std::slice::from_ref(&coins),
        &format!("The corpse gave you {coins}."),
        messages,
    )
}

/// Why a corpse could not be looted, in the official client's words where
/// it has its own and the installation has them.
fn loot_line(response: LootResponse, messages: Option<&Messages>) -> Said {
    let (reason, string_id) = loot_refusal(response);
    official(string_id, &[], reason, messages)
}

/// The official client's own words for a refusal or other notice, with
/// what they name, where the session names its string and the installation
/// has it; the session's words otherwise.
fn official(
    string_id: Option<u32>,
    arguments: &[String],
    reason: &str,
    messages: Option<&Messages>,
) -> Said {
    match (string_id, messages) {
        (Some(id), Some(messages)) => messages.said_or(id, arguments, reason),
        _ => Said::own(reason),
    }
}

/// What a bandaging's start or end says: the official client's words where
/// the installation has them.
fn bind_wound(update: &BindWoundUpdate, messages: Option<&Messages>) -> Option<Said> {
    Some(match update {
        BindWoundUpdate::Started { target: None } => official(
            Some(strings::STARTED_ON_SELF),
            &[],
            "You start bandaging yourself.",
            messages,
        ),
        BindWoundUpdate::Started { target: Some(name) } => official(
            Some(strings::STARTED_ON_OTHER),
            std::slice::from_ref(name),
            &format!("You start bandaging {name}."),
            messages,
        ),
        BindWoundUpdate::Ended(end) => official(Some(end.string_id()), &[], end.text(), messages),
        BindWoundUpdate::Unlocked => return None,
    })
}

/// Lines said in the chat.
fn in_chat(lines: Vec<Said>) -> Vec<(Place, Said)> {
    lines.into_iter().map(|line| (Place::Chat, line)).collect()
}

/// How a notice reads, and where each part shows.
#[allow(
    clippy::too_many_lines,
    reason = "a dispatch table: one arm per kind of notice, each a call"
)]
pub(super) fn wording(notice: &Notice, messages: Option<&Messages>) -> Vec<(Place, Said)> {
    let chat = |said: Said| vec![(Place::Chat, said)];
    let status = |text: String| vec![(Place::Status, Said::own(text))];
    match notice {
        Notice::Connection { link, dead } => status(connection_text(*link, *dead)),
        Notice::ServerString { id, arguments, .. } => chat(messages.map_or_else(
            || Said::own(format!("Server message {id}")),
            |messages| messages.said(*id, arguments),
        )),
        Notice::Consideration {
            consideration,
            name,
        } => messages.map_or_else(Vec::new, |messages| {
            let name = name.as_deref().map(display_name).unwrap_or_default();
            chat(consideration_text(messages, &name, consideration))
        }),
        Notice::Damage {
            damage,
            own_id,
            source,
            target,
        } => messages
            .and_then(|messages| {
                damage_text(
                    messages,
                    *own_id,
                    |id| {
                        let name = if id == damage.source_id {
                            source
                        } else {
                            target
                        };
                        name.as_deref()
                            .map_or_else(|| "someone".to_owned(), display_name)
                    },
                    damage,
                )
            })
            .map_or_else(Vec::new, chat),
        Notice::Heading(point) => messages
            .map(|messages| heading(*point, messages))
            .map_or_else(Vec::new, chat),
        Notice::Camp(camp) => match camp {
            CampStatus::Preparing => messages.map(|messages| messages.said(12293, &[])),
            CampStatus::Abandoned => messages.map(|messages| messages.said(12290, &[])),
            CampStatus::LoggingOut => Some("Logging out...".into()),
            CampStatus::Camped => None,
            CampStatus::Rejected(reason) => Some(reason.as_str().into()),
        }
        .map_or_else(Vec::new, chat),
        Notice::LootCoins(coins) => chat(loot_coins(*coins, messages)),
        Notice::LootRefused(response) => chat(loot_line(*response, messages)),
        Notice::ItemRefused => chat("You cannot take that item.".into()),
        Notice::ShopRefused => chat("That merchant will not trade with you.".into()),
        Notice::AbilityRefused {
            reason,
            string_id,
            arguments,
        }
        | Notice::RaidRefused {
            reason,
            string_id,
            arguments,
        } => chat(official(*string_id, arguments, reason, messages)),
        Notice::ConsumeRefused { reason, string_id }
        | Notice::CorpseRefused { reason, string_id }
        | Notice::PetRefused { reason, string_id }
        | Notice::CombineRefused { reason, string_id }
        | Notice::GroupRefused { reason, string_id }
        | Notice::ListingRefused { reason, string_id }
        | Notice::SocialRefused { reason, string_id } => {
            chat(official(*string_id, &[], reason, messages))
        }
        Notice::BindWound(update) => bind_wound(update, messages).map_or_else(Vec::new, chat),
        Notice::TradeRefused(reason)
        | Notice::TrainingRefused(reason)
        | Notice::ResurrectionRefused(reason)
        | Notice::ReadRefused(reason) => chat(reason.as_str().into()),
        // eqstr 1406, where the installation has it.
        Notice::ContainerInUse => chat(official(Some(1406), &[], "That is in use.", messages)),
        Notice::SkillUp { skill, value } => chat(skill_up(*skill, *value, messages)),
        Notice::Consent { consent, own } => chat(consent_line(consent, *own, messages)),
        Notice::WhoList(list) => who_lines(list, messages),
        Notice::NothingToEat { food, water } => in_chat(nothing_to_eat(*food, *water, messages)),
        // "You are too far away to trade."
        Notice::GiveRefused(reason) | Notice::GroundRefused(reason) => {
            chat(super::ground::refusal(reason).into())
        }
        // A click sent says nothing, as in the official client; a refused
        // one says why.
        Notice::Door { error, .. } => error
            .as_deref()
            .map_or_else(Vec::new, |error| chat(error.into())),
        // The player stays in the zone, so the status line no longer says
        // they are zoning.
        Notice::TransferRefused(reason) => [
            status(connection_text(Link::Connected, false)),
            chat(reason.to_string().into()),
        ]
        .concat(),
        Notice::ZoneLineRefused(reason) => chat(format!("Cannot cross zone line: {reason}").into()),
        Notice::TargetRefused(reason) => chat(format!("Target rejected: {reason}").into()),
        Notice::CastRefused { spell_id, reason } => {
            chat(format!("Cast rejected (spell {spell_id}): {reason}").into())
        }
        // The official client says why in the chat; the HUD keeps it on its
        // casting label as well.
        Notice::CastInterrupted { string_id } => chat(own_interruption(*string_id, messages)),
        Notice::Slain { victim, killer } => chat(slain(victim, killer, messages)),
        Notice::OtherCastInterrupted { string_id, caster } => {
            chat(other_interruption(*string_id, caster, messages))
        }
        Notice::Group(notice) => in_chat(group_lines(notice, messages)),
        Notice::Listing(notice) => chat(listing_line(*notice, messages)),
        Notice::Roll(roll) => in_chat(roll_lines(roll, messages)),
        Notice::Raid(notice) => in_chat(raid_lines(notice, messages)),
    }
}

/// News of the player's raid, in the installed client's strings for it:
/// 5064 for the player inviting someone; 5065 and then 5067 for an
/// invitation to the player; 5079 for agreeing to join and 5080 for
/// declining; 5061 for forming a raid; 5059 and 5083 for someone else and
/// the player joining, 5063 and 5062 for leaving, and 5070 and 5069 for
/// becoming the leader; and 5071 for the raid ending. This client's words
/// without the strings. That the official client says these lines at these
/// moments is inferred from what each says, since the server sends none of
/// them.
fn raid_lines(notice: &RaidNotice, messages: Option<&Messages>) -> Vec<Said> {
    let line = |id, name: Option<&String>, fallback: &str| {
        let arguments = name.map(std::slice::from_ref).unwrap_or_default();
        official(Some(id), arguments, fallback, messages)
    };
    match notice {
        RaidNotice::Inviting(name) => vec![line(
            5064,
            Some(name),
            &format!("You asked {name} to join your raid."),
        )],
        RaidNotice::Invited(name) => vec![
            line(
                5065,
                Some(name),
                &format!("{name} asks you to join a raid."),
            ),
            line(
                5067,
                None,
                "Type /raidaccept to join, or /raiddecline to say no.",
            ),
        ],
        RaidNotice::Accepting(name) => vec![line(
            5079,
            Some(name),
            &format!("You tell {name} you will join the raid."),
        )],
        RaidNotice::Declining(name) => vec![line(
            5080,
            Some(name),
            &format!("You turn down {name}'s raid."),
        )],
        RaidNotice::Formed => vec![line(5061, None, "Your raid is formed.")],
        RaidNotice::Joined(Party::Named(name)) => {
            vec![line(
                5059,
                Some(name),
                &format!("{name} is in the raid now."),
            )]
        }
        RaidNotice::Joined(_) => vec![line(5083, None, "You are in the raid now.")],
        RaidNotice::Left(Party::Named(name)) => {
            vec![line(
                5063,
                Some(name),
                &format!("{name} is out of the raid."),
            )]
        }
        RaidNotice::Left(_) => vec![line(5062, None, "You are out of the raid.")],
        RaidNotice::Leader(Party::Named(name)) => {
            vec![line(
                5070,
                Some(name),
                &format!("{name} leads the raid now."),
            )]
        }
        RaidNotice::Leader(_) => vec![line(5069, None, "You lead the raid now.")],
        RaidNotice::Disbanded => vec![line(5071, None, "The raid has ended.")],
        RaidNotice::Locked(true) => vec![line(
            8870,
            None,
            "Your raid is locked; invitations wait until it is unlocked.",
        )],
        RaidNotice::Locked(false) => vec![line(8871, None, "Your raid is unlocked.")],
    }
}

/// A die rolled nearby, in the installed client's strings for it: 1086
/// for who rolled it, then 1087 for its range and what it turned up; this
/// client's words without them. That the official client says these, so,
/// is inferred from what each says.
fn roll_lines(roll: &eq_client_core::socials::Roll, messages: Option<&Messages>) -> Vec<Said> {
    let range = [
        roll.low.to_string(),
        roll.high.to_string(),
        roll.result.to_string(),
    ];
    vec![
        official(
            Some(1086),
            std::slice::from_ref(&roll.name),
            &format!("{} rolls a die.", roll.name),
            messages,
        ),
        official(
            Some(1087),
            &range,
            &format!(
                "Rolling {} to {}, it came up {}.",
                range[0], range[1], range[2]
            ),
            messages,
        ),
    ]
}

/// The player's own listing changing, in the installed client's strings
/// for it: 13199 and 13200 for away and back, 13236 and 13237 for anonymous
/// and no longer, and 13233 and 13234 for roleplaying and no longer; this
/// client's words without them. That the official client says these, and
/// not the copies of the anonymous lines at 12034 and 12035, is inferred.
fn listing_line(notice: ListingNotice, messages: Option<&Messages>) -> Said {
    let (id, fallback) = match notice {
        ListingNotice::Away(true) => (13199, "You are away from the keyboard now."),
        ListingNotice::Away(false) => (13200, "You are back at the keyboard."),
        ListingNotice::Anonymous(true) => (13236, "You hide from /who now."),
        ListingNotice::Anonymous(false) => (13237, "You show in /who again."),
        ListingNotice::Roleplaying(true) => (13233, "You are in character now."),
        ListingNotice::Roleplaying(false) => (13234, "You are out of character again."),
    };
    official(Some(id), &[], fallback, messages)
}

/// News of the player's group, in the installed client's strings for it:
/// 12273 for the player inviting someone; 12280 and then 12281 for an
/// invitation to the player; 12283 for agreeing to join and 12289 for
/// declining; 12266 for the one invited declining; 12003 for forming a
/// group; 1399 and 12004 for someone else and the player joining, 12005
/// and 12001 for leaving, and 5041 and 5040 for a change of leader; and
/// 12002 for the group disbanding. This client's words without the
/// strings. That the official client says these lines at these moments is
/// inferred from what each says, since the server sends none of them.
fn group_lines(notice: &GroupNotice, messages: Option<&Messages>) -> Vec<Said> {
    let line = |id, name: Option<&String>, fallback: &str| {
        let arguments = name.map(std::slice::from_ref).unwrap_or_default();
        official(Some(id), arguments, fallback, messages)
    };
    match notice {
        GroupNotice::Inviting(name) => vec![line(
            12273,
            Some(name),
            &format!("You asked {name} to join your group."),
        )],
        GroupNotice::Invited(name) => vec![
            line(
                12280,
                Some(name),
                &format!("{name} asks you to join a group."),
            ),
            line(12281, None, "Type /follow to join, or /disband to say no."),
        ],
        GroupNotice::Following(name) => {
            vec![line(
                12283,
                Some(name),
                &format!("You tell {name} you will join."),
            )]
        }
        GroupNotice::Declining(name) => vec![line(
            12289,
            Some(name),
            &format!("You turn down {name}'s invitation."),
        )],
        GroupNotice::Declined(name) => vec![line(
            12266,
            Some(name),
            &format!("{name} turned down your invitation."),
        )],
        GroupNotice::Formed => vec![line(12003, None, "Your group is formed.")],
        GroupNotice::Joined(Party::Named(name)) => {
            vec![line(1399, Some(name), &format!("{name} joined the group."))]
        }
        GroupNotice::Joined(_) => vec![line(12004, None, "You are in the group now.")],
        GroupNotice::Left(Party::Named(name)) => {
            vec![line(12005, Some(name), &format!("{name} left the group."))]
        }
        GroupNotice::Left(_) => vec![line(12001, None, "You are out of the group.")],
        GroupNotice::Leader(Party::Named(name)) => {
            vec![line(
                5041,
                Some(name),
                &format!("{name} leads the group now."),
            )]
        }
        GroupNotice::Leader(_) => vec![line(5040, None, "You lead the group now.")],
        GroupNotice::Disbanded => vec![line(12002, None, "The group has disbanded.")],
    }
}

/// A death the player saw, in the installed client's string for it: 12107
/// for the player slain by someone and 12108 by no one they can name, 12113
/// for someone the player slew, 12114 for someone slain by someone else
/// and 12115 by no one the player can name; this client's words without
/// the string. That the official client words a death notice so is
/// inferred from what each string says.
fn slain(victim: &Party, killer: &Party, messages: Option<&Messages>) -> Said {
    let (id, arguments, fallback) = match (victim, killer) {
        (Party::Player, Party::Named(killer)) => (
            12107,
            vec![killer.clone()],
            format!("You died at the hands of {killer}."),
        ),
        (Party::Player, _) => (12108, Vec::new(), "You died.".to_owned()),
        (victim, Party::Player) => {
            let victim = party_name(victim);
            (12113, vec![victim.clone()], format!("You killed {victim}."))
        }
        (victim, Party::Named(killer)) => {
            let victim = party_name(victim);
            (
                12114,
                vec![victim.clone(), killer.clone()],
                format!("{victim} died at the hands of {killer}."),
            )
        }
        (victim, Party::Unseen) => {
            let victim = party_name(victim);
            (12115, vec![victim.clone()], format!("{victim} died."))
        }
    };
    official(Some(id), &arguments, &fallback, messages)
}

/// A party's name in a death line; the world never names an unseen victim,
/// so it reads as no one in particular.
fn party_name(party: &Party) -> String {
    match party {
        Party::Named(name) => name.clone(),
        _ => "someone".to_owned(),
    }
}

/// The player's own interruption: the server's string for why, or this
/// client's words without it.
fn own_interruption(string_id: u32, messages: Option<&Messages>) -> Said {
    match messages {
        Some(messages) => messages.interruption(string_id),
        None => Messages::default().interruption(string_id),
    }
}

/// Another caster's interruption: the server's string with the name it sent
/// in the string's place for it, or this client's words without the string.
/// Which chat filter and colour the official client gives it is not
/// checked.
fn other_interruption(string_id: u32, caster: &String, messages: Option<&Messages>) -> Said {
    let fallback = format!("{caster}: casting interrupted (server reason {string_id})");
    messages.map_or_else(
        || Said::own(fallback.clone()),
        |messages| messages.said_or(string_id, std::slice::from_ref(caster), &fallback),
    )
}

/// The line as a skill rises: the official client's (eqstr 12091), with its
/// own name for the skill where its string table has one, or this client's
/// words without the table.
fn skill_up(skill: u32, value: u32, messages: Option<&Messages>) -> Said {
    use eq_client_core::skills;
    let fallback = skills::name(skill).map_or_else(|| format!("Skill {skill}"), str::to_owned);
    let Some(messages) = messages else {
        return Said::own(format!("Your {fallback} skill rises to {value}."));
    };
    let name = skills::name_string(skill)
        .map_or_else(|| fallback.clone(), |id| messages.text(id, &fallback));
    messages.said(skills::BETTER_AT, &[name, value.to_string()])
}

/// The status line for where the connection stands; nothing while the
/// player is in the world.
const fn link_text(link: Link) -> &'static str {
    match link {
        Link::Offline | Link::Connected => "",
        Link::LoggingIn => "Logging in",
        Link::Entering => "Entering the world",
        Link::Zoning => "Zoning",
        Link::Ended => "Disconnected",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::Coins;

    #[test]
    fn loot_replies_say_what_the_corpse_gave_or_refused() {
        let line = |notice| wording(&notice, None);
        let coins = Coins {
            platinum: 0,
            gold: 1,
            silver: 0,
            copper: 2,
        };
        assert_eq!(
            line(Notice::LootCoins(coins)),
            [(Place::Chat, "The corpse gave you 1g 2c.".into())]
        );
        // With the installed strings, the official line names the coins.
        let messages = Messages::parse("EQST0002\n0 1\n12072 Took %1.\n");
        assert_eq!(
            wording(&Notice::LootCoins(coins), Some(&messages)),
            [(Place::Chat, Said::official("Took 1g 2c."))]
        );
        assert_eq!(
            line(Notice::ItemRefused),
            [(Place::Chat, "You cannot take that item.".into())]
        );
        assert_eq!(
            line(Notice::LootRefused(LootResponse::TooFar)),
            [(Place::Chat, "That corpse is out of reach.".into())]
        );
        // With the installed strings, each refusal the official client has
        // its own string for reads in it.
        let refusals = Messages::parse(
            "EQST0002
0 4
12073 Busy corpse.
12070 Not yet.
5154 Hostile near.
12390 Too far.
",
        );
        for (response, said) in [
            (LootResponse::SomeoneElse, "Busy corpse."),
            (LootResponse::NotAtThisTime, "Not yet."),
            (LootResponse::Hostiles, "Hostile near."),
            (LootResponse::TooFar, "Too far."),
        ] {
            assert_eq!(
                wording(&Notice::LootRefused(response), Some(&refusals)),
                [(Place::Chat, Said::official(said))]
            );
        }
        // An answer it has no string for stays in this client's words.
        assert_eq!(
            wording(
                &Notice::LootRefused(LootResponse::Other(9)),
                Some(&refusals)
            ),
            [(Place::Chat, "You cannot loot that corpse.".into())]
        );
        assert_eq!(
            line(Notice::ShopRefused),
            [(Place::Chat, "That merchant will not trade with you.".into())]
        );
    }

    #[test]
    fn bandaging_speaks_in_the_installed_words_with_whom_it_names() {
        use eq_client_core::bind_wound::{BindWoundEnd, BindWoundUpdate};
        let messages = Messages::parse(
            "EQST0002
0 3
420 Closer to %1, please.
1436 Moved, failed.
12437 Wrapping %1.
",
        );
        let started = Notice::BindWound(BindWoundUpdate::Started {
            target: Some("Firiona".into()),
        });
        assert_eq!(
            wording(&started, Some(&messages)),
            [(Place::Chat, Said::official("Wrapping Firiona."))]
        );
        assert_eq!(
            wording(&started, None),
            [(Place::Chat, "You start bandaging Firiona.".into())]
        );
        let ended = Notice::BindWound(BindWoundUpdate::Ended(BindWoundEnd::YouMoved));
        assert_eq!(
            wording(&ended, Some(&messages)),
            [(Place::Chat, Said::official("Moved, failed."))]
        );
        // A string the installation lacks falls back to the session's words.
        let complete = Notice::BindWound(BindWoundUpdate::Ended(BindWoundEnd::Complete));
        assert_eq!(
            wording(&complete, Some(&messages)),
            [(Place::Chat, BindWoundEnd::Complete.text().into())]
        );
        assert_eq!(
            wording(
                &Notice::BindWound(BindWoundUpdate::Unlocked),
                Some(&messages)
            ),
            []
        );
        let far = Notice::AbilityRefused {
            reason: "Firiona is too far away to bandage".into(),
            string_id: Some(420),
            arguments: vec!["Firiona".into()],
        };
        assert_eq!(
            wording(&far, Some(&messages)),
            [(Place::Chat, Said::official("Closer to Firiona, please."))]
        );
    }

    #[test]
    fn a_refused_combine_speaks_in_the_installed_words_when_it_can() {
        let notice = Notice::CombineRefused {
            reason: "Empty the cursor first.".into(),
            string_id: Some(12024),
        };
        let messages = Messages::parse(
            "EQST0002
0 1
12024 Hands full.
",
        );
        assert_eq!(
            wording(&notice, Some(&messages)),
            [(Place::Chat, Said::official("Hands full."))]
        );
        assert_eq!(
            wording(&notice, None),
            [(Place::Chat, "Empty the cursor first.".into())]
        );
    }

    #[test]
    fn an_interrupted_cast_says_why_in_the_chat() {
        let notice = |string_id| Notice::CastInterrupted { string_id };
        let messages = Messages::parse(
            "EQST0002
0 2
73 Synthetic interruption.
74 %1 interrupted.
",
        );
        assert_eq!(
            wording(&notice(73), Some(&messages)),
            [(Place::Chat, Said::official("Synthetic interruption."))]
        );
        // Without the string, or with one that needs what the server never
        // sends, the client says it in its own words.
        for (id, messages) in [(74, Some(&messages)), (73, None)] {
            assert_eq!(
                wording(&notice(id), messages),
                [(
                    Place::Chat,
                    Said::own(format!("Casting interrupted (server reason {id})"))
                )]
            );
        }
    }

    #[test]
    fn a_death_in_view_reads_in_the_installed_strings_for_whom_it_names() {
        let messages = Messages::parse(
            "EQST0002
0 5
12107 Felled by %1.
12108 Felled.
12113 You felled %1.
12114 %1 felled by %2.
12115 %1 felled.
",
        );
        let (player, bear, rat) = (
            Party::Player,
            Party::Named("a bear".into()),
            Party::Named("a rat".into()),
        );
        for ((victim, killer), official, own) in [
            (
                (&player, &bear),
                "Felled by a bear.",
                "You died at the hands of a bear.",
            ),
            ((&player, &Party::Unseen), "Felled.", "You died."),
            ((&rat, &player), "You felled a rat.", "You killed a rat."),
            (
                (&rat, &bear),
                "a rat felled by a bear.",
                "a rat died at the hands of a bear.",
            ),
            ((&rat, &Party::Unseen), "a rat felled.", "a rat died."),
        ] {
            let notice = Notice::Slain {
                victim: victim.clone(),
                killer: killer.clone(),
            };
            assert_eq!(
                wording(&notice, Some(&messages)),
                [(Place::Chat, Said::official(official))]
            );
            assert_eq!(wording(&notice, None), [(Place::Chat, own.into())]);
        }
    }

    #[test]
    fn group_news_reads_in_the_installed_strings_for_whom_it_names() {
        let messages = Messages::parse(
            "EQST0002
0 6
1399 %1 is in.
12001 Out.
12004 In.
12005 %1 is out.
12280 From %1.
12281 Answer.
",
        );
        let lines = |notice: GroupNotice, messages| {
            wording(&Notice::Group(notice), messages)
                .into_iter()
                .map(|(place, said)| {
                    assert_eq!(place, Place::Chat);
                    said
                })
                .collect::<Vec<_>>()
        };
        let friend = || Party::Named("Friend".into());
        for (notice, official, own) in [
            (
                GroupNotice::Joined(friend()),
                "Friend is in.",
                "Friend joined the group.",
            ),
            (
                GroupNotice::Joined(Party::Player),
                "In.",
                "You are in the group now.",
            ),
            (
                GroupNotice::Left(friend()),
                "Friend is out.",
                "Friend left the group.",
            ),
            (
                GroupNotice::Left(Party::Player),
                "Out.",
                "You are out of the group.",
            ),
        ] {
            assert_eq!(
                lines(notice.clone(), Some(&messages)),
                [Said::official(official)]
            );
            assert_eq!(lines(notice, None), [Said::own(own)]);
        }
        // An invitation says who it is from, then how to answer it.
        assert_eq!(
            lines(GroupNotice::Invited("Friend".into()), Some(&messages)),
            [Said::official("From Friend."), Said::official("Answer.")]
        );
        // A string the installation lacks reads in this client's words.
        assert_eq!(
            lines(GroupNotice::Disbanded, Some(&messages)),
            [Said::own("The group has disbanded.")]
        );
    }

    #[test]
    fn raid_news_reads_in_the_installed_strings() {
        let messages = Messages::parse(
            "EQST0002
0 3
5059 %1 in.
5065 From %1.
5067 Answer.
",
        );
        let said = |notice| wording(&Notice::Raid(notice), Some(&messages));
        assert_eq!(
            said(RaidNotice::Joined(Party::Named("Friend".into()))),
            [(Place::Chat, Said::official("Friend in."))]
        );
        assert_eq!(
            said(RaidNotice::Invited("Friend".into())),
            [
                (Place::Chat, Said::official("From Friend.")),
                (Place::Chat, Said::official("Answer.")),
            ]
        );
        // A string the installation lacks reads in this client's words.
        assert_eq!(
            said(RaidNotice::Disbanded),
            [(Place::Chat, Said::own("The raid has ended."))]
        );
    }

    #[test]
    fn a_roll_reads_in_the_installed_strings() {
        let messages = Messages::parse(
            "EQST0002
0 2
1086 Die by %1.
1087 From %1 to %2: %3.
",
        );
        let roll = eq_client_core::socials::Roll {
            name: "Friend".into(),
            low: 1,
            high: 6,
            result: 4,
        };
        assert_eq!(
            wording(&Notice::Roll(roll.clone()), Some(&messages)),
            [
                (Place::Chat, Said::official("Die by Friend.")),
                (Place::Chat, Said::official("From 1 to 6: 4.")),
            ]
        );
        assert_eq!(
            wording(&Notice::Roll(roll), None),
            [
                (Place::Chat, Said::own("Friend rolls a die.")),
                (Place::Chat, Said::own("Rolling 1 to 6, it came up 4.")),
            ]
        );
    }

    #[test]
    fn the_players_listing_reads_in_the_installed_strings() {
        let messages = Messages::parse(
            "EQST0002
0 2
13199 Gone.
13234 In.
",
        );
        let said = |notice| wording(&Notice::Listing(notice), Some(&messages));
        assert_eq!(
            said(ListingNotice::Away(true)),
            [(Place::Chat, Said::official("Gone."))]
        );
        assert_eq!(
            said(ListingNotice::Roleplaying(false)),
            [(Place::Chat, Said::official("In."))]
        );
        // A string the installation lacks reads in this client's words.
        assert_eq!(
            said(ListingNotice::Away(false)),
            [(Place::Chat, Said::own("You are back at the keyboard."))]
        );
    }

    #[test]
    fn another_casters_interruption_names_them_in_the_servers_string() {
        let notice = |string_id| Notice::OtherCastInterrupted {
            string_id,
            caster: "Examplar".into(),
        };
        let messages = Messages::parse(
            "EQST0002
0 1
74 %1 stopped short.
",
        );
        assert_eq!(
            wording(&notice(74), Some(&messages)),
            [(Place::Chat, Said::official("Examplar stopped short."))]
        );
        // Without the string, the client says it in its own words.
        for (id, messages) in [(75, Some(&messages)), (74, None)] {
            assert_eq!(
                wording(&notice(id), messages),
                [(
                    Place::Chat,
                    Said::own(format!(
                        "Examplar: casting interrupted (server reason {id})"
                    ))
                )]
            );
        }
    }

    #[test]
    fn hunger_says_what_was_missing_and_what_was_kept_back() {
        let messages = Messages::parse(
            "EQST0002
0 3
12488 Out of food.
12490 Out of drink.
12491 Out of both.
",
        );
        let lines = |food, water| -> Vec<String> {
            wording(&Notice::NothingToEat { food, water }, Some(&messages))
                .into_iter()
                .map(|(place, line)| {
                    assert_eq!(place, Place::Chat);
                    line.text
                })
                .collect()
        };
        assert_eq!(
            lines(Some(Shortage::Nothing), Some(Shortage::Nothing)),
            ["Out of both."]
        );
        assert_eq!(
            lines(Some(Shortage::OnlyModified), Some(Shortage::Nothing)),
            [
                "Out of drink.",
                concat!(
                    "You need food, but yours has modifiers, so it is only eaten when ",
                    "you right-click it."
                )
            ]
        );
        assert_eq!(
            lines(None, Some(Shortage::OnlyModified)),
            [concat!(
                "You need a drink, but yours has modifiers, so it is only drunk when ",
                "you right-click it."
            )]
        );
        // Without the installed strings, only what was kept back is said.
        assert_eq!(
            wording(
                &Notice::NothingToEat {
                    food: Some(Shortage::Nothing),
                    water: None
                },
                None
            ),
            []
        );
    }

    #[test]
    fn consents_read_for_the_owner_and_the_one_consented() {
        let messages = Messages::parse(
            "EQST0002\n0 4\n\
             1427 %1 may now drag your corpse in %2.\n\
             1428 %1 may no longer drag your corpse in %2.\n\
             2080 You may now drag %1's corpse in %2.\n\
             2103 You may no longer drag %1's corpse in %2.\n",
        );
        let line = |given, own| {
            wording(
                &Notice::Consent {
                    consent: eq_client_core::corpses::Consent {
                        granted: "Helper".into(),
                        owner: "Owner".into(),
                        given,
                        zone: "The Qeynos Hills".into(),
                    },
                    own,
                },
                Some(&messages),
            )
        };
        let chat = |text: &str| vec![(Place::Chat, Said::official(text))];
        assert_eq!(
            line(true, true),
            chat("Helper may now drag your corpse in The Qeynos Hills.")
        );
        assert_eq!(
            line(false, true),
            chat("Helper may no longer drag your corpse in The Qeynos Hills.")
        );
        assert_eq!(
            line(true, false),
            chat("You may now drag Owner's corpse in The Qeynos Hills.")
        );
        assert_eq!(
            line(false, false),
            chat("You may no longer drag Owner's corpse in The Qeynos Hills.")
        );
    }

    #[test]
    fn refusals_are_said_in_the_chat() {
        assert_eq!(
            wording(&Notice::TargetRefused("Too far away".into()), None),
            [(Place::Chat, "Target rejected: Too far away".into())]
        );
        // A door click sent says nothing; a refused one says why.
        let door = |error: Option<&str>| {
            wording(
                &Notice::Door {
                    door_id: 4,
                    error: error.map(str::to_owned),
                },
                None,
            )
        };
        assert_eq!(door(None), []);
        assert_eq!(door(Some("Locked")), [(Place::Chat, "Locked".into())]);
        let transfer = wording(
            &Notice::TransferRefused(eq_client_core::ZoneRejection::Cancelled),
            None,
        );
        assert_eq!(
            transfer,
            [
                (Place::Status, "".into()),
                (
                    Place::Chat,
                    eq_client_core::ZoneRejection::Cancelled.to_string().into()
                )
            ]
        );
        // Without the installed strings, a server string still says which it was.
        assert_eq!(
            wording(
                &Notice::ServerString {
                    id: 12293,
                    arguments: Vec::new(),
                    message_type: None,
                },
                None
            ),
            [(Place::Chat, "Server message 12293".into())]
        );
        assert_eq!(wording(&Notice::Camp(CampStatus::Camped), None), []);
    }
}
