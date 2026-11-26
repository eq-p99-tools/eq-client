# Assets and offline previews

[Documentation index](README.md)

## Installed data

The client reads your own installation at runtime: S3D/WLD zone geometry,
materials, placed objects, character/equipment archives, UI layouts and icons,
string tables, spells and supported audio. It does not bundle these files.
Keep archives, extracted assets, derived caches and screenshots outside Git.
Every archive is found by its role (a zone, its objects, character and
equipment archives) in `eq-client-assets/src/archives.rs`; only S3D archives
are read so far.

Classic model rendering includes animation, face/armor textures, robes, helmets
and held-item attachments where installed data and the model support them.
Helmet dyes follow the surface drawn, so an exposed face keeps its color.
Your own helm follows the Show My Helm option, at once. On a server that
offers that option (stock EQEmu today), every other spawn's helm follows its
own flag, as the original client draws it (read R49-13): players, NPCs and
corpses alike, from the spawn and each change of it. A change for another
spawn shows at its next head wear change, as the original's does (inferred),
not at once. On other servers every helm shows (checklist 6.513).
One-shot animations use the received playback rate when it is positive and
finite, with the authored rate as the fallback.

Placed WLD objects also animate complete texture sequences with authored timing.
Placements sharing a model material use one timer, including reflected copies
and doors supplied by the same zone model. This shared phase is an inferred
presentation choice. Incomplete or untimed sequences keep their initial image.
Named boat actors retain sequence metadata, while their separate renderer still
draws the initial image. Skeleton motion and sprite attachments on static objects
remain incomplete.

For the player and their paperdoll, a submitted Titanium equipment move can
show the item's base held model while the server's appearance is pending.
The estimate uses the item definition and can differ from ornamented wear.
Silence leaves it an estimate. Received wear, inventory corrections and a new
zone admission take precedence.
Other players see the server's appearance updates. EQEmu can relay a player's
wear message to other clients. [EQEmu wear handler](https://github.com/EQEmu/EQEmu/blob/master/zone/client_packet.cpp#L15096-L15112)

Missing models use visible fallbacks; an implemented loader is not a claim of
complete coverage for every race, armor set, animation or custom UI atlas.
Terrain, collision and zone regions share coordinate conversions in
`eq-client-axes`.

Spawned carriers keep every authored solid face from their posed actor,
including faces excluded for the same model as a zone placement. This restores
the collision geometry from before the placed-object no-hit split (inferred,
checklist 6.650). Zone vegetation and door collision keep their existing rules.
Actor assets and their scaled collision meshes are held only for the current
admission; the navigation disk cache excludes spawned obstacles. A failed mesh
build is logged and counted in the gap file once per incarnation, model tag,
and scale, rather than retried every frame.

| Open parity row | What remains unknown |
| --- | --- |
| 6.650: spawned actor collision mode | Which engine collision mode a spawned actor's model takes is unread; the needed read is the spawn-actor counterpart of R49-49d. Keeping the pre-split faces is inferred. Loading a deck mesh does not establish boarding parity; a matched carrier, dock state, approach and input remain to be compared. |

Item icons use the selected skin's `A_DragItem` grid: declared filenames,
frame origins and sizes, cell dimensions, sheet order and numbering direction.
The existing item-icon numbering starts at 500 for the first cell. Rectangles
must fit both declared texture bounds (when supplied) and the decoded image.
Missing, malformed or out-of-range metadata preserves the window's ordinary
placeholder or label; the log labels missing layout metadata as unverified.
Cell registration follows SIDL and still needs visual validation for a custom
skin. This does not establish official executable lookup parity.

Character creation also previews Luclin models where its server policy uses
those appearance options. It reads layered face textures, hair/beard attachments
and the installed executable's color palette locally. See the
[creation render check](development.md#character-creation-render-check) for an
offline probe and repeatable screenshots.

Nearby entity models follow the camera's scene reach; `--entity-distance` can
restrict loading further. There is no fixed 200-entity count cap. Distant entities
retain game state without model/animation work, and the map can still display
their known positions. Positions and animations interpolate for presentation;
rendering other entities never emits movement commands for them.

## Offline modes

```console
cargo run -p eq-client --release --locked -- --eq-dir "/path/to/EverQuest" --offline --zone ecommons
```

| Option | Preview |
| --- | --- |
| `--demo-entities` | Synthetic moving player/creature examples |
| `--demo-inventory` | Synthetic equipment, bags and cursor interactions |
| `--demo-bank` | Personal storage; requires `--demo-inventory` |
| `--demo-spellbook` | Synthetic spellbook with installed names/icons |
| `--demo-character-select` | Character selection without a connection |
| `--demo-trade` | Loot, merchant and trade windows |
| `--demo-weather` | Synthetic light rain, heavy rain, snow and clearing |
| `--terrain-only` | Hide placed objects to inspect terrain/materials |
| `--character-model HUM` | Choose the offline character model code |

Demo flags conflict with `--online`; demos and `--inspect-only` select offline
behavior. A screenshot or CLI script without a login step also starts offline
unless `--online` is explicit. In-game workbench scripts instead run in the
existing session. Use explicit `--offline` for clarity when exploring assets.

`--start-x` and `--start-y` must be paired; optional `--start-z` selects the floor
below that EQ location rather than the highest surface. These are offline initial
placement inputs, not a way to override a live character's server location.
`--debug-overlay` adds zone/position/movement diagnostics. `--window-position X,Y`
and `--window-size WIDTHxHEIGHT` select physical desktop placement and client area.

## Inspection and screenshots

Validate a zone without opening a window:

```console
cargo run -p eq-client --locked -- --eq-dir "/path/to/EverQuest" --zone ecommons --inspect-only
```

Render an offline frame in a hidden window and exit:

```console
cargo run -p eq-client --release --locked -- --eq-dir "/path/to/EverQuest" --demo-inventory --screenshot inventory.png
```

`--screenshot-after <seconds>` delays capture after readiness and requires
`--screenshot`. It differs from `--session-seconds`, which includes admission.
An explicitly online screenshot still logs into the server; keep that separate
from an offline render check. `--script` and `--screenshot` conflict; scripts have
their own screenshot action.

To inspect weather, launch an outdoor scene with `--demo-weather --zone ecommons`.
Use a preview profile with See the World Clearly off. The demo supplies its own
outdoor sky and sends light rain, heavy rain, snow and clear at four-second
intervals through the normal weather reducer. Run the following with
`--script` from a private scenario file to capture each settled phase.

```eqscript
wait_online()
wait(2000)
screenshot("rain-light.png")
wait(4000)
screenshot("rain-heavy.png")
wait(4000)
screenshot("snow.png")
wait(4000)
screenshot("clear.png")
```

Inspect a local character archive:

```console
cargo run -p eq-client-assets --example inspect_characters --locked -- "/path/to/EverQuest/global_chr.s3d" HUM
```

The `propose_select_spots` renderer example reads zone geometry and proposes
character-selection preview positions. Its output needs human review; it does
not publish or alter server placement.

## Navigation and remaining limits

The client builds zone navigation meshes in background work by default and
caches them under the settings directory. Script `walk` uses a ready mesh for
the character's body size. Without one it falls back to grid search and says
why in the log and the script's events: the mesh is still building, the zone
has none, or the character is taller than the largest mesh size. A mesh route
that can't reach the goal also falls back, with a warning. `--no-navmesh` (or
`EQ_CLIENT_NO_NAVMESH`) turns meshes off; startup then warns that every walk
uses grid search. The older `--navmesh` flag is still accepted and changes
nothing. This is route planning, not server permission to move or teleport.

Models, UI layouts and movement rules remain incomplete across client versions.

Standing navigation meshes include the movement headroom policy: more than
twice the scaled stand height plus 0.5 above the floor, in addition to the
existing capsule clearance. One cell height guards against voxel rounding.
The installed generation's model-scale rule supplies that height; a late zone
header replaces the mesh and its cache key when it changes the clearance.
Prebuild and offline select-spot tools use the conservative unreduced settings
until a header is available. Routes do not plan ducking or mounting. A scripted
walk that still meets the room gate stops with a headroom refusal.

Sound loading currently handles Titanium WAV assets, not the Mac-era sound
format. Day/night lighting has provisional presentation tuning. Bounded
swimming on Project 1999 and Titanium EQEmu uses the classic water regions.
The air meter samples the model's native head reference separately. Same-zone
teleport pads detected by the client remain incomplete. See
[gameplay](gameplay.md) for action limitations and
[support](../SUPPORT.md) for server-specific policy.

Rain and snow use procedural particles and the zone's sky marker. The client
waits for a sky report and skips precipitation in zones marked with no sky.
Particle density, drift and fades are provisional. How shelter within a zone
affects weather remains unresolved.
