# Controls, maps and chat

[Documentation index](README.md)

## Keyboard

Options → Keyboard is the authority for your current bindings. Click a primary
or alternate cell, then press the new key/chord. Escape cancels; Delete clears.
Assigning an already-used chord moves it to the chosen action. Bindings are
saved per character; `keys.txt` seeds characters without their own file.

Supported commands start from the installed generation's built-in key table.
Offline startup without a generation uses the same Titanium table as a fallback.
Saved assignments in this client's shared or character key file take precedence,
including an explicitly cleared action. Updating defaults does not reset those
files. Titanium's numeric `[KeyMaps]` movement and posture assignments apply
over the built-in table. A legacy entry takes priority over a numeric entry
for the same slot. Legacy clears apply, and other legacy entries and modified
bindings for held movement leave the slot empty with a diagnostic. Other
actions and generations use their built-in table. These are the built-in
defaults:

| Action | Titanium / offline fallback | Mac-era |
| --- | --- | --- |
| Forward/back | Up/Down or keypad 8/2 | Up/Down or keypad 8/2 |
| Turn | Left/Right or keypad 4/6 | Left/Right or keypad 4/6 |
| Strafe | Unbound | Unbound |
| Camera-relative movement | Unbound | Unbound |
| Walk/run toggle | Ctrl+R | Ctrl+R |
| Autorun | Num Lock | Num Lock |
| Attack | Q | A |
| Consider | C | C |
| Duck | D | D |
| Sit/stand toggle | Ctrl+S | Ctrl+S |
| Use nearby door/object | U | U |
| Inventory / spellbook | I / Ctrl+B | I / Ctrl+B |
| Target self / next | F1 / Tab | F1 / Tab |
| Spell gems / hotbuttons | Alt+1…8 / 1…0 | Alt+1…8 / 1…0 |
| Map / Options | Backspace or M / Alt+O | Backspace or M (map fallback) / Alt+O |
| Detached free camera / up / down | Unbound | Unbound |
| Keyboard hotbar bind / clear | Unbound | Unbound |

Direct loot, trade, separate sit/stand and previous-target shortcuts start
unbound. Use the normal pointer interaction or assign a shortcut in Options.
Unverified camera-relative movement defaults also stay unbound; W/A/S/D and
Q/E are not supplied as substitute gameplay keys. F12 is the original client's
mouse-look key, not its walk/run key.

EqMac's added map uses Titanium's Backspace/M defaults as a fallback, after
checking all read native assignments for conflicts. Unsupported native commands
still reserve their chords: Ctrl+M, F9, Page Up/Down and Ctrl+1…8 cannot become
defaults for unrelated extensions. Detached-camera controls and keyboard hotbar
editing remain assignable in Options, with no starting keys. An explicit saved
assignment can reuse a native chord.

A binding does not grant an unavailable server action. Text entry and focus
guards prevent gameplay keys from firing while composing chat, searching the
map, editing a script or changing a binding. Scripts using `press` use these
same assignments; their semantic `move` command is independent of the key map.

Titanium sessions can combine forward or backward movement with a sidestep
while grounded and holding a steady body heading. With mouse-look active,
the turn keys supply the sidestep. Each component has its own speed budget.
Collision that exceeds either component's budget and movement at jump or
fall transitions use the ordinary single-mode path.

Right-clicking a visible character uses its supported interaction, such as
looting a corpse or opening an in-reach merchant, bank or trainer. Other
characters, and NPCs beyond the generation's known interaction reach, are
considered through the normal session command. This keeps the current target.
The server still checks each requested action; an unread interaction radius
is left to its existing checks.

Double-click a visible door with the left button to use it within reach.
Two clicks must land on the same door. The Use key still finds a door or
object near the character. The double-click itself, its 500 ms window and
its 4-pixel slop are inferred (recording checklist 6.385).

## Camera and windows

The **EQ menu** at the bottom right opens and hides windows, including Map,
Options and Scripts. Lit squares mark windows currently shown; each entry also
shows its assigned shortcut. Choose an entry, click outside, or press Escape to
close the menu. Escape closes this menu before another window. Scripts starts
closed and remains available from this menu before login. The EQ button artwork
comes from your installed skin; a text button is used if it is unavailable.
The button is semi-transparent and stays behind movable windows, so a window
dragged over it receives the pointer instead. Its popup opens above the windows.

Right-drag changes the view without changing the character's heading. The
wheel zooms along the current sightline unless a UI surface owns the wheel.
Zooming close enters first person and hides the local body and gear. Zooming out
returns to the chase view. Death, zone/session changes and character previews
restore third person. The chase camera can look almost straight up or down
(89°; the limit and the first-person snap and return distances are inferred,
recording checklist 6.386). It pulls in when terrain, roofs or walls obstruct the view and returns to the
chosen zoom as the way clears. `--camera orthographic` selects the
isometric-style projection with its own zoom range.

Assign **Toggle Free Camera** in Options to detach the camera; press the assigned
key again to return to the character's current position. Movement keys fly
through the loaded scene, turn keys/right-drag change direction, and Jump raises
the camera. Free Camera Up/Down accept separate user assignments. The view
shows a return-key hint. Detaching stops character input, autorun and queued
movement/jump requests; character gravity and server updates still run. Zone or
session changes restore the attached view. Detaching does not turn camera
coordinates into character movement packets.

Drag a framed window's titlebar to move it; use its minimize/restore button when
present. Passive panels move by dragging their surface. Layouts are constrained
to the viewport and saved per character. A newly opened window uses saved
positions, compatible installed-client positions, or its default layout.
An installed chat screen that explicitly enables `Style_Sizable` also resizes
from its edges and corners. Its frame, nested chat content and scrollbar follow
the size while keeping the existing controls. Buttons and text boxes keep their
presses; a plain interior drag still moves the window. Minimized or locked
frames do not resize. Inventory and spellbook retain their fixed authored sizes.
The chat minimum of 160 by 100 logical pixels is this client's usability policy;
native numeric sizing limits have not been established. No numeric maximum is
imposed. Oversized frames retain the existing viewport reachability behavior.
The Scripts window has a Close button instead of minimize/restore. Closing it
preserves the document and any running script; use Stop to end execution.

## Spellbook search

Click **Search spells** to type, or paste with **Ctrl+V**. **Enter** and
**Escape** leave the results on screen and return keyboard control.
**Delete** while typing or **Clear** restores the normal book. Use the
book's page controls to browse more results.

## Map

**Backspace** or **M** opens the map on every server, including P99 and Quarm,
unless changed in saved bindings. EqMac uses these as a Titanium-aligned fallback.
It uses the
installed map window when usable and a movable, minimizable fallback otherwise.
Local map files add lines and labels; entity positions work without them.
The initial view covers a 1,200-unit neighborhood around your character, so a
distant invisible server helper cannot squeeze the playable area into a corner.
**Nearby** recenters that view. **Fit all** deliberately includes every reported
position and all local map geometry, including outliers.

The map shows all server-reported entities with known positions, independently
of 3D render distance. Purple arrows are players and point in their facing
direction, including while stopped. NPCs are circles with level-relative con
colors, with received consider results preferred over estimates. Corpses and
unknown entities are gray. Small gold squares show ground items within 240
units of your reported position; hover to see
their model and position. World fixtures such as forges are excluded, and map
item markers do not pick up or target anything. The current TAKP and Quarm adapters
do not expose ground items, so those servers have no ground-item markers.
Your larger arrow follows the displayed character and its facing and appears
above other markers and map labels. A short black line with a colored tip shows
the direction you or an NPC are moving. It disappears when movement stops or
the report goes stale. Remote positions remain the last ones reported by the
server.

Hover for details or click to pin an inspector; repeated clicks cycle overlapping
markers. Search matches part of a displayed name. **Track** saves an exact name;
**Track name** saves the search text even when no such entity is present. Watches
are per character and world. A newly reported match adds a System message, map
banner and background attention request even with the map closed. Initial zone
list matches also count as spotted; they cannot be distinguished from new spawns.

Hover, pin, search and track are local. Only **Target** asks to change the gameplay
target, through the same visibility, range and capability checks as scene
targeting. It does not attack, cast or move. **Unpin** closes the inspector without
clearing the gameplay target. Drag the map to pan; the wheel zooms around the
pointer without changing the game camera. Toolbar pan/zoom buttons also work.
Dragging a marker pans rather than selecting it.

**Skin / Plain** switches to a dark overlay without the parchment artwork, with
lighter lines and labels for contrast over the world.
**BG − / +** adjusts background opacity, including a fully transparent background;
map lines, text and markers remain readable. **Lock** prevents moving the window
by its frame; it does not disable map panning, zooming or controls. Use **Unlock**
to reposition it. Appearance and lock settings persist per character and world,
alongside the existing saved window placement. Search supports **Ctrl+V** paste;
**Delete** clears it and **Enter** finishes editing.
The map cannot reveal entities the server has not sent. An old false
`map_where_off` setting no longer disables this local feature.

## Chat and links

Chat has All/channel tabs, unread counts and independent scrolling, retaining up
to 200 messages per channel group. Scroll within chat to read history; **Latest**
resumes following. Installed string tables format game messages, and installed
color settings apply where their message types are known.

Enter or the input line starts composition. Enter/Send submits; plain text uses
the selected channel. `/say`, `/tell`, `/auction`, `/ooc`, `/guild`, `/group`,
`/shout` and `/raid` select channels explicitly, subject to server support.
Closing composition does not send its unfinished text.

`/ex`, `/exi`, and `/exit` disconnect immediately and close the client.
`/q`, `/qu`, `/qui`, and `/quit` disconnect immediately and return to the
login screen (or close an offline preview). These commands take no arguments
and skip the camp and logout sequence. That the official client's aliases
leave at once, and what they send first, is inferred (recording checklist
6.384). Use `/camp` to camp to character
select or `/camp desktop` to camp and then close the client.

Item links stay purple and retain their structured payload. Clicking one requests
item details where item inspection is offered; received definitions are cached
for the session and shown read-only. Inspection does not equip/use an item.
Quarm's current policy does not offer item inspection.

Explicit `http://` and `https://` URLs appear underlined and open the default
system browser on release over the pressed link. Bare domains and other schemes
stay plain text. Browser startup runs outside rendering; a launch failure appears
in System chat. Item links keep their own inspection behavior.
