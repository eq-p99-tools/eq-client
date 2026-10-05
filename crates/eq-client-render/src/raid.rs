//! The skin's Raid window: the members of the player's raid, those in a
//! raid group in one list and the rest in the other, each with their raid
//! group, level, class and rank, and how many there are and their average
//! level. A click on a row chooses that member, as the official client's
//! notes on raids (`raidsdoc.txt`) describe the window's target. Its buttons
//! (`skinned::raid_button`) invite, accept and decline; Disband removes the
//! member chosen, or else leaves; and the leader's lock and unlock the raid,
//! move the member chosen between raid groups and hand them the lead. Alt+R
//! and `/raidwindow` open and close it, as those notes say, as the window
//! stack toggles any window, and a raid invitation opens it (inferred: the
//! installed line for one points the player to its Accept button).
use super::{
    hud::messages::Messages,
    online::OnlineState,
    outbox::Outbox,
    theme::{self, Size},
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand,
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
    drawn: Option<Vec<Line>>,
}

/// The member chosen in the Raid window, by the name the server gave them:
/// the one Disband, the group buttons and Make Leader act on.
#[derive(Resource, Default)]
pub(crate) struct RaidChoice(pub(crate) Option<String>);

/// A row of a Raid window list, which a click chooses: the member it shows,
/// and its list and place from the top.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RaidRow {
    pub(crate) list: RaidList,
    pub(crate) index: usize,
    name: String,
}

/// What a Raid window button does that no slash command does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RaidAction {
    /// Removes the member chosen, or else leaves the raid.
    Disband,
    /// Locks the raid (true) or unlocks it.
    Lock(bool),
    /// Moves the member chosen into a raid group, 0 to 11, or out of every
    /// group.
    Move(Option<u8>),
    /// Hands the member chosen the lead.
    MakeLeader,
}

impl RaidAction {
    /// The command it sends, for the member chosen. With no member chosen,
    /// a command that needs one names no one, and the session says so.
    fn command(self, chosen: Option<&str>, session_id: u64) -> ClientCommand {
        let name = || chosen.unwrap_or_default().to_owned();
        match self {
            Self::Disband => match chosen {
                Some(name) => ClientCommand::RaidRemove {
                    session_id,
                    name: name.to_owned(),
                },
                None => ClientCommand::RaidLeave { session_id },
            },
            Self::Lock(locked) => ClientCommand::RaidLock { session_id, locked },
            Self::Move(group) => ClientCommand::RaidMove {
                session_id,
                name: name(),
                group,
            },
            Self::MakeLeader => ClientCommand::RaidMakeLeader {
                session_id,
                name: name(),
            },
        }
    }
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

/// A row as drawn: the member, their words, and whether they are chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    name: String,
    row: Row,
    chosen: bool,
}

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
        rank.map(|(id, fallback)| messages.said_or(id, &[], fallback).words.text)
            .unwrap_or_default(),
    ]
}

/// A list's rows: the members of the player's raid it holds, none outside
/// a raid, and which is chosen.
fn lines(
    world: &ClientWorld,
    list: RaidList,
    chosen: Option<&str>,
    messages: &Messages,
) -> Vec<Line> {
    let Some(raid) = world.raid() else {
        return Vec::new();
    };
    let members: Vec<&RaidMember> = match list {
        RaidList::Grouped => raid.grouped(),
        RaidList::Ungrouped => raid.ungrouped().collect(),
    };
    members
        .into_iter()
        .map(|member| Line {
            name: member.name.clone(),
            row: row(raid, member, messages),
            chosen: chosen == Some(member.name.as_str()),
        })
        .collect()
}

/// A list's rows' words.
#[cfg(test)]
fn rows(world: &ClientWorld, list: RaidList, messages: &Messages) -> Vec<Row> {
    lines(world, list, None, messages)
        .into_iter()
        .map(|line| line.row)
        .collect()
}

/// The height of a row of a list.
const ROW_HEIGHT: f32 = 16.0;

/// Fills the Raid window's lists again whenever the raid or the choice
/// changes, the member chosen lit.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn fill(
    mut commands: Commands,
    (online, choice): (Res<OnlineState>, Res<RaidChoice>),
    messages: Option<Res<Messages>>,
    mut lists: Query<(Entity, &mut RaidRows, Option<&Children>)>,
) {
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    for (entity, mut list, children) in &mut lists {
        let wanted = lines(online.world(), list.list, choice.0.as_deref(), messages);
        if list.drawn.as_ref() == Some(&wanted) {
            continue;
        }
        if let Some(children) = children {
            for child in children {
                commands.entity(*child).despawn();
            }
        }
        for (index, wanted) in wanted.iter().enumerate() {
            let line = commands
                .spawn((
                    Button,
                    RaidRow {
                        list: list.list,
                        index,
                        name: wanted.name.clone(),
                    },
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(if wanted.chosen {
                        theme::BUTTON
                    } else {
                        Color::NONE
                    }),
                    ChildOf(entity),
                ))
                .id();
            for (words, width) in wanted.row.iter().zip(&list.columns) {
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

/// Chooses the member whose row is clicked, forgets one no longer in the
/// raid, and sends what a pressed button asks for the member chosen.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    (online, outbox): (Res<OnlineState>, Res<Outbox>),
    mut choice: ResMut<RaidChoice>,
    rows: Query<(&Interaction, &RaidRow), Changed<Interaction>>,
    actions: Query<(&Interaction, &RaidAction), Changed<Interaction>>,
) {
    for (interaction, row) in &rows {
        if *interaction == Interaction::Pressed && choice.0.as_deref() != Some(row.name.as_str()) {
            choice.0 = Some(row.name.clone());
        }
    }
    let world = online.world();
    let gone = choice.0.as_ref().is_some_and(|name| {
        !world
            .raid()
            .is_some_and(|raid| raid.members.iter().any(|member| member.name == *name))
    });
    if gone {
        choice.0 = None;
    }
    for (interaction, action) in &actions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let chosen = choice.0.as_deref();
        let _ = outbox.post(world, |stamp| action.command(chosen, stamp.session_id));
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
    fn the_leaders_buttons_act_for_the_member_chosen() {
        let (queue, received) = std::sync::mpsc::sync_channel(8);
        let mut app = crate::testing::app();
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        news(
            &mut online,
            [
                RaidUpdate::Created {
                    leader: "Example".into(),
                },
                member("Example", None, 1, false),
                member("Friend", None, 2, false),
            ],
        );
        app.insert_resource(online)
            .insert_resource(Outbox::new(Some(queue)))
            .add_systems(Update, buttons);
        let mut spawn = |bundle| {
            app.world_mut()
                .spawn((Button, Interaction::None, bundle))
                .id()
        };
        let disband = spawn(RaidAction::Disband);
        let first_group = spawn(RaidAction::Move(Some(0)));
        let row = app
            .world_mut()
            .spawn((
                Button,
                Interaction::None,
                RaidRow {
                    list: RaidList::Ungrouped,
                    index: 1,
                    name: "Friend".into(),
                },
            ))
            .id();
        let press = |app: &mut App, control| {
            *app.world_mut().get_mut::<Interaction>(control).unwrap() = Interaction::Pressed;
            app.update();
            *app.world_mut().get_mut::<Interaction>(control).unwrap() = Interaction::None;
            app.update();
        };
        // With no one chosen, Disband leaves.
        press(&mut app, disband);
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::RaidLeave { .. })
        ));
        press(&mut app, row);
        assert_eq!(
            app.world().resource::<RaidChoice>().0.as_deref(),
            Some("Friend")
        );
        press(&mut app, first_group);
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::RaidMove { name, group: Some(0), .. }) if name == "Friend"
        ));
        press(&mut app, disband);
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::RaidRemove { name, .. }) if name == "Friend"
        ));
        // A member who left is no longer chosen.
        news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [RaidUpdate::Removed {
                member: "Friend".into(),
            }],
        );
        app.update();
        assert_eq!(app.world().resource::<RaidChoice>().0, None);
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
