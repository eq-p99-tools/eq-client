# Logging in

[Documentation index](README.md)

## Choose an installation

Use Titanium with P99 or EQEmu, and a TAKP installation with Quarm or TAKP.
The installation supplies terrain, models, UI, strings and spell data. P99 also
uses its patch-level files for validation checksums; Quarm does not run the P99
checksum scan. Keep the installation patched for the server you use.

```console
cargo run -p eq-client --release --locked -- --eq-dir "/path/to/EverQuest"
```

`EQ_CLIENT_DIR` can replace `--eq-dir`. Otherwise the starting preset's saved
installation is used, with the standard Sony EverQuest directory as a Windows
fallback. One process uses one installation. Connecting through a preset that
needs another installation relaunches the client with that directory.

## Login, world and character selection

Normal launch opens the login screen. Choose a preset, enter account and
password, and press Connect or Enter. Tab moves between fields. Choose a world
from the login server's list, then a character and Enter World. Up/Down and Enter
also navigate the lists. No preconfigured character name is required.

Escape backs out of an in-progress login or a list once other applicable UI
actions have been handled. Leaving character selection returns to login, not
the world list: the original login session has ended and the password is not
retained. Login failures leave their reason visible for another attempt.

The character list only previews a dressed model where the server policy exposes
appearance data (currently EQEmu and TAKP). A dark background on P99 or Quarm is
not a failed login.

## Create a character

Choose **Create Character**, click an empty slot, or press **C** at character
selection. Creation is offered on P99, Titanium EQEmu, Quarm and TAKP when fewer
than eight slots are occupied.

1. Type a name or choose Generate name, then choose race, class and gender.
2. Choose a face and the appearance options offered for that race and gender.
   The installed model shows the result; turn it, switch to a face close-up,
   or choose Randomize appearance.
3. Distribute the stat points using +/-, or choose Suggested allocation.
4. Choose a deity and starting city, review the choices, then Create Character.
   Titanium EQEmu also offers starting in its tutorial.

Race/class choices follow the session's era policy and reported expansion mask.
Changing race or class resets the stat allocation and reconciles deity/city
choices. The form enforces base stats, the point budget and each stat's limit;
it sends the correct city index or zone ID for the protocol generation. P99
uses classic faces. Titanium EQEmu, Quarm and TAKP offer Luclin hair, beard,
eye, head-detail and tattoo choices where the race supports them. Preview models,
textures and color palettes come from the local installation.

Generate name asks P99/Titanium worlds for a suggestion. EQMac worlds do not
map that request, so Quarm and TAKP use a local generator, as their normal client
does. Its phonemes are original data, not the official client's name tables.
Generating a name does not reserve it. The server still approves the name when
you submit; it can refuse names or custom combinations. Late suggestions cannot
replace manual edits, a different draft, or a character already submitted.

Enter advances pages and submits on the final page. Tab toggles name editing;
Ctrl+A replaces the name. Cancel or Escape discards an unsent draft. A refusal
keeps the draft editable. While waiting, editing and duplicate submission are
blocked; Disconnect leaves the session. After a 60-second timeout, reconnect and
check the character list before retrying. Success returns to the refreshed list
with the new character selected; Enter World remains a separate action.

Character deletion is not implemented. The optional scripting `create` command
remains available through the same networking policies.

## Login presets

`login-servers.txt` lives in this client's settings directory. First launch seeds
Project 1999, Project Quarm, Local EQEmu and Local TAKP presets. Local emulator
presets need an explicit host; they do not guess that a loopback listener is the
right service. For example, this is fake local configuration, not credentials:

```ini
# eq-client login servers v1
last = Home server

[Home server]
type = eqemu
host = 192.168.1.20
port = 5998
installation = C:\EverQuest
```

The type is `p99`, `quarm`, `eqemu` or `takp`. `--preset "Home server"` selects
one by name. For a preset needing a different installation, name it in the file
or start once with `--eq-dir <folder> --preset "<name>"`.

The client remembers the last preset, account name, world and installation, but
never stores a password in this file. It may seed account/world choices from the
installed Titanium client's `eqhost.txt` and `eqlsPlayerData.ini`; these are read
only. Presets and character settings still contain identifying information and
should stay private.

## Optional automatic login

Interactive login is the normal entry point. Launchers can skip individual
screens using these options:

| Option or environment | Effect |
| --- | --- |
| `--preset`, `EQ_PRESET` | Choose the starting login preset |
| `--online` | Skip login fields using `EQ_ACCOUNT` and `EQ_PASSWORD` |
| `EQ_PROTOCOL` | Choose `p99` (default), `quarm`, `eqemu` or `takp` when selecting a launch preset by type |
| `EQ_LOGIN_HOST`, `EQ_LOGIN_PORT` | Override the launch login endpoint |
| `--server`, `EQ_SERVER` | Choose the exact listed world name without its picker |
| `--character`, `EQ_CHARACTER` | Enter a named character without its picker |
| `--session-seconds` | End each network session after this many seconds, including admission |

World/character overrides apply to the starting preset. Omit them to retain
selection screens. For P99, choose the Blue or Green entry from the live world
list; its displayed name is the value to use for `--server`.

Account/password values are environment inputs, never command-line arguments.
Use a private launcher or secret manager; do not save them in shared scripts or
screenshots. For Quarm, select its preset (or `EQ_PROTOCOL=quarm` for an automatic
launch) and a TAKP directory. [Support](../SUPPORT.md) describes the intentionally
narrow Quarm gameplay policy.

The window's close action cancels and joins the session worker. Camp provides the
game's normal logout flow where offered. Before reconnecting a character from
another client, allow the server's logout timeout if it has not released the
previous session.

## Machine information

The P99 session policy sends the installed-client-style weekly machine report
before world entry: processor, display adapter, memory and screen dimensions.
Its last-send date is kept in this client's own `eqclient.ini`; installation
configuration is only read. Other server policies do not request this report in
the locked networking revision. Some non-Windows field mappings remain
provisional; native Windows and other platforms are not equivalent evidence.
