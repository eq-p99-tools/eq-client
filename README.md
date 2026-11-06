# eq-client

An experimental native EverQuest client with a top-down view, an offline asset
viewer, and online gameplay through the
[`eq-network`](https://github.com/eq-p99-tools/eq-network) libraries. It reads a
user-supplied EverQuest installation. Game assets are not included in this
repository or its builds.

The client includes character selection and creation, chat, inventory and equipment, spellbook
and hotbar controls, movement, targeting, combat and zone transfers. Online
availability depends on the server: P99, EQEmu, Quarm and TAKP do not offer identical
features. See [support and validation](SUPPORT.md) for the current policy and the
distinction between implementation and live proof.

## Start playing

Build with Rust 1.99 or newer and a matching installation: Titanium for P99/EQEmu,
or the TAKP client for Quarm/TAKP. Linux builds also need `pkg-config` and ALSA
development files (`libasound2-dev` on Debian/Ubuntu).

```console
cargo run -p eq-client --release --locked -- --eq-dir "/path/to/EverQuest"
```

In Windows PowerShell:

```powershell
cargo run -p eq-client --release --locked -- --eq-dir "C:\Program Files (x86)\Sony\EverQuest"
```

Choose a login preset, enter your credentials, select a world, then select a
character and enter the game, or click an empty Create New Character slot to make one. A character
name in configuration is optional.
The [login guide](docs/login.md) covers installations, custom servers and optional
automatic login. Keep credentials and private settings outside Git.

On a server whose official client has been watched doing it (P99 today), the
client tells the world what machine it was entered on, once a week, a few
seconds after entering: processor, display adapter, memory and screen sizes
as this machine reports them, the same record the official client sends. The
date of the last report is kept in the client's own settings directory, never
in the EverQuest installation, so the week's wait survives restarts.

## Explore the client

- **Scripts:** choose Scripts from the bottom-right EQ menu to edit, load, run and watch scripts using
  your current character. The Help tab renders the same
  [script guide](docs/scripts/README.md) available on GitHub.
- **Map:** Backspace or M opens known entity positions on every server. Search, inspect
  and track names locally; the separate Target action uses normal targeting checks.
- **Camera:** right-drag to look and use the wheel to zoom. Assign Toggle Free Camera
  in Options to detach and return. Free-camera controls do not walk the character.
- **Keyboard:** Options → Keyboard lists and edits primary/alternate bindings.
  Defaults depend on the installed client; Titanium does not bind WASD movement
  by default. Saved character bindings take precedence.
- **Chat:** Enter opens composition. Item links open item details where supported;
  explicit HTTP(S) links open the system browser.
- **Windows:** move titlebars and minimize framed windows where offered. Scripts
  opens and closes without minimizing. Passive panels move by dragging their
  surface. Layouts are saved per character.

See the [documentation index](docs/README.md) for
[controls, maps and chat](docs/controls.md), [detachable chat tabs](docs/chat-tabs.md), [gameplay](docs/gameplay.md),
[settings](docs/settings.md), and [assets and offline previews](docs/assets.md).
The shortcuts above are defaults and can be reassigned.

## Offline preview

```console
cargo run -p eq-client --release --locked -- --eq-dir "/path/to/EverQuest" --offline --zone ecommons
```

Add `--demo-entities`, `--demo-inventory`, `--demo-spellbook`,
`--demo-character-select` or `--demo-trade` for synthetic previews. Use
`--inspect-only` to check zone loading without a window, or
`--screenshot offline.png` for a hidden-window render that saves a frame and exits.
These modes still need your installation and do not prove live compatibility.

## Development

The workspace separates coordinate conversion (`eq-client-axes`), game state and
rules (`eq-client-core`), installed data (`eq-client-assets`), Bevy presentation
(`eq-client-render`), and application/session lifetime (`eq-client`). Wire codecs
and server policies live in the networking repository. Cargo resolves that Git
dependency through the committed lockfile; no sibling checkout is required.

Read [architecture contracts](ARCHITECTURE.md) and the
[development guide](docs/development.md) before extending a feature. Run locally:

```console
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Automatic PR checks run formatting only. The full Linux/Windows workflow runs
on demand; a green Format check is not a full test run.

## License

Original code is available under the [MIT License](LICENSE). The asset loader
uses a pinned [eq-p99-tools/libeq fork](https://github.com/eq-p99-tools/libeq) of
[cjab/libeq](https://github.com/cjab/libeq). Dependency licenses and the rights
to the user's installed game data remain separate.
