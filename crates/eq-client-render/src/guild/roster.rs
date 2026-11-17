//! Read-only guild roster. A full received list establishes membership;
//! deltas update it in core. Opening this window sends no request: the mapped
//! protocol has no roster-refresh command, and an absent list stays unknown.
use crate::{
    online::OnlineState,
    theme::{self, Size},
    windows::{self, Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_assets::{
    sidl::{Element, Listbox, Screen},
    ui::Area,
};
use eq_client_core::{Capability, guild::GuildMember, listing::GuildRank, world::ClientWorld};

#[cfg(test)]
mod tests;

/// Native list order; the timestamp column becomes an explicit received status.
const HEADINGS: [&str; 7] = [
    "Name",
    "Lvl",
    "Class",
    "Rank",
    "Status",
    "Zone",
    "Public Note",
];
const WIDTHS: [f32; 7] = [110.0, 36.0, 80.0, 70.0, 65.0, 100.0, 190.0];
const ROW_HEIGHT: f32 = 18.0;
type Row = [String; 7];

#[derive(Component)]
/// The roster frame created and retired by the shared window lifecycle.
pub(crate) struct RosterFrame;

/// Shared list contents for the authored and fallback guild windows.
#[derive(Component)]
pub(crate) struct GuildRows {
    columns: Vec<f32>,
    drawn: Option<Vec<Row>>,
}

impl GuildRows {
    pub(crate) const fn new(columns: Vec<f32>) -> Self {
        Self {
            columns,
            drawn: None,
        }
    }
}

#[derive(Component)]
/// The dynamic guild name and received member count.
pub(crate) struct Heading;
#[derive(Component)]
/// The roster's availability and last-received provenance line.
pub(crate) struct Summary;

/// A member's received fields. Numeric rank/class values remain visible when
/// their names are unknown. Only the decoded location field establishes status.
fn row(world: &ClientWorld, member: &GuildMember) -> Row {
    let rank = match member.rank {
        GuildRank::Member => "Member".into(),
        GuildRank::Officer => "Officer".into(),
        GuildRank::Leader => "Leader".into(),
        GuildRank::Other(value) => format!("Rank {value}"),
    };
    [
        member.name.clone(),
        if member.level == 0 {
            "Unknown".into()
        } else {
            member.level.to_string()
        },
        eq_client_core::classes::class_name(member.class)
            .map_or_else(|| format!("Class {}", member.class), str::to_owned),
        rank,
        if member.zone_id.is_some() {
            "Online"
        } else {
            "Offline"
        }
        .into(),
        member.zone_id.map_or_else(
            || "—".into(),
            |zone| {
                eq_client_core::zones::short_name(u32::from(zone))
                    .map_or_else(|| format!("Zone {zone}"), str::to_owned)
            },
        ),
        world
            .guild_member_note(&member.name)
            .unwrap_or("Unknown")
            .to_owned(),
    ]
}

fn rows(world: &ClientWorld) -> Vec<Row> {
    if !world.connected()
        || world
            .player()
            .and_then(|player| player.listing.guild)
            .is_none()
    {
        return Vec::new();
    }
    let mut members: Vec<_> = world.guild_members().unwrap_or_default().iter().collect();
    members.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then(left.name.cmp(&right.name))
    });
    members
        .into_iter()
        .map(|member| row(world, member))
        .collect()
}

fn words(world: &ClientWorld) -> (String, String) {
    if !world.connected() || world.player().is_none() {
        return ("Guild".into(), "Enter the world to view your guild.".into());
    }
    let Some(guild) = world.player().and_then(|player| player.listing.guild) else {
        return ("Guild".into(), "You are not in a guild.".into());
    };
    let name = world
        .guild_name(guild)
        .map_or_else(|| format!("Guild {guild}"), str::to_owned);
    let Some(members) = world.guild_members() else {
        let status = if world.can(Capability::GuildRoster) {
            "Waiting for the server's member list."
        } else {
            "Guild roster is unavailable in this session."
        };
        return (name, status.into());
    };
    (
        format!("{name} — {} members", members.len()),
        "Last received roster; status and ranks are displayed as received.".into(),
    )
}

/// Finds the member list inside authored tabs/views without retaining any
/// moderation controls, editable notes, tribute fields or sample guild names.
fn member_list(pieces: &[(String, Element)]) -> Option<&Listbox> {
    pieces.iter().find_map(|(name, element)| match element {
        Element::Listbox(list)
            if name == "GT_MemberList" || list.id.as_deref() == Some("MemberList") =>
        {
            Some(list)
        }
        Element::Tabs(tabs) => tabs.pages.iter().find_map(|page| member_list(&page.pieces)),
        Element::View(view) => member_list(&view.pieces),
        _ => None,
    })
}

/// Adapts the native guild frame and list to this read-only slice. A skin
/// without the member list keeps the usable fallback window.
pub(crate) fn adapt(screen: &mut Screen) -> bool {
    let Some(mut list) = member_list(&screen.pieces).cloned() else {
        return false;
    };
    list.id = Some("MemberList".into());
    list.anchors = None;
    list.area = Some(Area {
        x: 0.0,
        y: 24.0,
        width: (screen.area.width - 16.0).max(1.0),
        height: (screen.area.height - 82.0).max(1.0),
    });
    let scale = ((screen.area.width - 48.0).max(1.0) / WIDTHS.iter().sum::<f32>()).min(1.0);
    list.columns = HEADINGS
        .into_iter()
        .zip(WIDTHS)
        .map(|(heading, width)| eq_client_assets::sidl::Column {
            heading: heading.into(),
            width: width * scale,
            header: None,
        })
        .collect();
    screen.title = Some("Guild".into());
    screen.pieces = vec![("GT_MemberList".into(), Element::Listbox(list))];
    true
}

/// Dynamic labels shared by the authored and fallback frame.
pub(crate) fn labels(parent: &mut ChildSpawnerCommands, width: f32, height: f32) {
    for (summary, y) in [(false, 24.0), (true, height - 24.0)] {
        let mut label = parent.spawn((
            theme::text("", Size::Small, theme::INK_BRIGHT),
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
            crate::skinned::at(8.0, y, (width - 16.0).max(1.0), 20.0),
        ));
        if summary {
            label.insert(Summary);
        } else {
            label.insert(Heading);
        }
    }
}

/// Creates a movable/minimizable window on demand. The skin system replaces
/// its fallback children when the installation supplies a usable guild list.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn window(
    mut commands: Commands,
    online: Res<OnlineState>,
    mut shown: ResMut<Shown>,
    frames: Query<Entity, With<RosterFrame>>,
) {
    if !online.world().connected() || online.world().player().is_none() {
        shown.close(WindowId::Guild);
    }
    if !shown.displayed(WindowId::Guild) {
        for frame in &frames {
            commands.entity(frame).despawn();
        }
        return;
    }
    if !frames.is_empty() {
        return;
    }
    let (width, height) = (770.0, 400.0);
    let frame = windows::frame(
        &mut commands,
        WindowId::Guild,
        Node {
            width: px(width),
            height: px(height),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            ..default()
        },
    );
    commands
        .entity(frame)
        .insert(RosterFrame)
        .with_children(|window| {
            labels(window, width, height);
            window
                .spawn((
                    windows::KeepsPress,
                    crate::skinned::at(8.0, 48.0, width - 16.0, 20.0),
                ))
                .with_children(|heading| {
                    for (words, width) in HEADINGS.into_iter().zip(WIDTHS) {
                        cell(heading, words, width, theme::INK_DIM);
                    }
                });
            window.spawn((
                GuildRows::new(WIDTHS.to_vec()),
                windows::KeepsPress,
                windows::pointer::TakesWheel,
                ScrollPosition::default(),
                Node {
                    overflow: Overflow::scroll_y(),
                    flex_direction: FlexDirection::Column,
                    ..crate::skinned::at(8.0, 68.0, width - 16.0, height - 98.0)
                },
                BackgroundColor(theme::INSET),
            ));
        });
}

fn cell(parent: &mut ChildSpawnerCommands, words: &str, width: f32, ink: Color) {
    parent.spawn((
        theme::text(words, Size::Small, ink),
        crate::tooltip::Tooltip(words.to_owned()),
        TextLayout::new(Justify::Left, LineBreak::NoWrap),
        Node {
            width: px(width),
            flex_shrink: 0.0,
            overflow: Overflow::clip(),
            ..default()
        },
    ));
}

/// Redraws only changed rows and updates lifecycle/availability messages.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(crate) fn fill(
    mut commands: Commands,
    online: Res<OnlineState>,
    mut lists: Query<(Entity, &mut GuildRows, Option<&Children>)>,
    mut labels: Query<(&mut Text, Has<Heading>), Or<(With<Heading>, With<Summary>)>>,
) {
    let world = online.world();
    let wanted = rows(world);
    let (heading, summary) = words(world);
    for (mut text, is_heading) in &mut labels {
        let wanted = if is_heading { &heading } else { &summary };
        if text.0 != *wanted {
            text.0.clone_from(wanted);
        }
    }
    for (entity, mut list, children) in &mut lists {
        if list.drawn.as_ref() == Some(&wanted) {
            continue;
        }
        for child in children.into_iter().flatten() {
            commands.entity(*child).despawn();
        }
        for row in &wanted {
            commands
                .spawn((
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ChildOf(entity),
                ))
                .with_children(|line| {
                    for (words, width) in row.iter().zip(&list.columns) {
                        cell(line, words, *width, theme::INK_BRIGHT);
                    }
                });
        }
        list.drawn = Some(wanted.clone());
    }
}
