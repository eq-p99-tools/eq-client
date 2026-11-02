# Controls, maps and chat

[Documentation index](README.md)

## Keyboard

Options → Keyboard is the authority for your current bindings. Click a primary
or alternate cell, then press the new key/chord. Escape cancels; Delete clears.
Assigning an already-used chord moves it to the chosen action. Bindings are
saved per character; `keys.txt` seeds characters without their own file.

Titanium installations use the program's built-in key table for supported
commands. Mac-era installations use this client's default table. These are
starting defaults, before saved assignments:

| Action | Titanium | Client defaults (Mac-era) |
| --- | --- | --- |
| Forward/back | Up/Down or keypad | Up/Down |
| Turn | Left/Right or keypad | Q/E |
| Strafe | Unbound | Left/Right |
| Camera-relative movement | Unbound | W/A/S/D |
| Walk/run toggle | F12 | Insert |
| Autorun | Num Lock | Num Lock |
| Attack | Q | G |
| Consider | C | K |
| Duck | D | C |
| Use nearby door/object | U | F |
| Inventory / spellbook | I / B | I / B |
| Target self / next / previous | F1 / Tab / Shift+Tab | F1 / Tab / Shift+Tab |
| Spell gems / hotbuttons | Alt+1…8 / 1…0 | Alt+1…8 / 1…0 |
| Map / Options | Ctrl+M / Alt+O | Ctrl+M / Alt+O |
| Free camera / up / down | F9 / Page Up / Page Down | F9 / Page Up / Page Down |

A binding does not grant an unavailable server action. Text entry and focus
guards prevent gameplay keys from firing while composing chat, searching the
map, editing a script or changing a binding. Scripts using `press` use these
same assignments; their semantic `move` command is independent of the key map.

## Camera and windows

The **EQ menu** at the bottom right opens and hides windows, including Map,
Options and Scripts. Lit squares mark windows currently shown; each entry also
shows its assigned shortcut. Choose an entry, click outside, or press Escape to
close the menu. Escape closes this menu before another window. Scripts starts
closed and remains available from this menu before login. The EQ button artwork
comes from your installed skin; a text button is used if it is unavailable.
The button is semi-transparent and stays behind movable windows, so a window
dragged over it receives the pointer instead. Its popup opens above the windows.

Right-drag changes the view and the wheel zooms unless a UI surface owns the
wheel. `--camera orthographic` selects the isometric-style projection. The chase
camera pulls in before collidable roofs or walls obscure the character and
returns to the chosen zoom after the obstruction clears. This is camera
placement, not transparent roofs.

**F9** detaches the camera; press it again to return to the character's current
position. Movement keys fly through the loaded scene, turn keys/right-drag change
direction, Page Up or Jump raises the camera, and Page Down lowers it. The view
shows a return-key hint. Detaching stops character input, autorun and queued
movement/jump requests; character gravity and server updates still run. Zone or
session changes restore the attached view. Detaching does not turn camera
coordinates into character movement packets.

Drag a framed window's titlebar to move it; use its minimize/restore button when
present. Passive panels move by dragging their surface. Layouts are constrained
to the viewport and saved per character. A newly opened window uses saved
positions, compatible installed-client positions, or its default layout.
The Scripts window has a Close button instead of minimize/restore. Closing it
preserves the document and any running script; use Stop to end execution.

## Map

**Ctrl+M** opens the map on every server, including P99 and Quarm. It uses the
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
unknown entities are gray. Small gold squares are ground items; hover to see
their model and position. World fixtures such as forges are excluded, and map
item markers do not pick up or target anything. The current TAKP and Quarm adapters
do not expose ground items, so those servers have no ground-item markers.
Your arrow follows the displayed
character and its facing; smaller travel arrows beside you and NPCs disappear
on stops/stale reports. Remote positions remain the last ones reported by the server.

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

Item links stay purple and retain their structured payload. Clicking one requests
item details where item inspection is offered; received definitions are cached
for the session and shown read-only. Inspection does not equip/use an item.
Quarm's current policy does not offer item inspection.

Explicit `http://` and `https://` URLs appear underlined and open the default
system browser on release over the pressed link. Bare domains and other schemes
stay plain text. Browser startup runs outside rendering; a launch failure appears
in System chat. Item links keep their own inspection behavior.
