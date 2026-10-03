//! The player's raid, as the server describes it, and the invitation
//! waiting for the player's answer. The server sends no line with its raid
//! news, nor with the requests the session sends for the player, so the
//! official client words each itself (inferred from its strings); the world
//! says which happened.
//!
//! The server lists every member as the player joins, enters a zone or
//! moves, which the session tells apart from a member who joins
//! (`RaidUpdate::Listed`): only a member who joins says so. A member moved
//! between raid groups says nothing (inferred), nor does the leader named
//! again. Locking and unlocking say so as the leader
//! does it, not as the server tells a member who joins or enters a zone
//! while the raid is locked (inferred: the line names the leader): the
//! server's word names the leader as they lock it and the member as they
//! enter, and the leader's own answer follows their request.
use super::{Changes, ClientWorld, Notice, Party};
use crate::raid::{RaidMember, RaidUpdate};

/// The player's raid, as the server described it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Raid {
    /// Its leader, once the server has named them.
    pub leader: Option<String>,
    /// Its members, the player among them, in the order the server added
    /// them.
    pub members: Vec<RaidMember>,
    /// Whether it is locked, so that its leader may move members between
    /// raid groups.
    pub locked: bool,
}

/// A member's rank in the raid, as the official client's notes on raids
/// (`raidsdoc.txt`) list the ranks: the raid's leader, a raid group's
/// leader, or a member without a rank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaidRank {
    /// Leads the raid, whether or not they lead a raid group too.
    Leader,
    /// Leads a raid group.
    GroupLeader,
    /// Neither.
    Member,
}

impl Raid {
    /// The members in a raid group, by group from the first, each group's
    /// in the order the server added them. In what order the official
    /// client lists them is not checked yet.
    #[must_use]
    pub fn grouped(&self) -> Vec<&RaidMember> {
        let mut grouped: Vec<&RaidMember> = self
            .members
            .iter()
            .filter(|member| member.group.is_some())
            .collect();
        grouped.sort_by_key(|member| member.group);
        grouped
    }

    /// The members in no raid group, in the order the server added them.
    pub fn ungrouped(&self) -> impl Iterator<Item = &RaidMember> {
        self.members.iter().filter(|member| member.group.is_none())
    }

    /// The members' average level, rounded down (inferred); None for a raid
    /// the server has listed no one in yet.
    #[must_use]
    pub fn level_average(&self) -> Option<u32> {
        let count = u32::try_from(self.members.len())
            .ok()
            .filter(|count| *count > 0)?;
        let levels: u32 = self
            .members
            .iter()
            .map(|member| u32::from(member.level))
            .sum();
        Some(levels / count)
    }

    /// A member's rank: the raid's leader ranks above a raid group's.
    #[must_use]
    pub fn rank(&self, member: &RaidMember) -> RaidRank {
        if self.leader.as_deref() == Some(member.name.as_str()) {
            RaidRank::Leader
        } else if member.group_leader {
            RaidRank::GroupLeader
        } else {
            RaidRank::Member
        }
    }
}

/// What news of raids tells the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RaidNotice {
    /// The player invited someone to their raid.
    Inviting(String),
    /// Someone invited the player to their raid.
    Invited(String),
    /// The player agreed to join the inviter's raid.
    Accepting(String),
    /// The player declined the inviter's invitation.
    Declining(String),
    /// The first to join formed a raid with the player, who leads it.
    Formed,
    /// The player, or someone else, joined the player's raid.
    Joined(Party),
    /// The player, or someone else, left the player's raid or was removed.
    Left(Party),
    /// The player's raid has a new leader.
    Leader(Party),
    /// The player is out of their raid as it ended.
    Disbanded,
    /// The raid's leader locked the raid (true) or unlocked it.
    Locked(bool),
}

/// What the world remembers of raid news on the way: an invitation the
/// player sent or accepted, and a lock they asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RaidFlow {
    /// The player invited someone, and no raid has formed since.
    inviting: bool,
    /// The player accepted an invitation, and no raid has come since.
    joining: bool,
    /// The player asked to lock the raid (true) or unlock it, and the server
    /// has not answered yet.
    locking: Option<bool>,
}

impl ClientWorld {
    /// The player's raid, while they are in one.
    #[must_use]
    pub const fn raid(&self) -> Option<&Raid> {
        self.raid.as_ref()
    }

    /// Who invited the player to their raid, while the invitation waits for
    /// an answer.
    #[must_use]
    pub fn raid_invitation(&self) -> Option<&str> {
        self.raid_invitation.as_deref()
    }

    /// Whether the player leads their raid.
    #[must_use]
    pub fn leads_raid(&self) -> bool {
        self.raid
            .as_ref()
            .and_then(|raid| raid.leader.as_deref())
            .is_some_and(|leader| self.is_named(leader))
    }

    /// Who a name in raid news is: the player, or another by name.
    fn raid_party(&self, name: &str) -> Party {
        if self.is_named(name) {
            Party::Player
        } else {
            Party::Named(name.to_owned())
        }
    }

    /// Follows news of raids, and says what it tells the player.
    pub(super) fn raid_news(&mut self, update: &RaidUpdate, news: &mut Changes) {
        let notice = match update {
            RaidUpdate::Inviting { player } => {
                self.raid_flow.inviting = true;
                Some(RaidNotice::Inviting(player.clone()))
            }
            RaidUpdate::Accepting { inviter } => {
                self.raid_invitation = None;
                self.raid_flow.joining = true;
                Some(RaidNotice::Accepting(inviter.clone()))
            }
            RaidUpdate::Declining { inviter } => {
                self.raid_invitation = None;
                Some(RaidNotice::Declining(inviter.clone()))
            }
            // The server's answer says what came of it.
            RaidUpdate::Leaving => None,
            RaidUpdate::Invited { inviter } => {
                self.raid_invitation = Some(inviter.clone());
                Some(RaidNotice::Invited(inviter.clone()))
            }
            RaidUpdate::Created { leader } => self.raid_created(leader),
            RaidUpdate::Added(member) => self.raid_added(member, false),
            RaidUpdate::Listed(member) => self.raid_added(member, true),
            RaidUpdate::Removed { member } => {
                let party = self.raid_party(member);
                if party == Party::Player {
                    self.raid = None;
                } else if let Some(raid) = self.raid.as_mut() {
                    raid.members.retain(|known| known.name != *member);
                }
                Some(RaidNotice::Left(party))
            }
            // The end of a raid the player already left says nothing more.
            RaidUpdate::Disbanded => self.raid.take().map(|_| RaidNotice::Disbanded),
            RaidUpdate::Leader { name } => {
                let before = self
                    .raid
                    .as_mut()
                    .and_then(|raid| raid.leader.replace(name.clone()));
                // The leader named again, as every list names them, says
                // nothing.
                (before.as_ref() != Some(name)).then(|| RaidNotice::Leader(self.raid_party(name)))
            }
            // A member moved takes their new place; one the roster does not
            // hold joins it without a word, as the session's own roster
            // does (no EQEmu order sends one).
            RaidUpdate::Moved(member) => {
                let members = &mut self.raid.get_or_insert_with(Raid::default).members;
                match members.iter_mut().find(|known| known.name == member.name) {
                    Some(known) => known.clone_from(member),
                    None => members.push(member.clone()),
                }
                None
            }
            RaidUpdate::Locking { locked } => {
                self.raid_flow.locking = Some(*locked);
                None
            }
            RaidUpdate::Locked { locked, by } => {
                let asked = self.raid_flow.locking.take() == Some(*locked);
                let named_player = self.is_named(by);
                self.raid.as_mut().and_then(|raid| {
                    let changed = raid.locked != *locked;
                    raid.locked = *locked;
                    // The leader's word to every member, or the answer to the
                    // player's own request; not the word on entering a zone.
                    let told = asked || (!named_player && raid.leader.as_ref() == Some(by));
                    (changed && told).then_some(RaidNotice::Locked(*locked))
                })
            }
        };
        // Being in a raid answers any invitation.
        if self.raid.is_some() {
            self.raid_invitation = None;
        }
        news.notices.extend(notice.map(Notice::Raid));
    }

    /// The player is in a raid: one they formed by inviting, one they
    /// joined, or theirs again as they enter a zone or move, whose members
    /// follow. A raid listed again as the player moves stays as locked as it
    /// was.
    fn raid_created(&mut self, leader: &str) -> Option<RaidNotice> {
        let locked = self.raid.as_ref().is_some_and(|raid| raid.locked);
        self.raid = Some(Raid {
            leader: Some(leader.to_owned()),
            members: Vec::new(),
            locked,
        });
        let flow = std::mem::take(&mut self.raid_flow);
        if flow.inviting && self.is_named(leader) {
            return Some(RaidNotice::Formed);
        }
        flow.joining.then_some(RaidNotice::Joined(Party::Player))
    }

    /// A member in the player's raid: one who joined, or one the server
    /// lists, who says nothing.
    fn raid_added(&mut self, member: &RaidMember, listing: bool) -> Option<RaidNotice> {
        let party = self.raid_party(&member.name);
        let members = &mut self.raid.get_or_insert_with(Raid::default).members;
        let known = members.iter().position(|known| known.name == member.name);
        match known {
            Some(place) => members[place] = member.clone(),
            None => members.push(member.clone()),
        }
        (known.is_none() && !listing && party != Party::Player).then_some(RaidNotice::Joined(party))
    }
}
