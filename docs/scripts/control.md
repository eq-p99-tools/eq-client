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
