//! The skin's Raid window: the members of the player's raid, those in a
//! raid group in one list and the rest in the other, each with their raid
//! group, level, class and rank, and how many there are and their average
//! level; its buttons invite, leave, accept and decline
//! (`skinned::raid_button`). Alt+R and `/raidwindow` open and close it, as
//! the official client's notes on raids say (`raidsdoc.txt`), as the
//! window stack toggles any window, and a raid invitation opens it
//! (inferred: the installed line for one points the player to its Accept
//! button).
use super::{
    hud::messages::Messages,
    online::OnlineState,
    theme::{self, Size},
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{
    raid::RaidMember,
    world::{ClientWorld, Raid, RaidRank},
};

/// Which of the Raid window's lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RaidList {
    /// The members in a raid group.
    Grouped,
    /// The members in none.
    Ungrouped,
}

/// A list of the Raid window, which the client fills from the player's
/// raid.
#[derive(Component, Clone, Debug)]
pub(crate) struct RaidRows {
    list: RaidList,
    /// Each column's width, from the left.
    columns: Vec<f32>,
    /// The rows last drawn, to redraw only when one changes.
    drawn: Option<Vec<Row>>,
}

impl RaidRows {
    pub(crate) const fn new(list: RaidList, columns: Vec<f32>) -> Self {
        Self {
            list,
            columns,
            drawn: None,
        }
    }
}

/// A row's words, by the skin's columns: raid group, name, level, class,
/// role and rank.
type Row = [String; 6];

/// The installed strings that name the ranks, with this client's words for
/// an installation without them. That the official client's rank column
/// shows them is inferred from what they say.
const RAID_LEADER: (u32, &str) = (8657, "Leads raid");
const GROUP_LEADER: (u32, &str) = (8656, "Leads group");

/// A member's row: their raid group counted from one, as the window's group
/// buttons count them (inferred), their name, level and class, a role left
/// empty until the client follows who assists and marks, and their rank,
/// left empty for a member without one.
fn row(raid: &Raid, member: &RaidMember, messages: &Messages) -> Row {
    let rank = match raid.rank(member) {
        RaidRank::Leader => Some(RAID_LEADER),
        RaidRank::GroupLeader => Some(GROUP_LEADER),
        RaidRank::Member => None,
    };
    [
        member
            .group
            .map(|group| (u32::from(group) + 1).to_string())
            .unwrap_or_default(),
        member.name.clone(),
        member.level.to_string(),
        eq_client_core::classes::class_name(u32::from(member.class))
            .map_or_else(|| member.class.to_string(), str::to_owned),
        String::new(),
        rank.map(|(id, fallback)| messages.said_or(id, &[], fallback).text)
            .unwrap_or_default(),
    ]
}

/// A list's rows: the members of the player's raid it holds, none outside
/// a raid.
fn rows(world: &ClientWorld, list: RaidList, messages: &Messages) -> Vec<Row> {
    let Some(raid) = world.raid() else {
        return Vec::new();
    };
    let members: Vec<&RaidMember> = match list {
        RaidList::Grouped => raid.grouped(),
        RaidList::Ungrouped => raid.ungrouped().collect(),
    };
    members
        .into_iter()
        .map(|member| row(raid, member, messages))
        .collect()
}

/// The height of a row of a list.
const ROW_HEIGHT: f32 = 16.0;

/// Fills the Raid window's lists again whenever the raid changes.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn fill(
    mut commands: Commands,
    online: Res<OnlineState>,
    messages: Option<Res<Messages>>,
    mut lists: Query<(Entity, &mut RaidRows, Option<&Children>)>,
) {
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    for (entity, mut list, children) in &mut lists {
        let wanted = rows(online.world(), list.list, messages);
        if list.drawn.as_ref() == Some(&wanted) {
            continue;
        }
        if let Some(children) = children {
            for child in children {
                commands.entity(*child).despawn();
            }
        }
        for cells in &wanted {
            let line = commands
                .spawn((
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ChildOf(entity),
                ))
                .id();
            for (words, width) in cells.iter().zip(&list.columns) {
                commands.spawn((
                    theme::text(words.as_str(), Size::Small, theme::INK_BRIGHT),
                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    Node {
                        width: px(*width),
                        flex_shrink: 0.0,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    ChildOf(line),
                ));
            }
        }
        list.drawn = Some(wanted);
    }
}

/// Opens the Raid window as a raid invitation comes.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn window(online: Res<OnlineState>, mut shown: ResMut<Shown>, mut invited: Local<bool>) {
    let waiting = online.world().raid_invitation().is_some();
    if waiting && !*invited {
        shown.open(WindowId::Raid);
    }
    *invited = waiting;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{WorldEvent, raid::RaidUpdate};

    fn news(online: &mut OnlineState, updates: impl IntoIterator<Item = RaidUpdate>) {
        testing::news(online, updates.into_iter().map(WorldEvent::Raid));
    }

    fn member(name: &str, group: Option<u8>, class: u8, group_leader: bool) -> RaidUpdate {
        RaidUpdate::Added(RaidMember {
            name: name.into(),
            group,
            class,
            level: 20,
            group_leader,
        })
    }

    #[test]
    fn the_lists_show_each_member_by_raid_group_with_their_rank() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let messages = Messages::parse("EQST0002\n0 1\n8657 Raid chief\n");
        assert_eq!(
            rows(online.world(), RaidList::Grouped, &messages),
            Vec::<Row>::new()
        );
        news(
            &mut online,
            [
                RaidUpdate::Created {
                    leader: "Leader".into(),
                },
                member("Leader", None, 1, false),
                member("Second", Some(3), 2, true),
                member("First", Some(0), 5, false),
                RaidUpdate::Leader {
                    name: "Leader".into(),
                },
            ],
        );
        let text = |rows: Vec<Row>| {
            rows.into_iter()
                .map(|row| row.join("|"))
                .collect::<Vec<_>>()
        };
        // Raid groups count from one; a group leader's rank is this
        // client's words where the installation lacks the string.
        assert_eq!(
            text(rows(online.world(), RaidList::Grouped, &messages)),
            [
                "1|First|20|Shadow Knight||",
                "4|Second|20|Cleric||Leads group"
            ]
        );
        assert_eq!(
            text(rows(online.world(), RaidList::Ungrouped, &messages)),
            ["|Leader|20|Warrior||Raid chief"]
        );
    }

    #[test]
    fn an_invitation_or_raidwindow_opens_the_window() {
        let mut app = crate::testing::app();
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        app.insert_resource(online)
            .add_systems(Update, (super::window, crate::windows::toggle));
        let open = |app: &App| app.world().resource::<Shown>().is_open(WindowId::Raid);
        app.update();
        assert!(!open(&app));
        news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [RaidUpdate::Invited {
                inviter: "Leader".into(),
            }],
        );
        app.update();
        assert!(open(&app));
        // The player closes it while the invitation waits: it stays shut.
        app.world_mut()
            .resource_mut::<Shown>()
            .close(WindowId::Raid);
        app.update();
        assert!(!open(&app));
        // `/raidwindow` opens and closes it.
        for wanted in [true, false] {
            let mut chat = app.world_mut().resource_mut::<crate::chat::ChatState>();
            assert_eq!(
                crate::chat::client_request("/raidwindow", &mut chat),
                Some(Ok(()))
            );
            app.update();
            assert_eq!(open(&app), wanted);
        }
    }
}
