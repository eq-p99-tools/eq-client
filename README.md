# eq-client

A native EverQuest client written in Rust. The goal is a client that can stand
in for the original one for daily play on Project 1999 (P99), stock EQEmu,
Project Quarm and stock TAKP servers.

The client draws the game in 3D with a chase camera and first person, using the
EverQuest installation you supply. Networking comes from the
[`eq-network`](https://github.com/eq-p99-tools/eq-network) libraries. No game
assets are included in this repository or its builds.

The rule throughout is parity: the client answers each server the way that
server's original client does, from the same signals, with no per-server
special cases. Anything that departs from the original client is a
quality-of-life choice you turn on yourself.

The project is under active development. Each feature is described in three
stages: implemented in code, offered by a server's policy, and verified live on
that server. The [support and validation](SUPPORT.md) page keeps those apart for
each server.

## Servers

| Server | Installation the client reads | Protocol |
| --- | --- | --- |
| Project 1999 | Titanium | Titanium, with P99's protection |
| EQEmu (stock) | Titanium | Titanium |
| Project Quarm | TAKP | EQMac |
| TAKP (stock) | TAKP | EQMac |

What a session offers depends on the server type, and the client greys out
anything the session does not offer. The networking library decides which
features each server type gets. A server type gains a feature only after it has
been checked against that kind of server. The full per-feature table is in
[SUPPORT.md](SUPPORT.md#server-policy-snapshot).

## What it does

- **Logging in:** login presets for each server, the world list, character
  selection that shows the chosen character dressed and standing in its zone,
  and character
  creation with faces, hair and other appearance choices drawn from your
  installation. See [Logging in](docs/login.md).
- **Moving around:** walking, running, jumping, falling, ducking and sitting;
  swimming that follows your character's look pitch; lifts, platforms and
  boats; zoning; and camping out. Movement follows the original client's
  speeds and collision, such as stepping up low edges like stairs.
- **Playing:** targeting, considering, melee and abilities; casting, the
  spellbook, spell sets and buffs; pets; groups, and raids where the server
  offers them; inventory, containers, tradeskill combines and the bank;
  merchants, giving and trading; looting; doors and items on the ground; Bind
  Wound; training; and guild rosters. See [Gameplay](docs/gameplay.md).
- **Chat:** channels and tabs that you can detach and dock, item links that
  open item details, and links to web pages. See
  [Controls, maps and chat](docs/controls.md) and
  [chat tabs](docs/chat-tabs.md).
- **Interface:** your installed UI skin and its window layouts, a hotbar and
  social hotbuttons, key bindings that start from your installed client's
  defaults, and window positions saved for each character.
- **Map:** Backspace or M opens a map of the zone and the entities the client
  knows about, on every server, with search and tracking.
- **Camera:** right-drag to look and use the wheel to zoom. Zoom all the way in
  for first person. A free camera can be assigned in Options, and
  `--camera orthographic` gives an isometric-style view.
- **Sound:** Titanium sound effects from your installation.

The shortcuts above are defaults and can be reassigned in Options → Keyboard.

## Build and run

You need Rust 1.99 or newer and an EverQuest installation that matches your
server: Titanium for P99 and EQEmu, or the TAKP client for Quarm and TAKP. On
Linux you also need `pkg-config` and the ALSA development files
(`libasound2-dev` on Debian and Ubuntu).

```console
cargo run -p eq-client --release --locked -- --eq-dir "/path/to/EverQuest"
```

In Windows PowerShell:

```powershell
cargo run -p eq-client --release --locked -- --eq-dir "C:\Program Files (x86)\Sony\EverQuest"
```

`EQ_CLIENT_DIR` can stand in for `--eq-dir`. Choose a login preset, enter your
account and password, pick a world, then pick a character and enter the world,
or click an empty slot to create one. The [login guide](docs/login.md) covers
installations, adding your own login servers, and optional automatic login.
Keep credentials and private settings out of Git.

`cargo run -p eq-client --locked -- --help` lists every option. Some useful
ones:

| Option | What it does |
| --- | --- |
| `--settings-dir DIR` | Where this client keeps its own files (default: `eq-client` in your user settings directory) |
| `--preset NAME` | The login preset to start on |
| `--ui-skin NAME` | A UI skin under the installation's `uifiles` |
| `--max-fps N` | Frame cap; `0` leaves pacing to vsync |
| `--window-size WxH`, `--window-position X,Y` | Window size and placement in physical pixels |
| `--no-navmesh` | Turns off the zone navigation meshes that script walks use |
| `--script-editor` | Opens the Scripts window at launch |

The client keeps its own settings in its settings directory and only reads the
installation's files. On P99, where the original client has been seen sending
it, the client sends the same weekly report of the machine you play on just
before entering the world: processor, display adapter, memory and screen size.
The date of the last report is kept in the client's settings directory, so the
weekly wait carries over between runs.

## Settings and quality of life

Options → QoL holds choices that are saved for each character. Some are plain
improvements, such as interface scaling, chat timestamps, flashing on tells,
floating combat text, a longer chase camera, automatic looting and a clearer
view at night.

Anything that departs from the original client's behaviour sits in the
**Dangerous** section, and each starts at the original client's behaviour:
running up to 1.4 times faster, walking while stunned, memorizing without
sitting, flying, walking on water, going without air, and staying seated when
hit, among others. These are offered only where
the server type allows gameplay departures; elsewhere they are greyed out and
the session keeps the original client's rules. Whether to use them on a public
server is up to you.

A few improvements are always on, such as maps on every server, a countdown
while camping, buff time remaining and weapon ratios. See
[Settings](docs/settings.md) for every choice and the file it is saved in.

## Scripts

Scripts automate sequences of actions with the character you are playing.
Choose Scripts from the EQ menu at the bottom right. The Scripts window has an
editor, a view of the running script, and Help, which shows the same
[script guide](docs/scripts/README.md) that is on GitHub.

Scripts are written in a small subset of Python. Every action is a call, blocks
are indented, and variables have types:

```eqscript
# Open and close the inventory, then say whether the character is in the world
def toggle_inventory(times: int):
    for _ in range(times):
        press("I")
        wait(500)

toggle_inventory(2)
if client.ready:
    report("in the world")
else:
    report("not ready yet")
```

- **Editor:** syntax highlighting, error marks that update as you type, and a
  file library in your installation's `Scripts` folder.
- **Debugging:** click a line number to set a breakpoint. Run to cursor, Step
  over, Step into and Step out work alongside Run, Pause, Resume and Stop, and
  the Execution view follows the current line.
- **Functions:** `def name(parameter: type):` defines one, with `return`; the
  Execution view lists the calls in progress.
- **Conditions:** `if`, `elif`, `else`, `for` and bounded `while` loops can read
  the client's state, such as `client.ready`, `zone.name` or `target.selected`.
- **Outside the client:** `--check-script --script FILE` checks a file without
  starting the game, `--script FILE` runs one at launch, and `--script-control
  DIR` accepts runs through local files. A [Vim and Neovim plugin](contrib/vim)
  highlights and indents scripts.

Scripts use your normal controls and limits. Scripts on P99 and Quarm run only
while the window has focus, unless you choose otherwise. GM commands work only
on a local test server. See the [command reference](docs/scripts/reference.md)
and [run controls](docs/scripts/running.md).

## Offline preview

```console
cargo run -p eq-client --release --locked -- --eq-dir "/path/to/EverQuest" --offline --zone ecommons
```

Add `--demo-entities`, `--demo-inventory`, `--demo-spellbook`,
`--demo-character-select`, `--demo-trade` or `--demo-weather` for synthetic
previews. `--inspect-only` checks that a zone loads without opening a window,
and `--screenshot offline.png` renders a frame in a hidden window, saves it and
exits. These modes still need your installation and say nothing about live
compatibility. See [assets and offline previews](docs/assets.md).

## Documentation

The [documentation index](docs/README.md) lists every guide:
[logging in](docs/login.md), [controls, maps and chat](docs/controls.md),
[chat tabs](docs/chat-tabs.md), [gameplay](docs/gameplay.md),
[settings](docs/settings.md), [assets and offline previews](docs/assets.md),
the [script guide](docs/scripts/README.md), and
[support and validation](SUPPORT.md). Changes are recorded in the
[changelog](CHANGELOG.md).

## Development

The workspace is split into coordinate conversion (`eq-client-axes`), game state
and rules (`eq-client-core`), installed data (`eq-client-assets`), Bevy drawing
(`eq-client-render`), and the application and session lifetime (`eq-client`).
Wire formats and server policies live in
[`eq-network`](https://github.com/eq-p99-tools/eq-network). Cargo pins that
dependency by Git revision in the committed lockfile, so no sibling checkout is
needed.

Read the [architecture contracts](ARCHITECTURE.md) and the
[development guide](docs/development.md) before extending a feature. Checks to
run locally:

```console
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Pull requests run the Format check automatically. The full Linux and Windows
workflow runs on demand, so a green Format check is not a full test run.

## License

Original code is available under the [MIT License](LICENSE). The asset loader
uses a pinned [eq-p99-tools/libeq fork](https://github.com/eq-p99-tools/libeq) of
[cjab/libeq](https://github.com/cjab/libeq). Dependency licenses and the rights
to your installed game data are separate. Include the
[third-party notices](THIRD-PARTY-NOTICES.md) with binary distributions.

This project is not affiliated with Daybreak Game Company, Project 1999, The
Al'Kabor Project or Project Quarm.
