# Script workbench

Scripts are small UTF-8 text files in the client's own scenario language. One line performs one action. They use the normal client controls and the same executor whether launched from a file or the workbench. Loading a file never executes it.

Open Scripts from the button at the top right, or launch with `--script-editor`. The movable window has Editor, Execution and Help tabs. This help is also available on GitHub: both views use these Markdown files.

[Quick start and examples](examples.md)

[Language and command reference](reference.md)

[UI target reference](targets.md)

[Run controls and troubleshooting](running.md)

## Files and editing

Enter a path and choose Open to load an existing script. Edit it in the client or any external editor. Save writes the buffer; entering a new path and choosing Save creates a separate file. Reload reads the current document again. Unsaved edits require an explicit Discard edits confirmation before New, Open or Reload. Save before exiting the client; unsaved buffers live only for the current session.

If the file changed externally, Save refuses to replace it. Save your buffer to a new path or Reload. A script supplied with `--script` starts from its validated snapshot; Open or Reload its file before saving over it in the workbench.

The editor supports selection, multiline entry, clipboard copy/cut/paste, cursor navigation, and scrolling. Execution shows highlighted commands, numbers, key names and comments. The editable field uses uniform text styling; undo/redo and autocomplete are not implemented yet.

## Running

Run executes the current buffer without saving it. The Execution tab follows the current source line and retains the final result and recent events. During a run the source is read-only; edits made externally affect a later load and run.

Pause finishes the current action, then pauses with held inputs released. Step executes one action from a pause. Resume continues; Stop immediately releases synthetic input and stops the scenario. Closing or minimizing the window does not stop a run.

Scripts on P99 and Quarm remain attended: keep the client focused. The existing runtime bounds and local-only command restrictions still apply. See Run controls for pointer ownership and time limits.
