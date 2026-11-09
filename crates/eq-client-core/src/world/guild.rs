//! The player's guild as the server tells it: its members, kept from the
//! full list the server sends at zone entry and the changes after it, and
//! an invitation waiting for an answer.
use super::ClientWorld;
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
    /// The guild's members as the server last listed them and changed
    /// them since; None before a list came this admission.
    #[must_use]
    pub fn guild_members(&self) -> Option<&[GuildMember]> {
        self.guild_members.as_deref()
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
    /// Keeps the guild's news.
    pub(super) fn guild_news(&mut self, update: &GuildUpdate) {
        match update {
            GuildUpdate::Members(members) => self.guild_members = Some(members.clone()),
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
            GuildUpdate::MemberAdded {
                name,
                level,
                class,
                rank,
                zone_id,
                last_on,
                ..
            } => {
                if let Some(members) = &mut self.guild_members {
                    members.retain(|member| member.name != *name);
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
                }
            }
            GuildUpdate::MemberRemoved { name, .. } => {
                if let Some(members) = &mut self.guild_members {
                    members.retain(|member| member.name != *name);
                }
            }
            GuildUpdate::MemberLevel { name, level, .. } => {
                if let Some(member) = member(&mut self.guild_members, name) {
                    member.level = *level;
                }
            }
            GuildUpdate::MemberNote { name, note, .. } => {
                if let Some(member) = member(&mut self.guild_members, name) {
                    member.note.clone_from(note);
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
                }
            }
            GuildUpdate::Renamed { .. } => (),
            GuildUpdate::Disbanded { .. } => self.guild_members = None,
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
