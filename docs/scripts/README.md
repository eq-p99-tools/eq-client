# Script workbench

The script workbench lets you edit and run sequences of actions while playing your character. Log in normally, open Scripts from the button at the top right, and write a script or load one from disk. Run uses your current character, zone, target, inventory and spell setup. You do not need login commands, a character name in the script, or a special launch mode.

The movable window has Editor, Execution and Help tabs. Edit your script, choose Run, and watch the current line in Execution. Stop returns control so you can revise and run it again. You can keep playing with the workbench minimized and reopen it when needed.

Scripts are small UTF-8 text files in the client's own language, with one action per line. They use the normal client controls. Loading a file never executes it. This help is also available on GitHub: both views use these Markdown files.

[Quick start and examples](examples.md)

[Language and command reference](reference.md)

[UI target reference](targets.md)

[Run controls and troubleshooting](running.md)

## Files and editing

Enter a path and choose Open to load an existing script. Edit it in the client or any external editor. Save writes the buffer; entering a new path and choosing Save creates a separate file. Reload reads the current document again. Unsaved edits require an explicit Discard edits confirmation before New, Open or Reload. Save before exiting the client; unsaved buffers live only for the current session.

If the file changed externally, Save refuses to replace it. Save your buffer to a new path or Reload.

The editor supports selection, multiline entry, clipboard copy/cut/paste, cursor navigation, and scrolling. Execution shows highlighted commands, numbers, key names and comments. The editable field uses uniform text styling; undo/redo and autocomplete are not implemented yet.

## Running

Run executes the current buffer without saving it. The Execution tab follows the current source line and retains the final result and recent events. During a run the source is read-only; edits made externally affect a later load and run.

Actions use the state that exists when they execute. Select a target yourself before a script that acts on that target, and have the relevant spell, item or window available. The script does not choose another character or log you in automatically.

Pause finishes the current action, then pauses with held inputs released. Step executes one action from a pause. Resume continues; Stop immediately releases synthetic input and stops the scenario. Closing or minimizing the window does not stop a run.

Scripts on P99 and Quarm remain attended: keep the client focused. The existing runtime bounds and local-only command restrictions still apply. See Run controls for pointer ownership and time limits.

## Other entry points

Offline previews and command-line scripts use the same workbench and executor. `--script-editor` opens the window at launch; `--script` starts a file. These are optional tools for testing and repeatable startup, not prerequisites for in-game scripting. A file supplied with `--script` starts from its validated snapshot; Open or Reload it before saving over it in the workbench.
