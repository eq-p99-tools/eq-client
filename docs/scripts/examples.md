# Quick start and examples

[Help home](README.md)
[Command reference](reference.md)
[Run controls](running.md)

## Start while playing

Log in and enter the world normally. Open Scripts, stay on Editor, and enter this example. Choose Run to toggle inventory twice using its default key. A remapped key has its normal remapped behavior. Execution shows the action in progress; return to Editor when it finishes to change the script and run it again.

```eqscript
# Run in your current game session
# I uses your current key bindings
press I
wait 1000
press I
```

You can enter a file path and Save to reuse the script, or Open an existing file. Neither action runs it. Editing externally works too: save in your external editor, then Reload in the workbench.

## Sit briefly, then stand

This acts on the character you are already playing. Durations are milliseconds. Pause waits for the current action to finish; Stop ends the script immediately. If you stop during the wait, stand up yourself or run the final action separately.

```eqscript
slash sit
wait 5000
slash stand
```

## Capture your current view

Save the script first to give the screenshot an explicit destination folder. This captures your current character's view and writes a state summary to the client log. An unsaved document uses the client's working directory.

```eqscript
report current character
screenshot current-view.png
```

## Search the open spellbook

With the spellbook open, focus its Search field and type through the usual key
input. This example filters for `ward`, then restores the ordinary page.

```eqscript
click button spellbook Spellbook_Search
press W
press A
press R
press D
press Enter
wait 1000
click button spellbook Spellbook_SearchClear
```

## Open a service or inspect a received link

Run `report before action` with the intended service selected or after the
item link arrives. Copy that report's current identities into a short next
run. For example, a `target_interaction` tuple of `(6, 42, 12)` supplies
`interact 6 42 12`. A link reported under admission `6` with message `70`
and ordinal `0` supplies `inspect_link 6 70 0`. Run another report after the
action to see any received result.

## A camera and key sequence

The camera heading uses EQ's 0 to 512 units. The next values are pitch in degrees and optional camera distance. Key chords use `+` with no spaces. This example does not move the character.

```eqscript
camera 128 -55 50
wait 500
press I
wait 500
press I
report camera check complete
```

## Optional offline and startup use

The same inventory and camera examples also work in an offline preview. That is useful for trying a script without connecting, but normal in-game use needs no special launch arguments.

The existing runner accepts `--script path/to/scenario.eqscript`. Add `--script-editor` to watch it execute in the client. `--script-follow` continues reading complete lines appended to that file; it is an append-only workflow, separate from editing and rerunning a whole document.

For a script started before entering the world, `wait_online` can wait for admission before the next action. Login and character-selection commands are optional startup automation; leave them out when running from an already logged-in character.

## Local test servers

Existing local-only test runs may use `gm` and `chat`; ordinary public-server sessions cannot. Launch an EQEmu or TAKP preset with `--online --local-test --script-editor` to use these commands from the workbench without a CLI script. The networking layer then refuses servers outside the local network. P99 and Quarm presets cannot use `--local-test`.

Login scripts read credentials from the existing environment configuration, so account names and passwords do not belong in script files or examples. The existing CLI local test harness remains supported.
