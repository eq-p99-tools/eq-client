//! The player's group, as the server describes it, and the invitation
//! waiting for the player's answer. The server sends no line with its group
//! news, nor with the requests the session sends for the player, so the
//! official client words each itself (inferred from its strings); the world
//! says which happened.
use super::{Changes, ClientWorld, Notice, Party};
use crate::group::GroupUpdate;

/// The player's group, as the server described it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    /// Its leader, once the server has named them.
    pub leader: Option<String>,
    /// The other members: as the server last listed them, and then in the
    /// order they joined.
    pub members: Vec<String>,
}

/// What news of groups tells the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupNotice {
    /// The player invited someone to their group.
    Inviting(String),
    /// Someone invited the player to their group.
    Invited(String),
    /// The player agreed to join the inviter's group.
    Following(String),
    /// The player declined the inviter's invitation.
    Declining(String),
    /// The one the player invited declined.
    Declined(String),
    /// The first to join formed a group with the player, who leads it.
    Formed,
    /// The player, or someone else, joined the player's group.
    Joined(Party),
    /// The player, or someone else, left the player's group or was removed.
    Left(Party),
    /// The player's group has a new leader.
    Leader(Party),
    /// The player's group was disbanded.
    Disbanded,
}

impl ClientWorld {
    /// The player's group, while they are in one.
    #[must_use]
    pub const fn group(&self) -> Option<&Group> {
        self.group.as_ref()
    }

    /// Who invited the player to their group, while the invitation waits
    /// for an answer.
    #[must_use]
    pub fn group_invitation(&self) -> Option<&str> {
        self.group_invitation.as_deref()
    }

    /// Whether the player leads their group.
    #[must_use]
    pub fn leads_group(&self) -> bool {
        self.group
            .as_ref()
            .and_then(|group| group.leader.as_deref())
            .is_some_and(|leader| self.is_named(leader))
    }

    /// A group member in the zone, by name; None for one elsewhere.
    #[must_use]
    pub fn group_member(&self, name: &str) -> Option<&super::Spawn> {
        self.zone.spawns.values().find(|spawn| {
            spawn.state.kind == crate::SpawnKind::Player
                && spawn.state.name.eq_ignore_ascii_case(name)
        })
    }

    /// Whether a name is the player's.
    fn is_named(&self, name: &str) -> bool {
        self.player
            .as_ref()
            .is_some_and(|player| player.name.eq_ignore_ascii_case(name))
    }

    /// Who a name in group news is: the player, or another by name.
    fn group_party(&self, name: &str) -> Party {
        if self.is_named(name) {
            Party::Player
        } else {
            Party::Named(name.to_owned())
        }
    }

    /// Follows news of groups, and says what it tells the player.
    pub(super) fn group_news(&mut self, update: &GroupUpdate, news: &mut Changes) {
        let notice = match update {
            GroupUpdate::Inviting { player } => Some(GroupNotice::Inviting(player.clone())),
            GroupUpdate::Following { inviter } => {
                self.group_invitation = None;
                self.joining_group = true;
                Some(GroupNotice::Following(inviter.clone()))
            }
            GroupUpdate::Declining { inviter } => {
                self.group_invitation = None;
                Some(GroupNotice::Declining(inviter.clone()))
            }
            GroupUpdate::Invited { inviter } => {
                self.group_invitation = Some(inviter.clone());
                Some(GroupNotice::Invited(inviter.clone()))
            }
            // The member's join, which comes first, says it.
            GroupUpdate::Accepted { .. } => None,
            GroupUpdate::Declined { member } => Some(GroupNotice::Declined(member.clone())),
            GroupUpdate::Formed => {
                self.group = Some(Group {
                    leader: self.player.as_ref().map(|player| player.name.clone()),
                    members: Vec::new(),
                });
                Some(GroupNotice::Formed)
            }
            GroupUpdate::Joined { member } => {
                let party = self.group_party(member);
                let members = &mut self.group.get_or_insert_with(Group::default).members;
                if party != Party::Player
                    && !members.iter().any(|name| name.eq_ignore_ascii_case(member))
                {
                    members.push(member.clone());
                }
                Some(GroupNotice::Joined(party))
            }
            GroupUpdate::Left { member } => {
                let party = self.group_party(member);
                if party == Party::Player {
                    self.group = None;
                } else if let Some(group) = self.group.as_mut() {
                    group
                        .members
                        .retain(|name| !name.eq_ignore_ascii_case(member));
                }
                Some(GroupNotice::Left(party))
            }
            // The list the server sends a player who joined, and again after
            // each zone's admission; only joining says so. A list naming no
            // leader leaves the leader unknown.
            GroupUpdate::Members { leader, members } => {
                self.group = Some(Group {
                    leader: (!leader.is_empty()).then(|| leader.clone()),
                    members: members.clone(),
                });
                std::mem::take(&mut self.joining_group)
                    .then_some(GroupNotice::Joined(Party::Player))
            }
            GroupUpdate::Leader { name } => {
                if let Some(group) = self.group.as_mut() {
                    group.leader = Some(name.clone());
                }
                Some(GroupNotice::Leader(self.group_party(name)))
            }
            GroupUpdate::Disbanded => {
                self.group = None;
                Some(GroupNotice::Disbanded)
            }
        };
        // Being in a group answers any invitation, and leaving one ends any
        // joining.
        if self.group.is_some() {
            self.group_invitation = None;
        } else if matches!(update, GroupUpdate::Left { .. } | GroupUpdate::Disbanded) {
            self.joining_group = false;
        }
        news.notices.extend(notice.map(Notice::Group));
    }
}
