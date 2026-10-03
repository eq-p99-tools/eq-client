//! The player's raid, as the server describes it, and the invitation
//! waiting for the player's answer. The server sends no line with its raid
//! news, nor with the requests the session sends for the player, so the
//! official client words each itself (inferred from its strings); the world
//! says which happened.
//!
//! The server lists every member as one joins or as the player enters a
//! zone: after the raid's leader, each member in turn, then the leader
//! again. Only a member added after that list says they joined.
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
}

/// What the world remembers of raid news on the way: an invitation the
/// player sent or accepted, and the list the server is sending.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RaidFlow {
    /// The player invited someone, and no raid has formed since.
    inviting: bool,
    /// The player accepted an invitation, and no raid has come since.
    joining: bool,
    /// The server is listing the raid's members, until it names the leader.
    listing: bool,
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
            RaidUpdate::Added(member) => self.raid_added(member),
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
                if let Some(raid) = self.raid.as_mut() {
                    raid.leader = Some(name.clone());
                }
                // The leader named after a list ends it, and says nothing.
                (!std::mem::take(&mut self.raid_flow.listing))
                    .then(|| RaidNotice::Leader(self.raid_party(name)))
            }
        };
        // Being in a raid answers any invitation.
        if self.raid.is_some() {
            self.raid_invitation = None;
        }
        news.notices.extend(notice.map(Notice::Raid));
    }

    /// The player is in a raid: one they formed by inviting, one they
    /// joined, or theirs again as they enter a zone, whose members follow.
    fn raid_created(&mut self, leader: &str) -> Option<RaidNotice> {
        self.raid = Some(Raid {
            leader: Some(leader.to_owned()),
            members: Vec::new(),
        });
        let flow = std::mem::take(&mut self.raid_flow);
        if flow.inviting && self.is_named(leader) {
            return Some(RaidNotice::Formed);
        }
        self.raid_flow.listing = true;
        flow.joining.then_some(RaidNotice::Joined(Party::Player))
    }

    /// A member in the player's raid: one who joined, unless the server is
    /// listing the raid.
    fn raid_added(&mut self, member: &RaidMember) -> Option<RaidNotice> {
        let party = self.raid_party(&member.name);
        let listing = self.raid_flow.listing;
        let members = &mut self.raid.get_or_insert_with(Raid::default).members;
        let known = members.iter().position(|known| known.name == member.name);
        match known {
            Some(place) => members[place] = member.clone(),
            None => members.push(member.clone()),
        }
        (known.is_none() && !listing && party != Party::Player).then_some(RaidNotice::Joined(party))
    }
}
