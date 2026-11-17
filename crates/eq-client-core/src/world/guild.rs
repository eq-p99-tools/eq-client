//! The player's guild as the server tells it: its members, kept from the
//! full list the server sends at zone entry and the changes after it, and
//! an invitation waiting for an answer.
use super::{ClientWorld, Reset};
use crate::guild::{GuildMember, GuildUpdate};
use crate::listing::GuildRank;

/// An invitation into a guild, or to a higher rank in it, that waits for
/// the player's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildInvitation {
    /// Who invites.
    pub inviter: String,
    /// The guild.
    pub guild_id: u32,
    /// The rank offered.
    pub rank: GuildRank,
}

impl ClientWorld {
    /// The guild's members as the server last listed them and changed them
    /// since. A bound transfer of the same character keeps this last
    /// received evidence; other admission boundaries require a new list.
    #[must_use]
    pub fn guild_members(&self) -> Option<&[GuildMember]> {
        self.guild_members.as_deref()
    }

    /// A listed member's public note, once a full list or note update supplied
    /// it. An add-member update has no note field; that note remains unknown.
    #[must_use]
    pub fn guild_member_note(&self, name: &str) -> Option<&str> {
        if self.guild_unknown_notes.contains(name) {
            return None;
        }
        self.guild_members()?
            .iter()
            .find(|member| member.name == name)
            .map(|member| member.note.as_str())
    }

    /// Takes the oldest guild invitation not yet answered or asked about.
    pub fn take_guild_invitation(&mut self) -> Option<GuildInvitation> {
        self.guild_invitations.pop_front()
    }

    /// Whether looted coins are shared with the group, once the profile
    /// said.
    #[must_use]
    pub const fn auto_split(&self) -> Option<bool> {
        self.auto_split
    }
}

/// The listed member of this name.
fn member<'a>(
    members: &'a mut Option<Vec<GuildMember>>,
    name: &str,
) -> Option<&'a mut GuildMember> {
    members
        .as_mut()
        .and_then(|members| members.iter_mut().find(|member| member.name == name))
}

impl ClientWorld {
    /// Whether a positively identified character has a session-reported
    /// transfer. Link loss without such a destination clears the list.
    fn guild_transfer_pending(&self) -> bool {
        self.world_name
            .as_ref()
            .is_some_and(|name| !name.is_empty())
            && self.session_id.is_some()
            && self
                .player()
                .and_then(|player| player.listing.guild)
                .is_some()
            && self.pending_transfer.is_some()
    }

    /// Whether the next admission completes that same character's offered
    /// transfer. World changes withdraw roster evidence when their event arrives.
    pub(super) fn guild_follows_transfer(
        &self,
        player: &crate::PlayerState,
        zone_id: u16,
        session_id: u64,
    ) -> bool {
        self.guild_transfer_pending()
            && self.session_id != Some(session_id)
            && self
                .pending_transfer
                .as_ref()
                .is_some_and(|offer| offer.zone_id == zone_id)
            && self.player().is_some_and(|old| {
                old.name == player.name && old.listing.guild == player.listing.guild
            })
    }

    /// Discards roster and invitation evidence at a membership or admission boundary.
    pub(super) fn forget_guild(&mut self) {
        self.guild_members = None;
        self.guild_unknown_notes.clear();
        self.guild_invitations.clear();
    }

    /// Preserves last-received membership during a bound transfer;
    /// invitations remain zone-scoped even when the member list survives.
    pub(super) fn reset_guild(&mut self, reason: Reset) {
        if reason == Reset::Died {
            return;
        }
        let transferring = matches!(
            reason,
            Reset::Zoning { .. }
                | Reset::Lost {
                    ended: false,
                    transferring: true
                }
        );
        if !transferring || !self.guild_transfer_pending() {
            self.forget_guild();
        }
        self.guild_invitations.clear();
    }

    /// An add has no note or flags: retain prior reports for an existing
    /// member and keep a new member's absent note explicitly unknown.
    fn guild_member_added(&mut self, update: &GuildUpdate) {
        let GuildUpdate::MemberAdded {
            name,
            level,
            class,
            rank,
            zone_id,
            last_on,
            ..
        } = update
        else {
            return;
        };
        let Some(members) = &mut self.guild_members else {
            return;
        };
        if let Some(member) = members.iter_mut().find(|member| member.name == *name) {
            member.level = *level;
            member.class = *class;
            member.rank = *rank;
            member.zone_id = *zone_id;
            member.last_on = *last_on;
            return;
        }
        members.push(GuildMember {
            name: name.clone(),
            level: *level,
            class: *class,
            rank: *rank,
            banker: false,
            alt: false,
            last_on: *last_on,
            tribute: crate::guild::GuildTribute {
                on: false,
                total: 0,
                last: 0,
            },
            note: String::new(),
            zone_id: *zone_id,
        });
        self.guild_unknown_notes.insert(name.clone());
    }

    /// Keeps the guild's news.
    pub(super) fn guild_news(&mut self, update: &GuildUpdate) {
        let guild_id = match update {
            GuildUpdate::MemberLocation { guild_id, .. }
            | GuildUpdate::MemberAdded { guild_id, .. }
            | GuildUpdate::MemberRemoved { guild_id, .. }
            | GuildUpdate::MemberLevel { guild_id, .. }
            | GuildUpdate::MemberNote { guild_id, .. }
            | GuildUpdate::MemberRenamed { guild_id, .. }
            | GuildUpdate::Renamed { guild_id, .. }
            | GuildUpdate::Disbanded { guild_id } => Some(*guild_id),
            GuildUpdate::MemberRank { guild_id, .. } => *guild_id,
            GuildUpdate::Members(_) | GuildUpdate::Invite { .. } => None,
        };
        if let (Some(current), Some(received)) = (
            self.player().and_then(|player| player.listing.guild),
            guild_id,
        ) && current != received
        {
            return;
        }
        match update {
            GuildUpdate::Members(members) => {
                self.guild_members = Some(members.clone());
                self.guild_unknown_notes.clear();
            }
            GuildUpdate::MemberLocation {
                name,
                zone_id,
                last_on,
                ..
            } => {
                if let Some(member) = member(&mut self.guild_members, name) {
                    member.zone_id = *zone_id;
                    member.last_on = *last_on;
                }
            }
            GuildUpdate::MemberAdded { .. } => self.guild_member_added(update),
            GuildUpdate::MemberRemoved { name, .. } => {
                if self
                    .player()
                    .is_some_and(|player| player.name.eq_ignore_ascii_case(name))
                {
                    self.forget_guild();
                    return;
                }
                if let Some(members) = &mut self.guild_members {
                    members.retain(|member| member.name != *name);
                }
                self.guild_unknown_notes.remove(name);
            }
            GuildUpdate::MemberLevel { name, level, .. } => {
                if let Some(member) = member(&mut self.guild_members, name) {
                    member.level = *level;
                }
            }
            GuildUpdate::MemberNote { name, note, .. } => {
                if let Some(member) = member(&mut self.guild_members, name) {
                    member.note.clone_from(note);
                    self.guild_unknown_notes.remove(name);
                }
            }
            GuildUpdate::MemberRank {
                name,
                rank,
                banker,
                alt,
                guild_id,
            } => {
                if let Some(member) = member(&mut self.guild_members, name) {
                    member.rank = *rank;
                    member.banker = *banker;
                    // `OP_SetGuildRank` names no guild and sets no alt flag.
                    if guild_id.is_some() {
                        member.alt = *alt;
                    }
                }
            }
            GuildUpdate::MemberRenamed {
                old_name, new_name, ..
            } => {
                if let Some(member) = member(&mut self.guild_members, old_name) {
                    member.name.clone_from(new_name);
                    if self.guild_unknown_notes.remove(old_name) {
                        self.guild_unknown_notes.insert(new_name.clone());
                    }
                }
            }
            GuildUpdate::Renamed { .. } => (),
            GuildUpdate::Disbanded { .. } => self.forget_guild(),
            GuildUpdate::Invite {
                inviter,
                guild_id,
                rank,
            } => {
                self.guild_invitations.push_back(GuildInvitation {
                    inviter: inviter.clone(),
                    guild_id: *guild_id,
                    rank: *rank,
                });
            }
        }
    }
}
