use super::*;

fn at(second: i64) -> jiff::Timestamp {
    jiff::Timestamp::from_second(1_800_000_000 + second).unwrap()
}

fn gap(opcode: u16, kind: Kind, count: u64, sample: Option<&str>) -> NetworkGap {
    NetworkGap {
        stage: "zone".into(),
        dialect: "titanium".into(),
        server: "eq_emu".into(),
        zone: "qeynos".into(),
        opcode,
        name: Some("OP_Example".into()),
        count,
        min_length: 4,
        max_length: 9,
        kind,
        sample: sample.map(str::to_owned),
    }
}

fn reason(text: &str) -> String {
    text.to_owned()
}

#[test]
fn kinds_key_as_the_network_library_does() {
    assert_eq!(Kind::UnknownKind { kind: 8 }.key(), "unknown_kind:8");
    let unreadable = Kind::Unreadable {
        part: "world".into(),
        error: "too short".into(),
    };
    assert_eq!(unreadable.key(), "unreadable:world");
    assert_eq!(Kind::Unlisted.key(), "unlisted");
    assert!(
        !Kind::Skipped {
            reason: reason("x")
        }
        .is_gap()
    );
}

#[test]
fn repeats_add_up_and_keep_at_most_three_samples() {
    let mut log = GapLog::new(at(0));
    let unlisted = Kind::Unlisted;
    log.network(gap(0x0001, unlisted.clone(), 1, Some("aa")), at(1));
    log.network(gap(0x0001, unlisted.clone(), 999, None), at(11));
    for sample in ["bb", "cc", "dd", "aa"] {
        log.network(gap(0x0001, unlisted.clone(), 1, Some(sample)), at(20));
    }
    let skipped = Kind::Skipped {
        reason: reason("nothing in it"),
    };
    log.network(gap(0x0002, skipped, 5, None), at(2));
    let file = log.file();
    assert_eq!(file.entries.len(), 2);
    let first = &file.entries[0];
    assert_eq!((first.opcode, first.count), (Some(0x0001), 1004));
    assert_eq!(first.samples, ["aa", "bb", "cc"]);
    assert_eq!(first.first_seen, at(1).to_string());
    assert_eq!(first.last_seen, at(20).to_string());
    assert!(first.is_gap);
    assert!(!file.entries[1].is_gap);
}

#[test]
fn a_sample_is_cut_to_256_bytes() {
    let mut log = GapLog::new(at(0));
    let body = "ab".repeat(1000);
    log.network(gap(1, Kind::Unlisted, 1, Some(&body)), at(0));
    assert_eq!(log.file().entries[0].samples[0].len(), MAX_SAMPLE_CHARS);
}

#[test]
fn unshown_news_counts_by_kind_under_the_session_s_server() {
    let mut log = GapLog::new(at(0));
    log.network(gap(1, Kind::Unlisted, 1, None), at(0));
    log.unshown("Ding", at(1));
    log.unshown("Ding", at(2));
    log.unshown("Spun", at(3));
    let file = log.file();
    let ding = file
        .entries
        .iter()
        .find(|entry| entry.name.as_deref() == Some("Ding"))
        .unwrap();
    assert_eq!(ding.count, 2);
    assert_eq!(ding.stage, CLIENT_STAGE);
    assert_eq!(ding.kind, UNSHOWN);
    assert_eq!((ding.server.as_str(), ding.opcode), ("eq_emu", None));
}

#[derive(Serialize)]
enum News {
    Said(String),
    Struck { by: String, amount: u32 },
    Pair(String, String),
    Ding,
}

/// Fails the test if the serializer ever reaches the payload.
struct Tripwire;

impl Serialize for Tripwire {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        panic!("the payload was serialized");
    }
}

#[derive(Serialize)]
enum Guarded {
    Held(Tripwire),
    Fields { inner: Tripwire },
}

#[test]
fn a_kind_s_name_is_taken_without_its_payload() {
    assert_eq!(
        variant_name(&News::Said("Tester says hello".into())),
        Some("Said")
    );
    assert_eq!(
        variant_name(&News::Struck {
            by: "Tester".into(),
            amount: 3
        }),
        Some("Struck")
    );
    assert_eq!(
        variant_name(&News::Pair("a".into(), "b".into())),
        Some("Pair")
    );
    assert_eq!(variant_name(&News::Ding), Some("Ding"));
    assert_eq!(variant_name(&Guarded::Held(Tripwire)), Some("Held"));
    assert_eq!(
        variant_name(&Guarded::Fields { inner: Tripwire }),
        Some("Fields")
    );
    assert_eq!(variant_name(&5_u32), None);
    assert_eq!(variant_name("text"), None);
}

#[test]
fn a_world_event_is_named_by_its_variant_alone() {
    use crate::WorldEvent;
    assert_eq!(variant_name(&WorldEvent::Ding), Some("Ding"));
    assert_eq!(variant_name(&WorldEvent::Spun), Some("Spun"));
}

#[test]
fn the_report_ranks_gaps_first_and_prints_samples_only_when_asked() {
    let mut log = GapLog::new(at(0));
    log.network(gap(0x0001, Kind::Unlisted, 3, Some("feed")), at(0));
    log.network(
        gap(
            0x0002,
            Kind::Unread {
                reason: reason("planned with the bazaar"),
            },
            7,
            None,
        ),
        at(0),
    );
    log.network(
        gap(
            0x0003,
            Kind::Skipped {
                reason: reason("EQEmu leaves it zero"),
            },
            100,
            None,
        ),
        at(0),
    );
    log.unshown("Ding", at(1));
    let files = [log.file(), log.file()];
    let plain = report(&files, false);
    assert!(!plain.contains("feed"));
    let gaps = plain.find("## Gaps to close").unwrap();
    let skipped = plain.find("## Counted, nothing to read").unwrap();
    let bazaar = plain.find("planned with the bazaar").unwrap();
    let unlisted = plain.find("| 0x0001 |").unwrap();
    let zero = plain.find("EQEmu leaves it zero").unwrap();
    assert!(gaps < bazaar && bazaar < unlisted && unlisted < skipped && skipped < zero);
    assert!(plain.contains("| 14 | 2 | eq_emu | titanium | zone | 0x0002 |"));
    assert!(plain.contains("| Ding | unshown |"));
    assert!(report(&files, true).contains("`feed`"));
}

#[test]
fn the_folder_keeps_only_the_newest_sessions() {
    let directory = std::env::temp_dir().join(format!("eq-client-gaps-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    for day in 0..(KEEP_SESSIONS + 5) {
        fs::write(
            directory.join(format!("gaps-2026{day:04}T000000Z.json")),
            "{}",
        )
        .unwrap();
    }
    fs::write(directory.join("other.txt"), "kept").unwrap();
    prune(&directory, KEEP_SESSIONS - 1).unwrap();
    let left = files_in(&directory).unwrap();
    assert_eq!(left.len(), KEEP_SESSIONS - 1);
    assert!(left[0].ends_with(format!("gaps-2026{:04}T000000Z.json", 6)));
    assert!(directory.join("other.txt").exists());
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_partly_read_message_names_its_parts() {
    let kind = Kind::PartlyRead {
        parts: vec!["tribute".into(), "alternate advancement".into()],
    };
    assert_eq!(kind.detail(), "tribute; alternate advancement");
}

#[test]
fn past_the_entry_cap_arrivals_count_in_one_overflow_entry() {
    let mut log = GapLog::new(at(0));
    let opcodes = u16::try_from(MAX_ENTRIES + 10).unwrap();
    for opcode in 0..opcodes {
        log.network(gap(opcode, Kind::Unlisted, 2, Some("aa")), at(1));
    }
    log.network(gap(0, Kind::Unlisted, 1, None), at(2));
    let file = log.file();
    assert_eq!(file.entries.len(), MAX_ENTRIES + 1);
    let overflow = file
        .entries
        .iter()
        .find(|entry| entry.kind == OVERFLOW)
        .unwrap();
    assert_eq!((overflow.count, overflow.opcode), (20, None));
    assert_eq!(overflow.stage, CLIENT_STAGE);
    assert_eq!(overflow.server, "eq_emu");
    assert!(overflow.samples.is_empty() && overflow.is_gap);
    let first = file
        .entries
        .iter()
        .find(|entry| entry.opcode == Some(0))
        .unwrap();
    assert_eq!(first.count, 3);
    assert!(report(&[file], false).contains("| overflow | past the 4096 entries"));
}
