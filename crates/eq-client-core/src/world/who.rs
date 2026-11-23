//! The zone's own `/who`: the Titanium client lists the zone's players from
//! what it knows of them, the guilds they belong to by the world's guild
//! list, and how each wants to be listed.
use super::{Changes, ClientWorld, ListingNotice, Notice};
use crate::{
    SpawnKind,
    listing::{Anonymity, GuildRank, Listing, ListingChange},
    names::{NameParts, TitleChange},
    who::WhoFilter,
};

/// One player the zone's own `/who` lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZonePlayer {
    /// The player's name.
    pub name: String,
    /// Level.
    pub level: u8,
    /// Class number, when known.
    pub class: Option<u32>,
    /// Race number.
    pub race: u32,
    /// The guild's name, when the player has a guild the client knows.
    pub guild: Option<String>,
    /// How the player is listed.
    pub listing: Listing,
}

impl ZonePlayer {
    /// Whether a `/who` filter takes the player. Anonymous players hide
    /// their level, class and race, so a filter on those passes them by; a
    /// name, guild or zone start matches any player.
    fn matches(&self, filter: &WhoFilter, zone: &str) -> bool {
        let starts = |text: &str| {
            text.get(..filter.text.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(&filter.text))
        };
        let open = self.listing.anonymity == Anonymity::Open;
        (filter.text.is_empty()
            || starts(&self.name)
            || starts(zone)
            || self.guild.as_deref().is_some_and(starts))
            && filter.race.is_none_or(|race| open && race == self.race)
            && filter
                .class
                .is_none_or(|class| open && self.class == Some(u32::from(class)))
            && filter
                .levels
                .is_none_or(|(low, high)| open && (low..=high).contains(&self.level))
            && (!filter.game_masters || self.listing.game_master)
    }
}

/// What `/guildstatus` says of the target or a named player (checklist
/// 6.575). Read R49-6k: the program answers locally and sends nothing, from
/// the spawn a typed name finds or else the target, in strings 12256 to
/// 12260 by rank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuildStatus {
    /// No such spawn, one that isn't a player, or a game master.
    NoPlayer,
    /// The player holds a rank past the three the program words: its line
    /// names only the player.
    Ranked {
        /// The player's name.
        name: String,
    },
    /// The player is in no guild.
    NoGuild {
        /// The player's name.
        name: String,
    },
    /// The player leads the guild.
    Leader {
        /// The player's name.
        name: String,
        /// The guild's name.
        guild: String,
    },
    /// The player is one of the guild's officers.
    Officer {
        /// The player's name.
        name: String,
        /// The guild's name.
        guild: String,
    },
    /// The player is one of the guild's members.
    Member {
        /// The player's name.
        name: String,
        /// The guild's name.
        guild: String,
    },
}

/// Applies a change in how `/who` lists a player.
pub(super) fn relist(change: ListingChange, level: &mut u8, listing: &mut Listing) {
    match change {
        ListingChange::Level(new) => *level = new,
        other => listing.apply(other),
    }
}

/// What the player hears when their anonymity changes. Both programs keep
/// it as two bits that `/anonymous` and `/roleplay` set apart: `/roleplay`
/// never touches the anonymous bit, so a change of that bit is
/// `/anonymous`'s line, and otherwise the roleplaying bit's (Titanium
/// sitting 9l: 13233 when the value is over 1, else 13234).
fn anonymity_notice(before: Anonymity, after: Anonymity) -> Option<ListingNotice> {
    let roleplaying = |anonymity: Anonymity| anonymity.wire() & 2 != 0;
    if before.anonymous() != after.anonymous() {
        Some(ListingNotice::Anonymous(after.anonymous()))
    } else if roleplaying(before) != roleplaying(after) {
        Some(ListingNotice::Roleplaying(roleplaying(after)))
    } else {
        None
    }
}

impl ClientWorld {
    /// The session changed how `/who` lists the player, as they asked: the
    /// player's listing follows, and the player hears of it.
    pub(super) fn listing_set(
        &mut self,
        session_id: u64,
        change: ListingChange,
        news: &mut Changes,
    ) {
        if self.session_id != Some(session_id) {
            news.ignored = true;
            return;
        }
        let Some(player) = self.player.as_mut() else {
            return;
        };
        let before = player.listing.anonymity;
        let guild = player.listing.guild;
        relist(change, &mut player.level, &mut player.listing);
        let notice = match change {
            ListingChange::Away(away) => Some(ListingNotice::Away(away)),
            ListingChange::Anonymity(after) => anonymity_notice(before, after),
            _ => None,
        };
        if guild != player.listing.guild {
            self.forget_guild();
        }
        news.notices.extend(notice.map(Notice::Listing));
    }

    /// Gives a character, by the name they spawned with, a new last name.
    pub(super) fn last_name(&mut self, name: &str, last_name: &str) {
        let spawns = self.zone.spawns.values_mut().map(|spawn| &mut spawn.state);
        for state in spawns.filter(|state| state.name == name) {
            last_name.clone_into(&mut state.name_parts.last_name);
        }
        if let Some(player) = self.player.as_mut().filter(|player| player.name == name) {
            last_name.clone_into(&mut player.name_parts.last_name);
        }
    }

    /// A spawn's new title or suffix; the player's own too when the spawn
    /// is theirs. `EQMac` says neither which changed nor to what, so its
    /// change keeps both.
    pub(super) fn retitle(&mut self, spawn_id: u16, change: &TitleChange) {
        let retitle = |parts: &mut NameParts| match change {
            TitleChange::Title(title) => title.clone_into(&mut parts.title),
            TitleChange::Suffix(suffix) => suffix.clone_into(&mut parts.suffix),
            TitleChange::Unspecified => (),
        };
        if let Some(spawn) = self.zone.spawns.get_mut(&spawn_id) {
            retitle(&mut spawn.state.name_parts);
        }
        if self.is_player(spawn_id)
            && let Some(player) = self.player.as_mut()
        {
            retitle(&mut player.name_parts);
        }
    }

    /// A guild's name, by its number in the world's guild list.
    #[must_use]
    pub fn guild_name(&self, number: u32) -> Option<&str> {
        self.guild_names.get(&number).map(String::as_str)
    }

    /// Whether a living player of this name, in any case, is in the zone:
    /// whom the official client lets `/guildinvite` name (R11, 0x49bf15).
    #[must_use]
    pub fn player_in_zone(&self, name: &str) -> bool {
        self.player
            .as_ref()
            .is_some_and(|player| player.name.eq_ignore_ascii_case(name))
            || self
                .zone
                .spawns
                .values()
                .map(|spawn| &spawn.state)
                .any(|spawn| {
                    spawn.kind == SpawnKind::Player && spawn.name.eq_ignore_ascii_case(name)
                })
    }

    /// Whether a player of this name, in any case, is in the zone as a game
    /// master: the test the official client's ignore list makes of a sender
    /// (program read R6, 0x459b94 and 0x472739).
    #[must_use]
    pub fn game_master_in_zone(&self, name: &str) -> bool {
        self.zone
            .spawns
            .values()
            .map(|spawn| &spawn.state)
            .any(|spawn| {
                spawn.kind == SpawnKind::Player
                    && spawn.listing.game_master
                    && spawn.name.eq_ignore_ascii_case(name)
            })
    }

    /// What `/guildstatus` says (read R49-6k): of the player a typed name
    /// finds, or else of the target. None while the guild's name or the
    /// rank hasn't come.
    ///
    /// The program also refuses a spawn whose byte at +0x256 is set. Read
    /// R49-4 takes that byte as the GM flag (a spawn with it sends every
    /// tongue at skill 100), so a spawn the server marks a game master is
    /// refused here (inferred). Anonymity isn't tested: what the server sends
    /// of an anonymous player's guild is shown. The name lookup matches the
    /// player's name or shown name, ignoring case (inferred: 0x47c88e's
    /// rule wasn't read).
    #[must_use]
    pub fn guild_status(&self, named: Option<&str>) -> Option<GuildStatus> {
        let matches = |shown: &str, wanted: &str| {
            shown.eq_ignore_ascii_case(wanted)
                || crate::entities::display_name(shown).eq_ignore_ascii_case(wanted)
        };
        let found = match named {
            Some(wanted) => self
                .player
                .as_ref()
                .filter(|player| matches(&player.name, wanted))
                .map(|player| player.spawn_id)
                .or_else(|| {
                    self.zone
                        .spawns
                        .iter()
                        .find(|(_, spawn)| matches(&spawn.state.name, wanted))
                        .map(|(id, _)| *id)
                }),
            None => self.target.selected.filter(|_| !self.target_stale()),
        };
        let game_master = |id: &u16| {
            let flag = if self.is_player(*id) {
                self.player
                    .as_ref()
                    .and_then(|player| player.appearance.titanium_gm)
            } else {
                self.zone
                    .spawns
                    .get(id)
                    .and_then(|spawn| spawn.state.appearance.titanium_gm)
            };
            flag.is_some_and(|flag| flag != 0)
        };
        let listed = found.filter(|id| !game_master(id)).and_then(|id| {
            if self.is_player(id) {
                self.player
                    .as_ref()
                    .map(|player| (player.name.as_str(), player.listing))
            } else {
                self.zone
                    .spawns
                    .get(&id)
                    .map(|spawn| &spawn.state)
                    .filter(|spawn| spawn.kind == SpawnKind::Player)
                    .map(|spawn| (spawn.name.as_str(), spawn.listing))
            }
        });
        let Some((name, listing)) = listed else {
            return Some(GuildStatus::NoPlayer);
        };
        let name = name.to_owned();
        let Some(number) = listing.guild else {
            return Some(GuildStatus::NoGuild { name });
        };
        let guild = self.guild_name(number)?.to_owned();
        Some(match listing.guild_rank? {
            GuildRank::Leader => GuildStatus::Leader { name, guild },
            GuildRank::Officer => GuildStatus::Officer { name, guild },
            GuildRank::Member => GuildStatus::Member { name, guild },
            GuildRank::Other(_) => GuildStatus::Ranked { name },
        })
    }

    /// The players the zone's own `/who` lists, by name: the player and every
    /// other player the zone told the client of, as the filter takes them.
    #[must_use]
    pub fn zone_who(&self, filter: &WhoFilter) -> Vec<ZonePlayer> {
        let guild = |listing: &Listing| {
            listing
                .guild
                .and_then(|number| self.guild_name(number))
                .map(str::to_owned)
        };
        let mut players: Vec<ZonePlayer> = self
            .player
            .iter()
            .map(|player| ZonePlayer {
                name: player.name.clone(),
                level: player.level,
                class: player.class,
                race: player.race,
                guild: guild(&player.listing),
                listing: player.listing,
            })
            .chain(
                self.zone
                    .spawns
                    .values()
                    .map(|spawn| &spawn.state)
                    .filter(|spawn| {
                        spawn.kind == SpawnKind::Player && !self.is_player(spawn.spawn_id)
                    })
                    .map(|spawn| ZonePlayer {
                        name: spawn.name.clone(),
                        level: spawn.level,
                        class: spawn.class.map(u32::from),
                        race: spawn.race,
                        guild: guild(&spawn.listing),
                        listing: spawn.listing,
                    }),
            )
            .filter(|player| player.matches(filter, &self.zone_name))
            .collect();
        players.sort_by(|first, second| first.name.cmp(&second.name));
        players
    }
}
