# Local script control

[Help home](README.md)
[Run controls](running.md)
[Command reference](reference.md)

Start the client with `--script-control DIRECTORY` to control its existing script
executor through local files. Use a new private directory; the client creates it
and refuses an existing path. Each launch needs its own directory. This option
works without an initial script and can run scenarios after you log in normally.

For an initially non-activating window and runs that proceed while you use another
application, add `--script-background`: `--script-control new-control --script-background`.
No dummy initial script is needed. Every explicit new run inherits that launch's
background policy, including recovery after an error. You may focus the client
or hover the physical pointer to watch read-only progress. Genuine keyboard,
button or wheel input during held script input cancels the run; status reports
`waiting_for_input` when it is waiting to take ownership. Background clicks and
holds use the script pointer while native hit testing is isolated; the physical
pointer is never moved and cannot create a second synthetic click target.
The default remains attended on public servers. See Run controls for the full
input handoff rules and the platform limits on initial non-activation.

Use `--script-follow` to append actions during a healthy running scenario. The
control inbox lets you explicitly start another scenario after completion, Stop,
an error, a session ending or the runtime limit. It does not resume or retry a
failed scenario automatically. Every run uses the same parser, input ownership,
local-only commands and limits as the workbench.

## Submit a request

Read `status.json` and copy its `instance` into a request. Write a temporary file
beside `request.json`, then atomically rename/replace it as `request.json`. Do not
edit the inbox in place or append requests. Submit one request at a time and wait
for its receipt before replacing the inbox.

To start an immutable source snapshot:

```json
{"version":1,"instance":"copy-from-status","id":1,"op":"start","source":"report ready\nscreenshot ready.png\n"}
```

`source` is inline UTF-8 script text. Screenshot filenames are relative to the
control directory. Source paths, shell commands and remote requests are not
accepted by this interface. Unknown fields are rejected. The JSON file is capped
at 256 KiB; decoded script text retains the 128 KiB source, 500-action and
15-minute limits from Run controls. Request files must be ordinary files; links,
reparse points and directories are refused.

An active run, including a paused or followed run, rejects another start. There
is no gameplay queue or automatic replacement. To stop, use the run ID actually
reported by status:

```json
{"version":1,"instance":"copy-from-status","id":2,"op":"stop","run_id":1}
```

Stop requests the workbench's normal immediate Stop: release synthetic input,
cancel a pending click and discard remaining actions. It does not camp or close
the client. A delayed stop for an older run is rejected. Wait for that run's
`stopped` outcome, then use a new request ID to start another scenario.

IDs are positive unsigned integers and must increase. An exact repeat of the
last processed request returns its cached receipt and never executes again.
Changing a request while reusing its ID gives `id_conflict`; an older ID gives
`stale_id`. A rejected valid new request also consumes its ID. Correct a rejected
scenario and submit it with a higher ID, rather than retrying its old request.

## Observe the result

Poll `status.json`. It is replaced as a complete JSON document about four times
per second while file work can proceed. Reading it does not need a request;
`{"version":1,"instance":"copy-from-status","id":3,"op":"status"}` also
requests an observation.

| Field | Meaning |
| --- | --- |
| `version`, `instance` | Protocol version and this client launch's identity |
| `client_state`, `revision` | `open` or `closed`, and the publication heartbeat |
| `last_processed_id` | Highest consumed request ID |
| `last_request` | Last receipt: `id`, `outcome`, `code`, `run_id`, `line`, `message` |
| `run` | Current/latest run: `id`, `origin`, `phase`, `line`, `completed_steps`, `total_steps`, `elapsed_ms`, `message` |
| `recent_runs` | At most 16 terminal run snapshots, oldest first |
| `admission_id` | Current connected online admission outside a zone transfer, or null |
| `control_health` | State of the local file exchange. See [Control health](#control-health) |
| `runbook` | Current test checklist or null. See [Runbook checklist](#runbook-checklist) |
| `runbook_export`, `runbook_ui_export` | Latest harness and window export attempts, respectively, or null. See [Export a checklist](#export-a-checklist) |
| `protocol_error`, `io_error` | Malformed-request or file-channel failure, otherwise null |

For a linear run, `total_steps` is the accepted action count. For a structured
run, `total_steps` is null because branches and loops determine how many
actions will run. Structured snapshots also report `authored_actions` for the
accepted action lines and `executed_statements` for structured interpreter
steps, including condition checks and repeated action lines. `completed_steps`
counts actions that finished in the executor. Use `phase` to determine the
run's outcome.

`last_request.outcome` is `accepted`, `rejected`, `stop_requested` or `observed`.
Start acceptance means only that a validated local run was queued. It does not
mean an action ran or a server accepted it. A stop receipt also precedes the
executor's terminal result. Check the matching request ID and then the matching
run ID; CLI and workbench runs have their own run IDs with origin `other`.

Run phases are `queued`, `running`, `waiting_for_focus`, `waiting_for_pointer`,
`pausing`, `paused`, `following`, `completed`, `stopped` and `failed`. The source
line and bounded final message identify an error without copying the script into
status. A terminal result remains available after the active executor is removed.

Normal client shutdown publishes `client_state: "closed"`; an unfinished run is
reported as failed. This describes the client, not a clean logout. A crash or file
failure can leave stale status. A stopped heartbeat, missing file, acceptance
receipt or process exit never proves script completion or server acceptance.

## Control health

The Runbook window and `status.json` show the client's last observation of the
local file channel. It is `waiting` before the first successful exchange,
`ready` after an exchange (displayed as **Control files exchanged**),
`request_error` for malformed input, or `io_error` for
a file error. The client shows `stalled` when two seconds have elapsed since
the last successful exchange without a newer one. Normal shutdown publishes
`closed`. Monitor advancement of `revision` separately because a file error
or a stalled exchange can prevent a new status from reaching disk. This health
field describes the file channel. The script and checklist have their own states.

## Runbook checklist

When launched with `--script-control`, open **EQ → Runbook** while in the
world to watch the harness's current checklist. The window starts closed.
You can move, minimize or close it without losing the checklist. The local
harness can submit new checklists and results while a gameplay script is running.

Start a checklist through the same private request file and request ID
sequence. Supply the title and ordered items inline.

```json
{"version":1,"instance":"copy-from-status","id":4,"op":"runbook_start","runbook":{"title":"Spellbook search","items":[{"id":"search","label":"Search filters the spell list"},{"id":"clear","label":"Clear restores the full list"}]}}
```

The `runbook_started` receipt accompanies the new `runbook` snapshot. Its
`id` is the start request ID in this client instance. Every item begins
`pending`. To reset, send another `runbook_start` with a fresh request ID.
The new checklist replaces the old one and clears all results and notes,
even if you reuse the title and item IDs.

The harness can report progress or an outcome for each item independently.

```json
{"version":1,"instance":"copy-from-status","id":5,"op":"runbook_result","result":{"runbook_id":4,"item_id":"search","status":"running"}}
```

```json
{"version":1,"instance":"copy-from-status","id":6,"op":"runbook_result","result":{"runbook_id":4,"item_id":"search","status":"passed","after_request_id":5,"note":"Only matching spells were visible.","evidence":{"kind":"manual_witness","reference":"witness-search-1"}}}
```

The result must target the current `runbook.id` and an item in that
checklist. The client rejects stale checklist IDs with `runbook_changed`
and unknown items with `unknown_runbook_item`.

- `pending` means the harness has submitted no report for the item. Submit the
  first report without `after_request_id`. Existing direct `passed` and
  `failed` reports are still supported.
- `running` and `blocked` describe active work or a missing prerequisite.
  To change either report, supply its `report_request_id` as
  `after_request_id`. Capture that ID with the observation. A report based on
  an older row gives `runbook_item_changed`.
- `passed`, `failed` and `skipped` are final in the current checklist. A
  conflicting report gives `runbook_item_already_reported`. An identical
  report is idempotent and keeps the original row and checklist revision
  even after the admission changes or ends.

Check for `runbook_result_recorded` and read the item back in
`status.json`. The `runbook` object includes `id`, `title`, `revision`,
`reported_by: "producer"`, `health` and ordered `items`. The revision is zero
until the first report and then takes the ID of the last request that
changed a row. Each item includes `id`, `label`, `status`, `note`, `evidence`,
`admission_id` and `report_request_id`. Optional values and the request ID for
a pending row are null. The receipt's `run_id` is reserved for the gameplay
executor.

`health` contains the count for each row state. The overall `state` is
`complete` when every row is final (displayed as **All rows reported**),
`running` when any row is running,
`blocked` when work is blocked and no row is running, `pending` when all rows
are pending, or `incomplete` when final and pending rows coexist. Read the
individual counts to distinguish passes, failures and skips.

The optional `evidence` object has a `kind` of `scripted_observation`,
`manual_witness` or `received_data` and a `reference` of 1–96 ASCII letters,
digits, hyphens or underscores. Use a private artifact ID. The client displays
the producer's category and reference literally. It keeps them separate from
the client-observed script phase and online admission. A category describes
what the producer cites. A row pass means the harness reported a pass, and
server confirmation needs its own evidence. The client treats the reference
as opaque text. Omitted or null `note` and `evidence` represent absence.

Use 1–64 items with unique, case-sensitive IDs of 1–48 ASCII letters,
digits, hyphens or underscores. Titles, labels and optional notes are
nonblank single-line UTF-8 text, limited to 96, 120 and 180 bytes
respectively. Control characters, line separators and bidirectional formatting
controls are rejected. Use brief evidence summaries and keep credentials and
raw captures out of the request. Malformed payloads appear in `protocol_error`.

The checklist represents the harness's test run and can span login, zoning
and logout within this client instance. For a zone-specific observation, copy
the positive `admission_id` from status when collecting the evidence and
include it in the report. The client rejects new or changed reports scoped to a
different or no longer active admission with `runbook_admission_changed`.
Leave it absent for checklist
reports that intentionally span admissions or describe an offline case.
Previously recorded rows retain their original scope across zoning and
logout. Script progress, server traffic and window visibility leave row
state to the harness.

### Export a checklist

Select **Export JSON** in the window, or submit a control request naming
the current checklist.

```json
{"version":1,"instance":"copy-from-status","id":7,"op":"runbook_export","runbook_id":4}
```

Both entry points capture the checklist at its current revision and write to
the private control directory. A control request writes `runbook-export.json`;
**Export JSON** writes `runbook-ui-export.json`. Each successful export
atomically replaces the previous file from the same entry point. The export
contains the control version, instance, request ID (null for the window),
admission at capture time and the captured `runbook`. Keep it private
alongside any cited evidence.

A control receipt of `runbook_export_queued` records acceptance. Wait for
`runbook_export.state` to become `written` with the matching `request_id`,
`runbook_id` and `revision`, then read back the matching export before starting
another export. The attempt can also be `writing`, `failed` with an error,
or `unconfirmed` if the client closed before observing the write result. A
failed attempt can leave an earlier file in place. A request for a replaced
checklist gives `runbook_changed`, and a second harness export while its
previous one is writing gives `runbook_export_busy`. Window attempts appear
independently in `runbook_ui_export` and in the window's export label. Changes
to the checklist after capture appear in a subsequent export.

## Explicit recovery

After a terminal error you can inspect its line and submit a different small
scenario without restarting the client or rewriting an earlier script. For an
ordinary camp request, use the normal commands and wait for the character list:

```eqscript
slash camp
wait_select
report after_camp
```

Failure remains an error requiring attention. The bridge adds no automatic
retry, resume, quit or forced disconnect. Keep scripts, status, screenshots and
controller files private; a request file grants the same script actions as Run
in the workbench.
