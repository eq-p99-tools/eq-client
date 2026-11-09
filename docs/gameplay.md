# Gameplay

[Documentation index](README.md) · [Server support](../SUPPORT.md)

These are implemented client paths, not a claim that every action is available
on every server. Disabled controls explain whether the server policy lacks a
feature or this client lacks the UI. The session validates actions again before
sending them; queueing a request is not proof of a successful game action.

## Server banners

Received Titanium marquee notices appear above the scene and fade using the
supplied times. The client interprets the duration as hold time between fades
and applies the packed opacity. This timing is inferred from the native
overlay API (recording checklist 6.397). The client uses a short arrival-order
queue and a single fade cycle. Queued banners expire after ten seconds. World
resets other than the player's death clear them.
[EQEmu marquee fields](https://raw.githubusercontent.com/EQEmu/EQEmu/master/common/eq_packet_structs.h#L5962-L5974)
[Native overlay caller](https://raw.githubusercontent.com/macroquest/macroquest2/master/MQ2Main/MQ2Utilities.cpp#L4670-L4688)

## Movement, falling and zoning

Movement is available when the admitted session grants it; there is no separate
P99 opt-in switch. Local movement uses collision checks and bounded simulation
steps. The display interpolates accepted movement instead of waiting for each
remote position update. Speed, elapsed time and admission checks constrain the
outgoing requests; corrections replace the predicted position.

A local movement proposal completes when the worker returns a result for
its admission and request stamp. That result describes the worker's
working position. A pending proposal can defer a due air or lava check
until the worker position is usable. The Air Remaining gauge is hidden
after 250 ms without a result, and a late result allows one due check
against the current sample.

Movement waits for usable inventory, coin, attribute and buff state. Unknown
load or effects never become a zero-weight speed grant. Weight-preserving
ordinary cursor moves can keep movement available; unresolved weight/equipment
changes withdraw input until recovery. Withdrawal clears autorun, taps and
queued jumps while supported passive falls continue. The [readiness guide](movement-input-readiness.md)
records supported calculations, conservative limits and deferred live checks.

Use the current [key bindings](controls.md#keyboard). Autorun stops on the relevant
movement/posture input or when free camera takes control. Losing focus or typing
suppresses new movement input; an existing fall continues. The viewer does not
impose a fixed safe-area bounding box.

Falling and jumping are separate capabilities, both offered by all four server
policies. P99 jump notices are capture-backed; Quarm's movement and transfers
reuse the source-checked EQMac path. New public-server scenarios still need live
verification. Physics uses installed-client
rules and zone gravity where available, with an explicit provisional fallback
when those data cannot be read. Project 1999 and Titanium EQEmu sessions
offer bounded swimming in classic water marked by the installed zone.
Wet movement starts when the character's body origin enters that region.
Wade or fall into water, move in any
horizontal direction, and use first-person or mouse-look pitch to swim up or
down with forward or backward input. The surface holds the swimmer in the water,
and a supported bank returns them to ordinary movement. Automated ground
routes and EQMac-era sessions retain the deep-water guard. Ordinary
movement guards against entering lava.

**Jesus Mode** is an opt-in QoL setting where the session offers gameplay
departures. Walk from shore or land on mapped water or lava to use the
surface as ground. Your usual movement speed, solid collision, and falling
rules apply. A placement inside lava uses ordinary movement and recovery.
Turning the setting off restores ordinary movement on the next sample.
Scripted ground route finding uses solid terrain.

The swim rate is a conservative bound based on the session's current
movement grant. Exact native speed, skill scaling and live P99 wet movement
remain to be verified. The Titanium air meter samples the character's
model-dependent head reference separately. Drowning and lava reports are
available on local-only EQEmu. See [settings](settings.md).

A received placement replaces the local movement projection. A worker can
accept a movement sample and hold it for pacing. Server position and health
updates remain separate observations.

Installed WLD boundary regions supply zone-line detection. Numbered boundaries
resolve through the server's destination table; older absolute boundaries carry
their own destination. Crossing queues a request and the server decides whether
to accept it. Arriving inside a boundary does not immediately retrigger it, and
standing in a rejected boundary does not continuously resend requests.

Server-directed transfers and death/bind offers suspend old-zone actions, use
the world/zone handoff, and install a new admission with its own placement and
state. Same-zone server relocations and rejected-zone rewinds update the local
controller without unnecessarily reconnecting. If a fresh Titanium placement
is below a nearby installed floor with neither a lower landing surface nor
mapped liquid below, travel pauses. Turning still works at the received XYZ
when the session grants movement input. The client warns once for the held
location. The local safety hold can persist for the admission while the
installed geometry offers no way down. A different received location gets
its own check. A later collision probe can release the hold when it finds a route.
A server using different zone geometry can cause this conflict. On P99,
supported same-zone pads use installed boundary geometry and the current
zone-point table where required. `report [label]` exposes the local crossing
as `last_local_pad`. On local EQEmu, ordinary movement can enter a server-owned
same-zone pad. A received reposition supplies the absolute landing and resets
movement there. The received relocation report is separate from the local
pad crossing.
Initial login, boundary crossing, teleport, death, and respawn each need
their own validation.

## Bind Wound

On Project 1999, select your own character to use Bind Wound. The current
policy covers self-bandaging, with an eleven-second button wait. If an
attempt expires, the notice distinguishes missing stock evidence from a
missing bandaging result after stock was received.
A missing result holds the next P99 bandage request until the reply arrives
or you re-enter the zone. Unrelated inventory actions can continue.

## Inventory and banking

Inventory shows equipment, carried slots and bags together. A bag and its
contents share a tinted group; the small **+** control opens its color palette.
Colors belong to storage slots and last for the application session. They are
presentation only, not item data sent to the server.

- Left-click an item to pick it up onto the real EQ cursor, then click a slot to
  place, swap or merge it. Escape does not discard the cursor item.
- With an empty cursor, click a stack or its count to choose a quantity.
  Shift-click takes the whole stack; Ctrl-click takes one (also with Shift).
  These are this client's controls; official modifier parity is unverified
  (inferred, recording checklist 6.382).
  The quantity picker supports its slider, number input and confirmation/cancellation.
- **Auto inventory** fills compatible stacks and then empty carried slots. It
  waits between requests and stops on manual intervention or an error.
- Right-click a note or book to request its text where reading is offered;
  the reading window opens when the server replies. Other items show their
  received details. **Alt+right-click** requests a
  click effect where available; a selected target is used, otherwise self.
- Filled bags carry their contents. Equipment eligibility, bag capacity/size,
  quivers, dual-wield requirements and two-handed conflicts are checked.

The personal bank appears only while alive and connected near a visible banker.
It exposes the eight classic bank slots and their bags while carried equipment
remains accessible. Leaving range, death or disconnect hides banking; the worker
also rechecks proximity for bank actions. The client's 20-unit range is a local
guard, not a claim about every server's maximum range.

Coin boxes support purse, cursor, personal-bank and give-window moves where the
session offers them. Click to choose an amount, or Shift-click to pick up all;
click another box with coins on the cursor to place/convert them. Shared banking
and destroying coins have no client action yet.

On local-only Titanium EQEmu, **Drop cursor item** in Inventory asks before dropping one
ordinary, verified droppable item or whole stack. The server chooses the
location. An item moved onto the cursor only by local prediction needs a
received cursor update before it can be dropped. The last received item stays
visible while custody is unresolved. A nearby object appearing does not
identify the item that left the cursor. Use ordinary ground pickup with a
known empty cursor. After submitting a drop, re-enter the world for a fresh
inventory snapshot. A drop requires current item and carried coin information.
The hold pauses conflicting inventory actions. Movement
can continue after the worker accepts a conservative carried-weight report.
Falls use the retained weight bound when available. If a landing needs
weight and the bound becomes unknown, the client shows that in chat.
Choose **No**, press Escape or close Inventory to cancel before submitting.

Where the inventory policy supports it, **Destroy cursor item** asks before
destroying the whole cursor item or stack. **No**, Escape, closing inventory,
or any inventory or admission change cancels that confirmation. Place queued
cursor items and empty containers first.
A submission removes the item only as a local prediction. It is never retried,
and elapsed time does not confirm destruction. Until a received cursor update or
new inventory snapshot, the original item's weight remains in a conservative
load bound and another destruction is unavailable. Re-entering the world obtains
a fresh inventory snapshot when no such update arrives.

Inventory requests carry the admission and expected item state. Sent moves can
be predicted, but server corrections override them. Contradictory or unsupported
updates can mark inventory stale and stop further moves/autoinventory. If the
server cannot restore a coherent snapshot, reconnect instead of retrying an
uncertain move. No application-level retry silently repeats inventory actions.

On Quarm, submitting an item move does not confirm it: prediction labels and
origins persist until covering authoritative updates or a full snapshot. A
contradiction requires a complete snapshot. Quarm item/coin moves and personal
banking are offered independently of merchants, meals and exchanges, which
remain disabled. Coin moves use a local ledger because ordinary transfers have
no acceptance reply; compare authoritative balances after re-admission when
uncertain, rather than resending a transfer.

## Spells, buffs and the hotbar

The spellbook supports scribing from a cursor scroll, memorizing into eight gems,
and forgetting. Moving book entries and deleting spells are separate capabilities:
P99 offers moving entries but not deleting them; EQEmu offers both. TAKP and
Quarm offer casting and basic spellbook operations but not those edits. Quarm's
spell actions are source-checked and tested offline; public validation is pending.
When Titanium uses the installed spell table, a cursor scroll with a known
class or level restriction is checked before sending a scribe request.
Whether the official client makes that check locally is unread (inferred,
recording checklist 6.394). Server rules and received book and inventory
updates govern the result.

Select a spell and destination gem to memorize; click a populated gem or use its
binding to cast. Shift-click forgetting is separate from casting. Casts use the
selected target or self when none is selected. Movement and posture changes can
interrupt a cast. The cast timer estimates progress while the server reports
the result. On Titanium EQEmu, Bards can type `/stopsong` during a tracked
gem cast or while a song is pulsing. An item cast or a cast of unknown source
blocks the request. If the server gives no matching cast update within one
second, the client releases the tracked cast and reports that the stop is
unconfirmed. When no cast is held, the client reports unconfirmed song state
as soon as it submits the request. Song buffs and gem timers follow the
server's updates.

Spellbook operations wait for matching answers one at a time. After 30
seconds without a matching reply, the session reports the edit as unconfirmed
and allows edits whose replies it can tell apart. Re-enter the zone to retry
an edit with an indistinguishable reply. An interruption can leave the
client waiting for the session's result on the same admission. Late updates
still change the affected slots as reported by the server. The skinned
spellbook reports a worker timeout as unconfirmed and keeps a pending receipt
across a temporary link interruption in the same admission. A timed-out scribe
holds inventory actions until a received cursor update accounts for the
scroll or you leave the zone. Deletion confirmation is invalidated when its
selection or admission changes.

Click **Search spells** in the book to filter learned spells by name. The
search ignores case and fills each results page in original book-slot order.
Unknown names are shown as **Spell** followed by the ID and can be found by that
label. Select a result to inspect or memorize it. Clear the search to return
to the previous page and rearrange or delete spells where supported.

Open **Spell sets** next to the book search to manage this character's saved
gem layouts. Enter a name and click **Save current** to store all eight gems.
Use the arrows to choose a set and hover its name to see its slots. Edit the
name field and click **Rename**, or use **Delete** and **Confirm delete** to
remove the saved layout.

**Apply** restores the selected layout one gem change at a time, including
clearing gems saved as empty. The client checks that replacement spells are
learned before starting and follows the usual memorization rules, including
your **Memorize While Standing** setting. Progress counts slots that match the
saved layout. Moving, casting, changing posture, closing the book or clicking
**Cancel apply** stops subsequent requests. The current request may still
finish, and confirmed changes remain in your gems. Death, a transfer and a
temporary link interruption stop apply and keep the panel's selection and
draft until the admission changes. Apply again from your current layout
once the outstanding edit settles.

The 5-by-2 hotbar holds ten actions. Click or use the number bindings to activate
them. Hold a gem, supported item or Actions button to pick up its hotkey, then
place it on a hotbutton. This hotkey cursor is distinct from the EQ item cursor.
Ctrl+number binds the hovered gem/item and Ctrl+Shift+number clears the slot under
the default bindings. Per-character hotbar files retain assignments; compatible
official-client hotbuttons can seed a character's first layout. Item hotbuttons
stay bound to their original item rather than activating a replacement silently.

Spell metadata and icons come from the user's installation. Keep its spell file
matched to the server patch; the client does not receive a general era selector
that chooses among arbitrary spell-file versions. Local spell information is an
input to presentation and validation, not authority over the server.

Titanium spell-bar refreshes drive local recovery/recast displays; admission can
restore remaining per-gem timers. A refreshed gem is not proof that the spell
landed. Missing timing data stays unavailable rather than inventing a countdown.
Linked reuse groups remain incomplete. Quarm saved gem cooldowns are decoded.
Duplicate-cast guards and item-use click guards are not a statement about
server cooldown enforcement.

Buff icons and resource calculations share the core buff tracker: explicit slot
updates/removals take priority over locally inferred unslotted effects. Known
effects can contribute to resource-capacity estimates; unresolved replacements,
unknown modifiers or stale inventory can make those estimates unavailable.
HP/mana/endurance values, maximums and estimates are not interchangeable. A
missing bar percentage can mean a missing reliable denominator. Quarm does not
supply Titanium endurance data.

On Titanium servers offering buff cancellation, right-click a known beneficial
buff in the Effects or Songs window to request removal (the right button is
inferred, recording checklist 6.387). Harmful effects, effects
with unavailable spell data and effects whose server slot is unknown cannot be
cancelled. A buff stays visible until received state removes it. If the selected
slot changes while the button is held, release cancels the selection. The buff
may expire on its own during a request. With no matching fade, the client
reports the request as unconfirmed. Requests are never retried automatically.
P99 uses the native two-record request for a known server slot. A clear
record with only an empty-slot sentinel cannot identify which buff faded.
The P99 exchange awaits an attended check. Titanium EQEmu uses its
single current-record request.

## Ordinary social commands

The Project 1999 and EQEmu sessions offer `/assist`, `/random`, `/emote`,
`/afk`, `/anon` and `/roleplay`. These commands use the capabilities
reported by the current admission.

- Use `/assist` with a selected entity or `/assist Player` with an exact
  player name. Wait for the returned selection before using an action that
  depends on it. On P99 an unresolved request holds the next assist until
  its reply or zone entry. A reply that arrives after the five-second local
  selection window can release the hold.
- `/random` rolls from 0 to 100 by default. `/random high` rolls from 0 to
  that endpoint, and `/random low high` uses both. P99 orders the endpoints
  and accepts values from 0 through 32,000. Chat shows received rolls.
- `/emote words` displays a local line after submission. Plain emote text
  must fit in the P99 wire field after escaping. Received emotes arrive in
  chat independently.
- `/afk`, `/anonymous` (also `/anon`) and `/roleplay` toggle presence
  and listing. On P99 the client shows the requested value and updates the
  received listing on an appearance or admission update. Turn off
  anonymity before enabling roleplay. Custom away messages and automatic
  tell replies remain outside this feature.

The P99 paths follow Titanium source and await attended checks.

## Social hotbuttons

Type `/social set Greet | /pause 10,/say Hello! | /stand` to save a social
with two lines. `/social bind 1 Greet` puts it in slot 1 on the current
hotbar page. Click the named button, press its hotbar key, or type
`/social play Greet`. Use `/social list` to see your names and
`/social show Greet` to see its lines. Set the same name again to edit it,
or use `/social delete Greet`. Editing keeps the same hotbar bindings.
Deleting leaves any existing bindings inactive.

Each character can save up to 120 socials. A social has one to five lines.
Separate them with a pipe surrounded by
spaces when you enter them in chat. A standalone `/pause 10` waits one second. On a combined line such
as `/pause 10,/say Hello!`, the command runs first. The pause accepts
1 to 600 tenths of a second. Every action uses the client's ordinary
slash-command checks. For example, use `/sit`, `/stand` or
`/say Hello`. Select the target before starting a social that needs one.
The runner refuses `/target` and `/assist` because their results arrive
after submission. The runner stops on a local
refusal. A delay measures time after dispatch and carries no server result.

One social runs at a time. Use `/social cancel` to stop its remaining
lines. An admission reset or an edit to the running definition also stops
it. The editor accepts ordinary slash actions and pauses. Nested socials,
camp and exit commands are refused. Social definitions and bindings are
saved per exact world and character in this client's settings files. Older
hotbar files supply their ordinary bindings. Recreate and rebind socials
from legacy files to establish the exact profile that owns them.

The pause order follows the [EverQuest social hotbutton guide](https://www.everquest.com/news/imported-eq-enus-51004).

## Item links in chat

On Titanium, Ctrl+right-click a displayed inventory item to insert a link into
the chat draft. The selected item supplies the link metadata, including while
an item move is pending. Items with incomplete or unsupported link metadata
can still be linked by forwarding a received tag.

Shift-click a received item link retained in chat to add it at the caret in the
current draft. A regular click opens item details. A cached definition for the
same link can open while another inspection is waiting for a reply. Left, Right, Home and
End move the caret. Backspace or Delete removes the whole link when it meets
one. Choose a chat tab or use a channel command such as `/auction` or
`/tell Name` before sending. Item links belong in the message, after the
channel and any recipient. The client keeps the draft if the routed
message would exceed the chat limit, including EQEmu's link expansion.

Forwarding preserves the received link body. A link copied as plain clipboard
text remains plain text. Retained links and drafts can follow a known world
and character through zoning on the same connection and dialect. When that
scope ends, the draft keeps its text and link labels as plain text.

## Targeting and interaction

Click a rendered entity, use Tab/Shift+Tab to cycle the nearby rendered set, or
F1 for self. Escape clears the target after higher-priority UI actions. The target
panel shows identity and known HP. The map's explicit Target button shares these
checks; selecting alone does not attack or cast.

Titanium See Invisible can reveal an ordinary invisible player or NPC when
the worker knows the effect and the spawn's appearance. Unclassified
invisibility values remain hidden. Installed lights illuminate supported
Titanium interiors, and known race and buff effects can change low-light
vision. **See the World Clearly** and **Show Invisible Entities** are local
presentation settings.

On Titanium EQEmu and Project 1999, received server popups show the supplied
button layout and expiration. A supported response submits the selected reply.
An expired or replaced dialog cannot be answered. Select a nearby player and
choose **Inspect player** to see the received equipment names, icons and note.
An unanswered inspection holds the next request to the same player for up
to five seconds (inferred, recording checklist 6.400).
Automatic inspection replies use the known inspect note from the installed
character settings when available. Note import reads profiles up to 256 KiB.

Considering, hailing, auto-attack and class abilities use the target and session
rules. Loot lists received corpse contents; Loot all takes items sequentially.
Merchants support selection and buy/sell requests. Giving to NPCs and trading
with players have separate interaction state; a trade is not complete merely
because its window opened. Ground pickups and doors use reach checks. World
tradeskill containers and combines exist where `Tradeskills` is offered, rather
than being universally unsupported.

Titanium Paladins and Shadow Knights can use Lay on Hands or Harm Touch
from the first Combat ability button in Actions. Bind the button to the
hotbar the same way as a skill, or use `/layonhands` or `/harmtouch`.
Select the target first. Paladins can select themselves for Lay on Hands.
Sending starts a local recovery estimate, which the hotbar labels separately
from received profile recovery. The stock EQEmu estimate uses its 4,300-second
constants. Project 1999 uses a 72-minute local estimate, inferred (recording
checklist 6.399) from classic daily reuse and a P99 player's report of a Harm
Touch wait over 70 minutes; it is not a cited P99 value.
A valid ability-slot recovery from a profile replaces the estimate. The
client also preserves Titanium's ten-second button hold (`eqgame.exe`
0x411f3d). The server decides the cast result and actual reuse.
[Stock EQEmu timer constants](https://github.com/EQEmu/EQEmu/blob/4aceae18b94ffaafc08e2b17bc41cd72c77f795d/common/features.h)
[Classic Lay on Hands discussion](https://groups.google.com/g/alt.games.everquest/c/5NdS-bggNek)
[P99 player timing report](https://www.project1999.com/forums/archive/index.php/t-221620.html)

Titanium Warriors can use Evasive at level 52 and Defensive at level 55
when the server lists the discipline as learned. Both act on yourself and
share one reuse group. Use the Combat page buttons, bind them to the hotbar,
or type `/discipline evasive` or `/discipline defensive`. The hotbar shows
received recovery and active discipline slot changes. A named fade ends
the matching observed discipline. An explicit empty replacement of
Titanium slot 37 also clears it (`eqgame.exe` 0x45f095). The one-second
duplicate guard is local. An unreported timer remains unknown. EQEmu's
positive-reuse activation sends a timer. Post-use timer delivery on Project
1999 awaits wire verification. This client places these actions on its
Combat page. Titanium dispatches the class ability at `eqgame.exe`
0x41f4a0 and the named discipline through 0x46fc1b and 0x4bb39d.
[EQEmu discipline timer](https://github.com/EQEmu/EQEmu/blob/4aceae18b94ffaafc08e2b17bc41cd72c77f795d/zone/effects.cpp#L856-L885)
[Titanium buff slot limits](https://github.com/EQEmu/EQEmu/blob/master/common/patches/titanium_limits.h)
Spell gems also respect received linked spell reuse when the installed spell
definition identifies the group.

For a single Titanium ranged attack, equip a bow and arrows or a throwing
weapon and select a target. Click Ranged Attack in Actions or type `/ranged`.
The Actions control can be bound to the hotbar. The request checks the current
target, loadout, reach and local recovery. Recovery uses the current weapon's
delay and the first confirmed eligible quiver in general inventory for a bow.
Haste and slow are outside this local estimate. Damage and projectile
consumption appear when reported by the server.

Received arrows and thrown items can appear briefly in flight if both actors
are known and the installed files contain the item model named by the server.
The visual follows an estimated straight path, usually for 0.12 to 1.4 seconds.
Native flight angles, arcs and timing remain unverified. This first set
covers item flights. A despawn, replacement or zone transition ends the visual.

With Titanium, Buy and Sell open the existing quantity picker when more than
one unit is available; a single unit goes directly through the same checked
submission. The picker starts at one and is bounded by the received stack,
stock and known purse. The selected name and quote follow its count. Cancel,
closing the merchant or changing its catalog, inventory or admission discards
the selection. Only one merchant action can be pending. An unanswered action
shows an unknown outcome and is never retried automatically; received items
and coin updates remain the session's responsibility.

On Titanium EQEmu and P99, supported resurrection and translocate offers open
a Yes or No dialog.
An answer applies to the displayed offer. If the question changes while a
button is held, release and click again to answer the replacement. The dialog
closes on death, a zone transfer or a lost connection, and an offer left
unanswered past the session's deadline is declined for you. The server's
movement and recovery messages determine what follows a Yes. On P99 a
named translocate completion must match the accepted offer. A wire-identical
prompt already issued during the same zone visit stays withheld. Bind
translocates need an explicit server-directed destination. Attended P99
verification is pending.

On P99, interact with an in-reach guildmaster of your class, select a skill
from the received Training list and press Train once. The listed price
estimates the cost in copper denominations. The skill value follows server
updates. The point count goes blank as soon as the request is sent. A skill
increase leaves the practice outcome unknown. Paid practice leaves the
purse unknown until a new balance arrives. Re-enter the zone before
practicing again or asking the same guildmaster again. Done closes the
trainer. This path covers skills and awaits an attended P99 check.

Grouping, raids, pets, training, reading and consent dialogs have separate
capabilities by server. Pet Info can hold an early buff snapshot while the
spawn table identifies your pet. It keeps an unresolved snapshot for up to one
second from local receipt. It shows buffs once there is one living
owned pet with the matching spawn ID. A despawn, replacement, death or
ownership conflict invalidates the old snapshot for that ID.

Camp uses timed preparation and acknowledgement where offered. The local
`--demo-trade` mode previews loot, merchant and trade UI.
See the [policy matrix](../SUPPORT.md).
