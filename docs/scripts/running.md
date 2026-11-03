# Run controls and troubleshooting

[Help home](README.md)
[Examples](examples.md)
[Command reference](reference.md)

## Execution view

The current line is highlighted in an immutable snapshot of the running script. Progress counts completed actions. Recent events and the final outcome remain after the executor finishes, including the source line that failed. The event list is bounded to the most recent 64 entries; the workbench displays the latest entries.

Pause waits for an action boundary. A long wait, held key or navigation action finishes before the pause takes effect. Stop is immediate and releases synthetic input. Step is available while paused and executes one complete action before pausing again. Explicit pause time does not count toward the overall run limit.

If the connection ends after a run has entered the world, the run fails with `Session ended` and releases its synthetic input. A normal zone handoff does not end the run; use `wait_zone` before actions that require the destination zone.

## Focus and pointer ownership

P99 and Quarm runs are attended. Bring the client to the foreground if it is waiting for focus.

Losing focus while a key is held stops the run and releases it. Offline previews and network-constrained local test sessions do not require foreground focus.

Before a scripted click can be injected, move the real pointer outside the client window. This avoids one synthetic click also activating whatever is beneath the real pointer. The Execution tab reports this wait. You can move the pointer back to use Pause or Stop. Do not remove this ownership rule when writing a scenario.

Typing in the workbench belongs to the editor, not movement or chat. While a script runs, the editor gives up keyboard focus so script-generated keystrokes reach the intended controls.

## Limits and errors

- Source: at most 128 KiB of UTF-8 text.
- Queue: at most 500 actions, including appended actions in follow mode.
- Run: at most 15 minutes, excluding explicit pauses.
- Key holds and traces: at most 10 seconds each.
- Ordinary waits and pointer waits: at most 2 minutes each.
- World-entry waits: at most 3 minutes.

A parse error prevents a run from starting. Runtime failure retains its explanation and source line. A file error leaves the editor buffer intact. If Save reports an external change, save to a new path to preserve both versions, or explicitly reload the disk version.

Inventory clicks use the ordinary item controller and current inventory revision. Each valid move sends immediately after the preceding local result is available; scripts need no fixed wait for earlier server replies. The displayed placement remains a prediction until server evidence resolves it. A correction can require a fresh inventory snapshot. An explicit `wait` step still pauses for the requested duration.

## Privacy

Use fake names in shared examples. Keep credentials, private character names, screenshots, packet captures and live-test scripts in your own private files. The workbench saves only when asked and does not publish scripts.
