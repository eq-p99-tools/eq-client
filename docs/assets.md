# Assets and offline previews

[Documentation index](README.md)

## Installed data

The client reads your own installation at runtime: S3D/WLD zone geometry,
materials, placed objects, character/equipment archives, UI layouts and icons,
string tables, spells and supported audio. It does not bundle these files.
Keep archives, extracted assets, derived caches and screenshots outside Git.

Classic model rendering includes animation, face/armor textures, robes, helmets
and held-item attachments where installed data and the model support them.
Helmet dyes follow the surface drawn, so an exposed face keeps its color.
One-shot animations use the received playback rate when it is positive and
finite, with the authored rate as the fallback.
Missing models use visible fallbacks; an implemented loader is not a claim of
complete coverage for every race, armor set, animation or custom UI atlas.
Terrain, collision and zone regions share coordinate conversions in
`eq-client-axes`.

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

```text
wait_online
wait 2000
screenshot rain-light.png
wait 4000
screenshot rain-heavy.png
wait 4000
screenshot snow.png
wait 4000
screenshot clear.png
```

Inspect a local character archive:

```console
cargo run -p eq-client-assets --example inspect_characters --locked -- "/path/to/EverQuest/global_chr.s3d" HUM
```

The `propose_select_spots` renderer example reads zone geometry and proposes
character-selection preview positions. Its output needs human review; it does
not publish or alter server placement.

## Navigation and remaining limits

`--navmesh` (or `EQ_CLIENT_NAVMESH`) builds zone navigation meshes in background
work and caches them under the settings directory. Script `walk` uses a ready
mesh for the character's body size; otherwise it uses the existing grid search.
This is route planning, not server permission to move or teleport.

Models, UI layouts and movement rules remain incomplete across client versions.
Sound loading currently handles Titanium WAV assets, not the Mac-era sound
format. Day/night lighting has provisional presentation tuning. Swimming uses
mapped water on Titanium EQEmu; it does not establish breath or damage. Same-zone
teleport pads detected by the client remain incomplete. See
[gameplay](gameplay.md) for action limitations and
[support](../SUPPORT.md) for server-specific policy.

Rain and snow use procedural particles and the zone's sky marker. The client
waits for a sky report and skips precipitation in zones marked with no sky.
Particle density, drift and fades are provisional. How shelter within a zone
affects weather remains unresolved.
