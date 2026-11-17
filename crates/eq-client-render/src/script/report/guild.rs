//! Last-received guild evidence in the ordinary script report. No receipt
//! revision, acknowledgement, inferred refresh or absent add-member fields.
use eq_client_core::world::ClientWorld;
use serde_json::{Value, json};

const MAX_MEMBERS: usize = 80;

/// A bounded snapshot of known guild identity and received member fields.
pub(super) fn state(world: &ClientWorld) -> Value {
    let player = world.player();
    let guild = player.and_then(|player| player.listing.guild);
    let members = world.guild_members();
    json!({
        "current_admission": world.session_id(),
        "connected": world.connected(),
        "player_known": player.is_some(),
        "guild_id": guild,
        "guild_name": guild.and_then(|id| world.guild_name(id)),
        "guild_rank": player.and_then(|player| player.listing.guild_rank),
        "member_count": members.map(<[_]>::len),
        "members_truncated": members.map(|members| members.len() > MAX_MEMBERS),
        "members": members.map(|members| members.iter().take(MAX_MEMBERS).map(|member| json!({
            "name": member.name,
            "level": member.level,
            "class": member.class,
            "rank": member.rank,
            "zone_id": member.zone_id,
            "last_on": member.last_on,
            "note": world.guild_member_note(&member.name),
        })).collect::<Vec<_>>()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::{
        WorldEvent,
        guild::{GuildMember, GuildTribute, GuildUpdate},
        listing::GuildRank,
    };

    fn member(name: &str) -> GuildMember {
        GuildMember {
            name: name.into(),
            level: 5,
            class: 1,
            rank: GuildRank::Officer,
            banker: false,
            alt: false,
            last_on: 17,
            tribute: GuildTribute {
                on: false,
                total: 0,
                last: 0,
            },
            note: String::new(),
            zone_id: None,
        }
    }

    fn online() -> OnlineState {
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.listing.guild = Some(3);
        player.listing.guild_rank = Some(GuildRank::Other(9));
        testing::admit(&mut online, 1, player);
        testing::news(
            &mut online,
            [WorldEvent::GuildNames(vec![(3, "Synthetic Guild".into())])],
        );
        online
    }

    #[test]
    fn guild_report_distinguishes_unknown_empty_received_and_absent_note_fields() {
        let unknown = state(&ClientWorld::default());
        assert_eq!(unknown["player_known"], false);
        assert_eq!(unknown["guild_id"], Value::Null);
        assert_eq!(unknown["members"], Value::Null);
        let mut online = online();
        let waiting = state(online.world());
        assert_eq!(waiting["guild_id"], 3);
        assert_eq!(waiting["guild_name"], "Synthetic Guild");
        assert_eq!(waiting["guild_rank"], json!({"Other": 9}));
        assert_eq!(waiting["member_count"], Value::Null);
        assert_eq!(waiting["members"], Value::Null);
        testing::news(
            &mut online,
            [WorldEvent::Guild(GuildUpdate::Members(Vec::new()))],
        );
        let empty = state(online.world());
        assert_eq!(empty["member_count"], 0);
        assert_eq!(empty["members"], json!([]));
        testing::news(
            &mut online,
            [
                WorldEvent::Guild(GuildUpdate::Members(vec![member("Listed")])),
                WorldEvent::Guild(GuildUpdate::MemberAdded {
                    guild_id: 3,
                    name: "Added".into(),
                    level: 10,
                    class: 77,
                    rank: GuildRank::Other(8),
                    zone_id: Some(65534),
                    last_on: 23,
                }),
            ],
        );
        let received = state(online.world());
        assert_eq!(received["member_count"], 2);
        assert_eq!(
            received["members"][0],
            json!({
                "name": "Listed", "level": 5, "class": 1, "rank": "Officer",
                "zone_id": null, "last_on": 17, "note": "",
            })
        );
        assert_eq!(
            received["members"][1],
            json!({
                "name": "Added", "level": 10, "class": 77, "rank": {"Other": 8},
                "zone_id": 65534, "last_on": 23, "note": null,
            })
        );
    }

    #[test]
    fn guild_report_bounds_rows_without_hiding_the_received_count() {
        let mut online = online();
        let members = vec![member("Synthetic"); MAX_MEMBERS + 1];
        testing::news(
            &mut online,
            [WorldEvent::Guild(GuildUpdate::Members(members))],
        );
        let report = state(online.world());
        assert_eq!(report["member_count"], MAX_MEMBERS + 1);
        assert_eq!(report["members_truncated"], true);
        assert_eq!(report["members"].as_array().unwrap().len(), MAX_MEMBERS);
    }
}
