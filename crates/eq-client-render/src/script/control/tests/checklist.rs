use super::{app, frame, request, start, submit_request};
use crate::{
    online::{OnlineState, testing as session},
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use serde_json::{Value, json};

use super::super::{Bridge, HISTORY, Phase, Program, Receipt, Script, files, protocol, runbook};

fn definition() -> Value {
    json!({"title":"Synthetic session check","items":[
        {"id":"first","label":"First observation"},
        {"id":"second","label":"Second observation"}
    ]})
}

fn begin() -> Value {
    json!({"op":"runbook_start","runbook":definition()})
}

fn largest_definition() -> Value {
    json!({
        "title":"\"".repeat(96),
        "items":(0..64)
            .map(|id| json!({"id":format!("{id:048}"),"label":"\"".repeat(120)}))
            .collect::<Vec<_>>()
    })
}

fn result(runbook_id: u64, item_id: &str, status: &str) -> Value {
    json!({"op":"runbook_result","result":{
        "runbook_id":runbook_id,"item_id":item_id,"status":status,
        "note":"Synthetic evidence summary"
    }})
}

fn status(app: &App) -> Value {
    serde_json::from_slice(&app.world().resource::<Bridge>().snapshot()).unwrap()
}

fn parse(fields: Value) -> Result<protocol::Envelope, &'static str> {
    let mut envelope = json!({"version":1,"id":1,"instance":"test-1"});
    envelope
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    protocol::parse(&serde_json::to_vec(&envelope).unwrap())
}

#[test]
fn validates_inline_definitions_and_result_bounds() {
    assert!(parse(begin()).is_ok());
    assert!(parse(result(1, "first", "passed")).is_ok());
    let mut largest = json!({"op":"runbook_start","runbook":largest_definition()});
    largest["runbook"]["title"] = json!("é".repeat(48));
    assert!(parse(largest.clone()).is_ok());
    largest["runbook"]["items"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"extra","label":"Extra"}));
    assert!(parse(largest).is_err());
    for (field, value) in [
        ("title", json!("é".repeat(49))),
        ("title", json!("   ")),
        ("title", json!("multi\nline")),
        ("items", json!([])),
        ("items", json!([{"id":"first","label":"x".repeat(121)}])),
        ("items", json!([{"id":"x".repeat(49),"label":"Point"}])),
        ("items", json!([{"id":"../first","label":"Point"}])),
        (
            "items",
            json!([{"id":"first","label":"Point","status":"passed"}]),
        ),
        (
            "items",
            json!([{"id":"same","label":"One"},{"id":"same","label":"Two"}]),
        ),
        ("path", json!("external.json")),
    ] {
        let mut invalid = begin();
        invalid["runbook"][field] = value;
        assert!(parse(invalid.clone()).is_err(), "{invalid}");
    }
    let mut note = result(1, "first", "passed");
    note["result"]["note"] = json!("x".repeat(180));
    assert!(parse(note).is_ok());
    for (field, value) in [
        ("runbook_id", json!(0)),
        ("runbook_id", json!(1.5)),
        ("item_id", json!("first item")),
        ("status", json!("pending")),
        ("note", json!("é".repeat(91))),
        ("note", json!("two\u{2028}lines")),
        ("note", json!({"packet":[0, 1]})),
        ("packet", json!([0, 1])),
    ] {
        let mut invalid = result(1, "first", "passed");
        invalid["result"][field] = value;
        assert!(parse(invalid.clone()).is_err(), "{invalid}");
    }
}

#[test]
fn maximum_checklist_can_share_status_with_full_executor_history() {
    let mut bridge = Bridge::new("synthetic-control".into(), "test-1".into());
    let program = Program::parse("wait 1\n", std::path::Path::new(".")).unwrap();
    let diagnostic = "\0".repeat(protocol::MAX_MESSAGE_BYTES);
    for _ in 0..=HISTORY {
        let mut progress = Script::from_program(program.clone()).progress;
        bridge.bind(&progress, "control").unwrap();
        progress.finish(Phase::Failed, diagnostic.clone());
        bridge.observe(&progress, false);
    }
    let mut receipt = Receipt::rejected(65, "invalid_program");
    receipt.message = Some(diagnostic.clone());
    bridge.receipt = Some(receipt);
    bridge.io_error = Some(diagnostic);

    let mut run = runbook::Run::new(
        1,
        runbook::Definition::parse(&largest_definition()).unwrap(),
    );
    for id in 0..64 {
        let update = runbook::ResultUpdate::parse(&json!({
            "runbook_id":1,"item_id":format!("{id:048}"),"status":"failed","note":"\"".repeat(180)
        }))
        .unwrap();
        run.record(&update).unwrap();
    }
    bridge.runbook = Some(run);
    let snapshot = bridge.snapshot();
    assert!(snapshot.len() > 128 * 1024);
    assert!(snapshot.len() <= files::MAX_STATUS_BYTES);
}

#[test]
fn results_are_run_scoped_and_start_resets_without_touching_the_executor() {
    let mut app = app();
    let script = start(&mut app, 1, "wait 1000\n");
    assert_eq!(script.run_id, Some(1));
    assert_eq!(
        request(&mut app, 2, result(1, "first", "passed")).code,
        "runbook_changed"
    );
    let first = request(&mut app, 3, begin());
    assert_eq!(first.code, "runbook_started");
    assert_eq!(request(&mut app, 3, begin()), first);
    assert_eq!(status(&app)["runbook"]["id"], 3);
    assert_eq!(status(&app)["runbook"]["items"][0]["status"], "pending");
    assert_eq!(
        request(&mut app, 4, result(3, "unknown", "passed")).code,
        "unknown_runbook_item"
    );
    let report = request(&mut app, 5, result(3, "first", "passed"));
    assert_eq!(report.code, "runbook_result_recorded");
    assert_eq!(request(&mut app, 5, result(3, "first", "passed")), report);
    assert_eq!(
        request(&mut app, 6, result(3, "first", "failed")).code,
        "runbook_item_already_reported"
    );
    assert_eq!(status(&app)["runbook"]["items"][0]["status"], "passed");
    assert_eq!(request(&mut app, 7, begin()).code, "runbook_started");
    assert_eq!(request(&mut app, 3, begin()).code, "stale_id");
    assert_eq!(
        request(&mut app, 8, result(3, "first", "passed")).code,
        "runbook_changed"
    );
    let mut foreign = result(7, "first", "passed");
    foreign["instance"] = json!("different-client");
    assert_eq!(request(&mut app, 9, foreign).code, "wrong_instance");
    let snapshot = status(&app);
    assert_eq!(snapshot["runbook"]["id"], 7);
    assert!(snapshot["runbook"]["items"][0]["note"].is_null());
    assert_eq!(snapshot["runbook"]["items"][0]["status"], "pending");
    assert_eq!(snapshot["run"]["id"], 1);
    assert!(app.world().contains_resource::<Script>());
}

#[test]
fn repeated_results_and_malformed_replacements_preserve_the_checklist() {
    let mut app = app();
    assert_eq!(request(&mut app, 1, begin()).code, "runbook_started");
    let report = result(1, "first", "passed");
    assert_eq!(
        request(&mut app, 2, report.clone()).code,
        "runbook_result_recorded"
    );
    let recorded = status(&app)["runbook"].clone();
    assert_eq!(recorded["id"], 1);
    assert_eq!(recorded["items"][0]["status"], "passed");
    assert_eq!(recorded["items"][0]["note"], "Synthetic evidence summary");

    let duplicate = request(&mut app, 3, report.clone());
    assert_eq!(
        (duplicate.id, duplicate.code),
        (3, "runbook_result_recorded")
    );
    assert_eq!(status(&app)["runbook"], recorded);

    let mut changed_note = report;
    changed_note["result"]["note"] = json!("Different evidence summary");
    assert_eq!(
        request(&mut app, 4, changed_note).code,
        "runbook_item_already_reported"
    );
    let previous = status(&app);
    assert_eq!(previous["runbook"], recorded);

    let mut invalid = begin();
    invalid["runbook"]["items"][1]["id"] = json!("first");
    submit_request(&mut app, 5, invalid);
    let rejected = status(&app);
    assert_eq!(
        rejected["protocol_error"],
        "Runbook item IDs must be unique"
    );
    assert_eq!(rejected["runbook"], recorded);
    assert_eq!(rejected["last_processed_id"], 4);
    assert_eq!(rejected["last_request"], previous["last_request"]);
    assert_eq!(
        request(&mut app, 5, result(1, "second", "failed")).code,
        "runbook_result_recorded"
    );
}

#[test]
fn shows_current_results_and_reset_in_the_normal_frame() {
    let mut app = app();
    runbook::view::install(&mut app);
    assert_eq!(request(&mut app, 1, begin()).code, "runbook_started");
    assert_eq!(window_display(&mut app), Display::None);

    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::Runbook);
    frame(&mut app, 0);
    assert_eq!(window_display(&mut app), Display::Flex);
    assert_text(&mut app, "Synthetic session check");
    assert_text(&mut app, "[ ] Pending · First observation");

    assert_eq!(
        request(&mut app, 2, result(1, "first", "passed")).code,
        "runbook_result_recorded"
    );
    assert_eq!(
        request(&mut app, 3, result(1, "second", "failed")).code,
        "runbook_result_recorded"
    );
    assert_text(&mut app, "[x] Passed · First observation");
    assert_text(&mut app, "[!] Failed · Second observation");
    assert_text(&mut app, "Synthetic evidence summary");
    assert_text(
        &mut app,
        "Harness run #1 · 2/2 reported · 1 passed · 1 failed",
    );

    app.world_mut().resource_mut::<OnlineState>().enabled = true;
    frame(&mut app, 0);
    assert_eq!(window_display(&mut app), Display::None);
    assert!(app.world().resource::<Shown>().displayed(WindowId::Runbook));
    session::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        1,
        session::player(7),
    );
    frame(&mut app, 0);
    assert_eq!(window_display(&mut app), Display::Flex);
    assert_text(&mut app, "[!] Failed · Second observation");

    app.world_mut()
        .resource_mut::<Shown>()
        .close(WindowId::Runbook);
    frame(&mut app, 0);
    assert_eq!(window_display(&mut app), Display::None);
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::Runbook);
    frame(&mut app, 0);
    assert_text(&mut app, "[!] Failed · Second observation");

    assert_eq!(request(&mut app, 4, begin()).code, "runbook_started");
    assert_text(
        &mut app,
        "Harness run #4 · 0/2 reported · 0 passed · 0 failed",
    );
    assert_text(&mut app, "[ ] Pending · First observation");
    assert!(!has_text(&mut app, "Synthetic evidence summary"));
}

fn window_display(app: &mut App) -> Display {
    let world = app.world_mut();
    world
        .query::<(&WindowId, &Node)>()
        .iter(world)
        .find_map(|(id, node)| (*id == WindowId::Runbook).then_some(node.display))
        .unwrap()
}

fn assert_text(app: &mut App, expected: &str) {
    assert!(has_text(app, expected), "missing UI text: {expected}");
}

fn has_text(app: &mut App, expected: &str) -> bool {
    let world = app.world_mut();
    world
        .query::<&Text>()
        .iter(world)
        .any(|text| text.0 == expected)
}
