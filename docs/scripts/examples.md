# Quick start and examples

[Help home](README.md)
[Command reference](reference.md)
[Run controls](running.md)

## A small offline preview

Open the workbench in an offline preview, paste this example, and choose Run. It opens and closes inventory with its default key. A remapped key has its normal remapped behavior.

```eqscript
# Durations are milliseconds
report before
wait 750
press I
wait 1000
report inventory opened
press I
```

## Inspect an admitted character

This waits for world entry, reports state and saves a screenshot beside the script. Save the file before running to give the screenshot an explicit destination folder. An unsaved document uses the client's working directory.

```eqscript
wait_online
report world entry
wait 1000
screenshot world-entry.png
```

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

## CLI use

The workbench is available without a command-line script. The existing runner still accepts `--script path/to/scenario.eqscript`. Add `--script-editor` to watch it execute in the client. `--script-follow` continues reading complete lines appended to that file; it is an append-only workflow, separate from editing and rerunning a whole document.

## Local test servers

Existing local-only test runs may use `gm` and `chat`; ordinary public-server sessions cannot. Launch an EQEmu or TAKP preset with `--online --local-test --script-editor` to use these commands from the workbench without a CLI script. The networking layer then refuses servers outside the local network. P99 and Quarm presets cannot use `--local-test`.

Login scripts read credentials from the existing environment configuration, so account names and passwords do not belong in script files or examples. The existing CLI local test harness remains supported.
