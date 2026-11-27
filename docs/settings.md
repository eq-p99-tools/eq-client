# Settings

[Documentation index](README.md)

Use `--settings-dir <directory>` or `EQ_CLIENT_SETTINGS_DIR` to choose where this
client stores its own files. By default it uses `eq-client` under the per-user
settings directory. The installation's configuration is read as input; these
preferences are written to the separate settings directory.

`NoSafeDrop` in the options file stores the original Titanium ground-drop
preference per character. Its values are 0 to ask, 1 to drop immediately, and
-1 to refuse drops. Missing values use the installation's `[Options]`
preference or default 0. Invalid saved values safely restore the question.
`/fastdrop on|off|never` changes this same saved option.
The bare command and an unknown word leave the setting unchanged and show
usage through line 0x333f **[I]**. Both behaviors are inferred: the Titanium
read establishes the three mode lines 0x333c, 0x333d and 0x333e only.

Settings such as windows, options, hotbar, key bindings and map watches are kept
per world/character. Shared files seed characters that have no saved settings.
For example, `keys.txt` seeds `keys-<world>-<character>.txt`; file-name components
are sanitized. Saved spell sets live in `spell-sets-v1-<world>-<character>.txt`,
where both names are written as hexadecimal bytes so that two names the sanitizer
would merge keep separate files. `login-servers.txt` holds [login presets](login.md).
These files can contain account/character identifiers and should not be committed.

`select-locations-v1/` holds private last-known selection-preview locations,
separated by the exact login protocol, endpoint and account, then world and
character. Names are encoded without punctuation or case collisions. These
records include account identifiers and positions; keep this directory private.
There is no shared-history fallback or import from another settings directory.
Only a connected, living character in a ready matching zone scene updates its
record. Movement writes are coalesced to once a second, with the latest observed
position saved on camp or zoning; an abrupt stop can lose the most recent second.
Malformed, unsupported-version or out-of-bounds records use the curated preview.
Delete this directory to clear the preview history. Old settings without it need
no migration.

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

## Server filters

Options → Filters lists the original Titanium client's filter rows, in its
order: the chat channels, bad words, players' and creatures' spells, bard
songs, critical hits, spell damage, misses and hits, damage shields, damage
over time, pets, focus effects, heals over time, item speech, strikethrough
and stuns (read in the program, recording checklist 6.494b). Each row's
button steps through that row's choices in the original's order (show and
hide, and for some rows mine only or group only), and a label says the
current one. A row the generation's packet has no word for is hidden, so
Mac-era servers list 16. The row and choice words are this client's own.
`/filter` shows the Options window on this page, as the original's does; it
reads no words and sends nothing.

The rows are kept under the original's `eqclient.ini` `[Options]` keys
(`Channel00` to `Channel09`, `BadWord`, `PCSpells`, `NPCSpells`,
`BardSongs`, `CriticalSpells`, `CriticalMelee`, `SpellDamage`,
`HideDamageShield`, `DotDamage`, `PetAttacks`, `PetMisses`, `FocusEffects`,
`PetSpells`, `HealOverTimeSpells`, `ItemSpeech`, `Strikethrough`, `Stun`,
`SwarmPetDeath`), each the index of its choice, with the original's
defaults: every row shown but bad words, whose filter starts on. An index
outside a row's choices gives the row's default. **Per character, this
client's choice:** the original keeps one set per install in
`eqclient.ini`; this client keeps them in `options-<world>-<character>.txt`,
seeded from `options.txt` like the other options. Swarm pets' deaths have no
row in the original's window either; the file keeps the key and the packet
sends it.

Choosing a character at the character list gives the session that
character's saved rows, so its first zone-in carries them on every server
type. On Titanium servers (EQEmu and P99) a change is sent at once, as the
original sends it again on every change; on Mac-era servers it goes with the
next zone-in, as the page's note says. A character entered automatically at
connect starts from the defaults until its saved rows are sent after it
enters.

**What a hidden row does today:** it asks the server not to send those
lines, so it hides them only where the server honours that row (stock EQEmu
sends some lines, such as creatures' shouts, without checking). On Titanium
the original also hides some kinds itself, and so does this client for the
lines it prints: with **Missed me** hidden, another's melee attack on you
that does not land (a miss, block, parry, riposte, dodge, invulnerable or
rune) is not shown, whatever the server sends. Which of those outcomes the
original's test covers is inferred. Its other local tests cover lines this
client does not print yet (stuns, other players' environmental damage,
others' fights, others' spells), and apply when they are added. Mac-era
servers hide nothing locally: that program is unread. The packet's
first word, an ini-only switch, is sent as 0, the original's default.

Scripts read a row with `filter.<kind>` (`show`, `hide`, `mine` or `group`)
and step it with `click("filter", "<kind>")`, by the kind's stable name
(`ooc`, `shouts`, `bard_songs` and the rest).

## Display and audio

Options provides Max FPS, Far Clip Plane and Mouselook Sensitivity controls.
The frame cap starts from the installed `eqclient.ini` `MaxFPS`, or 60 if absent;
`--max-fps` overrides it, and the character's saved Options choice takes priority.
Zero leaves pacing to vsync. `--camera-distance` sets the initial distance;
`--entity-distance` optionally limits model loading in EQ units.

`--ui-skin <name>` selects a skin under the installation's `uifiles`, otherwise
the character's compatible installed skin/settings can be used. Missing or
unsupported controls may use this client's own presentation.

`/loadskin <name>` switches to another skin under `uifiles` in game, as the
official client's command does, and the windows redraw at once. The trailing
`0` or `1` the official client takes is accepted, and window placements stay
either way. The choice is kept per character in this client's settings folder
(`skin.v2-…txt`), never in the installation's `UI_<character>_<world>.ini`.
It applies each time that character enters the world, unless `--ui-skin`
names another skin for that run. A skin loaded in game wins over `--ui-skin`
until the player camps or plays another character or world.
Skins differ in what they show. The Velious skin's inventory holds the bags
alone: its equipment sits in Hot Button window 1, and it shows no statistics or
hit point and mana numbers anywhere. The default skin's inventory shows all of
them.

Sound enablement starts from the installation's `Sound` option. The Sound Volume
slider uses this client's scale. Titanium sound assets are loaded on demand;
Mac-era sound loading is not implemented. If no audio device is available the
client reports that audio is disabled and continues.

Named Titanium cues can play installed WAV and MP3 files. MP3 lookup uses the
file name in the installation root or its `sounds` folder. Cues play once,
with limits on pending requests, concurrent voices, and playback time. World
resets clear pending cues and stop their playback.

Zone music and ambient sound come from each zone's own files in the
installation: `<zone>_sounds.eff` places the sound emitters and music regions,
`<zone>_sndbnk.eff` names their sounds, and `<zone>.xmi` holds the music.
Emitters follow the original program's reading of these files (item 6.503,
read R49-7 first pass and its engine follow-up):

- A record whose day and night settings match, or a type-2 record, is one
  sound that plays day and night. Any other record is a day sound and a night
  sound, each with its own sound, type, delay and volume.
- A radius below 0 reaches 10,000 units; a radius of 0 is heard nowhere.
- The files place their sounds with x and y the other way round from the
  server's coordinates (`/loc`); the client swaps them (an estimate from a
  local look).
- A delay above 0 repeats the sound after the delay plus 500 and up to the
  record's random extra. A positive volume is an attenuation in millibels:
  2,000 is a tenth of full volume, and above 10,000 is silent. Otherwise a
  type-0 sound plays at a fifth of full volume and any other at full.
- The sound bank's `EMIT` list is numbered from 1 and its `LOOP` list from
  162; carriage returns and spaces are taken out of each name.

Still this client's estimate until a later read: that loudness falls in a
line from the centre to the radius, that delays are in milliseconds, that a
delay of 0 loops, that a bank name without a dot ends in `.wav`, that the
numbers between the two lists (32 to 161) name the installation's numbered
sounds, and that a new track starts at once rather than fading in.

Music plays through a synthesizer on a SoundFont (`.sf2`) that you name with
`--soundfont <file>` or `EQ_CLIENT_SOUNDFONT`, a stated departure: the
instruments are your file's, and how the original program plays its music is
unread. An installation may ship `.sf2` files of its own, but this client's
synthesizer refuses them as they stand, so name another, such as a General
MIDI SoundFont. Without one, or with a file that is missing, unreadable,
refused or larger than 512 MB, music is silent and the log says why once.
A music region plays its zone's sequence by number, counted from 0, as the
original program does. Where regions overlap, the smallest one you stand in
plays; outside every region no zone music plays. Night (hours before 5 AM
and after 6 PM) takes a region's night track where it has one. A change of
track fades the old one over 1.5 seconds, and leaving every region fades it
over the region's own time, at most 20 seconds. A region that names an
entry of the installation's mp3 list (a negative number) plays nothing yet.

These options are saved for each character, under Options → General where the
skin has the controls. A character with no choice of their own starts from the
installation's `eqclient.ini` `[Defaults]`, where it has the key:

- **Music** (`music`, on): plays the zone's music. `Music=0` in
  `eqclient.ini` starts it off; its scale is unread, so other values leave it
  on.
- **Music Volume** (`music_volume`, 100): the music's own volume, separate
  from Sound Volume.
- **Environment Sounds** (`environment_sounds`, on): plays the zone's ambient
  emitters at the Sound Volume. As in the original client, turning it off
  also silences the zone's music regions. `EnvSounds` in `eqclient.ini` is read as TRUE
  or FALSE, which is this client's estimate; its reader is unread.
- **Combat Music** (`combat_music`, on): `CombatMusic` in `eqclient.ini`, and
  `/combatmusic` turns it on or off. This client plays no combat music yet.

A `nosound.txt` in the installation folder turns all sound off, as it does
for the original client; this client takes that to silence music too.

Turning **Sound** off silences the emitters along with every other sound; music
follows its own Music option. The emitters use at most eight voices of their
own, nearest first, so they never take a voice from a spell, combat or
interface sound. A zone change, a camp, a logout or a lost connection stops
all of them.

## Inspect Others

**Inspect Others** (`inspect_others`, on) is the original client's
`InspectOthers` in `eqclient.ini` `[Defaults]` (read R49-16): on when its
value starts with t, T or 1, and on when the key is missing. A character
with no choice of their own starts from it; this client then keeps the
choice per character, where the original keeps one per installation.

`/toggleinspect on` and `/toggleinspect off` set it, and any other word or
none gives the original's two usage lines. Each ends with the original's
line saying where it stands. Nothing is sent, on any server.

It decides only whether your own right-click on a player inspects them
(checklist 6.644, on Titanium; see [Controls](controls.md)). It does not
stop others inspecting you: the original answers their inspections
whatever it says, and so does this client.

## Attack On Assist

Titanium installations keep `attack_on_assist` in the character's options file.
When it is on, the server's assist answer that sets the target also starts auto
attack, through the attack key's own refusals and line. `/assist on` and
`/assist off` set it and say its state, so a social or a script sets it as a
player does. It starts off: the program's own default is unread (checklist
6.490), and the installation's `[Defaults]` `AttackOnAssist` seeds it where
present. The Mac-era client has no such option.

## Quality of life

Options → QoL stores these choices per character. Availability can depend on the
session; a setting does not manufacture an unoffered server capability.
The Interface scale slider appears with the ordinary settings. A drag on it
shows the size it chooses, and the windows take that size when you let go.
Right-click the slider to return to 100%, or type `/uiscale 150` (any 5% step
from 50 to 200) or `/uiscale` alone for 100% when the slider is hard to reach.

| Setting | File key | Default | Effect |
| --- | --- | --- | --- |
| Interface scale | `ui_scale` | 100% | Scales the interface from 50% to 200% in 5 percentage point steps |
| Skip Food With Modifiers | `skip_modified_food` | On | Leaves stat-bearing food/drink for manual consumption |
| Draw Windows the Skin Hides | `hidden_windows` | Off | Supplies a default layout for windows the skin sizes to nothing |
| Ask Before Deleting Spells | `ask_before_deleting_spells` | On | Confirms deletion where the server offers it |
| Show Times in Chat | `chat_times` | Off | Shows local arrival times for messages |
| Flash on Tells | `flash_on_tells` | On | Requests attention for a tell while the client is in the background |
| Corpses Lie Along the Ground | `corpses_along_ground` | On | Tilts bodies to the floor slope |
| Long Chase Camera | `long_chase_camera` | On | Extends chase zoom beyond the classic 200-unit limit, up to 20,000 |
| Free Camera | `free_camera` | On | Lets a user-assigned Toggle Free Camera key detach the camera and fly it; off, the key does nothing |
| See the World Clearly | `clear_view` | Off | Lights outdoor night as noon, brightens supported Titanium interiors and hides distance fog, rain and snow |
| Rich Lighting | `rich_lighting` | Off | Experimental: lights every zone per pixel, with flickering lamps and torches, highlights and a soft glow; outdoors the lamps show at night. Departs from the original look |
| Torch Shadows | `torch_shadows` | Off | With Rich Lighting, your own light casts shadows all around you. Six shadow views a frame: the largest part of Rich Lighting's cost at night |
| Better Water | `better_water` | Off | Draws water and lava with flowing motion, sky reflections, sun glints, depth tint, shoreline foam and an underwater haze; the original client's flat water when off |
| Loot Corpses Automatically | `auto_loot` | Off | Opens newly slain NPC corpses naming the player as killer within 20 units, then uses reply-driven Loot All; leaves NO DROP items for manual looting |
| Floating Combat Text | `floating_combat_text` | Off | Draws received positive damage above known targets, in addition to chat |
| Read the Spellbook Standing | `read_while_standing` | Off | Reads the spellbook without automatic sitting |
| Show Invisible Entities | `show_invisible` | Off | Draws known invisible spawns with parenthesized names; targeting rules remain unchanged |
| Answer List Presence Polls | `answer_list_polls` | Off | Answers supported P99 presence polls after a local delay; does not join a list or answer numeric challenges |
| Duel Answer Card | `duel_answer_card` | On | Asks Yes or No when another player challenges you to a duel; Yes accepts as `/duel` does and No sends nothing. The original client has no card and takes only `/duel`; turn it off to play that way |
| Use Picks Nearest | `use_picks_nearest` | Off | Lets the Use key use the nearest door, item on the ground or world container within reach, wherever you face, as this client used to. The original client uses only what lies at the centre of the view: the first character there, else a door (read R49-19) |

These choices take effect in the current session. Controls are unavailable where
their required session feature is absent. Walk While Winded, Move While
Feigned, Stand When Hit (when turned off), Memorize Without Sitting, Run a Little
Faster, Fly and Jesus Mode depart from the official client's behaviour, so they are offered only where the server type lists gameplay
departures; where it does not, they are greyed out and the session keeps the
official client's movement and memorization whatever the saved settings say.
Every checked server type lists them while they are under test (2026-11-06). Automatic looting requires supported
item results; receipt-only echoes cannot advance it. It attempts each observed
death once, retains corpse identity, and stops on refusal. Turning it off stops
further automatic requests while leaving the ordinary loot window available.

The **Dangerous** section sits below the ordinary settings and includes the
**Run a Little Faster** slider. Scroll down in the QoL page if needed.

| Setting | File key | Default | Effect |
| --- | --- | --- | --- |
| Walk While Winded | `walk_while_winded` | Off | Allows horizontal movement while stunned; other stun restrictions remain |
| Move While Feigned | `move_while_feigned` | Off | Feign Death without lying down: the press sends only the feign, and a Feign Death spell landing on you sends nothing, so you stay standing, other players see you standing, and you move normally while the server keeps the feign. Your next stand ends it, as a duck or a sit does, and so does your first step if you pressed it sitting. A jump keeps it: it sends no duck or stand of its own. The original client lies you down |
| Stand When Hit | `stand_when_hit` | On | A hit stands you up from sitting or kneeling to loot, as the original client does; a lying player is never stood. Turning it off is the departure: you stay seated |
| Stay in Melee Range | `melee_positioning` | Off | Off by default; while you request auto-attack, it turns and moves your character without held movement input to face the current valid target and approach estimated melee reach on clear ground. Manual movement, mouse look, scripts and /follow take priority. Stops at obstacles, never chooses a target or enables attack. Uses the existing generation-specific reach estimate, omits the fleeing bonus and adds a conservative height check; the server still decides every swing |
| Memorize Without Sitting | `memorize_while_standing` | Off | Memorizes without automatic sitting or the client preparation pause; scribing keeps its normal rules |
| Take Environmental Damage | `take_environmental_damage` | On | Reports supported falls, drowning and lava heat. Air continues running with reports off |
| Gills for Days | `gills_for_days` | Off | Disables the local air calculation and hides Air Remaining on Titanium sessions that offer gameplay departures |
| Fly | `fly` | Off | Bounded flight through Jump and Duck. Collides with solid geometry and passes through mapped water and lava. Turning it off restores gravity. |
| Jesus Mode | `jesus_mode` | Off | Stand and move on mapped water or lava reached from above, using ordinary ground speed and collision |
| Exit at Once | `exit_at_once` | Off | Requests logout without automatic sitting or camp preparation; waits for the server outcome |
| Zone Anywhere | `zone_anywhere` | Off | Crosses any zone line whatever level the zone asks for. That the official client turns players away below a zone's level is inferred; this client blocks no crossing today, so the setting changes nothing yet |
| Run a Little Faster | `run_boost` | 1.00x | Slider from 1.00x to 1.40x in 0.01x steps. Scales calibrated forward running and the forward component of diagonals |

Stay in Melee Range stops on target death or loss, while a new target choice is
pending, beyond 200 horizontal units, and after a zone/session change. It can
resume for a newly confirmed current target or one that returns within range.
Assisted steps stop before mapped zone-line triggers and loaded carrier
floors; manual crossing and boarding remain available. This guard cannot
identify absent region metadata or unloaded carrier geometry.

For stalled starts, opt in to `RUST_LOG=eq_client_render::motion::melee=trace`.
This trace is off at ordinary log levels. It records the input owner, target,
context, collision or timing gate holding the approach, and the selected speed
and sample interval for a proposal. Compare displacement with the request's
sample interval; the gaps between local worker results can vary with delivery
latency and do not measure the movement budget or server acknowledgment.

The run speed slider saves the multiplier in hundredths, such as
`run_boost = 120` for 1.20x. A saved checkbox value of `true` loads as 1.10x,
and `false` loads as 1.00x. Move the slider to 1.00x to restore ordinary speed.

Movement waits while the session worker applies a change to movement
permissions. While a speed-only change settles, the client uses the lowest
applicable speed.

Gameplay departures are explicit client policies, taken only where the server
type offers them. They do not establish server acceptance, permission, or
absence of detection. Movement identity, freshness,
known-speed and finite-coordinate checks remain in effect.

The Options window's Particles page contains Show Spell Effects. It starts from
`eqclient.ini` (`ShowSpellEffects`, with `SpellParticleDensity` as a fallback)
and is kept per character. It controls particle drawing, leaving spell sounds,
chat and server outcomes unchanged. See the World Clearly controls weather
visibility separately. The client retains the server's report while particles
are hidden, so turning the setting off shows the current weather.

On Titanium, Air Remaining shows while a resolved head probe is submerged and
the client knows the inputs to count air. The next one-second breath check
refills the reserve if the character has surfaced or can breathe underwater.
In thegrey, whose air can't be breathed, Air Remaining counts even with the
head out of water unless the character breathes water. With **Gills for
Days** enabled, the client skips the air calculation. Turning
it off starts with a fresh reserve. Because skipping the counter withholds the
drowning reports every Titanium session takes, the setting needs the session's
gameplay departures like Fly and Jesus Mode. Titanium `eqgame.exe` checks
breath at 0x4bdb3c, heat at 0x41d4ba and the head region at 0x489b0a.
Unsupported geometry and unresolved character inputs withhold the
applicable calculation.

Every Titanium session, EQEmu and Project 1999 alike, takes drowning and lava
heat reports, as the Titanium program sends them on any server it plays on.
Characters on public servers can drown and burn. Actual body contact with lava
drives the heat check independently of the air counter.
**Take Environmental Damage** controls all supported reports, including falls,
and is on by default. Turning it off departs from the original client's
reporting. Server health and combat updates remain authoritative. The amounts,
their timing and the chat lines after each report follow the Titanium
program (reads R49-3 and R49-3b). Project 1999's handling of the liquid
reports and their damage records is not yet checked live. With the setting
off, nothing is reported and no line prints for the player's own reports; a
record the server sends, such as a trap's, still prints its line.

The two spellbook choices are independent. Enable both to read and memorize
through the book UI while remaining standing.

Long Chase Camera extends the orbit distance and compensates the far plane
beyond 200 units so distant camera placement still shows the scene around the
character. Disabling it restores the 200-unit limit. Top-down projection has
its own range.

Mouse look always turns the chase camera around the character's eyes at its
zoom distance, up to 89 degrees above or below level, so mouse look can look
straight down from overhead or straight up from beside the feet. The floor and
walls still pull the camera in. First person also looks straight up or down.
Report commands always send nonblank text through the normal command path;
bare or whitespace-only `/bugreport` and `/feedback` open their report windows.
Neither behavior has a quality-of-life switch.

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
