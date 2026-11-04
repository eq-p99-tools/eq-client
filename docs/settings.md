# Settings

[Documentation index](README.md)

Use `--settings-dir <directory>` or `EQ_CLIENT_SETTINGS_DIR` to choose where this
client stores its own files. By default it uses `eq-client` under the per-user
settings directory. The installation's configuration is read as input; these
preferences are written to the separate settings directory.

Settings such as windows, options, hotbar, key bindings and map watches are kept
per world/character. Shared files seed characters that have no saved settings.
For example, `keys.txt` seeds `keys-<world>-<character>.txt`; file-name components
are sanitized. `login-servers.txt` holds [login presets](login.md). These files can
contain account/character identifiers and should not be committed.

Keyboard defaults come from the installed client's supported native commands;
startup without a generation falls back to Titanium. Existing key files remain
overrides, including lines with an empty value. Use Options → Keyboard to change
or clear a saved assignment; a default change does not migrate it automatically.
The installation's original `[KeyMaps]` overrides are not imported. See
[Keyboard](controls.md#keyboard) for native defaults and local UI extensions.

Map background opacity, plain/skin style and position lock use
`map-view-<world>-<character>.txt`, seeded from `map-view.txt`. Change them with
the [map's controls](controls.md#map); window coordinates remain in the normal
window-layout file.

## Display and audio

Options provides Max FPS, Far Clip Plane and Mouselook Sensitivity controls.
The frame cap starts from the installed `eqclient.ini` `MaxFPS`, or 60 if absent;
`--max-fps` overrides it, and the character's saved Options choice takes priority.
Zero leaves pacing to vsync. `--camera-distance` sets the initial distance;
`--entity-distance` optionally limits model loading in EQ units.

`--ui-skin <name>` selects a skin under the installation's `uifiles`, otherwise
the character's compatible installed skin/settings can be used. Missing or
unsupported controls may use this client's own presentation.

Sound enablement starts from the installation's `Sound` option. The Sound Volume
slider uses this client's scale. Titanium sound assets are loaded on demand;
Mac-era sound loading is not implemented. If no audio device is available the
client reports that audio is disabled and continues.

## Quality of life

Options → QoL stores these choices per character. Availability can depend on the
session; a setting does not manufacture an unoffered server capability.

| Setting | File key | Default | Effect |
| --- | --- | --- | --- |
| Skip Food With Modifiers | `skip_modified_food` | On | Leaves stat-bearing food/drink for manual consumption |
| Draw Windows the Skin Hides | `hidden_windows` | Off | Supplies a default layout for windows the skin sizes to nothing |
| Ask Before Deleting Spells | `ask_before_deleting_spells` | On | Confirms deletion where the server offers it |
| Show Times in Chat | `chat_times` | Off | Shows local arrival times for messages |
| Flash on Tells | `flash_on_tells` | On | Requests attention for a tell while the client is in the background |
| Take Environmental Damage (unsafe) | `take_environmental_damage` | On | Reports supported environmental damage; disabling it withholds reports |
| Corpses Lie Along the Ground | `corpses_along_ground` | On | Tilts bodies to the floor slope |
| Long Chase Camera | `long_chase_camera` | On | Extends chase zoom beyond the classic 200-unit limit, up to 20,000 |
| Free Camera | `free_camera` | On | Lets the Toggle Free Camera key (F9) detach the camera and fly it; off, the key does nothing |

Environmental damage is offered on P99, EQEmu and TAKP in the locked policy, with
different supported hazards. The setting's unsafe label concerns withholding
reports the server expects; it does not grant immunity or disable server damage.
Water/lava movement guards remain in place either way.

Long Chase Camera retains the chase height range and compensates the far plane
beyond 200 units so distant camera placement still shows the scene around the
character. Disabling it restores the classic chase limit; top-down projection
keeps its own range.

Always-on improvements:

| Name | Effect |
| --- | --- |
| Maps on Every Server | Shows local geometry and known entities, including on P99 and Quarm |
| Cap Frames at 60 | Supplies a frame-cap default; explicit settings can override it |
| Grey Out What Is Unavailable | Explains disabled controls |
| Count Down Camping | Displays the camp preparation countdown |
| Say Each Refusal Once | Suppresses repeated identical refusals for three seconds |
| Keep Hotbar Items to Their Item | Prevents activating a replacement item in an old slot |
| Show Buff Time Left | Displays estimated remaining duration |
| Show Weapon Ratio | Displays damage divided by delay |
| Open Windows Apart | Avoids overlap in the initial layout |

The legacy `map_where_off` key is accepted for file compatibility but cannot
turn maps off.

`--auto-eat-anything` changes the launch default for modified food/drink;
the character's saved QoL choice wins. `--hide-own-helm` similarly supplies a
launch default before the character's display choice. These local appearance
choices and any server-supported helm notification are separate concerns.

Definitions, keys and defaults live in
[`eq-client-core::qol`](../crates/eq-client-core/src/qol.rs); key bindings are
described in [Controls](controls.md#keyboard).
