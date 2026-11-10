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
Cargo.lock: `1116f094a3e74a919927ac7eaddf1ffcb5a33bd1`. Its
[server policy and tests](https://github.com/eq-p99-tools/eq-network/blob/1116f094a3e74a919927ac7eaddf1ffcb5a33bd1/crates/eq-network/src/client/session/servers.rs)
are the source for this table. **Yes means provided by that policy**, not a live
certification, and individual operations still validate their prerequisites.

| Area | P99 | EQEmu | Quarm | TAKP |
| --- | --- | --- | --- | --- |
| Protocol / installation | Titanium | Titanium | EQMac / TAKP | EQMac / TAKP |
| Login, character selection, world state and chat | Yes | Yes | Yes | Yes |
| Character appearance preview at selection | Yes | Yes | Yes | Yes |
| Movement and falling | Yes | Yes | Yes | Yes |
| Drowning and lava heat reports | No | Local-only Titanium sessions | No | No |
| Jumping | Yes | Yes | Yes | Yes |
| Sitting, standing and ducking | Yes | Yes | Yes | Yes |
| Casting and basic spellbook | Yes | Yes | Yes | Yes |
| Beneficial-buff cancellation | Source-backed, public verification pending | Yes | No | No |
| Move / delete book entries | Yes / No | Yes / Yes | No / No | No / No |
| Read the spellbook standing | Local preference, every server | Local preference, every server | Local preference, every server | Local preference, every server |
| Memorize without sitting | Yes (opt-in) | Yes (opt-in) | Yes (opt-in) | Yes (opt-in) |
| Run speed multiplier, flight and movement while stunned | Yes (opt-in) | Yes (opt-in) | Yes (opt-in) | Yes (opt-in) |
| Item/coin moves and personal banking | Yes | Yes | Yes | Yes |
| Cursor-item destruction (Titanium inventory policy) | Yes | Yes | No | No |
| Cursor ground drop (Titanium inventory policy) | No (withheld pending attended validation) | Local-only pending live validation | No | No |
| Merchants | Yes | Yes | Yes | Yes |
| Merchant quantity picker | Yes | Yes | No | No |
| Exchanges/giving | Yes | Yes | No | Yes |
| Supported abilities | Yes | Yes | Yes | Yes |
| Targeting | Yes | Yes | Yes | Yes |
| Combat and considering | Yes | Yes | Yes | Yes |
| Corpse looting | Yes | Yes | Yes (manual) | No |
| Door interaction | Yes | Yes | Yes (ordinary) | Yes (ordinary) |
| Ground-item state and pickup | Yes | Yes | Yes | Yes |
| Camping and zone transfers | Yes | Yes | Yes | Yes |
| Groups | Yes | Yes | No | Yes |
| Ordinary assist, dice and emotes | Source-backed, public verification pending | Yes | No | No |
| AFK, anonymity and roleplay | Source-backed, public verification pending | Yes | No | Anonymity and roleplay |
| Raids | No | Yes | No | No |
| Skill training | Source-backed, public verification pending | Yes | No | No |
| Read inventory books and notes | Yes | Yes | Yes | Yes |
| Tradeskills | Carried combines and ordinary forge candidates. Live forge validation pending. | Yes | No | No |
| Answer list presence polls | Yes (opt-in) | No | No | No |
| Pets and corpse commands | Yes | Yes | No | No |
| Server dialogs and player inspection | Source-backed, public verification pending | Yes | No | No |
| Resurrection / translocate answers | Source-backed, public verification pending | Yes / Yes | No / No | No / Yes |
| Character creation UI and script | Yes | Yes | Yes | Yes |

Quarm additionally offers ground movement, jumps, falls, posture, camping and
zone transfers through the shared EQMac path, plus target selection/clearing.
It also offers casting, basic spellbook actions, the supported EQMac abilities,
and guarded item/coin moves. Normal attack toggles and considering are now offered;
local submission/refusal is not an authoritative combat outcome. The Quarm
policy offers merchants and meals. Exchanges remain unavailable. Valid item moves
send immediately in order, without a pending-move hold or pacing timer;
predictions retain their uncertainty until server evidence resolves it.
Contradictory updates require a complete
snapshot. Coin transfers remain a local ledger until server updates replace it.
These confirmation limits also govern movement readiness.
Posture controls use `Capability::Posture`, independently of `Moving`. The newly
enabled combat, loot and ground-item paths are source-derived and tested with
synthetic fixtures; their live validation is tracked separately. Swimming, boats
and client-reported bleed-out remain Quarm gaps. P99 still does not offer deletion
of spellbook entries.

The merchant quantity picker is available with the Titanium installation. The
Mac-era merchant controls buy one unit or sell the selected stack directly.
The movement and memorization options are offered on all current server
policies for testing and start off. Reading the spellbook standing is a
separate preference. The P99 list reply is off by default and applies to
supported presence polls without joining a list.

Quarm offers individual corpse-item requests; TAKP looting remains unavailable.
An EQMac receipt retires only the corpse-view row and matching manual pending
request. Ordinary inventory/coin updates alone establish holdings, including
delivery after the loot window closes. Automatic EQMac loot-all is unsupported;
Titanium retains its existing automatic path. See
[EQMac loot receipts](docs/eqmac-loot-receipts.md).

Both EQMac policies offer ground-item state and guarded pickup. An object removal
or an unrelated inventory change does not establish delivery. Quarm and TAKP
offer ordinary door use with an empty cursor, within the generation's reach and
with a settled hinge; specialized, static and unknown mechanisms stay visible
without use, and sending a click does not open the door.

On EQEmu and P99, doors move and return by the Titanium program's own rules:
lifts and sinking floors step by their type's travel and speed, slides go
their type's distance, and each door returns after its type's delay counted
from its arrival. A door is a floor or a wall only where the program says, so
a hinged door holds no one up. A rider standing on a rising lift is carried,
even stunned, and the movement guard allows that rise only for the floor-solid
lift the client names, within its reach, by what it rose. Zone-in states 2 and
3, the rider radius, a blocked rider, a changed door field, door reach
measured to the door's origin rather than the clicked point on it (a
shortfall of the read rule), and walking out of a door that moved into the
player stay labelled (checklist 6.423-6.428, 6.436, 6.439); Quarm and TAKP keep their own doors as before.

Current movement requires complete load/STR/buff inputs. Unknown state withdraws
player input without inventing a zero load or a root, and bounded passive falls
remain available. Supported spell values scale by caster level. Equipment, food,
bag reduction, coins and EQMac fatigue contribute where established; unsupported
effects and affecting or unknown equipment histories remain unavailable.
Provably stat-neutral equipment history can remain usable without confirming
placement. The HUD explains movement
waits; clearing one requires current local input acceptance and a positive grant,
not a server acknowledgement. See
[movement limits and the deferred live matrix](docs/movement-input-readiness.md).

P99 spell mechanics use effective definitions from the installed base and the
current server registration/replay. Incomplete or invalid provider state keeps
affected mechanics unavailable while chat remains usable. See [installed spell
providers](docs/spell-providers.md). These source and synthetic checks add no live
compatibility certification.

The pinned networking [combat](https://github.com/eq-p99-tools/eq-network/blob/1116f094a3e74a919927ac7eaddf1ffcb5a33bd1/docs/quarm-combat.md),
[loot](https://github.com/eq-p99-tools/eq-network/blob/1116f094a3e74a919927ac7eaddf1ffcb5a33bd1/docs/eqmac-looting.md) and
[world-object](https://github.com/eq-p99-tools/eq-network/blob/1116f094a3e74a919927ac7eaddf1ffcb5a33bd1/docs/eqmac-world-objects.md)
contracts record their limits and controlled live checklists. The dated
[P99/Quarm gap report](https://github.com/eq-p99-tools/eq-network/blob/1116f094a3e74a919927ac7eaddf1ffcb5a33bd1/docs/public-server-gaps.md)
retains the earlier inspection and its scenario-specific evidence.

### Public-parity checks, 2026-11-02

Against networking `d713d0b4` and the desktop changes based on `9be679a`:
these historical observations predate the current spell/ability/inventory/
movement-input integration and do not validate those additions.

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

The combined spell/ability/inventory/movement-input integration pins networking
`ba81b4216195851a53cbb8b729f3037172edcb31`, the merge of that stack with the
reviewer's fixes. All 1,308 offline client workspace
tests pass (32 existing asset/live probes ignored), with strict workspace/all-target
Clippy and formatting. The networking pin passes 850 tests and warnings-as-errors
API documentation. Regression coverage includes unknown cursor coins versus a
known empty cursor, irrelevant bank balances, prediction uncertainty, scaled
buff values, fatigue, revision recovery and bounded passive falls. These are
offline checks; the dedicated live batch remains pending.

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

The native client still has incomplete model/UI coverage and some
cooldown/resource information. Supported P99 same-zone pads use installed
boundaries, with deployed crossings awaiting an attended check. Bounded swimming
in mapped classic water is available on Project 1999 and Titanium EQEmu.
The Titanium air meter requires resolved local inputs. Live P99 wet movement,
P99 liquid damage reports and EQMac swimming remain open. Character
creation has UI and scenario paths on all four policies; named deletion with an
explicit confirmation is offered on all four, and an absence from a refreshed
list never claims permanent deletion.
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
  pickup removed its ground-item marker; TAKP exposed no ground-item state at
  that pin (EQMac ground objects are decoded since the 2026-11-03 stack).
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
