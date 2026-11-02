# Script workbench

The script workbench lets you edit and run sequences of actions while playing your character. Log in normally, click the EQ menu at the bottom right, choose **Scripts**, and write a script or load one from disk. Scripts starts closed unless you explicitly request `--script-editor`. Run uses your current character, zone, target, inventory and spell setup. You do not need login commands, a character name in the script, or a special launch mode.

The movable window has Editor, Execution and Help tabs. Edit your script, choose Run, and watch the current line in Execution. Stop returns control so you can revise and run it again. You can keep playing with the workbench minimized and reopen it when needed.

Scripts are small UTF-8 text files in the client's own language, with one action per line. They use the normal client controls. Loading a file never executes it. This help is also available on GitHub: both views use these Markdown files.

[Quick start and examples](examples.md)

[Language and command reference](reference.md)

[UI target reference](targets.md)

[Run controls and troubleshooting](running.md)

[Vim and Neovim syntax plugin](vim.md)

The in-game Help tab keeps a clickable breadcrumb above the article as you scroll. The links inside each document remain available in both the game and GitHub.

## Files and editing

Keep scripts in the `Scripts` folder inside your EQ installation. The Editor's library lists folders and `.eqscript`, `.script`, and `.txt` files. The `.eqscript` suffix is hidden in the list; actual filenames are unchanged. Click a folder to browse it, Up to return, and a file to load it. Refresh picks up files added or renamed in an external editor. Nested folders are supported; no full path is needed. **Files** collapses or restores the library panel to give the editor more room without losing your folder or document.

New starts a blank buffer. Give it a name relative to Scripts, such as `practice/camera.eqscript`, then Save. Missing folders are created on save; an installation you cannot write to reports an error and preserves the buffer. Save writes the buffer; a different name creates a separate file. Reload reads the current document again. Unsaved edits require an explicit Discard edits confirmation before New, opening another file, or Reload. Save before exiting the client; unsaved buffers live only for the current session.

If the file changed externally, Save refuses to replace it. Save your buffer to a new path or Reload.

The editor includes syntax highlighting, line numbers, selection, multiline entry, clipboard copy/cut/paste, undo/redo, cursor navigation, and scrolling. Invalid lines are underlined in red; hover one for the parser's explanation. The validation summary reports the error count and first error even when that line is out of view. Execution also shows highlighted source and follows the running line. Autocomplete is not enabled.

## Running

Run executes the current buffer without saving it. The Execution tab follows the current source line and retains the final result and recent events. During a run the source is read-only; edits made externally affect a later load and run.

Actions use the state that exists when they execute. Select a target yourself before a script that acts on that target, and have the relevant spell, item or window available. The script does not choose another character or log you in automatically.

Pause finishes the current action, then pauses with held inputs released. Step executes one action from a pause. Resume continues; Stop immediately releases synthetic input and stops the scenario. Closing or minimizing the window does not stop a run.

Scripts on P99 and Quarm remain attended: keep the client focused. The existing runtime bounds and local-only command restrictions still apply. See Run controls for pointer ownership and time limits.

## Other entry points

Offline previews and command-line scripts use the same workbench and executor. `--script-editor` opens the window at launch; `--script` can still start a file from an explicit path. These are optional tools for testing and repeatable startup, not prerequisites for in-game scripting. A file supplied with `--script` starts from its validated snapshot; Reload it before replacing it, or save a copy under Scripts with a relative name.
