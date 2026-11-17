use super::*;
use crate::online::testing;
use eq_client_core::{
    WorldEvent, WorldUpdate,
    guild::{GuildTribute, GuildUpdate},
    world::Link,
};

fn online() -> OnlineState {
    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.listing.guild = Some(3);
    testing::admit(&mut online, 1, player);
    testing::news(
        &mut online,
        [WorldEvent::GuildNames(vec![(3, "Synthetic Guild".into())])],
    );
    online
}

fn member(name: &str, zone_id: Option<u16>) -> GuildMember {
    GuildMember {
        name: name.into(),
        level: 10,
        class: 1,
        rank: GuildRank::Member,
        banker: false,
        alt: false,
        last_on: 0,
        tribute: GuildTribute {
            on: false,
            total: 0,
            last: 0,
        },
        note: String::new(),
        zone_id,
    }
}

fn list(online: &mut OnlineState) {
    let mut unknown = member("Zed", Some(65534));
    unknown.rank = GuildRank::Other(17);
    unknown.class = 77;
    unknown.level = 0;
    testing::news(
        online,
        [WorldEvent::Guild(GuildUpdate::Members(vec![
            unknown,
            member("Alice", None),
        ]))],
    );
}

#[test]
fn deterministic_roster_snapshot_keeps_unknowns_and_received_offline_distinct() {
    let mut online = online();
    assert!(rows(online.world()).is_empty());
    assert_eq!(
        words(online.world()).1,
        "Waiting for the server's member list."
    );
    list(&mut online);
    let snapshot = rows(online.world())
        .into_iter()
        .map(|row| row.join(" | "))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        snapshot,
        "Alice | 10 | Warrior | Member | Offline | — | \nZed | Unknown | Class 77 | Rank 17 | Online | Zone 65534 | "
    );
    testing::news(
        &mut online,
        [WorldEvent::Guild(GuildUpdate::MemberAdded {
            guild_id: 3,
            name: "Bob".into(),
            level: 5,
            class: 2,
            rank: GuildRank::Officer,
            zone_id: Some(2),
            last_on: 0,
        })],
    );
    assert_eq!(rows(online.world())[1][6], "Unknown");
    assert_eq!(words(online.world()).0, "Synthetic Guild — 3 members");
    testing::news(
        &mut online,
        [WorldEvent::Guild(GuildUpdate::MemberLocation {
            guild_id: 3,
            name: "Bob".into(),
            zone_id: None,
            last_on: 1,
        })],
    );
    assert_eq!(rows(online.world())[1][4], "Offline");
}

#[test]
fn received_empty_unavailable_and_disconnected_rosters_are_different() {
    let mut online = online();
    testing::news(
        &mut online,
        [WorldEvent::Guild(GuildUpdate::Members(Vec::new()))],
    );
    assert_eq!(words(online.world()).0, "Synthetic Guild — 0 members");
    list(&mut online);
    testing::news(
        &mut online,
        [WorldEvent::Guild(GuildUpdate::Disbanded { guild_id: 3 })],
    );
    assert!(rows(online.world()).is_empty());
    testing::apply(&mut online, &WorldUpdate::Connection(Link::Offline));
    assert!(rows(online.world()).is_empty());
    assert_eq!(
        words(online.world()).1,
        "Enter the world to view your guild."
    );
    let mut player = testing::player(7);
    player.listing.guild = Some(3);
    testing::news(
        &mut online,
        [WorldEvent::Entered {
            capabilities: Vec::new(),
            choices: Vec::new(),
            session_id: 2,
            zone: "qeytoqrg".into(),
            zone_id: 1,
            player: Box::new(player),
            far_clip: None,
            spell_landing_flag: None,
        }],
    );
    testing::connect(&mut online, true);
    assert_eq!(
        words(online.world()).1,
        "Guild roster is unavailable in this session."
    );
    testing::news(
        &mut online,
        [WorldEvent::Listing {
            spawn_id: 7,
            change: eq_client_core::listing::ListingChange::Guild(None),
        }],
    );
    assert_eq!(words(online.world()).1, "You are not in a guild.");
}

#[test]
fn fallback_rows_follow_deltas_and_closing_reopening_and_admission_changes() {
    let mut app = App::new();
    let mut online = online();
    list(&mut online);
    let mut shown = Shown::default();
    shown.open(WindowId::Guild);
    app.insert_resource(online)
        .insert_resource(shown)
        .add_systems(Update, (window, fill).chain());
    app.update();
    let frame = app
        .world_mut()
        .query_filtered::<Entity, With<RosterFrame>>()
        .single(app.world())
        .unwrap();
    let list = app
        .world_mut()
        .query_filtered::<Entity, With<GuildRows>>()
        .single(app.world())
        .unwrap();
    assert_eq!(app.world().get::<Children>(list).unwrap().len(), 2);
    app.update();
    assert!(app.world().get_entity(frame).is_ok());
    assert_eq!(app.world().get::<Children>(list).unwrap().len(), 2);
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Guild(GuildUpdate::MemberRemoved {
            guild_id: 3,
            name: "Alice".into(),
        })],
    );
    app.update();
    assert_eq!(app.world().get::<Children>(list).unwrap().len(), 1);
    app.world_mut()
        .resource_mut::<Shown>()
        .close(WindowId::Guild);
    app.update();
    assert!(app.world().get_entity(frame).is_err());
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::Guild);
    app.update();
    let list = app
        .world_mut()
        .query_filtered::<Entity, With<GuildRows>>()
        .single(app.world())
        .unwrap();
    let mut player = testing::player(8);
    player.listing.guild = Some(3);
    testing::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        2,
        player,
    );
    app.update();
    assert!(
        app.world()
            .get::<Children>(list)
            .is_none_or(|children| children.is_empty())
    );
}

#[test]
fn native_adaptation_retains_only_the_member_list_and_safe_dynamic_labels() {
    use eq_client_assets::sidl::{Page, Sizable, TabBox};
    let list = Listbox {
        id: Some("MemberList".into()),
        area: None,
        anchors: None,
        template: None,
        columns: Vec::new(),
        font: None,
        scrollbar: None,
        header: None,
    };
    let mut screen = Screen {
        name: "GuildManagementWnd".into(),
        title: Some("Sample Guild".into()),
        title_color: None,
        font: None,
        area: Area {
            x: 0.0,
            y: 0.0,
            width: 770.0,
            height: 400.0,
        },
        template: None,
        title_bar: None,
        border: false,
        sizable: Sizable::Fixed,
        transparent: false,
        tooltip: None,
        tab_frame: None,
        pieces: vec![(
            "GT_Subwindows".into(),
            Element::Tabs(TabBox {
                name: "GT_Subwindows".into(),
                area: None,
                tab_frame: None,
                page_frame: None,
                pages: vec![Page {
                    name: "GT_MemberPage".into(),
                    title: None,
                    area: None,
                    template: None,
                    icon: [None, None],
                    title_colors: [None, None],
                    tooltip: None,
                    pieces: vec![
                        ("GT_MemberList".into(), Element::Listbox(list)),
                        ("GT_Promote".into(), Element::Other("button".into())),
                    ],
                }],
            }),
        )],
    };
    assert!(adapt(&mut screen));
    assert_eq!(screen.title.as_deref(), Some("Guild"));
    assert_eq!(screen.pieces.len(), 1);
    let list = member_list(&screen.pieces).unwrap();
    assert_eq!(
        list.columns
            .iter()
            .map(|column| column.heading.as_str())
            .collect::<Vec<_>>(),
        HEADINGS
    );
    assert_eq!(WindowId::Guild.key(), "guild");
    assert!(WindowId::Guild.can_minimize());
    assert!(WindowId::Guild.describe().persists);
    assert!(WindowId::Guild.describe().closes_on_escape);
}
