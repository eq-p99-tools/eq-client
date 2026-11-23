//! Words the world's notices: what each says to the player and where it
//! shows. The server's strings come from the installed client's table.
use super::chat::Said;
use super::{
    combat::{consideration_text, damage_text},
    hud::messages::Messages,
    trade::coin_text,
};
use eq_client_core::chat::Source;
use eq_client_core::{
    CampStatus,
    bind_wound::{BindWoundUpdate, strings},
    creation::CreationOutcome,
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
    /// Movement prerequisites, shown while the session has no status to report.
    pub movement: Line,
}

impl Lines {
    /// One HUD line, with connection, death and camping taking priority over
    /// movement readiness. Keeping the lines separate lets readiness recover
    /// without erasing a newer session notice.
    pub(crate) fn hud_status(&self) -> &str {
        if self.status.text().is_empty() {
            self.movement.text()
        } else {
            self.status.text()
        }
    }
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

/// What the select screen says of a character not created. The world
/// says only which step it refused, never why, and the official client's
/// words for each are not known, so these are this client's (inferred,
/// item 6.278).
pub(crate) fn creation_refusal(name: &str, outcome: CreationOutcome) -> String {
    match outcome {
        CreationOutcome::NameRefused => format!("The server refused the name {name}."),
        CreationOutcome::CreationRefused => {
            format!("The server approved the name {name} but could not create the character.")
        }
        CreationOutcome::NotSent => {
            format!("{name} was not sent: this client cannot create that character on this server.")
        }
        CreationOutcome::Created => format!("{name} was created."),
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

/// One of the program's own lines, with each argument that is a line of
/// its own put in by its words, as a bearing names its side word. Where the
/// installation lacks the line or a word it names, the client's own words
/// say it all.
fn program_line(
    string_id: u32,
    arguments: &[String],
    lines: &[usize],
    said: &str,
    messages: Option<&Messages>,
) -> Said {
    let Some(messages) = messages else {
        return Said::own(said);
    };
    let mut named = arguments.to_vec();
    for index in lines {
        let word = named
            .get(*index)
            .and_then(|id| id.parse::<u32>().ok())
            .and_then(|id| messages.formatted(id, &[]));
        match (named.get_mut(*index), word) {
            (Some(argument), Some(word)) => *argument = word,
            _ => return Said::own(said),
        }
    }
    messages.said_or(string_id, &named, said)
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
        BindWoundUpdate::RequestedSelf => {
            Said::own("Binding your wounds was requested. Stay still while waiting for the server.")
        }
        BindWoundUpdate::Bandaged => Said::own("The server reported bandaging."),
        BindWoundUpdate::Released => {
            Said::own("The server released your bandage attempt without a result.")
        }
        BindWoundUpdate::Unconfirmed {
            stock_uncertain: true,
        } => Said::own(
            "The bandage result is unconfirmed. Wait for an inventory update before using the affected bandages.",
        ),
        BindWoundUpdate::Unconfirmed {
            stock_uncertain: false,
        } => Said::own(
            "Bandage stock updated. The server did not report a bandaging result in time.",
        ),
        BindWoundUpdate::Unlocked => return None,
    })
}

/// How a line of the chat server's channels reads. Every form is this
/// client's, inferred until read R49-12 b: a message names its sender, then
/// the channel and its slot in the last list of joined channels, or the
/// channel alone before a list names it. The server's words keep their item
/// links and print as any server chat does.
fn channel_line(line: &eq_client_core::world::ChannelLine) -> Said {
    use eq_client_core::chat::RichText;
    use eq_client_core::world::ChannelLine;
    match line {
        ChannelLine::Message {
            channel,
            slot,
            sender,
            text,
        } => {
            let head = match slot {
                Some(slot) => format!("{sender} tells {channel}:{slot}, '"),
                None => format!("{sender} tells {channel}, '"),
            };
            let mut words = RichText::from(format!("{head}{}'", text.text));
            words.insert_links(text, head.len());
            Said {
                words,
                source: Source::Server,
                message_type: None,
            }
        }
        ChannelLine::Notice(text) => Said {
            words: text.clone(),
            source: Source::Server,
            message_type: None,
        },
        ChannelLine::Member {
            channel,
            name,
            joined: true,
        } => Said::own(format!("{name} has entered channel {channel}.")),
        ChannelLine::Member {
            channel,
            name,
            joined: false,
        } => Said::own(format!("{name} has left channel {channel}.")),
        ChannelLine::Lost => Said::own("The chat server link has ended."),
    }
}

/// Lines said in the chat.
fn in_chat(lines: Vec<Said>) -> Vec<(Place, Said)> {
    lines.into_iter().map(|line| (Place::Chat, line)).collect()
}

/// The Titanium program's lines for environmental damage the player took:
/// after its own report the amount line in its colour (`eqgame.exe`
/// 0x41c905, read R49-3 e), then the record's line, built with the
/// you-form text, in the record's colour (0x4a56e3, read R49-3b).
fn environmental_lines(
    messages: &Messages,
    reported: Option<u32>,
    record: Option<(u32, u32)>,
) -> Vec<Said> {
    use eq_client_core::environment::{REPORT_COLOR, REPORT_LINE, YOU_TEXT};
    let amount = reported.map(|amount| {
        messages
            .said(REPORT_LINE, &[amount.to_string()])
            .typed(REPORT_COLOR)
    });
    let record = record.map(|(line, color)| {
        messages
            .said(line, &[messages.format(YOU_TEXT, &[])])
            .typed(color)
    });
    amount.into_iter().chain(record).collect()
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
        Notice::CreationRefused { name, outcome } => status(creation_refusal(name, *outcome)),
        Notice::ServerString { id, arguments, .. } => chat(
            messages
                .unwrap_or(&Messages::default())
                .linked(*id, arguments),
        ),
        Notice::Consideration {
            consideration,
            name,
            color,
        } => messages.map_or_else(Vec::new, |messages| {
            let name = name.as_deref().map(display_name).unwrap_or_default();
            chat(consideration_text(
                messages,
                &name,
                consideration.faction,
                *color,
            ))
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
        Notice::EnvironmentalDamage { reported, record } => messages
            .map(|messages| environmental_lines(messages, *reported, *record))
            .map_or_else(Vec::new, in_chat),
        Notice::Heading(point) => messages
            .map(|messages| heading(*point, messages))
            .map_or_else(Vec::new, chat),
        Notice::Camp(camp) => in_chat(camp_lines(camp, messages)),
        Notice::LootCoins(coins) => chat(loot_coins(*coins, messages)),
        Notice::LootRefused(response) => chat(loot_line(*response, messages)),
        Notice::ItemRefused => chat("You cannot take that item.".into()),
        Notice::ShopRefused => chat("That merchant will not trade with you.".into()),
        Notice::ProgramLine {
            string_id,
            arguments,
            lines,
            said,
        } => chat(program_line(*string_id, arguments, lines, said, messages)),
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
        | Notice::ItemUseRefused { reason, string_id }
        | Notice::CorpseRefused { reason, string_id }
        | Notice::PetRefused { reason, string_id }
        | Notice::CombineRefused { reason, string_id }
        | Notice::GroupRefused { reason, string_id }
        | Notice::GuildRefused { reason, string_id }
        | Notice::CoinsRefused { reason, string_id }
        | Notice::ListingRefused { reason, string_id }
        | Notice::SocialRefused { reason, string_id } => {
            chat(official(*string_id, &[], reason, messages))
        }
        // The official client always says where `/autosplit` left it (R8).
        Notice::AutoSplit(on) => chat(official(
            Some(if *on { 0x338d } else { 0x338e }),
            &[],
            if *on {
                "You will now share looted coins with your group."
            } else {
                "You will no longer share looted coins with your group."
            },
            messages,
        )),
        Notice::BindWound(update) => bind_wound(update, messages).map_or_else(Vec::new, chat),
        Notice::TradeRefused(reason)
        | Notice::MerchantUnanswered(reason)
        | Notice::CombatRefused(reason)
        | Notice::TrainingRefused(reason)
        | Notice::ResurrectionRefused(reason)
        | Notice::TranslocateRefused(reason)
        | Notice::PlayerInspectionRefused(reason)
        | Notice::ReadRefused(reason)
        | Notice::AlternateAdvancementRefused { reason, .. } => chat(reason.as_str().into()),
        Notice::TrainingPractice(update) => {
            use eq_client_core::training::PracticeUpdate;
            chat(match update {
                PracticeUpdate::Requested { estimated_cost, .. } => {
                    format!("Practice requested (estimated price: {estimated_cost} copper). Practice points are unknown until a fresh admission.").into()
                }
                PracticeUpdate::SkillRaised { .. } => {
                    "The skill increased. The practice outcome is still unknown.".into()
                }
                PracticeUpdate::Unanswered { .. } => {
                    "The practice outcome is unknown. Re-enter the zone to refresh practice points.".into()
                }
                PracticeUpdate::OutcomeUnknown { estimated_cost, .. } => {
                    format!("The language practice outcome is unknown (estimated price: {estimated_cost} copper). Re-enter the zone to refresh practice points and balances.").into()
                }
                PracticeUpdate::PointsUncertain => {
                    "Practice points are unknown after the level change. Re-enter the zone to refresh them.".into()
                }
            })
        }
        Notice::Channel(line) => chat(channel_line(line)),
        Notice::ChannelRefused { reason } => chat(Said::own(format!("{reason}."))),
        Notice::ConsentExpired(kind) => {
            use eq_client_core::confirmations::ConsentKind;
            chat(match kind {
                ConsentKind::Resurrection => "The resurrection offer expired.".into(),
                ConsentKind::Translocate => "The translocate offer expired.".into(),
                // A duel's question has no local deadline; worded all the same.
                ConsentKind::Duel => "The challenge to a duel expired.".into(),
            })
        }
        // Stock EQEmu says nothing to the player when it takes a petition
        // or a bug report, so these lines, this client's own, stand in for
        // the official client's, which are unread (6.511b). Feedback's
        // servers answer it themselves, so it gets none.
        Notice::ReportSent(report) => {
            use eq_client_core::reports::ReportKind;
            match report {
                ReportKind::Petition => chat(Said::own("Your petition was sent.")),
                ReportKind::Bug => chat(Said::own("Your bug report was sent.")),
                ReportKind::Feedback => Vec::new(),
            }
        }
        Notice::ReportRefused { reason } | Notice::ChatRefused { reason } => {
            chat(Said::own(reason.clone()))
        }
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
        // one says why, in the program's words where the session names
        // them (1462 out of reach).
        Notice::Door {
            error, string_id, ..
        } => error.as_deref().map_or_else(Vec::new, |error| {
            chat(official(*string_id, &[], error, messages))
        }),
        // The player stays in the zone, so the status line no longer says
        // they are zoning.
        Notice::TransferRefused(reason) => [
            status(connection_text(Link::Connected, false)),
            chat(reason.to_string().into()),
        ]
        .concat(),
        Notice::ZoneLineRefused(reason) => chat(format!("Cannot cross zone line: {reason}").into()),
        // eqstr 12357, naming the zone, where the installation has it.
        // Inferred: that the official client says it on every arrival,
        // Enter World included, as a system line.
        Notice::Arrived(zone) => chat(official(
            Some(12357),
            std::slice::from_ref(zone),
            &format!("Arrived in {zone}."),
            messages,
        )),
        Notice::TargetRefused(reason) => chat(format!("Target rejected: {reason}").into()),
        Notice::BuffCancellation(update) => {
            use eq_client_core::buffs::cancel::Progress;
            match &update.progress {
                Progress::Refused(reason) => {
                    chat(format!("Buff cancellation refused: {reason}").into())
                }
                Progress::Unresolved => chat(
                    "Buff cancellation is unconfirmed. No matching buff fade was received.".into(),
                ),
                Progress::PartiallySubmitted => chat(
                    "The first buff cancellation packet was sent. The outcome is unknown.".into(),
                ),
                Progress::Submitted | Progress::FadeObserved | Progress::Changed => Vec::new(),
            }
        }
        Notice::BuffSlotMismatch => {
            bevy::log::warn!("The server contradicted a modelled buff slot in this zone visit");
            Vec::new()
        }
        Notice::SongStopUnconfirmed { released_cast } => chat(
            if *released_cast {
                "Song stop unconfirmed. The previous casting hold was released."
            } else {
                "Song stop requested. Song state is unconfirmed."
            }
            .into(),
        ),
        Notice::CastRefused {
            reason,
            string_id: Some(string_id),
            ..
        } => chat(official(Some(*string_id), &[], reason, messages)),
        Notice::CastRefused {
            spell_id,
            reason,
            string_id: None,
        } => chat(format!("Cast rejected (spell {spell_id}): {reason}").into()),
        // The official client says why in the chat; the HUD keeps it on its
        // casting label as well.
        Notice::CastInterrupted { string_id } => chat(own_interruption(*string_id, messages)),
        Notice::Slain { victim, killer } => chat(slain(victim, killer, messages)),
        Notice::OtherCastInterrupted { string_id, caster } => {
            chat(other_interruption(*string_id, caster, messages))
        }
        Notice::Group(notice) => in_chat(group_lines(notice, messages)),
        Notice::Listing(notice) => chat(listing_line(*notice, messages)),
        Notice::ListingRequested(change) => {
            use eq_client_core::listing::{Anonymity, ListingChange};
            let status = match change {
                ListingChange::Away(true) => "AFK on",
                ListingChange::Away(false) => "AFK off",
                ListingChange::Anonymity(Anonymity::Open) => "Open listing",
                ListingChange::Anonymity(Anonymity::Anonymous) => "Anonymous listing",
                ListingChange::Anonymity(Anonymity::Roleplaying) => "Roleplaying listing",
                ListingChange::Anonymity(Anonymity::AnonymousRoleplaying) => {
                    "Anonymous roleplaying listing"
                }
                _ => return Vec::new(),
            };
            chat(format!("{status} requested.").into())
        }
        Notice::Roll(roll) => in_chat(roll_lines(roll, messages)),
        Notice::Raid(notice) => in_chat(raid_lines(notice, messages)),
        Notice::Duel(notice) => in_chat(duel_line(notice, messages).into_iter().collect()),
        Notice::YellForHelp { name, bearing } => chat(yell_line(name, *bearing, messages)),
        Notice::QuestReward { coins, giver } => {
            in_chat(quest_reward_lines(*coins, giver, messages))
        }
        Notice::LevelLost { lost, level } => chat(level_lost_line(*lost, *level, messages)),
        Notice::OthersStatChanged { lowered } => chat(others_stat_line(*lowered, messages)),
        Notice::Reward(coins) => chat(Said::own(reward_line(*coins))),
    }
}

/// News of a duel, in the program's line for it where the installation has
/// it and with the colour the program gives it (read R49-6 a, d, e and h):
/// colour 15 for the challenge, its start and `/duel`'s own, 0x111 for
/// `/duel` with no player or at the player, and 0x10d for a duel's end.
/// The fallbacks, and the TAKP refusals and the session's own, are this
/// client's words. A turn-down with a reason the program has no line for
/// says nothing.
fn duel_line(
    notice: &eq_client_core::world::DuelNotice,
    messages: Option<&Messages>,
) -> Option<Said> {
    use eq_client_core::{socials::DuelRefusal, world::DuelNotice};
    let line = |id: u32, arguments: &[&String], fallback: String, colour: u32| {
        let arguments: Vec<String> = arguments.iter().map(|name| (*name).clone()).collect();
        official(Some(id), &arguments, &fallback, messages).typed(colour)
    };
    Some(match notice {
        DuelNotice::Challenged(name) => line(
            0x592,
            &[name],
            format!("{name} challenges you to a duel."),
            15,
        ),
        DuelNotice::Begun(name) => {
            line(0x590, &[name], format!("Your duel with {name} begins."), 15)
        }
        DuelNotice::Refused { from, reason } => Said::own(match reason {
            DuelRefusal::Declined => format!("{from} declined your challenge."),
            DuelRefusal::Dueling => format!("{from} is already dueling."),
            DuelRefusal::Considering => format!("{from} is considering another challenge."),
            DuelRefusal::Other(_) => format!("{from} turned your challenge down."),
        }),
        DuelNotice::AnswerRefused(reason) => Said::own(format!("No answer was sent: {reason}")),
        DuelNotice::Challenging(name) => line(
            0x33c6,
            &[name],
            format!("You challenge {name} to a duel."),
            15,
        ),
        DuelNotice::Accepting(name) => line(
            0x33c4,
            &[name],
            format!("You accept {name}'s challenge to a duel."),
            15,
        ),
        DuelNotice::NoPlayerTarget => line(
            0x33c0,
            &[],
            "You must first target another player to duel.".to_owned(),
            0x111,
        ),
        DuelNotice::NotYourself => line(0x33c1, &[], "You cannot duel yourself.".to_owned(), 0x111),
        DuelNotice::AlreadyDueling { at_opponent: true } => {
            line(0x33c2, &[], "You are already dueling them.".to_owned(), 15)
        }
        DuelNotice::AlreadyDueling { at_opponent: false } => {
            line(0x33c3, &[], "You are already in a duel.".to_owned(), 15)
        }
        DuelNotice::ConsideringAnother => line(
            0x33c5,
            &[],
            "You are considering another challenge.".to_owned(),
            15,
        ),
        DuelNotice::Won { own, over } => line(
            0x2f44,
            &[own, over],
            format!("{own} has defeated {over} in a duel."),
            0x10d,
        ),
        DuelNotice::Ended(name) => line(
            0x2f45,
            &[name, name],
            format!("The duel with {name} is over."),
            0x10d,
        ),
        DuelNotice::TurnedDown { by, reason } => {
            let (id, fallback) = turned_down(by, (*reason)?);
            line(id, &[by], fallback, 15)
        }
        DuelNotice::Lapsed(name) => line(
            0x581,
            &[name],
            format!("{name} left; your challenge is withdrawn."),
            15,
        ),
        DuelNotice::Forfeited { own, opponent } => line(
            0x580,
            &[own, opponent, opponent],
            format!("{opponent} fled the duel; {own} wins."),
            0x10d,
        ),
    })
}

/// The line for a challenge the other program turned down, and this
/// client's words for it: 0x567 ignored, 0x568 dueling, 0x569 considering
/// another (read R49-6d).
fn turned_down(by: &str, reason: eq_client_core::socials::DuelAutoDecline) -> (u32, String) {
    use eq_client_core::socials::DuelAutoDecline;
    match reason {
        DuelAutoDecline::Ignored => (0x567, format!("{by} turned your challenge down.")),
        DuelAutoDecline::Dueling => (0x568, format!("{by} is already dueling.")),
        DuelAutoDecline::Considering => (0x569, format!("{by} is considering another challenge.")),
    }
}

/// Another spawn's stat increase, as the Titanium program tells it: a line
/// that names no one (read R49-6f, 0x455fff), 0x3273 for an amount below
/// zero and 0x3274 otherwise, in colour 15. The fallbacks are this
/// client's words.
fn others_stat_line(lowered: bool, messages: Option<&Messages>) -> Said {
    let (id, fallback) = if lowered {
        (0x3273, "A stat was lowered.")
    } else {
        (0x3274, "A stat was raised.")
    };
    official(Some(id), &[], fallback, messages).typed(15)
}

/// A yell for help, on Titanium in the program's line for where the yeller
/// stands, in colour 0x11a (read R49-6d); elsewhere, and where the
/// installation lacks the line, in this client's words.
fn yell_line(
    name: &str,
    bearing: Option<eq_client_core::world::YellBearing>,
    messages: Option<&Messages>,
) -> Said {
    let fallback = format!("{name} yells for help!");
    match bearing {
        Some(bearing) => official(
            Some(bearing.string_id()),
            &[name.to_owned()],
            &fallback,
            messages,
        )
        .typed(0x11a),
        None => Said::own(fallback),
    }
}

/// A quest's coins as the Titanium program says them (read R49-6g): a line
/// for each coin given, copper first, 0x59e, 0x5a0, 0x5a1 and 0x5a2, each
/// with the amount and 0x59f naming the giver, in colour 0x111. The
/// fallbacks are this client's words.
fn quest_reward_lines(
    coins: eq_client_core::Coins,
    giver: &str,
    messages: Option<&Messages>,
) -> Vec<Said> {
    let from = messages
        .and_then(|messages| messages.formatted(0x59f, &[giver.to_owned()]))
        .unwrap_or_default();
    [
        (coins.copper, 0x59e, "copper"),
        (coins.silver, 0x5a0, "silver"),
        (coins.gold, 0x5a1, "gold"),
        (coins.platinum, 0x5a2, "platinum"),
    ]
    .into_iter()
    .filter(|(count, ..)| *count > 0)
    .map(|(count, id, coin)| {
        official(
            Some(id),
            &[count.to_string(), from.clone()],
            &format!("You receive {count} {coin}."),
            messages,
        )
        .typed(0x111)
    })
    .collect()
}

/// Levels lost, as the Titanium program says it (read R49-6c): 0x1ba for
/// one, with the level now, and 0x1bb for more, with how many and the
/// level now, in colour 13.
fn level_lost_line(lost: u8, level: u8, messages: Option<&Messages>) -> Said {
    let (id, arguments, fallback) = if lost == 1 {
        (
            0x1ba,
            vec![level.to_string()],
            format!("You lost a level! You are now level {level}."),
        )
    } else {
        (
            0x1bb,
            vec![lost.to_string(), level.to_string()],
            format!("You lost {lost} levels! You are now level {level}."),
        )
    };
    official(Some(id), &arguments, &fallback, messages).typed(13)
}

/// The coins a quest or task gave, largest first, naming only those it
/// gave. This client's words (checklist 6.492, pending R49-6g).
fn reward_line(coins: eq_client_core::Coins) -> String {
    let parts: Vec<String> = [
        (coins.platinum, "platinum"),
        (coins.gold, "gold"),
        (coins.silver, "silver"),
        (coins.copper, "copper"),
    ]
    .into_iter()
    .filter(|(count, _)| *count != 0)
    .map(|(count, coin)| format!("{count} {coin}"))
    .collect();
    let listed = match parts.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    format!("You receive {listed}.")
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
/// The roleplaying lines take message type 273's colour (sitting s9l, read).
/// Looking for a group is told in this client's words: the program's
/// strings for `/lfg` are not read (checklist 6.515, pending R49-6i).
fn listing_line(notice: ListingNotice, messages: Option<&Messages>) -> Said {
    let (id, fallback) = match notice {
        ListingNotice::Away(true) => (13199, "You are away from the keyboard now."),
        ListingNotice::Away(false) => (13200, "You are back at the keyboard."),
        ListingNotice::Anonymous(true) => (13236, "You hide from /who now."),
        ListingNotice::Anonymous(false) => (13237, "You show in /who again."),
        ListingNotice::Roleplaying(true) => (13233, "You are in character now."),
        ListingNotice::Roleplaying(false) => (13234, "You are out of character again."),
        ListingNotice::Looking(true) => return Said::own("You are now looking for a group."),
        ListingNotice::Looking(false) => {
            return Said::own("You are no longer looking for a group.");
        }
    };
    let line = official(Some(id), &[], fallback, messages);
    match notice {
        ListingNotice::Roleplaying(_) => line.typed(273),
        _ => line,
    }
}

/// A camp's lines, in the installed client's strings for them (Titanium
/// sittings 9p and 9q, read): 12293 as it starts, 12684 with the seconds
/// left at 25, 20, 15, 10 and 5, and 12290 when it is abandoned. Standing
/// ends it with 12683 and moving with 12290 (sitting 9p, read); the line
/// for ducking, a door, a pick-up or death is unread, and says 12290 as
/// before (6.309). A stun's 12683 comes with the stun in 3b-2. Sitting 9p
/// also read 12682 near the camp's countdown, but 12682 is the line for a
/// hide that moving ended (the `EQMac` program says it when a swing ends a
/// hide, TAKP batch 1 R1), and a camp doesn't say it: the project owner
/// saw it at every camp on P99 while not hiding (11-01). Where Titanium
/// says it is unread (6.324).
fn camp_lines(camp: &CampStatus, messages: Option<&Messages>) -> Vec<Said> {
    let said = |id, arguments: &[String]| messages.map(|messages| messages.said(id, arguments));
    match camp {
        CampStatus::Preparing => said(12293, &[]).into_iter().collect(),
        CampStatus::Countdown { seconds_left } => said(12684, &[seconds_left.to_string()])
            .into_iter()
            .collect(),
        CampStatus::Abandoned { by } => {
            let id = match by {
                eq_client_core::CampEnd::Standing | eq_client_core::CampEnd::Stunned => 12683,
                eq_client_core::CampEnd::Moving | eq_client_core::CampEnd::Other => 12290,
            };
            said(id, &[]).into_iter().collect()
        }
        CampStatus::LoggingOut => vec!["Logging out...".into()],
        CampStatus::Camped => Vec::new(),
        CampStatus::ClosedWithoutReply => {
            vec!["The zone connection closed after logout; no logout reply was received.".into()]
        }
        CampStatus::Rejected(reason) => vec![reason.as_str().into()],
    }
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
            line(
                12281,
                None,
                "Press Follow in the group window to join, or Decline to say no.",
            ),
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
    fn environmental_damage_prints_the_programs_lines_in_its_colours() {
        // Synthetic stand-ins for 0x30c1, 0x2f9b, 0x2f60 and 0x2f63.
        let messages = Messages::parse(
            "EQST0002\n0 4\n12481 synthetic hurt for %1.\n12187 SYNTHETIC-YOU\n\
             12128 %1 synthetic drown.\n12131 %1 synthetic burn.\n",
        );
        let lines = |reported, record| {
            wording(
                &Notice::EnvironmentalDamage { reported, record },
                Some(&messages),
            )
        };
        assert_eq!(
            lines(Some(13), Some((12128, 266))),
            [
                (
                    Place::Chat,
                    Said::official("synthetic hurt for 13.").typed(283)
                ),
                (
                    Place::Chat,
                    Said::official("SYNTHETIC-YOU synthetic drown.").typed(266)
                ),
            ]
        );
        // No record went: the amount line alone.
        assert_eq!(
            lines(Some(13), None),
            [(
                Place::Chat,
                Said::official("synthetic hurt for 13.").typed(283)
            )]
        );
        // A record from the server: its line alone, in its colour.
        assert_eq!(
            lines(None, Some((12131, 329))),
            [(
                Place::Chat,
                Said::official("SYNTHETIC-YOU synthetic burn.").typed(329)
            )]
        );
        // Without the installation's strings nothing prints.
        let unworded = Notice::EnvironmentalDamage {
            reported: Some(13),
            record: None,
        };
        assert_eq!(wording(&unworded, None), []);
    }

    #[test]
    fn a_bearing_names_its_side_word_by_its_words() {
        let messages = Messages::parse(
            "EQST0002\n0 2\n12677 %1 synthetic ahead, %2 side\n12674 synthetic-right\n",
        );
        let bearing = |side: &str| Notice::ProgramLine {
            string_id: 12677,
            arguments: vec!["a bat".into(), side.into()],
            lines: vec![1],
            said: "Tracking: a bat ahead to one side".into(),
        };
        assert_eq!(
            wording(&bearing("12674"), Some(&messages)),
            [(
                Place::Chat,
                Said::official("a bat synthetic ahead, synthetic-right side")
            )]
        );
        // A side word the installation lacks: the client's words say it all,
        // never a bare string number.
        assert_eq!(
            wording(&bearing("12675"), Some(&messages)),
            [(Place::Chat, "Tracking: a bat ahead to one side".into())]
        );
    }

    #[test]
    fn a_report_says_this_clients_line_only_where_the_server_says_none() {
        use eq_client_core::reports::ReportKind;
        let line = |notice| wording(&notice, None);
        assert_eq!(
            line(Notice::ReportSent(ReportKind::Petition)),
            [(Place::Chat, Said::own("Your petition was sent."))]
        );
        assert_eq!(
            line(Notice::ReportSent(ReportKind::Bug)),
            [(Place::Chat, Said::own("Your bug report was sent."))]
        );
        // Feedback's servers answer it themselves.
        assert_eq!(line(Notice::ReportSent(ReportKind::Feedback)), []);
        assert_eq!(
            line(Notice::ReportRefused {
                reason: "Wait a little before sending another petition.".into()
            }),
            [(
                Place::Chat,
                Said::own("Wait a little before sending another petition.")
            )]
        );
    }

    #[test]
    fn a_chat_pace_refusal_is_the_players_own_line() {
        let reason = "You are sending chat too fast. Wait 1 second.";
        assert_eq!(
            wording(
                &Notice::ChatRefused {
                    reason: reason.into()
                },
                None
            ),
            [(Place::Chat, Said::own(reason))]
        );
    }

    #[test]
    fn a_refused_creation_says_which_step_on_the_status_line() {
        let line = |outcome| {
            wording(
                &Notice::CreationRefused {
                    name: "Testcleric".into(),
                    outcome,
                },
                None,
            )
        };
        assert_eq!(
            line(CreationOutcome::NameRefused),
            [(
                Place::Status,
                "The server refused the name Testcleric.".into()
            )]
        );
        assert_eq!(
            line(CreationOutcome::CreationRefused),
            [(
                Place::Status,
                "The server approved the name Testcleric but could not create the character."
                    .into()
            )]
        );
        assert_eq!(
            line(CreationOutcome::NotSent),
            [(
                Place::Status,
                "Testcleric was not sent: this client cannot create that character on this server."
                    .into()
            )]
        );
    }

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
    fn expired_bandaging_distinguishes_the_received_stock_from_an_unknown_result() {
        for (stock_uncertain, expected) in [
            (
                true,
                "The bandage result is unconfirmed. Wait for an inventory update before using the affected bandages.",
            ),
            (
                false,
                "Bandage stock updated. The server did not report a bandaging result in time.",
            ),
        ] {
            assert_eq!(
                wording(
                    &Notice::BindWound(BindWoundUpdate::Unconfirmed { stock_uncertain }),
                    None,
                ),
                [(Place::Chat, Said::own(expected))]
            );
        }
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
    fn duel_news_reads_in_the_programs_lines_and_colours() {
        use eq_client_core::{socials::DuelAutoDecline, world::DuelNotice};
        let messages = Messages::parse(
            "EQST0002
0 9
1424 Begun %1.
1426 Challenged %1.
1383 Ignored %1.
1408 Fled %1 %2 %3.
12100 Won %1 %2.
12101 Ended %1 %2.
13248 No player.
13254 Challenging %1.
13252 Accepting %1.
",
        );
        let said = |notice: DuelNotice| {
            wording(&Notice::Duel(notice), Some(&messages))
                .into_iter()
                .map(|(_, said)| said)
                .collect::<Vec<_>>()
        };
        let friend = || "Friend".to_owned();
        let cases = [
            (DuelNotice::Begun(friend()), "Begun Friend.", 15),
            (DuelNotice::Challenged(friend()), "Challenged Friend.", 15),
            (DuelNotice::Challenging(friend()), "Challenging Friend.", 15),
            (DuelNotice::Accepting(friend()), "Accepting Friend.", 15),
            (DuelNotice::NoPlayerTarget, "No player.", 0x111),
            (
                DuelNotice::Won {
                    own: "Me".into(),
                    over: friend(),
                },
                "Won Me Friend.",
                0x10d,
            ),
            (DuelNotice::Ended(friend()), "Ended Friend Friend.", 0x10d),
            (
                DuelNotice::TurnedDown {
                    by: friend(),
                    reason: Some(DuelAutoDecline::Ignored),
                },
                "Ignored Friend.",
                15,
            ),
            (
                DuelNotice::Forfeited {
                    own: "Me".into(),
                    opponent: friend(),
                },
                "Fled Me Friend Friend.",
                0x10d,
            ),
        ];
        for (notice, line, colour) in cases {
            assert_eq!(said(notice), [Said::official(line).typed(colour)]);
        }
        // A line the installation lacks is this client's, in the same colour.
        assert_eq!(
            said(DuelNotice::NotYourself),
            [Said::own("You cannot duel yourself.").typed(0x111)]
        );
        // A turn-down the program has no line for says nothing.
        assert_eq!(
            said(DuelNotice::TurnedDown {
                by: friend(),
                reason: None
            }),
            []
        );
    }

    #[test]
    fn yell_reward_and_level_news_read_in_the_programs_lines() {
        use eq_client_core::{Coins, world::YellBearing};
        let messages = Messages::parse(
            "EQST0002
0 6
442 Down to %1.
443 Lost %1, now %2.
1438 %1 copper%2.
1439 , from %1
1442 %1 platinum%2.
12908 %1 behind.
",
        );
        let said = |notice: Notice| {
            wording(&notice, Some(&messages))
                .into_iter()
                .map(|(_, said)| said)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            said(Notice::YellForHelp {
                name: "Friend".into(),
                bearing: Some(YellBearing::Behind)
            }),
            [Said::official("Friend behind.").typed(0x11a)]
        );
        // A band the installation lacks is this client's words.
        assert_eq!(
            said(Notice::YellForHelp {
                name: "Friend".into(),
                bearing: Some(YellBearing::Ahead)
            }),
            [Said::own("Friend yells for help!").typed(0x11a)]
        );
        assert_eq!(
            said(Notice::QuestReward {
                coins: Coins {
                    platinum: 2,
                    gold: 0,
                    silver: 0,
                    copper: 5,
                },
                giver: "Guard".into()
            }),
            [
                Said::official("5 copper, from Guard.").typed(0x111),
                Said::official("2 platinum, from Guard.").typed(0x111)
            ]
        );
        assert_eq!(
            said(Notice::LevelLost { lost: 1, level: 9 }),
            [Said::official("Down to 9.").typed(13)]
        );
        assert_eq!(
            said(Notice::LevelLost { lost: 2, level: 8 }),
            [Said::official("Lost 2, now 8.").typed(13)]
        );
    }

    #[test]
    fn duel_yell_stat_and_coin_news_read_in_this_clients_words() {
        use eq_client_core::{Coins, socials::DuelRefusal, world::DuelNotice};
        let said = |notice: Notice| {
            wording(&notice, None)
                .into_iter()
                .map(|(_, said)| said)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            said(Notice::Duel(DuelNotice::Challenged("Friend".into()))),
            [Said::own("Friend challenges you to a duel.").typed(15)]
        );
        assert_eq!(
            said(Notice::Duel(DuelNotice::Begun("Friend".into()))),
            [Said::own("Your duel with Friend begins.").typed(15)]
        );
        assert_eq!(
            said(Notice::Duel(DuelNotice::Refused {
                from: "Friend".into(),
                reason: DuelRefusal::Dueling
            })),
            [Said::own("Friend is already dueling.")]
        );
        assert_eq!(
            said(Notice::YellForHelp {
                name: "Friend".into(),
                bearing: None
            }),
            [Said::own("Friend yells for help!")]
        );
        assert_eq!(
            said(Notice::OthersStatChanged { lowered: true }),
            [Said::own("A stat was lowered.").typed(15)]
        );
        assert_eq!(
            said(Notice::OthersStatChanged { lowered: false }),
            [Said::own("A stat was raised.").typed(15)]
        );
        let coins = |platinum, gold, silver, copper| Coins {
            platinum,
            gold,
            silver,
            copper,
        };
        assert_eq!(
            said(Notice::Reward(coins(0, 0, 0, 7))),
            [Said::own("You receive 7 copper.")]
        );
        assert_eq!(
            said(Notice::Reward(coins(2, 0, 4, 1))),
            [Said::own("You receive 2 platinum, 4 silver and 1 copper.")]
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
            [(Place::Chat, Said::official("In.").typed(273))]
        );
        // A string the installation lacks reads in this client's words.
        assert_eq!(
            said(ListingNotice::Away(false)),
            [(Place::Chat, Said::own("You are back at the keyboard."))]
        );
        // Looking for a group is always in this client's words (6.515).
        assert_eq!(
            said(ListingNotice::Looking(true)),
            [(Place::Chat, Said::own("You are now looking for a group."))]
        );
        assert_eq!(
            said(ListingNotice::Looking(false)),
            [(
                Place::Chat,
                Said::own("You are no longer looking for a group.")
            )]
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
                    line.words.text
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
                    string_id: None,
                },
                None,
            )
        };
        assert_eq!(door(None), []);
        assert_eq!(door(Some("Locked")), [(Place::Chat, "Locked".into())]);
        // Out of reach, the session names the program's string 1462.
        let messages = Messages::parse("EQST0002\n0 1\n1462 Too far.\n");
        let far = Notice::Door {
            door_id: 4,
            error: Some("Out of reach".into()),
            string_id: Some(1462),
        };
        assert_eq!(
            wording(&far, Some(&messages)),
            [(Place::Chat, Said::official("Too far."))]
        );
        assert_eq!(wording(&far, None), [(Place::Chat, "Out of reach".into())]);
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
        assert_eq!(
            wording(&Notice::Camp(CampStatus::ClosedWithoutReply), None),
            [(
                Place::Chat,
                "The zone connection closed after logout; no logout reply was received.".into()
            )]
        );
    }

    #[test]
    fn a_camp_says_its_start_and_each_five_seconds_left() {
        use eq_client_core::CampEnd;
        let messages = Messages::parse(
            "EQST0002\n0 1\n12290 Abandoned.\n12293 Camping.\n12682 Hide ended.\n12683 Stood.\n12684 %1 left.\n",
        );
        let said = |camp| wording(&Notice::Camp(camp), Some(&messages));
        // The start says 12293 alone: 12682 is not a camp's line.
        assert_eq!(
            said(CampStatus::Preparing),
            [(Place::Chat, Said::official("Camping."))]
        );
        assert_eq!(
            said(CampStatus::Countdown { seconds_left: 25 }),
            [(Place::Chat, Said::official("25 left."))]
        );
        // Standing ends it in its own words; moving and the rest in 12290.
        assert_eq!(
            said(CampStatus::Abandoned {
                by: CampEnd::Standing
            }),
            [(Place::Chat, Said::official("Stood."))]
        );
        assert_eq!(
            said(CampStatus::Abandoned {
                by: CampEnd::Stunned
            }),
            [(Place::Chat, Said::official("Stood."))]
        );
        for by in [CampEnd::Moving, CampEnd::Other] {
            assert_eq!(
                said(CampStatus::Abandoned { by }),
                [(Place::Chat, Said::official("Abandoned."))]
            );
        }
    }

    #[test]
    fn an_arrival_names_the_zone_in_the_installed_clients_words() {
        let arrived = Notice::Arrived("The Qeynos Hills".into());
        let messages = Messages::parse("EQST0002\n0 1\n12357 Test arrival: %1\n");
        assert_eq!(
            wording(&arrived, Some(&messages)),
            [(
                Place::Chat,
                Said::official("Test arrival: The Qeynos Hills")
            )]
        );
        // Without the installed strings, in this client's words.
        assert_eq!(
            wording(&arrived, None),
            [(Place::Chat, Said::own("Arrived in The Qeynos Hills."))]
        );
    }
}
