//! The player's guild in the chat and the confirmation dialog, as the
//! Titanium client has it (program read R11): an invitation asks on the
//! dialog or is answered at once, and the guild's slash commands check
//! what the official client checks before they send.
//!
//! An invitation to a player in no guild, or to a higher rank in their own,
//! asks Yes or No; Yes takes the rank offered and No declines. Answered at
//! once and only said in the chat: an offer from another guild while the
//! player is in one, and any offer while another invitation is asked about.
//! An offer of the rank held is answered without a word, and of a member's
//! rank to one above it with a line. The official client's dialog closes by
//! itself after ten seconds; what it answers then is not read, so this one
//! withdraws the question and sends nothing (recording checklist 6.412).
//! Its option to refuse every guild invitation is not offered yet.
use crate::chat::{ChatState, Said, system_line};
use crate::confirm::{Asked as Dialog, Question};
use crate::contacts::abbreviates;
use crate::hud::messages::Messages;
use crate::online::OnlineState;
use crate::outbox::Outbox;
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand,
    guild::InviteAnswer,
    listing::GuildRank,
    world::{ClientWorld, GuildInvitation},
};
use std::time::{Duration, Instant};

/// How long the official client's invitation dialog waits (R11, [I]).
const DIALOG_WAIT: Duration = Duration::from_secs(10);

/// The official client's lines for its guild, by `eqstr_us.txt` number.
mod lines {
    /// An invitation into a guild as a member, an officer or its leader.
    pub(super) const INVITED: [u32; 3] = [0x216, 0x217, 0x218];
    /// An invitation to an officer's or the leader's rank in the guild.
    pub(super) const PROMOTED: [u32; 2] = [0x224, 0x225];
    /// An offer of a member's rank to one who holds more.
    pub(super) const DEMOTED: u32 = 0x223;
    /// Not in a guild.
    pub(super) const NO_GUILD: u32 = 0x2fd2;
    /// `/getguildmotd` and `/guildmotd` outside a guild.
    pub(super) const NO_GUILD_MOTD: u32 = 0x32a6;
    /// A member may not invite.
    pub(super) const MEMBER_INVITES: u32 = 0x2fd3;
    /// Only the leader offers an officer's rank.
    pub(super) const OFFICER_OFFER: u32 = 0x2fd7;
    /// No one named and no player targeted.
    pub(super) const INVITE_WHOM: u32 = 0x2fd8;
    /// The one named is not a player in the zone.
    pub(super) const NOT_HERE: u32 = 0x2fd9;
    /// An invitation sent, to a member's or an officer's rank.
    pub(super) const INVITE_SENT: [u32; 2] = [0x2fd5, 0x2fd6];
    /// The leader cannot remove themself.
    pub(super) const LEADER_LEAVES: u32 = 0x2fdb;
    /// A member may remove only themself.
    pub(super) const MEMBER_REMOVES: u32 = 0x2fdd;
    /// No one named and no player targeted for removal.
    pub(super) const REMOVE_WHOM: u32 = 0x2fdf;
    /// Leaving the guild, and removing another.
    pub(super) const REMOVED: [u32; 2] = [0x2fdc, 0x2fde];
    /// Only the leader hands on the lead.
    pub(super) const NOT_LEADER: u32 = 0x2fe5;
    /// The lead handed on.
    pub(super) const LEADER_SENT: u32 = 0x2fe6;
}

/// The invitation the dialog asks about, since when, and its words.
#[derive(Resource, Default)]
pub(crate) struct Invitations {
    asking: Option<(GuildInvitation, Instant, Said)>,
}

impl Invitations {
    /// What the dialog asks.
    pub(crate) fn question(&self) -> String {
        self.asking
            .as_ref()
            .map(|(_, _, said)| said.words.text.clone())
            .unwrap_or_default()
    }
}

/// A rank's number as the official client compares ranks.
const fn rank_number(rank: GuildRank) -> u32 {
    match rank {
        GuildRank::Member => 0,
        GuildRank::Officer => 1,
        GuildRank::Leader => 2,
        GuildRank::Other(value) => value,
    }
}

/// What to do with an invitation as it comes.
#[derive(Debug, PartialEq, Eq)]
enum Handling {
    /// Ask on the dialog, in these words.
    Ask(Said),
    /// Answer at once, saying this first where there is a line.
    Answer(InviteAnswer, Option<Said>),
    /// Neither ask nor answer: a case the read does not cover.
    Unread,
}

/// How the official client handles an invitation (R11, 0x455134).
fn handling(
    invitation: &GuildInvitation,
    world: &ClientWorld,
    busy: bool,
    messages: &Messages,
) -> Handling {
    let listing = world.player().map(|player| player.listing);
    let guild = listing.and_then(|listing| listing.guild);
    let held = listing
        .and_then(|listing| listing.guild_rank)
        .map_or(0, rank_number);
    let offered = rank_number(invitation.rank);
    let inviter = invitation.inviter.clone();
    if guild == Some(invitation.guild_id) {
        return match offered.cmp(&held) {
            std::cmp::Ordering::Equal => Handling::Answer(InviteAnswer::AlreadyHeld, None),
            std::cmp::Ordering::Less => {
                // Only a member's rank is answered so; another lower offer is
                // not read, and goes unanswered until it is.
                if offered == 0 {
                    Handling::Answer(
                        InviteAnswer::Accept,
                        Some(messages.said_or(
                            lines::DEMOTED,
                            std::slice::from_ref(&inviter),
                            &format!("{inviter} makes you a member of your guild."),
                        )),
                    )
                } else {
                    Handling::Unread
                }
            }
            std::cmp::Ordering::Greater => {
                let id = lines::PROMOTED[usize::from(offered >= 2)];
                Handling::Ask(messages.said_or(
                    id,
                    std::slice::from_ref(&inviter),
                    &format!("{inviter} offers you a higher rank in your guild. Accept?"),
                ))
            }
        };
    }
    let name = world
        .guild_name(invitation.guild_id)
        .map_or_else(|| "their guild".to_owned(), str::to_owned);
    let id = lines::INVITED[usize::try_from(offered.min(2)).unwrap_or(0)];
    let said = messages.said_or(
        id,
        &[inviter.clone(), name.clone()],
        &format!("{inviter} invites you to join {name}. Accept?"),
    );
    if busy {
        Handling::Answer(InviteAnswer::Busy, Some(said))
    } else if guild.is_some() {
        Handling::Answer(InviteAnswer::InAnotherGuild, Some(said))
    } else {
        Handling::Ask(said)
    }
}

/// Sends an answer to an invitation; the outbox says a refusal itself.
fn send(online: &OnlineState, outbox: &Outbox, inviter: &str, answer: InviteAnswer) {
    let _ = outbox.post(online.world(), |stamp| ClientCommand::AnswerGuildInvite {
        session_id: stamp.session_id,
        inviter: inviter.to_owned(),
        answer,
    });
}

/// Asks about each guild invitation as it comes, or answers it at once,
/// and sends the dialog's answer.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn invitations(
    (mut online, outbox): (ResMut<OnlineState>, Res<Outbox>),
    (mut dialog, mut pending): (ResMut<Dialog>, ResMut<Invitations>),
    mut chat: ResMut<ChatState>,
    messages: Option<Res<Messages>>,
) {
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    if let Some((invitation, since, _)) = &pending.asking {
        if let Some(yes) = dialog.answer(Question::GuildInvite) {
            let answer = if yes {
                InviteAnswer::Accept
            } else {
                InviteAnswer::Decline
            };
            send(&online, &outbox, &invitation.inviter, answer);
            pending.asking = None;
        } else if since.elapsed() >= DIALOG_WAIT {
            if dialog.question() == Some(Question::GuildInvite) {
                dialog.withdraw();
            }
            pending.asking = None;
        }
    }
    while let Some(invitation) = online.take_guild_invitation() {
        let busy = pending.asking.is_some();
        match handling(&invitation, online.world(), busy, messages) {
            Handling::Ask(said) => {
                if dialog.ask(Question::GuildInvite) {
                    pending.asking = Some((invitation, Instant::now(), said));
                } else {
                    // The dialog asks something of the client's own.
                    chat.history.push(system_line(said));
                    send(&online, &outbox, &invitation.inviter, InviteAnswer::Busy);
                }
            }
            Handling::Answer(answer, said) => {
                if let Some(said) = said {
                    chat.history.push(system_line(said));
                }
                send(&online, &outbox, &invitation.inviter, answer);
            }
            Handling::Unread => (),
        }
    }
}

/// A guild slash command the chat runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// `/guildinvite [Name] [o]`.
    Invite(String),
    /// `/guildremove [Name]`.
    Remove(String),
    /// `/guildmotd [text]`.
    SetMotd(String),
    /// `/guildleader [Name]`.
    Leader(String),
    /// `/getguildmotd`.
    GetMotd,
}

impl Command {
    /// The guild command a typed line is, by the official parser's prefix
    /// rule; None for any other line.
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let command = input.strip_prefix('/')?.trim();
        let (name, words) = command
            .split_once(char::is_whitespace)
            .unwrap_or((command, ""));
        let words = words.trim().to_owned();
        if abbreviates(name, "guildinvite", 6) {
            Some(Self::Invite(words))
        } else if abbreviates(name, "guildremove", 6) {
            Some(Self::Remove(words))
        } else if abbreviates(name, "guildmotd", 6) {
            Some(Self::SetMotd(words))
        } else if abbreviates(name, "guildleader", 6) {
            Some(Self::Leader(words))
        } else if abbreviates(name, "getguildmotd", 3) {
            Some(Self::GetMotd)
        } else {
            None
        }
    }
}

/// The player targeted, other than the player themself.
fn targeted_other(world: &ClientWorld) -> Option<String> {
    let id = world.target().selected?;
    if world.is_player(id) {
        return None;
    }
    world
        .spawn(id)
        .filter(|spawn| spawn.state.kind == eq_client_core::SpawnKind::Player)
        .map(|spawn| spawn.state.name.clone())
}

/// What a guild command sends, and the line said when it goes; or the line
/// that refuses it. None in the second place says nothing.
fn command(
    command: &Command,
    world: &ClientWorld,
    messages: &Messages,
) -> Result<(ClientCommand, Option<Said>), Option<Said>> {
    let player = world
        .player()
        .ok_or_else(|| Some(Said::own("Enter the world first")))?;
    let session_id = world.session_id().unwrap_or_default();
    let refused = |id, fallback: &str| Err(Some(messages.said_text(id, fallback)));
    let guild = player.listing.guild;
    let rank = player.listing.guild_rank.map_or(0, rank_number);
    let own = player.name.clone();
    if guild.is_none() {
        return match command {
            Command::GetMotd | Command::SetMotd(_) => {
                refused(lines::NO_GUILD_MOTD, "You are not in a guild.")
            }
            _ => refused(lines::NO_GUILD, "You are not in a guild."),
        };
    }
    Ok(match command {
        Command::GetMotd => (ClientCommand::RequestGuildMotd { session_id }, None),
        Command::SetMotd(motd) => (
            ClientCommand::SetGuildMotd {
                session_id,
                motd: motd.clone(),
            },
            None,
        ),
        Command::Invite(words) => return invite(words, world, messages, (session_id, rank, &own)),
        Command::Remove(words) => {
            let name = match words.split_whitespace().next() {
                Some(name) => name.to_owned(),
                None => targeted_other(world).ok_or_else(|| {
                    Some(messages.said_text(lines::REMOVE_WHOM, "Name or target a member."))
                })?,
            };
            // The program compares the player's own name case-sensitively.
            let leaving = name == own;
            if leaving && rank >= 2 {
                return refused(lines::LEADER_LEAVES, "The leader cannot leave the guild.");
            }
            if !leaving && rank == 0 {
                return refused(lines::MEMBER_REMOVES, "Members cannot remove others.");
            }
            let said = messages.said_or(
                lines::REMOVED[usize::from(!leaving)],
                std::slice::from_ref(&name),
                &if leaving {
                    "You leave your guild.".to_owned()
                } else {
                    format!("You remove {name} from your guild.")
                },
            );
            (ClientCommand::GuildRemove { session_id, name }, Some(said))
        }
        Command::Leader(words) => {
            if rank < 2 {
                return refused(lines::NOT_LEADER, "Only the leader can hand on the lead.");
            }
            let name = words
                .split_whitespace()
                .next()
                .map(str::to_owned)
                .or_else(|| targeted_other(world))
                .unwrap_or_default();
            let said = messages.said_or(
                lines::LEADER_SENT,
                std::slice::from_ref(&name),
                &format!("You make {name} the leader of your guild."),
            );
            (
                ClientCommand::GuildMakeLeader { session_id, name },
                Some(said),
            )
        }
    })
}

/// `/guildinvite`, after the guild check: what it sends and says, or the
/// line that refuses it (R11, 0x49bf15).
fn invite(
    words: &str,
    world: &ClientWorld,
    messages: &Messages,
    (session_id, rank, own): (u64, u32, &str),
) -> Result<(ClientCommand, Option<Said>), Option<Said>> {
    if rank == 0 {
        return Err(Some(
            messages.said_text(lines::MEMBER_INVITES, "Members cannot invite."),
        ));
    }
    let mut words = words.split_whitespace();
    let first = words.next();
    let (named, officer) = match first {
        None => (None, false),
        Some(flag) if flag == "o" || flag == "l" => (None, flag == "o"),
        Some(name) => (Some(name.to_owned()), words.next() == Some("o")),
    };
    let name = match named {
        Some(name) => name,
        None => targeted_other(world).ok_or_else(|| {
            Some(messages.said_text(lines::INVITE_WHOM, "Name or target a player."))
        })?,
    };
    if name.eq_ignore_ascii_case(own) {
        return Err(None);
    }
    if officer && rank < 2 {
        return Err(Some(messages.said_text(
            lines::OFFICER_OFFER,
            "Only the leader offers an officer's rank.",
        )));
    }
    if !world.player_in_zone(&name) {
        return Err(Some(
            messages.said_text(lines::NOT_HERE, "That player is not here."),
        ));
    }
    let said = messages.said_or(
        lines::INVITE_SENT[usize::from(officer)],
        std::slice::from_ref(&name),
        &format!("You invite {name} into your guild."),
    );
    Ok((
        ClientCommand::GuildInvite {
            session_id,
            name,
            officer,
        },
        Some(said),
    ))
}

/// Runs the guild commands the chat noted.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn commands(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut chat: ResMut<ChatState>,
    messages: Option<Res<Messages>>,
) {
    let Some(asked) = chat.guild_asked.take() else {
        return;
    };
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    match command(&asked, online.world(), messages) {
        Ok((command, said)) => {
            if outbox.send(online.world(), command).is_ok()
                && let Some(said) = said
            {
                chat.history.push(system_line(said));
            }
        }
        Err(Some(said)) => chat.history.push(system_line(said)),
        Err(None) => (),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guild_commands_follow_the_official_prefix_rule() {
        assert_eq!(
            Command::typed("/guildi Friend o"),
            Some(Command::Invite("Friend o".into()))
        );
        assert_eq!(
            Command::typed("/guildr"),
            Some(Command::Remove(String::new()))
        );
        assert_eq!(
            Command::typed("/guildmotd Raid at 8"),
            Some(Command::SetMotd("Raid at 8".into()))
        );
        assert_eq!(
            Command::typed("/guildl Second"),
            Some(Command::Leader("Second".into()))
        );
        assert_eq!(Command::typed("/get"), Some(Command::GetMotd));
        for other in [
            "/guild hello",
            "/guildd",
            "/guild",
            "/guildsay hi",
            "/ge",
            "/gu hi",
        ] {
            assert_eq!(Command::typed(other), None, "{other}");
        }
    }

    #[test]
    fn invitations_ask_or_answer_as_the_official_client_does() {
        let messages = Messages::default();
        let world = ClientWorld::default();
        let invitation = |rank| GuildInvitation {
            inviter: "Inviter".into(),
            guild_id: 3,
            rank,
        };
        // No player yet: the player is in no guild, so the dialog asks.
        assert!(matches!(
            handling(&invitation(GuildRank::Member), &world, false, &messages),
            Handling::Ask(_)
        ));
        assert!(matches!(
            handling(&invitation(GuildRank::Member), &world, true, &messages),
            Handling::Answer(InviteAnswer::Busy, Some(_))
        ));
        assert_eq!(rank_number(GuildRank::Leader), 2);
        assert_eq!(InviteAnswer::AlreadyHeld.wire(GuildRank::Officer), 11);
    }
}
