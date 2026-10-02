//! Words the world's notices: what each says to the player and where it
//! shows. The server's strings come from the installed client's table.
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
    world::{Link, Notice},
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
fn heading(point: u8, messages: &Messages) -> String {
    let point = messages.format(12427 + u32::from(point), &[]);
    messages.format(12435, &[point])
}

/// What a hungry or thirsty player hears: the official client's words for
/// food or drink it could not find, and what was left for them to eat or
/// drink by hand because it has modifiers.
fn nothing_to_eat(
    food: Option<Shortage>,
    water: Option<Shortage>,
    messages: Option<&Messages>,
) -> Vec<String> {
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
        .map(|(id, messages)| messages.format(id, &[]))
        .into_iter()
        .chain(kept.map(String::from))
        .collect()
}

/// A `/who all` answer's lines; without the installed strings, each still
/// says which string it is.
fn who_lines(
    list: &eq_client_core::who::WhoList,
    messages: Option<&Messages>,
) -> Vec<(Place, String)> {
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
) -> String {
    let (id, who) = match (own, consent.given) {
        (true, true) => (1427, &consent.granted),
        (true, false) => (1428, &consent.granted),
        (false, true) => (2080, &consent.owner),
        (false, false) => (2103, &consent.owner),
    };
    messages
        .unwrap_or(&Messages::default())
        .format(id, &[who.clone(), consent.zone.clone()])
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
/// client's string where it has its own (eqstr 12390 for a corpse too far
/// away).
const fn loot_refusal(response: LootResponse) -> (&'static str, Option<u32>) {
    match response {
        LootResponse::SomeoneElse => ("Someone else is looting that corpse.", None),
        LootResponse::NotAtThisTime => ("You cannot loot that corpse at this time.", None),
        LootResponse::Hostiles => ("You cannot loot while a hostile is nearby.", None),
        LootResponse::TooFar => ("That corpse is out of reach.", Some(12390)),
        LootResponse::Normal | LootResponse::Other(_) => ("You cannot loot that corpse.", None),
    }
}

/// The official client's line for coins taken from a corpse, which names
/// the coins (eqstr 12072).
const LOOT_COINS: u32 = 12072;

/// Coins taken from a corpse, in the official client's words where the
/// installation has them.
fn loot_coins(coins: eq_client_core::Coins, messages: Option<&Messages>) -> String {
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
fn loot_line(response: LootResponse, messages: Option<&Messages>) -> String {
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
) -> String {
    match (string_id, messages) {
        (Some(id), Some(messages)) => messages.official(id, arguments, reason),
        _ => reason.to_owned(),
    }
}

/// What a bandaging's start or end says: the official client's words where
/// the installation has them.
fn bind_wound(update: &BindWoundUpdate, messages: Option<&Messages>) -> Option<String> {
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

/// How a notice reads, and where each part shows.
pub(super) fn wording(notice: &Notice, messages: Option<&Messages>) -> Vec<(Place, String)> {
    let chat = |text: String| vec![(Place::Chat, text)];
    match notice {
        Notice::Connection { link, dead } => vec![(Place::Status, connection_text(*link, *dead))],
        Notice::ServerString { id, arguments } => chat(messages.map_or_else(
            || format!("Server message {id}"),
            |messages| messages.format(*id, arguments),
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
        Notice::Camp(status) => match status {
            CampStatus::Preparing => messages.map(|messages| messages.format(12293, &[])),
            CampStatus::Abandoned => messages.map(|messages| messages.format(12290, &[])),
            CampStatus::LoggingOut => Some("Logging out...".into()),
            CampStatus::Camped => None,
            CampStatus::Rejected(reason) => Some(reason.clone()),
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
        } => chat(official(*string_id, arguments, reason, messages)),
        Notice::ConsumeRefused { reason, string_id }
        | Notice::CorpseRefused { reason, string_id }
        | Notice::PetRefused { reason, string_id }
        | Notice::CombineRefused { reason, string_id } => {
            chat(official(*string_id, &[], reason, messages))
        }
        Notice::BindWound(update) => bind_wound(update, messages).map_or_else(Vec::new, chat),
        Notice::TradeRefused(reason)
        | Notice::TrainingRefused(reason)
        | Notice::ResurrectionRefused(reason)
        | Notice::ReadRefused(reason) => chat(reason.clone()),
        // eqstr 1406, where the installation has it.
        Notice::ContainerInUse => chat(official(Some(1406), &[], "That is in use.", messages)),
        Notice::SkillUp { skill, value } => chat(skill_up(*skill, *value, messages)),
        Notice::Consent { consent, own } => chat(consent_line(consent, *own, messages)),
        Notice::WhoList(list) => who_lines(list, messages),
        Notice::NothingToEat { food, water } => nothing_to_eat(*food, *water, messages)
            .into_iter()
            .map(|line| (Place::Chat, line))
            .collect(),
        // "You are too far away to trade."
        Notice::GiveRefused(reason) | Notice::GroundRefused(reason) => {
            chat(super::ground::refusal(reason))
        }
        // A click sent says nothing, as in the official client; a refused
        // one says why.
        Notice::Door { error, .. } => error.clone().map_or_else(Vec::new, chat),
        // The player stays in the zone, so the status line no longer says
        // they are zoning.
        Notice::TransferRefused(reason) => vec![
            (Place::Status, connection_text(Link::Connected, false)),
            (Place::Chat, reason.to_string()),
        ],
        Notice::ZoneLineRefused(reason) => chat(format!("Cannot cross zone line: {reason}")),
        Notice::TargetRefused(reason) => chat(format!("Target rejected: {reason}")),
        Notice::CastRefused { spell_id, reason } => {
            chat(format!("Cast rejected (spell {spell_id}): {reason}"))
        }
    }
}

/// The line as a skill rises: the official client's (eqstr 12091), with its
/// own name for the skill where its string table has one, or this client's
/// words without the table.
fn skill_up(skill: u32, value: u32, messages: Option<&Messages>) -> String {
    use eq_client_core::skills;
    let fallback = skills::name(skill).map_or_else(|| format!("Skill {skill}"), str::to_owned);
    let Some(messages) = messages else {
        return format!("Your {fallback} skill rises to {value}.");
    };
    let name = skills::name_string(skill)
        .map_or_else(|| fallback.clone(), |id| messages.text(id, &fallback));
    messages.format(skills::BETTER_AT, &[name, value.to_string()])
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
            [(Place::Chat, "Took 1g 2c.".into())]
        );
        assert_eq!(
            line(Notice::ItemRefused),
            [(Place::Chat, "You cannot take that item.".into())]
        );
        assert_eq!(
            line(Notice::LootRefused(LootResponse::TooFar)),
            [(Place::Chat, "That corpse is out of reach.".into())]
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
            [(Place::Chat, "Wrapping Firiona.".into())]
        );
        assert_eq!(
            wording(&started, None),
            [(Place::Chat, "You start bandaging Firiona.".into())]
        );
        let ended = Notice::BindWound(BindWoundUpdate::Ended(BindWoundEnd::YouMoved));
        assert_eq!(
            wording(&ended, Some(&messages)),
            [(Place::Chat, "Moved, failed.".into())]
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
            [(Place::Chat, "Closer to Firiona, please.".into())]
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
            [(Place::Chat, "Hands full.".into())]
        );
        assert_eq!(
            wording(&notice, None),
            [(Place::Chat, "Empty the cursor first.".into())]
        );
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
                    line
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
        let chat = |text: &str| vec![(Place::Chat, text.to_owned())];
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
                (Place::Status, String::new()),
                (
                    Place::Chat,
                    eq_client_core::ZoneRejection::Cancelled.to_string()
                )
            ]
        );
        // Without the installed strings, a server string still says which it was.
        assert_eq!(
            wording(
                &Notice::ServerString {
                    id: 12293,
                    arguments: Vec::new()
                },
                None
            ),
            [(Place::Chat, "Server message 12293".into())]
        );
        assert_eq!(wording(&Notice::Camp(CampStatus::Camped), None), []);
    }
}
