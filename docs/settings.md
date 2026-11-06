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

Window-layout v2 records retain an optional full chat size alongside position
and minimized state. Existing v1 position-only records remain readable and use
the authored size. A size belongs to its effective skin and authored layout;
another layout or a fixed screen uses its own geometry. Returning to the same
layout can reuse the retained preference until another size replaces it.
Invalid sizes are ignored while valid placement fields remain available.
Size changes use the normal per-character file and atomic save path after the
gesture ends; minimized title height is never a preferred full height. Original
client position imports do not invent a size preference.

Keyboard defaults come from the installed client's supported native commands;
startup without a generation falls back to Titanium. Existing key files remain
overrides, including lines with an empty value. Use Options → Keyboard to change
or clear a saved assignment; a default change does not migrate it automatically.
The client reads Titanium's numeric movement and posture overrides from
`[KeyMaps]` in `defaults.ini` and `eqclient.ini`, with `eqclient.ini` taking
precedence. A key file written by Options keeps independent primary and
alternate slots. See [Keyboard](controls.md#keyboard) for the import scope.

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
| Corpses Lie Along the Ground | `corpses_along_ground` | On | Tilts bodies to the floor slope |
| Long Chase Camera | `long_chase_camera` | On | Extends chase zoom beyond the classic 200-unit limit, up to 20,000 |
| Free Camera | `free_camera` | On | Lets a user-assigned Toggle Free Camera key detach the camera and fly it; off, the key does nothing |
| See the World Clearly | `clear_view` | Off | Lights outdoor night as noon and removes distance fog |
| Walk While Winded | `walk_while_winded` | Off | Allows horizontal movement while stunned; other stun restrictions remain |
| Loot Corpses Automatically | `auto_loot` | Off | Opens newly slain NPC corpses naming the player as killer within 20 units, then uses reply-driven Loot All; leaves NO DROP items for manual looting |
| Floating Combat Text | `floating_combat_text` | Off | Draws received positive damage above known targets, in addition to chat |
| Read the Spellbook Standing | `read_while_standing` | Off | Reads the spellbook without automatic sitting |
| Memorize Without Sitting | `memorize_while_standing` | Off | Memorizes without automatic sitting or the client preparation pause; scribing keeps its normal rules |
| Show Invisible Entities | `show_invisible` | Off | Draws known invisible spawns with parenthesized names; targeting rules remain unchanged |
| Answer List Presence Polls | `answer_list_polls` | Off | Answers supported P99 presence polls after a local delay; does not join a list or answer numeric challenges |

These choices take effect in the current session. Controls are unavailable where
their required session feature is absent. Walk While Winded, Memorize Without
Sitting, Run a Little Faster and Fly depart from the official client's
behaviour, so they are offered only where the server type lists gameplay
departures (a stock EQEmu server); on P99, Quarm and TAKP they are greyed out
and the session keeps the official client's movement and memorization whatever
the saved settings say. Automatic looting requires supported
item results; receipt-only echoes cannot advance it. It attempts each observed
death once, retains corpse identity, and stops on refusal. Turning it off stops
further automatic requests while leaving the ordinary loot window available.

Unsafe choices sit together at the page's end:

| Setting | File key | Default | Effect |
| --- | --- | --- | --- |
| Take Environmental Damage | `take_environmental_damage` | On | Reports supported environmental damage; disabling it withholds reports |
| Run a Little Faster | `run_boost` | Off | Increases verified forward running speed by 10 percent; walk, strafe and backward speed remain unchanged |
| Fly | `fly` | Off | Bounded flight through Jump and Duck bindings for ascent/descent, retaining solid collision; disabling restores gravity |
| Exit at Once | `exit_at_once` | Off | Requests logout without automatic sitting or camp preparation; waits for the server outcome |

Gameplay departures are explicit client policies, taken only where the server
type offers them. They do not establish server acceptance, permission, or
absence of detection. Movement identity, freshness,
known-speed and finite-coordinate checks remain in effect.

The Options window's Particles page contains Show Spell Effects. It starts from
`eqclient.ini` (`ShowSpellEffects`, with `SpellParticleDensity` as a fallback)
and is kept per character. It controls particle drawing, leaving spell sounds,
chat and server outcomes unchanged. See the World Clearly currently changes
lighting and distance fog; this client does not draw weather particles.

Environmental damage is offered on P99, EQEmu and TAKP in the locked policy, with
different supported hazards. The setting's unsafe label concerns withholding
reports the server expects; it does not grant immunity or disable server damage.
Water/lava movement guards remain in place either way.
The renderer currently computes falling damage; it does not synthesize lava or
drowning reports. Turning reports off also suppresses the local falling-damage
line. Server health and combat updates remain authoritative.

The two spellbook choices are independent. Enable both to read and memorize
through the book UI while remaining standing.

Long Chase Camera extends the orbit distance and compensates the far plane
beyond 200 units so distant camera placement still shows the scene around the
character. Disabling it restores the 200-unit limit. Top-down projection has
its own range.

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
