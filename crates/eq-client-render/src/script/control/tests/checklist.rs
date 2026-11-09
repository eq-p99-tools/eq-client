use super::{app, clean, frame, request, start, submit_request, temporary};
use crate::{
    online::{OnlineState, testing as session},
    windows::{SelectorButton, Shown, WindowId, menu},
};
use bevy::prelude::*;
use serde_json::{Value, json};
use std::{fs, time::Instant};

use super::super::{
    Bridge, HISTORY, Phase, Program, Receipt, RunControl, RunStatus, Script, files, install,
    protocol, runbook,
};

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
    let Value::Object(fields) = fields else {
        panic!("control request fields must be an object");
    };
    let mut envelope = json!({"version":1,"id":1,"instance":"test-1"});
    envelope.as_object_mut().unwrap().extend(fields);
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
        ("note", json!("unsafe\u{200f}direction")),
        ("note", json!({"packet":[0, 1]})),
        ("packet", json!([0, 1])),
        ("admission_id", json!(0)),
        ("after_request_id", json!(-1)),
        (
            "evidence",
            json!({"kind":"server_confirmed","reference":"obs-1"}),
        ),
        (
            "evidence",
            json!({"kind":"received_data","reference":"../capture"}),
        ),
        (
            "evidence",
            json!({"kind":"received_data","reference":"x".repeat(97)}),
        ),
        (
            "evidence",
            json!({"kind":"manual_witness","reference":"obs-1","verified":true}),
        ),
    ] {
        let mut invalid = result(1, "first", "passed");
        invalid["result"][field] = value;
        assert!(parse(invalid.clone()).is_err(), "{invalid}");
    }
    assert!(parse(json!({"op":"runbook_export","runbook_id":1})).is_ok());
    for fields in [
        json!({"op":"runbook_export","runbook_id":0}),
        json!({"op":"runbook_export","runbook_id":1,"path":"../escape.json"}),
    ] {
        assert!(parse(fields).is_err());
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
            "runbook_id":1,"item_id":format!("{id:048}"),"status":"failed","note":"\"".repeat(180),
            "evidence":{"kind":"scripted_observation","reference":"x".repeat(96)},
            "admission_id":u64::MAX
        }))
        .unwrap();
        run.record(u64::MAX - 64 + id, &update, Some(u64::MAX))
            .unwrap();
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
fn emitted_null_note_roundtrips_without_rewriting_terminal_results() {
    let mut app = app();
    assert_eq!(request(&mut app, 1, begin()).code, "runbook_started");
    let omitted = json!({"op":"runbook_result","result":{
        "runbook_id":1,"item_id":"first","status":"passed"
    }});
    let pending = status(&app);
    let row = &pending["runbook"]["items"][0];
    assert_eq!(row.get("note"), Some(&Value::Null));
    let mut explicit = omitted.clone();
    explicit["result"]["note"] = row["note"].clone();
    let report = request(&mut app, 2, explicit);
    assert_eq!(report.code, "runbook_result_recorded");
    let reported = status(&app);
    let row = &reported["runbook"]["items"][0];
    assert_eq!(row["status"], "passed");
    assert_eq!(row.get("note"), Some(&Value::Null));
    assert_eq!(request(&mut app, 2, omitted.clone()), report);
    assert_eq!(
        request(&mut app, 3, omitted).code,
        "runbook_result_recorded"
    );

    let mut noted = result(1, "second", "failed");
    assert_eq!(
        request(&mut app, 4, noted.clone()).code,
        "runbook_result_recorded"
    );
    let recorded = status(&app)["runbook"].clone();
    noted["result"]["note"] = Value::Null;
    assert_eq!(
        request(&mut app, 5, noted).code,
        "runbook_item_already_reported"
    );
    assert_eq!(status(&app)["runbook"], recorded);
}

#[test]
fn progress_updates_compare_with_the_row_and_skips_are_final() {
    let mut app = app();
    request(&mut app, 1, begin());
    let running = result(1, "first", "running");
    request(&mut app, 2, running.clone());
    assert_eq!(
        request(&mut app, 3, running).code,
        "runbook_result_recorded"
    );
    assert_eq!(status(&app)["runbook"]["items"][0]["report_request_id"], 2);
    assert_eq!(status(&app)["runbook"]["health"]["state"], "running");

    let mut blocked = result(1, "first", "blocked");
    blocked["result"]["after_request_id"] = json!(2);
    blocked["result"]["note"] = json!("Awaiting prerequisite");
    assert_eq!(
        request(&mut app, 4, blocked).code,
        "runbook_result_recorded"
    );
    assert_eq!(status(&app)["runbook"]["health"]["state"], "blocked");

    let mut resumed = result(1, "first", "running");
    resumed["result"]["after_request_id"] = json!(4);
    assert_eq!(
        request(&mut app, 5, resumed).code,
        "runbook_result_recorded"
    );
    let mut delayed = result(1, "first", "passed");
    delayed["result"]["after_request_id"] = json!(2);
    assert_eq!(request(&mut app, 6, delayed).code, "runbook_item_changed");
    assert_eq!(status(&app)["runbook"]["items"][0]["status"], "running");

    let mut skipped = result(1, "first", "skipped");
    skipped["result"]["after_request_id"] = json!(5);
    assert_eq!(
        request(&mut app, 7, skipped).code,
        "runbook_result_recorded"
    );
    assert_eq!(
        request(&mut app, 8, result(1, "first", "passed")).code,
        "runbook_item_already_reported"
    );
    assert_eq!(status(&app)["runbook"]["health"]["state"], "incomplete");
    request(&mut app, 9, result(1, "second", "passed"));
    assert_eq!(
        status(&app)["runbook"]["health"],
        json!({
            "state":"complete","pending":0,"running":0,"blocked":0,"skipped":1,"passed":1,"failed":0
        })
    );
}

#[test]
fn only_reports_that_name_an_admission_are_bound_to_it() {
    let mut app = app();
    {
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        online.enabled = true;
        session::admit(&mut online, 11, session::player(7));
    }
    request(&mut app, 1, begin());
    assert_eq!(status(&app)["admission_id"], 11);

    let mut observed = result(1, "first", "passed");
    observed["result"]["admission_id"] = json!(11);
    observed["result"]["evidence"] = json!({"kind":"received_data","reference":"item-list-1"});
    session::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        12,
        session::player(7),
    );
    assert_eq!(
        request(&mut app, 2, observed.clone()).code,
        "runbook_admission_changed"
    );
    assert_eq!(status(&app)["runbook"]["items"][0]["status"], "pending");
    observed["result"]["admission_id"] = json!(12);
    assert_eq!(
        request(&mut app, 3, observed.clone()).code,
        "runbook_result_recorded"
    );
    let recorded = status(&app)["runbook"].clone();
    session::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        13,
        session::player(7),
    );
    assert_eq!(
        request(&mut app, 4, observed.clone()).code,
        "runbook_result_recorded"
    );
    assert_eq!(status(&app)["admission_id"], 13);
    assert_eq!(status(&app)["runbook"], recorded);
    observed["result"]["note"] = json!("Changed after zoning");
    assert_eq!(
        request(&mut app, 5, observed).code,
        "runbook_admission_changed"
    );
    app.world_mut().resource_mut::<OnlineState>().enabled = false;
    let mut retry = result(1, "first", "passed");
    retry["result"]["admission_id"] = json!(12);
    retry["result"]["evidence"] = json!({"kind":"received_data","reference":"item-list-1"});
    assert_eq!(request(&mut app, 6, retry).code, "runbook_result_recorded");
    assert!(status(&app)["admission_id"].is_null());
    assert_eq!(status(&app)["runbook"], recorded);
    assert_eq!(
        request(&mut app, 7, result(1, "second", "passed")).code,
        "runbook_result_recorded"
    );
    let state = status(&app);
    assert_eq!(state["runbook"]["reported_by"], "producer");
    assert_eq!(state["runbook"]["items"][0]["admission_id"], 12);
    assert_eq!(
        state["runbook"]["items"][0]["evidence"]["kind"],
        "received_data"
    );
    assert!(state["runbook"]["items"][1]["admission_id"].is_null());
    assert!(state["runbook"]["items"][1]["evidence"].is_null());
}

#[test]
fn control_health_reports_local_exchange_errors_without_changing_checklist_rows() {
    let mut app = app();
    assert_eq!(status(&app)["control_health"], "waiting");
    request(&mut app, 1, begin());
    assert_eq!(status(&app)["control_health"], "ready");

    submit_request(&mut app, 2, result(1, "first", "pending"));
    assert_eq!(status(&app)["control_health"], "request_error");
    {
        let mut bridge = app.world_mut().resource_mut::<Bridge>();
        bridge.last_exchange = Instant::now().checked_sub(super::super::EXCHANGE_STALL);
        assert_eq!(bridge.channel_health(), "stalled");
        bridge.job.start(0, || Err("Synthetic file error".into()));
    }
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    loop {
        frame(&mut app, 0);
        if status(&app)["control_health"] == "io_error" {
            break;
        }
        assert!(Instant::now() < deadline, "file error did not settle");
        std::thread::yield_now();
    }
    request(&mut app, 2, json!({"op":"status"}));
    assert_eq!(status(&app)["control_health"], "ready");
    assert_eq!(status(&app)["runbook"]["health"]["state"], "pending");
}

#[test]
fn export_captures_only_the_named_checklist_and_preserves_it_on_failure() {
    let root = temporary();
    files::prepare(&root).unwrap();
    let mut app = app();
    app.insert_resource(Bridge::new(root.clone(), "test-1".into()));
    request(&mut app, 1, begin());
    let mut witness = result(1, "first", "passed");
    witness["result"]["note"] = json!("\"Visible\" \\");
    witness["result"]["evidence"] = json!({"kind":"manual_witness","reference":"witness-1"});
    request(&mut app, 2, witness);
    let captured = status(&app)["runbook"].clone();
    let export = json!({"op":"runbook_export","runbook_id":1});
    let receipt = request(&mut app, 3, export.clone());
    assert_eq!(receipt.code, "runbook_export_queued");
    assert_eq!(request(&mut app, 3, export.clone()), receipt);
    request(&mut app, 4, begin());
    let exported = wait_for_export(&mut app, "runbook_export");
    assert_eq!(exported["state"], "written");
    assert_eq!(exported["request_id"], 3);
    let path = root.join(files::RUNBOOK_EXPORT);
    let before = fs::read(&path).unwrap();
    let document: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(document["request_id"], 3);
    assert_eq!(document["instance"], "test-1");
    assert_eq!(document["runbook"], captured);
    assert!(document.get("recent_runs").is_none());
    assert_eq!(request(&mut app, 5, export).code, "runbook_changed");

    // A foreign temporary entry cannot be truncated or moved by an export.
    let occupied = root.join("runbook-export.json.tmp");
    fs::write(&occupied, "preexisting").unwrap();
    assert_eq!(
        request(&mut app, 6, json!({"op":"runbook_export","runbook_id":4})).code,
        "runbook_export_queued"
    );
    assert_eq!(
        wait_for_export(&mut app, "runbook_export")["state"],
        "failed"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&occupied).unwrap(), b"preexisting");
    drop(app);
    clean(&root);
}

fn wait_for_export(app: &mut App, field: &str) -> Value {
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    loop {
        frame(app, 0);
        let value = status(app)[field].clone();
        if value["state"] != "writing" {
            return value;
        }
        assert!(Instant::now() < deadline, "runbook export did not settle");
        std::thread::yield_now();
    }
}

#[test]
fn export_button_writes_its_own_snapshot_and_shows_the_actual_outcome() {
    use crate::clicks::testing::{click, press, release};

    let root = temporary();
    files::prepare(&root).unwrap();
    let mut app = app();
    crate::systems::configure_stages(&mut app);
    app.insert_resource(Bridge::new(root.clone(), "test-1".into()));
    runbook::view::install(&mut app);
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::Runbook);
    frame(&mut app, 0);
    let button = export_button(&mut app);
    click(&mut app, button);
    assert!(status(&app)["runbook_ui_export"].is_null());

    request(&mut app, 1, begin());
    request(&mut app, 2, json!({"op":"runbook_export","runbook_id":1}));
    let harness = wait_for_export(&mut app, "runbook_export");
    assert_eq!(harness["state"], "written");
    assert_eq!(harness["request_id"], 2);
    let harness_path = root.join(files::RUNBOOK_EXPORT);
    let harness_bytes = fs::read(&harness_path).unwrap();
    request(&mut app, 3, result(1, "first", "passed"));
    let captured = status(&app)["runbook"].clone();
    press(&mut app, Some(button));
    assert!(status(&app)["runbook_ui_export"].is_null());
    release(&mut app, Some(button));
    assert_text(&mut app, "Writing export…");
    let ui = wait_for_export(&mut app, "runbook_ui_export");
    assert_eq!(ui["state"], "written");
    assert!(ui["request_id"].is_null());
    assert_eq!(ui["revision"], 3);
    assert_eq!(ui["path"], files::RUNBOOK_UI_EXPORT);
    assert_text(&mut app, "Saved runbook-ui-export.json");
    let path = root.join(files::RUNBOOK_UI_EXPORT);
    let window_bytes = fs::read(&path).unwrap();
    let document: Value = serde_json::from_slice(&window_bytes).unwrap();
    assert_eq!(document["instance"], "test-1");
    assert!(document["request_id"].is_null());
    assert_eq!(document["runbook"], captured);

    request(&mut app, 4, result(1, "second", "failed"));
    assert_text(&mut app, "Export contains an earlier checklist revision");
    fs::write(root.join("runbook-ui-export.json.tmp"), "occupied").unwrap();
    click(&mut app, button);
    assert_eq!(
        wait_for_export(&mut app, "runbook_ui_export")["state"],
        "failed"
    );
    assert_text(&mut app, "Export failed");
    assert_eq!(fs::read(&path).unwrap(), window_bytes);
    assert_eq!(fs::read(&harness_path).unwrap(), harness_bytes);
    assert_eq!(status(&app)["runbook_export"], harness);
    drop(app);
    clean(&root);
}

#[test]
fn a_window_export_can_finish_while_a_harness_export_is_unconfirmed_at_shutdown() {
    let root = temporary();
    files::prepare(&root).unwrap();
    let mut app = app();
    app.insert_resource(Bridge::new(root.clone(), "test-1".into()));
    runbook::view::install(&mut app);
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::Runbook);
    request(&mut app, 1, begin());
    let export = json!({"op":"runbook_export","runbook_id":1});
    assert_eq!(
        request(&mut app, 2, export.clone()).code,
        "runbook_export_queued"
    );
    // Simulate a worker whose completion remains unavailable through shutdown.
    app.world_mut()
        .resource_mut::<Bridge>()
        .export
        .hold_completion();
    assert_eq!(request(&mut app, 3, export).code, "runbook_export_busy");
    let button = export_button(&mut app);
    crate::clicks::testing::click(&mut app, button);
    let ui = wait_for_export(&mut app, "runbook_ui_export");
    assert_eq!(ui["state"], "written");
    let harness = status(&app)["runbook_export"].clone();
    assert_eq!(harness["state"], "writing");
    assert_eq!(harness["request_id"], 2);

    app.add_systems(Last, super::super::publish);
    frame(&mut app, 0);
    drop(app);
    let closed: Value =
        serde_json::from_slice(&fs::read(root.join("status.json")).unwrap()).unwrap();
    assert_eq!(closed["client_state"], "closed");
    assert_eq!(closed["runbook_export"]["state"], "unconfirmed");
    assert_eq!(closed["runbook_export"]["request_id"], 2);
    assert_eq!(closed["runbook_export"]["path"], files::RUNBOOK_EXPORT);
    assert_eq!(closed["runbook_ui_export"], ui);
    clean(&root);
}

#[test]
fn an_export_press_cannot_apply_to_a_replacement_checklist() {
    use crate::clicks::testing::{click, press, release};

    let root = temporary();
    files::prepare(&root).unwrap();
    let mut app = app();
    crate::systems::configure_stages(&mut app);
    app.insert_resource(Bridge::new(root.clone(), "test-1".into()));
    runbook::view::install(&mut app);
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::Runbook);
    request(&mut app, 1, begin());
    let button = export_button(&mut app);
    press(&mut app, Some(button));
    assert_eq!(request(&mut app, 2, begin()).code, "runbook_started");
    release(&mut app, Some(button));
    assert!(status(&app)["runbook_ui_export"].is_null());
    assert!(!root.join(files::RUNBOOK_UI_EXPORT).exists());

    click(&mut app, button);
    let outcome = wait_for_export(&mut app, "runbook_ui_export");
    assert_eq!(outcome["state"], "written");
    assert_eq!(outcome["runbook_id"], 2);
    drop(app);
    clean(&root);
}

fn export_button(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query::<(&Text, &ChildOf)>()
        .iter(world)
        .find_map(|(text, parent)| (text.0 == "Export JSON").then_some(parent.parent()))
        .unwrap()
}

#[test]
fn no_runbook_is_installed_or_listed_without_a_control_receiver() {
    let root = temporary();
    fs::create_dir(&root).unwrap();
    for (path, expected_error) in [
        (None, None),
        (Some(root.clone()), Some(std::io::ErrorKind::AlreadyExists)),
    ] {
        let mut app = crate::testing::app();
        menu::install(&mut app);
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .script_control = path;
        assert_eq!(
            install(&mut app).err().map(|err| err.kind()),
            expected_error
        );
        app.update();
        assert!(!app.world().contains_resource::<Bridge>());
        assert_eq!(menu_entry(&mut app, WindowId::Runbook), None);
        assert_eq!(menu_entry(&mut app, WindowId::Scripts), Some(Display::Flex));
        let world = app.world_mut();
        assert!(
            !world
                .query::<&WindowId>()
                .iter(world)
                .any(|id| *id == WindowId::Runbook)
        );
    }
    assert!(!root.join("status.json").exists());
    clean(&root);
}

#[test]
fn a_control_receiver_installs_a_closed_runbook_and_lists_it_in_world() {
    let root = temporary();
    let mut app = crate::testing::app();
    app.insert_resource(Time::<Real>::new(Instant::now()))
        .init_resource::<RunStatus>()
        .init_resource::<RunControl>();
    menu::install(&mut app);
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .script_control = Some(root.clone());
    install(&mut app).unwrap();
    assert!(app.world().contains_resource::<Bridge>());

    app.world_mut().resource_mut::<OnlineState>().enabled = true;
    app.update();
    assert_eq!(menu_entry(&mut app, WindowId::Runbook), Some(Display::None));
    assert_eq!(window_display(&mut app), Display::None);

    session::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        1,
        session::player(7),
    );
    app.update();
    assert_eq!(menu_entry(&mut app, WindowId::Runbook), Some(Display::Flex));
    assert_eq!(window_display(&mut app), Display::None);
    drop(app);
    clean(&root);
}

fn menu_entry(app: &mut App, window: WindowId) -> Option<Display> {
    let world = app.world_mut();
    world
        .query::<(&SelectorButton, &Node)>()
        .iter(world)
        .find_map(|(button, node)| (button.0 == window).then_some(node.display))
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
    assert_text(&mut app, "Control files exchanged");
    assert_text(&mut app, "[ ] Pending · First observation");

    let mut running = result(1, "first", "running");
    running["result"]["evidence"] = json!({
        "kind":"scripted_observation","reference":"attempt-1"
    });
    assert_eq!(
        request(&mut app, 2, running).code,
        "runbook_result_recorded"
    );
    assert_text(&mut app, "[…] Running · First observation");
    assert_text(
        &mut app,
        "Reported evidence · Scripted observation · attempt-1",
    );
    let mut blocked = result(1, "first", "blocked");
    blocked["result"]["after_request_id"] = json!(2);
    request(&mut app, 3, blocked);
    assert_text(&mut app, "[!] Blocked · First observation");
    assert!(!has_text(
        &mut app,
        "Reported evidence · Scripted observation · attempt-1"
    ));

    app.world_mut()
        .resource_mut::<Shown>()
        .close(WindowId::Runbook);
    let mut passed = result(1, "first", "passed");
    passed["result"]["after_request_id"] = json!(3);
    request(&mut app, 4, passed);
    assert_eq!(window_display(&mut app), Display::None);
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::Runbook);
    frame(&mut app, 0);
    assert_eq!(
        request(&mut app, 5, result(1, "second", "failed")).code,
        "runbook_result_recorded"
    );
    assert_text(&mut app, "[x] Passed · First observation");
    assert_text(&mut app, "[!] Failed · Second observation");
    assert_text(&mut app, "Synthetic evidence summary");
    assert_text(
        &mut app,
        "Harness run #1 · 0 pending · 0 running · 0 blocked · 0 skipped · 1 passed · 1 failed · All rows reported",
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

    assert_eq!(request(&mut app, 6, begin()).code, "runbook_started");
    assert_text(
        &mut app,
        "Harness run #6 · 2 pending · 0 running · 0 blocked · 0 skipped · 0 passed · 0 failed",
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
