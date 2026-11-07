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
| `runbook` | Current test checklist or null. See [Runbook checklist](#runbook-checklist) |
| `protocol_error`, `io_error` | Malformed-request or file-channel failure, otherwise null |

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

After observing the required evidence, the harness submits an explicit
result for one item.

```json
{"version":1,"instance":"copy-from-status","id":5,"op":"runbook_result","result":{"runbook_id":4,"item_id":"search","status":"passed","note":"Only the matching spells were visible."}}
```

The result must target the current `runbook.id` and an item in that
checklist. The client rejects stale checklist IDs with `runbook_changed`
and unknown items with `unknown_runbook_item`. A `passed` or `failed`
report is final for that item in the current checklist. Sending an
identical result again leaves it as-is. Changing its status or note gives
`runbook_item_already_reported`.

Omit `note` or send `"note": null` to report without a note. Both forms
represent the same result. An existing terminal report keeps its original note.

Check for `runbook_result_recorded` and read the item back in
`status.json`. The `runbook` object includes `id`, `title` and ordered
`items`. Each item includes `id`, `label`, `status` and `note` (null when
the item has no note). The receipt's `run_id` is reserved for the gameplay executor.

Use 1–64 items with unique, case-sensitive IDs of 1–48 ASCII letters,
digits, hyphens or underscores. Titles, labels and optional notes are
nonblank single-line UTF-8 text, limited to 96, 120 and 180 bytes
respectively. Use brief evidence summaries and keep credentials and raw
captures out of the request. The window renders the text literally.
Malformed payloads appear in `protocol_error`.

The checklist represents the harness's test run and can span login, zoning
and logout within this client instance. The harness must correlate any
packet or gameplay evidence with the correct admission itself. Script
progress, server traffic and opening the window never change an item.
Checklist receipts describe local display state. A passed row means the
harness reported a pass.

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
