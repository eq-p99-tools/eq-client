# Support and validation

[Documentation index](docs/README.md) · [Gameplay guide](docs/gameplay.md)

Three separate questions determine support:

1. **Implemented:** a codec, reducer or UI path exists.
2. **Offered:** the session advertises a capability under its server policy.
3. **Verified:** a named scenario passed against a specified server and revision.

An offered capability is a policy decision, not proof that all its scenarios
have been tested live. Some provisions are source-derived and still need live
checks. Shared Titanium/EQMac layouts do not imply identical P99/EQEmu or
Quarm/TAKP behavior. Frontends use runtime capabilities, not server-name flags.

## Server policy snapshot

The following summarizes the networking revision pinned by this application's
Cargo.lock: `182e99dd2b40e3e58feaacdbff415f9797def76d`. Its
[server policy and tests](https://github.com/eq-p99-tools/eq-network/blob/182e99dd2b40e3e58feaacdbff415f9797def76d/crates/eq-network/src/client/session/servers.rs)
are the source for this table. **Yes means provided by that policy**, not a live
certification, and individual operations still validate their prerequisites.

| Area | P99 | EQEmu | Quarm | TAKP |
| --- | --- | --- | --- | --- |
| Protocol / installation | Titanium | Titanium | EQMac / TAKP | EQMac / TAKP |
| Login, character selection, world state and chat | Yes | Yes | Yes | Yes |
| Character appearance preview at selection | Yes | Yes | Yes | Yes |
| Movement and falling | Yes | Yes | Yes | Yes |
| Jumping | Yes | Yes | Yes | Yes |
| Sitting, standing and ducking | Yes | Yes | Yes | Yes |
| Casting and basic spellbook | Yes | Yes | Yes | Yes |
| Move / delete book entries | Yes / No | Yes / Yes | No / No | No / No |
| Inventory, trading and giving | Yes | Yes | No | Yes |
| Targeting | Yes | Yes | Yes | Yes |
| Combat and considering | Yes | Yes | No | Yes |
| Looting, doors and ground-object features | Yes | Yes | No | No |
| Camping and zone transfers | Yes | Yes | Yes | Yes |
| Groups | No | Yes | No | Yes |
| Raids, training, reading and tradeskills | No | Yes | No | No |
| Pets and corpse commands | Yes | Yes | No | No |
| Resurrection / translocate answers | No / No | Yes / Yes | No / No | No / Yes |
| Character creation UI and script | Yes | Yes | Yes | Yes |

Quarm additionally offers ground movement, jumps, falls, posture, camping and
zone transfers through the shared EQMac path, plus target selection/clearing,
gem and item-effect casting, scribing, memorizing and forgetting.
Its authoritative inventory and coins are observed for carried weight and item
clicks; item/coin moves and merchant actions remain disabled.
Posture controls use `Capability::Posture`, independently of `Moving`. The newly
enabled public-server paths are source/capture-backed and regression-tested;
their live validation is tracked separately. Swimming, boats and client-reported
bleed-out remain gaps. P99 still does not offer deletion of spellbook entries.

See the networking [P99/Quarm gap report](https://github.com/eq-p99-tools/eq-network/blob/182e99dd2b40e3e58feaacdbff415f9797def76d/docs/public-server-gaps.md)
for missing, disabled, unverified and server-restricted behavior.

### Public-parity checks, 2026-11-02

Against networking `d713d0b4` and the desktop changes based on `9be679a`:

- Local TAKP with Quarm policy: character selection and a rendered appearance
  preview passed. Further local testing was interrupted by Docker shutdown.
- Public Quarm: appearance preview, admission, authoritative inventory/coins,
  self-targeting, brief forward/backward/left/right movement and jump/landing
  were exercised while connected. HP increased during the recorded checks;
  no movement-proposal rejection or stamina decoding error was logged. Guild
  chat was received. Local acceptance is not an independent server-side speed
  measurement or proof that position persisted after reconnecting.
- The operator subsequently relocated the character to a safer area and closed
  the client. NPC targeting/clear, walking/turning, camp cancellation, zone
  handoffs and death/bind remain unverified at this head. P99 previews/jumping
  still need the integrated-client live check.
- The operator reported that `/camp desktop` appeared to return to selection.
  The log records a desktop-camp exit followed by application status 0; the
  visible transition remains a focused retest item, not a reproduced defect.

Native Windows formatting, strict workspace/all-target Clippy and 1,299 tests
passed. The 32 ignored asset/live probes were not counted as passes. The
networking stack separately passed 791 tests and warnings-as-errors API docs.
Private scripts, screenshots, logs and credentials are not published.

The subsequent networking pin `182e99dd` adds Quarm casting and basic spellbook
support. All 797 networking tests pass, including six new synthetic Quarm
assembly checks, alongside strict Clippy, formatting and API docs. These new
spell actions have not been tested on public Quarm; the earlier session does
not validate them. The desktop was rebuilt and rechecked against this exact pin:
1,299 tests passed (32 ignored probes excluded), alongside strict Clippy and
formatting.

Maps, free camera, key editing, chat-link presentation and the script editor are
local client features. The map shows received entity positions on every server
without requiring `Capability::Map`. Its explicit Target action still requires
normal targeting support. A script can manipulate local UI anywhere but does
not bypass gameplay capabilities or attended-run restrictions.

## Evidence and remaining boundaries

A previous successful login/chat or individual action test is useful evidence for
that scenario and revision, not a full current-client certification. Synthetic
protocol/reducer tests, official-client observations, offline asset renders and
live custom-client runs are different kinds of evidence. Do not label an entire
server either fully tested or wholly untested based on one of them.

The native client still has incomplete model/UI coverage, swimming, client-side
same-zone teleporter detection and some cooldown/resource information. Character
creation has UI and scenario paths on all four policies; deletion is unavailable.
The creation form includes the appearance choices supported by each policy,
with a model preview from the user's installation. Classic/Titanium names come
from the world; EQMac uses a local generator. Titanium also offers tutorial
entry. [Gameplay](docs/gameplay.md) and [Assets](docs/assets.md) describe practical
limitations outside creation.

Creation choices were checked against the installed clients' functional rules.
Synthetic tests cover all offered race/class combinations, deity/city choices,
expansion limits, all offered cosmetic values, both packet generations,
refusals, repeated submissions, stale lists and delayed name suggestions.
Offline checks cover all 30 playable race/gender models and the creation UI.
Quarm creation was checked against its own published world handlers and record
layout. These checks add no live-server creation certification.

For live checks, record the client and network commits, server policy, steps,
duration and result. Test initial admission, continued connection, movement,
zone-line transfer, server relocation, death/respawn and reconnect separately.
Keep assets, raw packets, keys, credentials, account/character identifiers and
installed-client text private. Public regression fixtures should be synthetic.

### Consolidated UI validation, 2026-11-02

The client changes through `69157ab`, combined with the networking revision above,
passed native Windows checks against local EQEmu and TAKP. Live coverage includes:

- Character-creation pages, appearance, name paste, duplicate refusal, refreshed
  selection and new-character admission. Tutorial entry was checked on EQEmu.
- Script editing, diagnostics, clipboard and undo/redo, nested files, external
  edit conflicts, help navigation, run controls, held-input cancellation,
  continued execution with the window closed, and real zone handoffs.
- Free-camera controls and saved bindings, window/menu stacking, chat paste,
  actual HTTP(S) browser launches and item-link coexistence.
- Map pan/zoom/opacity, target eligibility, tracking with the map closed,
  reconnect persistence, and real remote-player heading changes. A real EQEmu
  pickup removed its ground-item marker; TAKP exposes no ground-item state.
- Terminal disconnects during held input: both protocols retained a failed run
  with `Session ended` and skipped the remaining actions.

P99 and Quarm each completed an attended two-minute script while connected and
receiving server state. These compatibility checks used existing characters,
without sending chat, moving them or creating characters. The final Quarm
stamina-layout fix still requires a public retest. Consult the PR descriptions
for later results; this is not a certification of all gameplay on either server.

## Dependency integration and CI

Cargo.toml pins the reviewed networking integration, and Cargo.lock selects that
exact commit. Use `cargo metadata --locked --format-version 1` to inspect it. A newer
network README may describe work this client has not integrated. For an intentional
update, review old/new revisions and lockfile changes, run local checks, then
validate affected server scenarios with authorization.

Automatic PR/main CI runs **formatting only**. The full CI workflow runs on demand
and performs Linux Clippy/tests and native Windows tests without proprietary
assets or live logins. Windows CI is not a GPU/desktop-input check. Ignored
installation/render probes need separate opt-in execution. Rust 1.99 is the
minimum supported compiler; CI uses stable rather than a dedicated minimum-version
job. See [Development](docs/development.md) for commands and maintenance checks.
