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
Cargo.lock: `652179282165579ab935ab89cfb91f48e6b7baa5`. Its
[server policy and tests](https://github.com/eq-p99-tools/eq-network/blob/652179282165579ab935ab89cfb91f48e6b7baa5/crates/eq-network/src/client/session/servers.rs)
are the source for this table. **Yes means provided by that policy**, not a live
certification, and individual operations still validate their prerequisites.

| Area | P99 | EQEmu | Quarm | TAKP |
| --- | --- | --- | --- | --- |
| Protocol / installation | Titanium | Titanium | EQMac / TAKP | EQMac / TAKP |
| Login, character selection, world state and chat | Yes | Yes | Yes | Yes |
| Character appearance preview at selection | No | Yes | No | Yes |
| Movement and falling | Yes | Yes | No | Yes |
| Jumping | No | Yes | No | Yes |
| Casting and basic spellbook | Yes | Yes | No | Yes |
| Move / delete book entries | Yes / No | Yes / Yes | No / No | No / No |
| Inventory, trading and giving | Yes | Yes | No | Yes |
| Targeting and combat | Yes | Yes | No | Yes |
| Looting, doors and ground-object features | Yes | Yes | No | No |
| Camping and zone transfers | Yes | Yes | No | Yes |
| Groups | No | Yes | No | Yes |
| Raids, training, reading and tradeskills | No | Yes | No | No |
| Pets and corpse commands | Yes | Yes | No | No |
| Resurrection / translocate answers | No / No | Yes / Yes | No / No | No / Yes |
| Character creation UI and script | Yes | Yes | Yes | Yes |

Quarm admission, incoming character/entity state and chat have been exercised;
that does not grant the richer actions implemented for TAKP. Quarm currently
advertises only `Talking` as an actionable capability. Conversely, P99 is not
stationary-only: movement, falls, inventory, spells and other paths above are
provided. P99 does not yet offer jumping or deletion of spellbook entries.

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
The creation form includes classic face selection, with other cosmetics using
defaults and no creation model preview. [Gameplay](docs/gameplay.md) and
[Assets](docs/assets.md) describe practical limitations.

Creation choices were checked against the installed clients' functional rules.
Synthetic tests cover all offered race/class combinations, deity/city choices,
expansion limits, both packet generations, refusals, repeated submissions and
stale lists. Quarm creation was checked against its own published world handlers
and record layout. These checks add no live-server creation certification.

For live checks, record the client and network commits, server policy, steps,
duration and result. Test initial admission, continued connection, movement,
zone-line transfer, server relocation, death/respawn and reconnect separately.
Keep assets, raw packets, keys, credentials, account/character identifiers and
installed-client text private. Public regression fixtures should be synthetic.

The documentation refresh itself adds no new live evidence. Free camera,
HTTP(S) browser launch and the all-server entity-map changes have local automated
coverage; their PR descriptions distinguish that from live/manual validation.

## Dependency integration and CI

Cargo.toml pins networking's creation prerequisite, and Cargo.lock selects that
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
