# Run controls and troubleshooting

[Help home](README.md)
[Examples](examples.md)
[Command reference](reference.md)
[Variables, conditions and loops](language.md)

## Execution view

The current line is highlighted in the running source snapshot. Linear scripts show completed and total actions. Structured scripts show how many statements ran and how many actions finished, since the path depends on observations and loop counts. The Execution state panel shows the last branch result, current loop iterations and initialized variables. Scroll that panel for more bindings. Recent events and the final outcome remain after execution, including the source line that failed. The event list retains the most recent 64 entries.

Pause waits for a statement boundary. A long wait, held key or navigation action finishes before the pause takes effect. Stop is immediate and releases synthetic input. Step runs one complete action, assignment, condition or loop header before pausing again. Explicit pause time does not count toward the overall run limit.

If the connection ends after a run has entered the world, the run fails with `Session ended` and releases its synthetic input. A normal zone handoff does not end the run; use `wait_zone` before actions that require the destination zone.

## Focus and pointer ownership

P99 and Quarm runs require focus by default. Bring the client to the foreground if it is waiting for focus.

Losing focus while a key is held stops the run and releases it. Offline previews and network-constrained local test sessions do not require foreground focus.

For a default attended run, move the real pointer outside the client window before a scripted click can be injected. This avoids one synthetic click also activating whatever is beneath the real pointer. The Execution tab reports this wait. You can move the pointer back to use Pause or Stop. Explicit background runs isolate native hit testing instead, as described below.

Typing in the workbench belongs to the editor, not movement or chat. While a script runs, the editor gives up keyboard focus so script-generated keystrokes reach the intended controls.

### Explicit background runs

Use `--script scenario.eqscript --script-background` to run through the same in-client input paths while you use another application. Control-only startup also works: `--script-control new-control --script-background`. The background option requires a script or control inbox; both can be supplied together. It works with every server provider and with `--script-follow`. It requests an initially non-activating window; it never changes the focus of an existing window. The platform controls whether that request is honored.

You may focus the client and hover the physical pointer to watch a background run, including waits, reports, clicks, right-clicks and holds. While the script owns input, its own pointer routes UI actions and the physical pointer cannot create a second target. Named-control visibility, clipping and coverage checks still apply. The physical pointer is never moved. Genuine native keyboard, button or wheel input hands native hit testing back before that input is processed and makes an idle runner wait. If the runner is holding a key, movement action or mouse press, that activity cancels the press, releases only the script's input and ends the run instead of resuming a partial action. A new run is required after cancellation. Genuine input remains available to the client.

A background run can minimize and restore the Runbook through its ordinary window controls; dragging and resizing still need real window focus.

Background mode does not enable `gm` or `chat`, change the selected server, or imply an online launch. Network restrictions, gameplay validation, named-control visibility and geometry checks, source and runtime limits, and Stop remain in effect. The default workbench run remains attended on public servers. The background policy is explicit for each run, including any replacement created by an opt-in controller.

## Limits and errors

- Source: at most 128 KiB of UTF-8 text.
- Source statements may occupy up to 500 nonblank, noncomment lines, including block delimiters and accepted follow appends.
- Structured snapshots can execute up to 5,000 statements and 500 actions. Structured actions start at least 100 ms apart when handed to the driver. Legacy linear scripts keep their usual cadence.
- Run: at most 15 minutes, excluding explicit pauses.
- Key holds and traces: at most 10 seconds each.
- Ordinary waits and pointer waits: at most 2 minutes each.
- World-entry waits: at most 3 minutes.

Use `--script` with a complete UTF-8 file to run control flow from an external editor. `--script-follow` can load that initial snapshot. A snapshot without control flow then accepts appended ordinary action lines. A snapshot with control flow takes no appended lines, because they would run ahead of the program's next statement: any append stops follow mode with a line error before it runs, as an appended control-flow or assignment line does. Run a fresh snapshot to add more structured code.

A parse error prevents a run from starting. Runtime failure retains its explanation and source line. A file error leaves the editor buffer intact. If Save reports an external change, save to a new path to preserve both versions, or explicitly reload the disk version.

The opt-in [local file control](control.md) inbox can submit a distinct new run
after a terminal outcome without restarting the client. It uses this executor's
same limits and controls, and requires an explicit request for every new run.

Inventory clicks use the ordinary item controller and current inventory revision. Valid moves apply to the local projection and enqueue in order, with no wait between moves for worker or server replies. Scribing still requires preceding moves to be submitted locally. A local rejection or intervening inventory update discards speculative descendants; queued results drain before another selection can start. The displayed placement remains a prediction until server evidence resolves it. A correction can require a fresh inventory snapshot. An explicit `wait` step still pauses for the requested duration.

## Privacy

Use fake names in shared examples. Keep credentials, private character names, screenshots, packet captures and live-test scripts in your own private files. The workbench saves only when asked and does not publish scripts.
