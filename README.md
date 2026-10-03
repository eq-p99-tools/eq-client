# eq-client

`eq-client` is an experimental native, top-down EverQuest client. The first
milestone is an offline viewer for classic S3D/WLD zones. It loads game data
from an EverQuest installation supplied by the user; this repository and its
build artifacts do not contain or redistribute EverQuest assets.

The code is intentionally split at stable boundaries:

- `eq-client-axes` is the one place EQ's world and file axes meet the
  renderer's. EQ's frame is the mirror image of a right-handed one, and every
  position, rotation, heading and triangle conversion is derived from that one
  definition.
- `eq-client-core` owns engine-independent world coordinates and updates.
- `eq-client-assets` turns local EQ archives into owned, renderer-independent
  meshes and textures.
- `eq-client-render` is the reusable Bevy presentation layer.
- `eq-client` owns configuration and the network worker lifecycle.

The stationary P99 preview translates typed events from the
[`eq-network`](https://github.com/eq-p99-tools/eq-network) crates into
`eq-client-core`. Packet parsing, connection state, game state, and rendering
remain separate, so movement, zoning, inventory, combat, and other systems
can be added without coupling the UI to a specific server protocol.

## Offline East Commonlands demo

Install current stable Rust, then point the viewer at your own EQ directory:

```console
cargo run -p eq-client --release -- \
  --eq-dir "/path/to/EverQuest" \
  --zone ecommons
```

On Windows PowerShell:

```powershell
cargo run -p eq-client --release -- `
  --eq-dir "C:\Program Files (x86)\Sony\EverQuest" `
  --zone ecommons
```

Use WASD to move the locally loaded character across the terrain, drag with the
right mouse button to orbit, and use the wheel to zoom. Add `--camera
orthographic` for a locked isometric-style projection.

To validate asset loading without opening a graphics window:

```console
cargo run -p eq-client -- --eq-dir "/path/to/EverQuest" --inspect-only
```

For a repeatable visual smoke test, save a frame after the scene loads:

Screenshot runs render with a hidden window and exit after saving, without taking
desktop input. Add `--demo-spellbook` for a synthetic two-page book, or combine it
with `--demo-inventory` to inspect both windows. These previews cannot be used with
`--online`; local spell names and icons still come from the installed assets.

```console
cargo run -p eq-client --release -- \
  --eq-dir "/path/to/EverQuest" \
  --screenshot offline-demo.png
```

`EQ_CLIENT_DIR` may be used instead of `--eq-dir`. Zone archives, extracted
files, caches, credentials, and packet captures must remain outside the
repository. On Windows, the viewer also detects the standard
`Program Files (x86)\Sony\EverQuest` installation when neither setting is
provided. `--start-x` and `--start-y` select an initial EQ location, and
`--camera-distance` adjusts the initial view distance. `--terrain-only` hides
placed objects when inspecting terrain materials. The client draws at most 60
frames a second, or the `MaxFPS` in the installation's `eqclient.ini`;
`--max-fps` sets another cap over both, and `--max-fps 0` leaves the rate to
vsync, which is the monitor's refresh rate. The Options window (Alt+O) sets
the cap per character with its Max FPS slider, which wins once moved, and its
Far Clip Plane and Mouselook Sensitivity sliders set how far the scene is
drawn and how fast the camera turns; its Keyboard page lists the keys.

## Stationary online preview

Set `EQ_ACCOUNT`, `EQ_PASSWORD`, and `EQ_SERVER` in the process
environment, then run with `--online`. The server's character list appears first;
choose a character and click **Enter World** (or use Up/Down and Enter).
`EQ_CHARACTER` is optional and retains automatic entry for existing launchers.
An empty account displays guidance instead of entering a made-up character.
Use a secret manager or a private launcher;
do not put credentials in command arguments, scripts checked into Git, or screenshots.
The local P99 installation supplies both assets and validation checksums. For Quarm,
set `EQ_PROTOCOL=quarm` and point `--eq-dir` to your TAKP installation.
Use `--demo-character-select` without `--online` for a synthetic UI preview.

Each bag and its contents share a tinted group. Click the small **+** color control
beside its icon to choose a background color. Colors belong to the storage slot and
last for this application session; moving an item never sends a color choice to the server.

```console
cargo run -p eq-client -- --eq-dir "/path/to/EverQuest" --online
```

The server chooses the zone, saved position, heading, race, and gender. The viewer
loads the corresponding local terrain and character model. Characters on the
classic player models, your own and the paperdoll's included, wear their armor:
each body part draws with the leather, chain or plate texture the server
reports for its slot, tinted, and faces follow the face chosen at creation.
Robes, helmets, weapons in hand and later race-specific armor still draw as
the base look. HP, mana, endurance,
experience updates, memorized spell IDs, and communication text feed the HUD.
Unknown values remain blank. Chat has All and channel tabs, unread counts, and
independent scroll positions. It retains 200 messages per channel group, preserving
quiet guild/tell history during busy auction traffic. The exact default colors
come from the mobile client's `src/App.css`; structured item-link data stays in
memory for the item inspection panel. Scroll over chat to
read history without zooming the camera; **Latest** resumes following. Press
**Enter** or click the input line to compose, then press **Enter** or **Send**.
Plain text uses the selected channel; `/say`, `/tell`, `/auction`, `/ooc`,
`/guild`, `/group`, `/shout`, and `/raid` select one explicitly. Closing the window cancels and joins the session
worker. `--session-seconds 150` bounds a test; `--screenshot frame.png
--screenshot-after 120` captures a frame after two minutes of admitted online time.
Allow the usual server logout timeout before another client uses the character.

Online sessions are stationary by default. The opt-in P99 movement path is described
below. `EQ_PROTOCOL` selects `p99` (default) or `quarm`, or a stock server for
local testing: `eqemu` (Titanium) or `takp` (a TAKP server, which speaks
Quarm's protocol).
For Quarm, use the TAKP installation as `--eq-dir`; P99 checksum scanning is skipped.
Both protocols supply server-selected terrain, character state, nearby spawns,
position updates, despawns, HP, mana, and experience. Quarm's compressed profile and
spawn layouts are decoded separately and normalized to the renderer's coordinate
units. The renderer uses the same distance limits and 200-entity cap for both.
Quarm does not provide Titanium endurance; its HUD leaves that value unknown.
Iksar models are loaded from the installation's expansion character archives.
Target packets, item inspection, and death/zone-transfer handling remain P99-only;
this stage adds Quarm presentation, not outbound gameplay support.

## Status and remaining stages

Implemented locally:

- S3D/WLD terrain, repeating and transparent materials, and static trees/buildings.
- Classic character meshes with rigid skeletal animation from the user's install.
- A closer, steep top-down camera; compact 5-by-2 hotbar, eight spell slots,
  character resources, and communication text.
- Offline walking with normalized input, bounded frame steps, solid triangle
  collision, limited step height, and cliff/steep-slope rejection.
- Typed P99 zone entry, own-character position/corrections, and resource updates.
  Optional resource-capacity estimates remain explicitly approximate. Ordinary storage
  moves preserve them; unconfirmed equipment changes, unresolved stat-bearing food/drink,
  and stale inventory invalidate them. Pre-move contents are retained until server
  confirmation so picking up stat food cannot accidentally hide its contribution.
  Instant spell notifications with explicitly zero primary and alternate durations
  in the installed spell data do not create buff icons or invalidate these estimates.
  Missing or ambiguous duration data stays unresolved; explicit server buff-slot
  updates always take precedence over local duration classification.
  Unslotted lasting effects preserve mana/stamina estimates when every potentially
  active buff is known to leave those capacities unchanged (for example HP/AC-only
  effects). Resource-changing or unknown buffs still require slot/replacement
  reconciliation; the client never adds both sides of an unresolved replacement.
  The engine-independent `eq-client-core::buffs::BuffTracker` owns buff snapshots,
  identified removals, explicit slot replacements, and observed unslotted effects.
  Icons and resource calculations consume that shared tracker. A live P99 self
  recast of Courage arrived as an identified slot fade, an empty-slot fade and the
  new action (no slot update), which is replayed as a tracker test; replacement by
  a different, resource-changing buff has not been traced yet.
- Initial/incremental spawn batches, despawns, and interpolated nearby players/NPCs.
  `--entity-distance 200` selects a three-dimensional EQ-unit radius; entities
  already drawn remain until 240 units to avoid boundary flicker. At most 200
  nearest entities are rendered. Distant entities retain state but have no render
  objects or animation work. Model textures/materials are shared, poses update
  at most 30 times per second, and new instances are created gradually.
- `--demo-entities` provides an explicitly offline moving player/wolf/orc scene
  using the same renderer. The player cycles through sitting, ducking, and walking
  in six-second phases. It cannot be combined with `--online`.
- Opt-in P99 WASD input through collision checks, session/time/displacement guards,
  and the movement packet sender. Synthetic tests cover stale input, failed sends,
  stop packets, corrections, death, and transfer state; live walking is unverified.

A native Windows P99 stationary test on September 16, 2026 remained in East
Commonlands for two minutes, received 1,190 application packets, and rendered
live HP plus MOTD communication records. It sent no movement or chat commands.
A subsequent nearby-entity test rendered one other entity at the saved location
and received auction messages. The offline visual test rendered a moving human,
wolf, and orc. Live NPC movement, a 200-entity performance load, combat, and zoning
have not yet been verified.

### P99 movement calibration

The viewer accepts `--online --movement-calibration /path/to/private-calibration.json`.
The JSON object requires three fields:

| Field | Meaning |
| --- | --- |
| `units_per_second` | Measured horizontal speed in EQ coordinate units per second. |
| `velocity_scale` | Multiplier converting coordinate units per second to packet velocity units. |
| `animation` | Moving animation value in the packet's signed 10-bit field; accepted values are 1 through 511. |

An optional `backward` object accepts the same three fields, with a positive speed
and velocity scale but an animation from -512 through -1. These must be measured
separately for backward movement; forward calibration is never substituted.
Existing files without `backward` still work, with backing up disabled.

An optional `walk` object accepts the same three fields with a positive animation
(1 through 511) and a speed no greater than the forward calibration. It must be
measured separately. Insert toggles forward walking when this grant is available;
the on-screen movement label shows Walk or Run. Backing up retains its independent
calibration. Chat entry and unfocused input cannot toggle walking. Files without
`walk` retain the existing forward mode. Mode switches do not grant extra time or
distance; corrections and new admissions reset the toggle.

An optional `strafe` object supplies separately measured speed, velocity scale and
nonzero signed 10-bit animation for pure sideways motion in both directions.
Left/Right Arrow strafe relative to the character while preserving facing; Q/E
still turn. Without that grant, sideways input cannot borrow forward speed.
Opposite sideways keys cancel. Forward/back arrows take priority over sideways
arrows; diagonal strafing is not implemented until its packet behavior is verified.
The walk toggle does not alter this independent sideways calibration.

There are no built-in calibration values. Parsing validates numeric ranges, not
whether the values match P99. Establish the official client's packet fields,
effective walking rate, cadence, and behavior under roots/stuns, buffs, and
encumbrance before a live walk. Spawn base speeds and NPC movement formulas do
not establish the player's current rate; the offline demo speed is not used online.

A supplied calibration enables the initial P99 admission. WASD uses
camera-relative direction and terrain collision, with at most one pending proposal
and a 100 ms displacement budget. After the worker accepts a position, the player
model and camera interpolate toward it over 100 ms, independently of the movement
simulation. This adds visual latency but avoids displaying each sample as a jump;
it never extrapolates beyond the accepted position. Collision and outgoing movement
use the simulation position, not the displayed position. Worker acceptance is not
a server acknowledgment. Server corrections snap immediately and clear interpolation,
as do death, disconnect, and transfer resets. This smoothing has offline test
coverage; its live presentation remains unverified.
Expired input produces a zero-velocity stop, followed by stationary heartbeats.
Up Arrow moves along the character's facing; Down Arrow backs up without turning
the character, if backward calibration was supplied. Q/E can turn during either
motion. Both modes use the same collision and input-expiry checks, with separate
speed limits and wire animation/velocity values. These controls still require live
validation; diagonal strafing and online airborne movement remain incomplete.
Server corrections, death, and transfer offers clear calibration and queued input.
A normal zone handoff can reapply that explicitly supplied calibration with a fresh
destination session ID and timestamp. This requires an active source movement grant
and unchanged advertised race, size, walk speed and run speed. Corrections, death,
bind transfers, denied transfers and changed metadata prevent automatic resumption.
Matching metadata is not proof that buffs, roots, encumbrance or effective speed
are unchanged; those movement effects still need implementation and validation.
The option is rejected for Quarm.
Q/E turn a calibrated P99 character in place. Facing changes follow the shortest
arc, capped at 240 heading units per second, and packets include the signed
turn-rate field. This conversion is inferred from steady stationary turns in the
official-client capture and still needs live verification. Turn expiry sends a
zero-rate stop. Grounded collision now advances to contact and slides along walls;
cliffs and unsupported vertical movement remain blocked.
All sliding contacts within one proposal share the original grounded height
allowance. A slope-and-wall regression verifies that collision cannot accumulate
extra descent between contacts and produce a proposal rejected by the worker.

Protocol headings use horizontal `(sin(h), cos(h))` in server X/Y, consistent with
[EQEmu's position-around-target calculation](https://github.com/EQEmu/Server/blob/master/zone/mob.cpp)
and the captured forward samples. One shared conversion maps that to renderer yaw
for movement, the player and nearby entities, with synthetic cardinal-direction tests.

For an admitted P99 character, click a populated spell gem or press Alt+1 through
Alt+8 to request a cast. The selected entity is the target; with no selection,
the character targets itself for the request. X requests sitting, C requests
ducking (including server-side cast interruption), and V requests standing.
Typing in chat or losing window focus suppresses these controls. The spell panel
can be moved and minimized using its title bar.

The 5-by-2 action bar accepts clicks and unmodified number keys. Defaults are
1–8 for spell gems, 9 to sit, and 0 to stand. Hover a spell gem and press
Ctrl+number to bind that gem to an action slot; Ctrl+Shift+number clears a slot.
Hover an inventory item with a click effect and use Ctrl+number to bind it instead.
Item shortcuts show installed item artwork and use the same activation checks as
Alt+right-click in inventory, including cursor, casting, charges and pending-request
guards. They use the selected target, or self when no target is selected. Worker
feedback appears on the HUD even with inventory closed. Each item shortcut retains
its slot and item ID: moving or replacing it leaves an unavailable binding until
you rebind it. It never searches for another copy or silently uses another item.
This path has offline command-submission coverage and awaits live validation.
Bindings follow the current contents of the gem and are kept only in memory.
The action bar uses the same cast validation and cooldowns as direct gem input.
Bound spells show local-install icons and hover details for the current gem contents,
including cast acknowledgement and cooldown status. Empty gems and cleared bindings
remain visibly empty; rememorizing updates the action without rebinding it.
It has a title bar for dragging and minimizing.

Press B to browse the admitted character's spellbook, eight known spells per
page. Labels come from `spells_us.txt` in the configured installation, with an
ID fallback for missing definitions. Select a spell, then a gem button to request
memorization. Selected spells are highlighted, unused rows are hidden, and queued
requests show their destination gem. Pending book actions suppress repeated
scribe/memorize clicks. New admissions reset the selection and page.
Gem choices display their current spell icons; hovering identifies the current
occupant and whether selecting it would fill or replace a spell. Unavailable
choices are dimmed, and an already-memorized selection does not queue another
request. Chat composition and window focus gate book actions; restoring focus
does not replay a held click.
The worker requests sitting and waits five seconds before submitting
the assignment. This path has succeeded with Minor Healing on P99 Blue; the delay
is a conservative client policy, not a measured minimum for every spell. Movement,
casting, or posture requests cancel the pending action. Server position corrections,
same-zone relocations, death, and zone transfers also cancel local preparation and
report the reason; a denied transfer does not restart it. Server gem notifications
confirm the assignment; the UI does not assign the gem optimistically. Local
worker progress appears in the book as preparing, submitted, cancelled, or rejected;
submitted requests remain distinct from server-confirmed slot updates. Server
scribe notifications update the book, and zone changes clear the previous book.
To scribe, pick up a scroll, open the book, and click **Scribe cursor scroll**.
The scroll's spell ID comes from the server item definition. The worker selects
the requested empty book slot, rejects duplicates, requests sitting, and rechecks
the cursor and book after the same provisional five-second interval. Moving the
scroll or changing inventory during that interval cancels submission. Server
updates alone consume the scroll and add the book entry; eligibility is enforced
by the server. Live P99 Blue testing confirmed scribing and subsequent memorization
of Minor Healing. That test exposed a cursor-reconciliation bug: the library now
recognizes zero-quantity removal and correlates it with the exact confirmed scribe
before treating it as consumption. This reconciliation fix has synthetic regression
coverage and still awaits a fresh live scribe test.
Shift-click a populated spell gem to forget its memorized assignment while retaining
the spell in the book. The worker rejects stale requests and mismatched spell IDs;
the gem stays visible until the server sends its slot update.

Spell and posture requests expire after one second and are tied to the current
zone admission. The worker checks the memorized slot and target before sending.
The spell panel briefly explains empty gems, casts already in progress, reuse
timers, and unavailable request queues. A queued request is not a confirmed cast;
server cast notifications replace local queue feedback, and reconnecting clears it.
P99 worker validation failures also reach the spell panel as typed, admission-scoped
rejections. Rejecting a new request does not interrupt or clear an existing cast.
The server determines spell success, interruption, and mana consumption; sending
a request does not confirm a cast. Server begin-cast messages drive the countdown;
matching casting-related mana notifications clear it. Own-caster interruption
packets clear the timer and briefly display the reason from the installation's
`eqstr_us.txt`; missing strings or strings requiring unavailable format arguments
fall back to "Casting interrupted (server reason N)". Notifications
about nearby casters leave our timer unchanged. Numeric interruption reason IDs
remain available in the network event without guessing localized text.
Timer expiry alone displays
an awaiting-result state. Gem notifications refresh the memorized slots. Successful
Titanium deletion replies remove only the reported book slot; failed replies retain
it. This decoder follows EQEmu's layout and has synthetic coverage, not a P99 capture.
To delete a book entry, select it, click **Delete spell**, then **Confirm** or **Cancel**.
Changing the selected entry or admission invalidates confirmation. Queued requests
leave the book unchanged until the reply; the worker rejects stale or changed slots.
Select a spell and use **Earlier** or **Later** to exchange it with the adjacent
physical book slot, including empty slots. Rows show one-based slot numbers while
retaining the compact view of known spells. Both expected slot contents are validated,
and the display changes only after the server echoes the swap. Reordering cancels
an unsubmitted deletion confirmation and blocks repeated requests while waiting.
The worker also serializes these edits independently of the UI, including submitted
scribe and memorize requests. Only the matching deletion slot, ordered swap pair,
or scribe/memorize slot, spell and mode releases the outstanding edit; unrelated
spell notifications cannot release it. If no matching reply arrives within 30 seconds,
the session ends so a fresh admission can restore authoritative book contents.
The edit is never automatically replayed after an unknown result.
The spellbook stays in its waiting state until the worker reports that matching
reply. Unrelated book/gem notifications still update their slots, but do not clear
the pending action. A matched failed deletion displays the server rejection.
These controls have local
build/protocol-test coverage but still require live validation.

P99 and Quarm appearance updates drive model posture for known entities, including the player.
Sitting, crouching, kneeling, and lying transitions hold their final frame; frozen
posture preserves the current pose. Movement while crouched uses the crouch-walk
clip. These animations never change position or infer death. Classic playable
models inherit missing tracks from their documented donor models, while their own
tracks retain priority. Human sitting and crouching have been rendered offline
from the installed assets; live posture acknowledgements remain unverified.

Mode-3 server spell-bar refreshes start numeric gem cooldowns. Base recovery and
recast durations come from the installed `spells_us.txt`; the server's reuse-time
reduction applies to that spell's recast timer. The shared recovery interval also
blocks casting other gems. Active casts and known cooldowns suppress repeated gem
cast requests, while Shift-click forgetting remains available during cooldown.
The Titanium worker also rejects cast, scribe, memorize, and forget commands while
an own-character cast is confirmed by the server. Movement and posture changes
remain available to interrupt it. A server-confirmed cast cancels pending book
preparation; matching result notifications or an own-character interruption
release the guard, not the estimated cast duration. Successful transport submission
also blocks duplicate requests until acknowledgement and displays a separate
waiting message. After five seconds without acknowledgement the local retry policy
allows a fresh manual attempt; it never resends automatically or claims success.
Confirmed casts do not time out under this policy. The protocol has no request ID
to distinguish a late reply from a retry of the same spell.
No refresh or missing local timing produces no invented timer. Disconnects and
death clear local timers; Titanium admission replaces them with the profile's
remaining per-gem milliseconds, including when local spell data is unavailable.
Countdowns start when the admission event is processed, before zone asset loading;
network/event-queue delay may make this slightly conservative. Quarm profile timers
remain explicitly unavailable. Linked reuse groups are not yet modeled, and a refresh is not
proof that the spell landed on its target. The behavior is based on the capture
and [EQEmu spell-bar handling](https://github.com/EQEmu/Server/blob/master/zone/spells.cpp).

Optional bounds remain available in the library guard; the viewer does not impose
a fixed safe volume. Local WLD boundary regions now trigger P99 zone-line requests.
Server-offered transfers have a separate lifecycle so denied transfers preserve
death state and an approved handoff cannot resume movement in the old zone.
Pending transfers disable gameplay input while retaining the current spellbook and
cooldown deadlines. A rejected transfer restores access to that data; a new zone
admission replaces it, and a terminal disconnect clears it.

Remaining presentation/gameplay work includes equipment and appearance variants,
additional model races and appearance updates, animated world textures and water.
Space jumps, and walking off a ledge falls, with gravity, terminal speed, floor
landing and ceiling collision: offline, and online on stock `EQEmu` sessions.
P99 and Quarm sessions keep grounded movement until official-client jumps and
falls are measured. Each landing exposes local fall distance and impact speed for
diagnostics; it does not calculate damage or send a damage report, so online falls
do not hurt yet. The explicit tuning (gravity 32, terminal speed 40, jump speed 10
in world units and seconds) is not calibrated EQ physics. Falls continue after
movement keys are released or the window loses focus; chat/focus guards suppress
new movement input. Spell slots
show hover names, shortcuts, local base mana cost, cast time, range, reuse timing
and remaining cooldowns, with distinct
empty/available/waiting colors. Spell gems and book rows load icons from the user's
default Titanium UI sheets using `spells_us.txt` field 144 (see
[EQEmu's spell layout](https://github.com/EQEmu/Server/blob/master/common/spdat.h)).
Missing sheets or icon metadata retain text fallbacks. Custom UI atlas layouts
are not supported yet. No spell artwork is bundled. Unknown model
races and corpses currently use visible markers; known playable and selected
classic creature models load from zone/global archives. Invisible entities are
not drawn. Entity movement is visual interpolation only and emits no commands.

Interactive windows have a compact title bar: drag it to move the window and
use **_** / **+** to minimize or restore it. Actions and spells use these title bars
too. Passive character-resource, target and diagnostic panels have no title bar;
drag anywhere on their surface to move them, and they do not minimize.
Window positions and minimized states survive HUD reconstruction during zone
changes within the running application. They are not yet saved across app restarts.
Moved panels are kept within the viewport after resizing or changing UI scale;
oversized panels keep their title bar at the top left so they remain reachable.

The follow camera checks the sightline from the player against collision geometry
and pulls in before a roof or wall can obscure the player. The chosen zoom remains
unchanged and returns when the obstruction clears. This is camera placement, not
roof transparency; scenery without collision geometry is not included in that check.

Inspect a character archive (optionally load a specific model such as `HUM`):

```console
cargo run -p eq-client-assets --example inspect_characters -- "/path/to/EverQuest/global_chr.s3d" HUM
```

Development currently uses sibling path dependencies into
`../network-refactor/eq-network`. Both checkouts contain local, unpublished API
changes and must be kept together until a coordinated library release.

## License

The original code in this repository is available under the [MIT License](LICENSE).
The loader currently uses the MIT-licensed
[`libeq`](https://github.com/cjab/libeq) project at a pinned revision.

Target selection: click a nearby entity, press **Tab** to cycle through nearby rendered entities, or **Shift+Tab** to cycle backward. **F1** selects your own character even when it is absent from the nearby-entity list. **Escape** clears the target. Cycling wraps in stable entity-ID order and excludes entities outside the nearby rendering set. The target panel shows identity and server-provided HP when available. P99 selections use the target packet without moving or attacking; Quarm targeting is not implemented yet.
Worker-side validation failures clear only the matching selection and show
their reason. "Request sent" means transport submission, not server acceptance.
Titanium server-rejection decoding remains unimplemented: EQEmu's opcode table
leaves `OP_TargetReject` unmapped, and its command-target path differs from the
mouse-target packet used here.

Target input is disabled while composing chat or when the game window lacks
focus. Escape used to close the chat editor does not also clear the target.

Item links are preserved in chat and displayed as clickable purple names inline within each message. Selecting one requests the definition from the current P99 zone session and opens a read-only stats panel. Results are cached within that session; reconnects clear them. Inspection never equips or activates the item. Quest say-links are rejected. Quarm item inspection is not implemented.

Mouse selection intersects the current posed model triangles, including child
transforms and scale, and chooses the nearest body under the cursor. A gold ground
ring follows the selected rendered entity (or self), respects scene depth, and
hides for unavailable, invisible, culled or disconnected targets. Its radius is
bounded by the entity's size; missing ground geometry falls back to estimated feet.
The offline entity demo also accepts `--target-nearest-player-once` for a
repeatable target preview without sending packets. Solid terrain,
objects and door collision block clicks behind them. This is camera visibility,
not a claim about the server's spell line-of-sight rules; Tab selection keeps its
existing nearby-entity behavior. Texture transparency is not sampled by picking.

For explicit stationary live checks, `--online --target-nearest-player-once` selects one nearby player, while `--online --inspect-first-chat-item-once` inspects one actual incoming item link. Both use the same typed command path as the UI and are disabled by default.

## Combat, looting, merchants, giving and camping

The target panel lists the keys that apply to the current target:

- **K** considers the target. The reply prints the Titanium standing and level
  text and colors the target name by level; the level phrases are a best-effort
  mapping from the server's color code.
- **H** hails the target with `/say Hail, <name>`.
- **G** toggles melee auto-attack against a creature. The client stops
  attacking when the target changes, dies or goes away.
- **L** opens the targeted corpse: coins received are reported in chat, items are
  listed in the LOOT window, clicking one takes it into the inventory, and
  **Loot all** takes one item at a time, waiting for each acknowledgement. Killed
  creatures become corpses in place, keeping their spawn ID. With an
  installation, the skin's loot window (`LootWnd`) shows the corpse instead: its
  name, and each item in the slot for its corpse slot (the first place is corpse
  slot 22, as Titanium servers number them), scrolling where the skin gives the
  slots a scrollbar. Clicking an item takes it into the inventory, and **Done**
  or its close box ends the loot. The skin has no Loot all, and Link all is
  greyed out.
- **U** opens the targeted merchant: stock with the server's prices, one-click
  purchases, and a sell button for each carried item. The window shows the purse
  as the networking session keeps it: the server's last money update, with loot
  coins and purchases applied kind by kind as servers do. **Escape** closes the
  window opened last first. With an installation, the skin's merchant window
  (`MerchantWnd`) shows the merchant instead: its wares in the skin's list, each
  with its icon, how many are left where the merchant has only so many, and its
  price under each coin. Clicking a ware chooses it, and **Buy** buys one;
  clicking a carried item in the inventory or a bag chooses it instead of
  picking it up, and **Sell** sells it. The chosen item shows with its name and,
  for a ware, its price (what the merchant pays for a carried item is not known
  yet). **Done** or the close box ends the shopping.
- Clicking an NPC with an item on the cursor asks it to take the item, as the
  official client does. Its answer opens the skin's give window (`GiveWnd`)
  with the item in the first of its four slots; more items go in from the
  cursor, **Give** hands them over (a quest NPC answers in chat, and what it
  does not want comes back on the cursor), and **Cancel** or **Escape**
  takes them back. Only an NPC within 20 units is asked.
- Clicking another player with an item or coins on the cursor asks them to
  trade: their client answers on its own and the skin's trade window
  (`TradeWnd`) opens on both sides, with what the cursor held in the first
  slot. Another player's request opens it the same way, unless a trade is
  already open. Their items and coins show on their side (a right click
  inspects an item); the trade goes through once both players click
  **Trade**, and anything put in undoes both clicks (a name lit green has
  clicked). **Cancel** or **Escape** takes everything back. NO DROP items
  stay with the player.
- Coins move as in the official client: clicking a coin box in the purse or
  the bank picks coins up onto the cursor (the quantity picker asks how many;
  Shift takes them all), and clicking a box with coins on the cursor puts
  them down there, the give window's boxes included. Coins dropped on another
  kind's box change kind as servers do (11 gold into the bank's platinum
  leaves 1 gold on the cursor). Clicking an NPC with coins on the cursor
  opens the give window with them. Every coin box shows what the networking
  session says is there; it refuses a move a place cannot cover before it is
  sent, and the reason shows in chat.
- Food and drink: when the server counts the player hungry or thirsty (3000
  of 6000 or less), the client eats or drinks from the inventory on its
  own, as the official client does, and says so when there is nothing left.
  By default it leaves food and drink with modifiers for the player to eat
  or drink by hand: attributes, resists, HP, mana, endurance, AC, HP or mana
  regeneration, haste, or a click, proc, worn or focus effect. When only
  such food or drink is left, it says so. `--auto-eat-anything` eats and
  drinks whatever comes first instead, as the official client does. A
  right click on food or drink eats or drinks it by hand, whatever it is.
- **/camp** sits, waits the 30-second preparation, logs out and returns to
  character selection. Standing, moving, zoning or dying abandons it. **/sit** and
  **/stand** change posture from chat.
- **/consent Name** lets another player drag the player's corpses (also
  `group`, `raid` or `guild`) and **/deny Name** takes it back; the server's
  answer prints in the official client's words for both players.
  **/corpse** pulls the targeted player corpse close when it lies within
  reach, **/corpsedrag** starts dragging it, and **/corpsedrop** stops
  dragging it, or every corpse when none is targeted.
- **/who all** asks the world who is online and prints its answer in the
  official client's words, from the installed `eqstr_us.txt`: class titles
  from level 51, race names, guilds and zones. Words after it narrow the
  list as the official client's do: a class (`wizard` or `wiz`), a race
  (`dark elf` or `def`), a level or two for a range, `gm`, and the start of
  a name, guild or zone. A plain **/who** lists the zone's players from what
  the client knows of them, as the Titanium client does: the same words
  narrow it, ` AFK ` or `* GM * ` comes before a line and ` LFG` after it,
  guilds are named from the world's guild list, and the count names the
  zone's long name.

Day and night follow the time in Norrath the server gives as a zone admits the
player, run on by the client between updates (an hour every three real
minutes). Where a zone has a sky, the picture darkens through dawn and dusk to
night, the sky and fog colors follow, and each zone's fog closes in from its
own distances; below ground the light stays as it is. The night look is
provisional (`PROVISIONAL_DAY_NIGHT` in eq-client-core) until official-client
captures settle it.

Melee and non-melee damage involving the player prints Titanium-style combat
text. Server string-table messages (for example experience, skill-up and range
errors) are formatted from the installed `eqstr_us.txt`. Casting with less
server-reported mana than the installed spell cost is refused locally, like the
official client. These paths have synthetic tests; live verification is pending.
`--demo-trade` previews the loot, merchant and trade windows offline.

## Attended scripts

`--script <file>` runs a bounded key script through the ordinary input paths, for
repeatable live or offline checks. One step per line, `#` starts a comment:
`wait_select`, `select <name>`, `wait_online`, `wait_zone <short name>`,
`press <keys>`, `hold <keys> <ms>`, `wait <ms>`, `camera <heading> <pitch>`,
`camera player <offset> <pitch>`, `trace <ms>`, `click slot|scribe|store|book|memorize|loot|loot_all|loot_done|buy|sell|shop_done|give ...`, `click merchant_row <slot>`, `click buy_chosen` and `click sell_chosen` (the skin's merchant window),
`give` (asks the target to take the cursor item, as clicking it does),
`click coins purse|bank|give platinum|gold|silver|copper`, `click pick less|more|min|max|confirm|cancel|amount` (`amount` is the skin's quantity window's number box, which takes typed keys once clicked),
`click slider clip_plane|max_fps|mouse_sensitivity|quantity <percent>` (an Options window slider, or the quantity window's, pressed that far along),
`click actions` (opens the skin's Actions window), `click tab <n>`, `click ability combat|abilities <n>`, `click attack`,
`click window <key>` (a button that opens and closes the window, or hides and shows it, such as its selector button or the inventory's Skills button, by the window's key such as `skills`), `click close_box <window>` and `click minimize_box <window>` (a skinned window's title-bar boxes), `click scroll_up <window>` and `click scroll_down <window>` (a skinned window's scrollbar arrows),
`right_click` with the same targets (a bag's slot opens its window),
`hover` with the same targets (rests the pointer there, so its tooltip shows and what rides the cursor hangs there, until the next click or hover),
`slash camp|sit|stand`, `report <label>`, `screenshot <file.png>` and `quit`.
Keys combine with `+` (for example `alt+1`). Scripts only run while the client
window is focused (except offline, or on a local `EQEmu` or TAKP server), stop if focus is lost while a key is held, cap each hold, wait
and the whole run, inject clicks only while the real pointer is outside the
window, never send chat, and save screenshots beside the script file.

The P99 lifecycle decodes own-character death, pauses old-zone actions, acknowledges server-directed zoning/bind offers, and returns through world with the zoning flag before admitting the destination session. The new admission supplies the zone, position and character resources; Freeport is not hard-coded. Packet-layout and presentation tests cover these paths, but live death/respawn, calibrated movement, and client-generated boundary transfers remain unverified. Movement bounds are optional; timing, stale-session and speed validation remain required.

Zone-line detection uses the installed WLD's BSP regions. Numbered boundaries resolve
through the current server-provided destination table; older absolute boundaries
retain their embedded destination coordinates. Admission inside a boundary does
not trigger a transfer. Entering a boundary queues one fresh request tied to the
current admission and last accepted position. The server still decides whether to
approve it. Missing routes and same-zone teleports are rejected; standing inside a
rejected boundary does not retry continuously. A current-zone cancellation applies
the server's rewind coordinates instead of reconnecting to world.
Rejected transfers retain the server's signed response code, including unknown
values. The UI records the reason in System chat so a subsequent connection-status
update cannot erase it; known not-ready, expansion and entry-restriction codes
receive readable labels. Rejection events from an old admission are ignored.

Offline checks against two official-client crossings resolved the same destination
zone and coordinates from the installed assets. Synthetic tests cover malformed
tables, unknown references, axis conversion, repeated entries and cancellation.
This is not live validation of the new client. Non-bind server relocation offers
to the current zone and instance now update position without requesting a world
handoff, following EQEmu's `ZonePC` behavior. Bind/respawn offers retain the transfer
path. Relocations and zone-cancel rewinds update the movement controller, stored
player position and fallback heartbeat together; motion calibration is revoked.
Client-initiated same-zone teleporter detection and live validation remain pending.
Door spawn definitions and move
notifications now decode into typed zone-local state, including actions received
during admission. Unknown open types and action bytes remain intact instead of
being guessed as hinged-door animations. Nearby definitions now reuse local zone
object meshes for their base placement (position, heading and scale), within 240
units. Missing models are omitted. Types 0–8 with zero incline now move between
their quarter-turn hinged endpoints on server actions 2/3; initial spawn state
already includes inversion and is not inverted a second time. Transitions use
local visual interpolation, not measured official-client timing. Hinge direction
and pivots still need visual comparison with real doors. Unknown actions preserve
the last known pose. Sliding doors, lifts, continuous rotation and incline remain
unimplemented. Nearby door collision now follows the same rendered transform,
including scale, and is removed with the door or session. Only a changed door's
collision mesh is rebuilt; the static zone mesh is retained. Servers close ordinary
doors on their own timers without telling clients, so an opened door closes
locally after five seconds, `EQEmu`'s default timer; the official client's delay
is not measured yet. This does not yet handle a closing door pushing an
intersecting character or moving-platform carrying.
Press **F** to request ordinary use of the nearest door within a local 20-unit
three-dimensional reach limit. The HUD identifies that door and distinguishes
queued/submitted requests from local rejections. The worker rechecks admission,
request age and its latest accepted position before sending a Titanium click.
Use does not predict an open state, supply a lockpick skill or invent a cursor key.
Chat and unfocused windows suppress the shortcut. This source-based request path
handles server removal of all doors by clearing definitions, models and the use
hint; queued uses predating a definition reload are rejected even if IDs are reused.
The request path works live on a local `EQEmu` server and still requires
official-client capture comparison and a P99 check; mouse selection, lockpicking
and special cursor-item interactions remain unfinished.

Items on the ground (dropped items, ground spawns) and world tradeskill
containers draw within 240 units with the zone's own model or, for items, the
installed item model from `gequip*.s3d`; an item without an installed model
shows the default bag. **Left-click** an item, or press **F** when it is the
nearest thing within reach, to pick it up onto an empty cursor. The session
checks the cursor, unsettled moves and the same 20-unit reach as doors, and a
refusal appears in chat. Containers are not supported yet: they draw but cannot
be used, and one that opens for a click is closed again. Pickups work live on a
local `EQEmu` server; every ground object in the P99 recordings decodes, but
picking up on P99 has not been tried.
Destination admission starts
stationary until a fresh calibration grant; normal handoffs can carry the supplied
calibration under the continuity checks described above.

## Inventory

Press **I** or select **Inventory** to open one combined view of equipment,
carried slots, bags, their contents, and the real EQ cursor. A non-interactive
item icon, name, and stack count follow the pointer while the cursor is occupied,
including when the inventory is closed. With an installation, the skin's cursor
attachment (`CursorAttachment`) draws it instead: the item's icon where the skin
places its picture, with the stack's count, or the skin's picture of the coins
with their count, hanging from the pointer. P99 snapshots include
all of those locations. Stack counts and finite charges are shown separately.
The **Bank** tab appears while connected and alive within 20 EQ units of a visible
NPC whose server class is Banker (40). It shows the eight classic personal-bank
slots, including empty slots and bag contents. Equipment, carried bags, the cursor
and quantity picker remain available in that view, so items can move directly
between carried and bank storage. Leaving range, losing the banker, dying or
disconnecting hides the tab without discarding the cursor or an outstanding move.
The worker rechecks banker proximity before each bank move; UI visibility alone
does not authorize sending. This is a conservative client range, not a verified
P99 server limit. Bank moves are source-based and **not yet validated live**;
shared banking and bank currency are not implemented. Quarm's unverified NPC
class field remains unavailable, so it cannot enable banking.

- **Left-click an item** to pick it up onto EQ's cursor, then click a destination
  to place or swap it. Equipment and bags stay visible together so items
  can move between them without changing views.
- Click a matching stack while holding one to fill it up to the server-reported
  stack limit. Any excess remains on the cursor; unknown limits disable merging.
- Click a stack's **count**, or **Shift-click** its icon, to choose a quantity, then **Pick up** to place that
  amount on the cursor. Min/Max and +/- controls adjust the amount; Cancel closes
  the picker. Inventory changes invalidate the selection. With the skin's
  inventory, the skin's quantity window (`QuantityWnd`) asks instead: drag its
  slider (one at the left end, all at the right), or click its number box and
  type the number (Backspace takes the last digit away), then click **Accept**
  or press Enter. Its close box or Escape takes nothing. How the official box
  takes typing is not checked yet.
- **Auto inventory** fills matching carried stacks, then stores any remainder in
  an empty compatible slot. It waits for each worker result and rechecks the current
  inventory before continuing. Escape, closing inventory, a manual move, or an error
  stops it; any unplaced item remains on the cursor.
- **Right-click** an item to inspect its already received definition. With an
  installation, the skin's item display (`ItemDisplayWindow`) shows it: the
  item's name on its title bar, its picture in the skin's box (an item opened
  from a chat link has none yet), its details in the skin's text box, and its
  close box closes it. Details longer than the box scroll in it, with the
  wheel and with the skin's scrollbar where the skin gives one, and each item
  shows from the top; the skin's other text boxes, such as a book's pages,
  do the same.
- **Escape** does not discard an item held on the cursor.

Slots use the original 40-pixel item icons from `uifiles/default/dragitem*.tga`
in the local install. Each bag sits beside its contents in a five-column grid.
The cursor item has a blue border; full names appear on hover. Missing
icons fall back to initials; unavailable contents show a question mark.

Moves support carried slots, personal-bank slots, bag slots, and compatible equipment slots, including
cursor swaps and stack merges. Filled bags carry their contents with them. Bag capacity, item size,
quivers, class/race/level restrictions and two-handed conflicts are checked.
Deity-restricted equipment uses the Titanium profile deity and item eligibility mask,
including both agnostic profile IDs. Unknown deity data rejects restricted equipment
without blocking ordinary storage or unrestricted items. The UI and worker apply
the same check; the server remains authoritative. Off-hand weapons require a
dual-wield-capable class and positive skill 22 from the Titanium profile; shields
and non-weapons do not. Two-handed weapons exclude the secondary slot. Profile
skills are preserved by index. Titanium skill-up/training notifications update the
worker and UI immediately, including changes during admission; unsupported skill
IDs remain events without expanding the profile. Titanium level gains and losses
update the character display, experience bar and worker equipment eligibility,
including updates received during admission. Quarm skill and level-change decoding
remain outstanding. Trades with other players, the shared bank and the
bank's Change button are unavailable.
Items on the ground can be picked up (see above); dropping and destruction remain
unimplemented.
Inventory instances now retain typed server click-effect metadata separately from
scroll spells: effect ID/category, both level fields, base cast time, reuse delay
and group, and the raw instance recast timestamp. Unknown categories remain
explicit, and timestamps are not converted into assumed local cooldowns. This is
the data prerequisite for item activation. A validated library encoder now builds
Titanium's item-cast body from the current inventory revision and effect, checking
carried/equipped slot restrictions, required level and charges without consuming
anything locally. P99 `UseItem` commands now pass through the admitted session's
target/age checks and shared cast exclusion, cancel pending memorization, and
report cast pending only after transport succeeds. Generic encoding rejects item
use outside that controller, including Quarm. **Alt+right-click** an inventory
square to queue its click effect; ordinary right-click still inspects. Hover text
identifies the effect ID and required level. The selected target is used, or self
when no target is selected. Chat composition, window focus, pending casts/moves,
and a held cursor item prevent activation. An item-use request remains pending until
its matching worker reply or an admission reset; elapsed time alone cannot replace it.
A separate one-second local duplicate-click guard is not an item cooldown; server
cast and inventory updates remain authoritative.
The UI distinguishes queued requests, worker rejection reasons, and successful
packet submission, without claiming the effect landed. Admission and unique
request IDs prevent delayed replies from overwriting a newer request or a new
character's state. Live item-use
validation remains unfinished; no item-use packets have been sent in testing.

Each request is bound to the current admission and inventory revision. The
worker validates it again before sending one reliable Titanium move packet.
Only after transport submission does the UI predict the result. **Move sent is
not server confirmation**: ordinary EQEmu moves have no separate success reply.
Incoming item corrections and removals override local predictions; there is no
automatic application-level retry. Death, zoning and disconnects cancel local action state.
Predicted source, destination, and container slots are tracked individually. A
contradictory update pauses moves and autoinventory until all outstanding slots
receive authoritative updates, or a complete snapshot replaces them. Matching
updates retire the corresponding predictions without blocking normal moves.
If the server does not refresh every outstanding slot after a contradiction,
reconnect to obtain a complete inventory rather than retrying uncertain moves.

Inventory received before admission is replayed after the new session is
announced. New admissions and terminal disconnects clear the old inventory.
Malformed or unsupported updates mark contents stale and disable moves until
a full snapshot arrives. Known stack/charge consumption and own-inventory
resynchronization packets are applied; unrecognized move/swap updates still
invalidate the view.

Add `--demo-bank` to `--demo-inventory` to preview personal bank storage offline.
It cannot be combined with `--online` and never grants access in a live admission.
Use `--demo-inventory` to try the same placement checks with synthetic items,
without a network connection. It cannot be combined with `--online`.
All test fixtures are synthetic. P99/Titanium inventory actions are implemented
but **not yet validated live**; Quarm inventory/actions remain unimplemented.
