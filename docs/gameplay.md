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

A standing Titanium walk climbs onto an edge it runs into up to 1.25 above
the body's origin, 5 units for a human of size 6, as the official client was
seen to climb a pier rail and its posts. A ducking or wading walk climbs only
what the floor sweep reaches, and so do feet that land, fall or are placed
under a floor. The tallest edge the official client climbs is
unmeasured, so a taller one stops here even where it might not there. The
body's width is unchanged (recording checklist 6.482).

A standing Titanium jump or fall passes over an edge whose top the floor
sweep reaches, 3.375 above the feet for a human of size 6, and lands on
it with no vertical speed left, where the move ends on that top. A
rail thinner than a stride still stops the jump, and an edge never adds
height to it. A running jump from the Erud's Crossing
planks clears the docked ship's side and lands on its deck, and a
standing jump mounts a ledge up to its own rise plus that reach, about
8.3 for a human. The program sweeps the air with the same raised body as
the walk and has no test below it (read R42). Using the floor sweep's
reach rather than the origin is the stricter reading (recording
checklist 6.484). Ducking, wet feet and the Mac-era generation keep the
old behaviour, where an edge at the body stops it.

A local movement proposal completes when the worker returns a result for
its admission and request stamp. That result describes the worker's
working position. A pending proposal can defer a due air or lava check
until the worker position is usable. The Air Remaining gauge is hidden
after 250 ms without a result, and a late result allows one due check
against the current sample.

Local EQEmu breathing uses received AA selections and definitions alongside
parsed equipment and received buff effects. Every selected definition must be
known before its signed water-breathing contribution can participate. A known
positive total resolves breathing independently of reserve capacity. Missing
definitions, unconfirmed buff replacements and unsupported effect inputs remain
unavailable. AA breath-level percentages are supported; AA changes to effective
stamina, stat caps or invulnerability still withhold unprotected reserve/damage
calculation. Modelled landing contributions require a known non-Bard caster and
an unambiguous native refresh decision; their duration remains modelled.
Other server policies retain their existing calculation limits.

Movement initially waits for usable inventory, coin, attribute and buff inputs.
Once a grant is in force, ordinary belongings changes, buff updates and spell
landings ask for fresh inputs while the existing speeds continue. A usable new
report updates the speeds without clearing held keys, autorun or a jump already
under way. A server correction also preserves those controls at the corrected
position.
Unresolved terms use the bounded-input rules in the
[readiness guide](movement-input-readiness.md); they do not confirm inventory
placement or server acceptance. This deliberate policy can temporarily allow
more speed than the original client until all inputs are proven.

A root stops travel while preserving turning; on Titanium, autorun resumes when
the root ends. Stun, freeze, posture, death and zoning still apply their movement
holds. When input is withdrawn, autorun, taps and queued jumps clear while
supported passive falls continue. The readiness guide records calculation
limits and deferred live checks.

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
On Titanium, water moves the body as the original client's own water step
does: velocity carries from frame to frame, the water damps it, and gravity
still pulls, so **an idle swimmer slowly sinks**, about two thirds of a unit
a second, until they rest on the bottom. Once the head goes under, Air
Remaining counts down and a character who can't breathe water drowns;
**Gills for Days** is the switch for those who don't want to. Characters
in water swim when they move across it and tread water otherwise, sinking,
still or resting on the bottom, as the original does; a swim or tread
carries on while wading out with the body above the water. Wade or fall
into water, move in any horizontal direction, and swim up or down by your
character's look pitch with forward or backward input: the pitch adds a
climb or a dive without taking anything from forward speed. Look Up and
Look Down (Page Up and Page Down by default) tilt that pitch in every
camera view, and first-person mouse look moves it too; in the chase views
the mouse moves only the camera, as in the original client. The chase view
does not show the pitch, so after Look Up a later stroke still rises:
Center View levels it in first person, and each zone entry or mouse-wheel
change between first person and the chase view levels it too. A strafe or
standing still gives no climb. Jump pushes up for half a second, and
holding it keeps pushing.

Swim speed is the land speed plus the Swimming skill: at a run speed of
0.7, level swimming goes 7 units a second untrained and 17 at skill 100, and
looking straight up also climbs at about twice that rate. Sneaking or
ducked swimming keeps the land speed. The client steps water 60 times a
second, where the original steps once a frame, so the sinking rate matches
the original at 60 frames a second (recording checklist 6.483). A swimmer
entering water starts within the water's speeds. Rising out of the water
carries the climb on as an ordinary fall back in. Unknown regions and dry
gaps still hold the stroke. A supported bank returns the swimmer to
ordinary movement. At the surface, look up and swim forward toward a nearby
bank to use Titanium's shoreline boost.
The client checks that the capsule can reach a supported landing. Automated
ground routes and EQMac-era sessions retain the deep-water guard. Ordinary
movement guards against entering lava.

Group-member `/follow` keeps its horizontal input when entering water, including
while typing in chat or the window is unfocused, and uses the ordinary swim rate
and stopping distance. It supplies no vertical stroke; manual movement ends
following.

The shoreline profile is derived from Titanium movement code and checked with
synthetic traces. Attended P99 shoreline traversal is still pending; it does not
claim identical trajectories across frame rates or water depths. Its damping
runs once per frame and each frame counts for at most 50 ms, so a slow frame
rate gives a lower arc, never a higher one; the frame bound, the 1.5 s arc
lifetime and the per-frame damping are inferred (recording checklist 6.407).
As with ordinary swimming, the session trusts the client's water and ceiling
checks for each frame.

**Jesus Mode** is an opt-in QoL setting where the session offers gameplay
departures. Walk from shore or land on mapped water or lava to use the
surface as ground. Your usual movement speed, solid collision, and falling
rules apply. A placement inside lava uses ordinary movement and recovery.
Turning the setting off restores ordinary movement on the next sample.
Scripted ground route finding uses solid terrain.

The session bounds each axis of a Titanium swim by the water's steady
rates; EQMac-era sessions keep a conservative single bound. Live P99 wet
movement and a side-by-side check against the original client's sinking
and swim speeds remain to be done. The Titanium air meter samples the character's
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
zone-point table where required. `report("label")` exposes the local crossing
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
has no client action yet.

With coins on the cursor and no item there, **Destroy cursor item** destroys
the coins instead, where the server takes it (stock EQEmu today; elsewhere it
says "This server does not take destroying coins."). As the Titanium program
does (read R49-11), it asks first, with the installation's own question, and
an unanswered question closes as a No after 10 seconds. On Yes it destroys the
first kind on the cursor, the most valuable first, in one go. **No**, Escape,
closing inventory, or any change to the cursor's coins cancels it. No answer
comes from the server, so the coins leave the cursor at once. If the server
says the cursor holds more than one kind, the others stay shown there, as the
server still holds them; the program would hide them (checklist 6.512c).
Scripts use the same control and dialog.

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
Typing `/fastdrop` turns the question off, so **Drop cursor item** drops at
once through the same checks and the same one-at-a-time hold; typing it
again brings the question back, and each says which way it now is. It is off
at start, is not saved, and turns off at a world entry, a camp, a return to
login or a lost link; a zone change keeps it. Only the item
drop's question is skipped: destroying still asks. A social, a hotbutton or
a script's `slash` turns it over as typing does (6.584). The program's handler (0x4685ba) is
unread, so this behaviour and its lines are inferred (checklist 6.536,
pending R49-6n).

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

### Apply Poison

Right-click a poison (item type 42) in a top-level slot, the cursor included, to apply it, as the Titanium program does it (recording checklist 6.493):

- Without the Apply Poison skill, you are told so and nothing else happens.
- Standing, you sit down. If you still aren't sitting (ducking, lying or feigning), you are told to sit.
- Your poison starts going on, with the program's line. After 10 s seated, the request goes to the server.
- Standing up, or any other posture change before then, stops it with the program's line, and nothing is sent. Zoning drops it without a line.
- A click while one is going on or waiting for the server's answer is ignored, as the program ignores it.
- The server decides whether it takes (class, weapon, level) and removes the poison either way. Its answer prints the program's line for success or failure.

The lines come from your installation's `eqstr_us.txt`: the start, the refusals and the stop in the program's colour 0x111, and the answer in colour 0x10e. A poison inside a bag isn't applied by a right-click, because the program only uses top-level slots this way. Alt+right-click and hotkeys keep their ordinary item use.

The program's wait is effect 234's total in ms, from your buffs (a type-4 record's taken away) and the AAs your class may use at your level, when that isn't 0; worn items don't count (read R49-1f, 6.493b). This client sums the same and tells the session, which then waits that long. Where it can't know the total (a buff whose spell your installation lacks, a formula or bard modifier it doesn't evaluate, an AA not yet described, or a spell that landed without a slot), the wait stays 10 s. A total below 0 never ends the wait, as in the program. The EQMac servers (Quarm, TAKP) take the same flow; that is inferred, because their program isn't read.

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

Select a spell and destination gem to memorize. Left-click a populated gem or
use its binding to cast, and right-click the gem to forget it (a press and
release over the same gem, as the official client is seen to do; its source is
unread, inferred, recording checklist 6.405). Casts use the
selected target or self when none is selected. Movement and posture changes can
interrupt a cast. The cast timer estimates progress while the server reports
the result. On Titanium EQEmu, Bards can type `/stopsong` during a tracked
gem cast or while a song is pulsing. An item cast or a cast of unknown source
blocks the request. If the server gives no matching cast update within one
second, the client releases the tracked cast and reports that the stop is
unconfirmed. When no cast is held, the client reports unconfirmed song state
as soon as it submits the request. Song buffs and gem timers follow the
server's updates. On P99 and Titanium EQEmu, when a song stops, the gems
whose songs have no recast time are ready again at once, and so is the short
wait before any spell, as in the official client.

On P99 and Titanium EQEmu, a Bard can type `/melody` followed by gem numbers,
such as `/melody 1 3 2`, to play those songs in turn, as the official client
does. The first gem is pressed at once. Each time your own gem refreshes after
a song, the client stops the song and presses the next gem, going back to the
first after the last. Every press goes through the same checks as a click, so
a gem on its timer or short of mana says why and the melody moves on at the
next refresh. Nothing else moves it, so a melody whose press was refused with
no later refresh stops there. `/melody` with no numbers clears the list, and
while a song plays it only stops the song. Clicking a gem while singing also
clears the list. Gems 1 to 8 are taken; the official client also takes 9,
which this client has no gem for, so a 9 is passed over. Elsewhere, and for
other classes, `/melody` does nothing. Scripts can run it as typed, for
example `slash("melody 1 2")`.

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
cancelled. Right-clicking an effect with no known slot explains this in chat.
A buff stays visible until received state removes it. If the selected
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
  player name. Under a Titanium installation `/assist Name` takes any spawn
  whose name starts with the word, the shortest such name first, without
  changing your target; no match says nothing, and your own name says the
  installed line and sends nothing. `/assist on` and `/assist off` set
  [Attack On Assist](settings.md#attack-on-assist). Wait for the returned selection before using an action that
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

## Surname

`/surname Name` asks the server for a surname, as the Titanium program
asks (read R49-10). It runs these checks in order and sends nothing when
one refuses:

- Under level 20, a line says so.
- `/surname` alone shows its usage. With a surname already set, the
  program asks a different question whose answer hasn't been read yet, so
  this client says that in its own words and sends nothing.
- Words after the command 32 bytes or more long are refused as too long.
  The whole argument is measured, not just the first word (inferred).
- Otherwise the first word, with `'` or `"` quotes taken whole, is cut to
  19 bytes and kept in its case. A question names that exact word for
  10 seconds. Yes sends it and says the request went. No, the time running
  out or the session ending sends nothing.

The server's answer is always shown: a refusal, or the line naming your
new full name. The name tag follows the zone's own last-name update.
Scripts run the same command with `slash("surname Name")` and answer the
question with the confirmation window's buttons, as a player does.

The Mac-era client's `/surname` isn't read yet. Its servers' handlers say
the client refuses under 20 and cuts to 19, as Titanium does, so the same
flow runs there (inferred). Project 1999 doesn't offer it from this client
yet. A quoted surname's spaces go as typed.

`/ttell message` sends a tell to the player you target, exactly as
`/tell Name message` would, and `/whotarget` lists the target's name in the
zone's `/who`. A tell goes only to another player whose name is letters
alone. An NPC, a pet, a corpse or yourself gets this client's own refusal
line and nothing is sent; `/whotarget` takes any target, since it sends
nothing. Socials may run both; a script tells only with its own tell step.
The forms, the refusal lines and the target kinds are inferred until the
original client's handling is read (recording checklist 6.528).

## Petitions, bug reports and feedback

`/petition words`, `/guidehelp words`, `/bugreport words` (also `/bug`) and
`/feedback words` send a report to the server's staff. A typed line sends
it, whoever types it: the keyboard, a social's line or a script's `slash()`
step, each with the same checks and pace. A button's game command answers
that the client does not take the command that way yet.

On Titanium servers these follow the official client (read R49-23,
recording checklist 6.511), in the installation's own lines:
- `/petition` sends its words and echoes them in the official line. With no
  words it prints the official line for that and sends nothing.
  `/guidehelp` is the same, with its own two lines.
- Where the world turns the petition window on, the official client shows
  its petition window instead and ignores the words. Which window that is
  is unread, so this client sends the words as above: a departure.
- The official `/bugreport` and `/feedback` open a window and ignore the
  words. This client has no such windows yet, so the **Typed Reports**
  setting (on by default until the windows are built) sends the words
  instead. A bug report then prints the official sent line, or its empty-box
  line with no words; feedback prints nothing, as the official window does.
  With the setting off, the words are ignored and a line says there is no
  window yet.

Elsewhere, or on any server:
- A bug report's first word may name its category with a colon, such as
  `/bug pathing: the guard walks into the wall`. A word that names no
  category, or one without its colon, leaves the whole line as the text,
  under Other. Bug reports and feedback carry the player's place and
  heading, and a bug report the selected entity where there is one.
- On Mac-era servers, whose official client is unread, a line with nothing
  to say sends nothing and answers `Use /petition with what to say.`, and
  this client says `Your petition was sent.` or `Your bug report was sent.`
  in its own words.
- One report of a kind is sent at a time: another of the same kind within
  ten seconds is refused with a line asking to wait. Text too long for the
  server's field is refused, never cut.

Stock EQEmu sessions offer all three; it drops feedback, as it drops the
official client's. Quarm and TAKP offer petitions and bug reports. Project
1999 offers none of them until its own attended check; there the command
answers as it does today.

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

Lines run a tenth of a second apart, as the original's social runner steps
them. A social or the chat also takes these Titanium commands:

- `/yell` calls for help from the players near you.
- `/stopcast` ends your own cast, or a Bard's song. It's refused while you
  ride a mount or have a buff whose effects the spell data can't name.
- `/doability 1` to `/doability 10` press the Actions window's ability
  buttons: 1 to 6 on the Abilities page, 7 to 10 on the Combat page.
- `/memspellset 3` or `/memspellset Heal` memorizes a saved spell set, by its
  number in name order or the start of its name. It opens the Spellbook.
- `/hotbutton Name 4 /say Hello` makes a social named Name, or rewrites the
  first line of the one with that name, and puts it on the cursor to drop on
  the hotbar. The number is the button's colour and can be left out.

Where the installed client has no string for a line, the line is in this
client's words, such as "You yell for help." or "Use /doability 1 through
10.".

The pause order follows the [EverQuest social hotbutton guide](https://www.everquest.com/news/imported-eq-enus-51004).

## Chat server channels

On a stock EQEmu server that runs its chat server (`ucs`), the session opens a
link to it when the world offers one, and these commands go to it:

- `/join channel` joins a channel. Several can be given with commas, and a
  channel can take a password as `channel:password`.
- `/leave channel` and `/leaveall` leave one channel or all of them.
- `/list` lists the channels you are on, and `/list channel` lists who is on
  a channel.
- `/announce` (or `/announce on` and `/announce off`) asks to be told when
  someone joins or leaves a channel.
- `/1` to `/10` followed by text say it on the channel in that slot. The
  first channel joined is `/1`.
- `/chat words` sends the chat server's own channel words as typed: `join`,
  `leave`, `leaveall`, `list`, `listall`, `announce`, and the owner and
  moderator words (`setowner`, `oplist`, `invite`, `grant`, `moderate`,
  `voice`, `kick`, `password`, `toggleinvites`), which are passed through
  unchecked for the server to judge. Any other word is refused and nothing is
  sent. That includes the chat server's mail, friends and ignore words and
  `set`, which saves the channels joined at login.

These commands work from the chat line, socials and hotbuttons alike, and
they are as free as `/say`. There is no send limit for any chat yet. A script's
`slash()` runs them as typing does (6.584).
Channel and player names from the chat server print without control
characters, so a name can't start an item link.

A line on a channel reads `Name tells channel:2, 'text'`. The slot comes from
the last list of joined channels the server sent. Before any list names the
channel, the line shows the channel's name alone. A member who joins or leaves
reads `Name has entered channel trade.` or `Name has left channel trade.`, and
the chat server's own lines print as it sends them. The client remembers the
joined list across zones, and forgets it when the link ends or you camp.

When the server offers no chat server, or the link has not come up, a command
says "You are not connected to the chat server." and nothing is sent. When
the link ends, the chat says "The chat server link has ended." There is no
reconnect: a new link only comes from the world's next offer. `/autojoin`,
the list the official client joins at login, is not kept yet.

These lines show in the System tab and are not a chat filter of their own. Every
wording, colour and slot form above is this client's and is inferred; the
official client's are pending read R49-12 (recording checklist 6.509).
Project 1999, Quarm and TAKP open no link: on P99, chat channels are withheld
until there has been an attended session there.

## Spoken languages

Each chat window speaks a language of its own, chosen with `/language` in that
window. The choice lasts for the session and is never saved, as in the
original. A new session starts every window on the first language your
character knows, the common tongue.

- `/language` says the language the window speaks and its number.
- `/language help` (the installed word) lists the languages your character
  knows, numbered from 1 in the original's order.
- `/language N` chooses the Nth of that list. Any other number, or a word, is
  refused with the installed line.

Every line you send goes out in the active window's language, on every
channel: say, shout, group, tell, out of character and the rest. The servers
turn out-of-character and some other chat back to the common tongue
themselves. The skill sent with a line is your skill in the language less a
quarter of your intoxication, as the original sends it. A game master sends
100. Your intoxication is read from your profile when you zone, so a drink
after zoning isn't counted until the next zone (a known shortfall).

A line heard in another language names it in the place the installed speech
line keeps for a tongue: the installed tag with the language's name when you
know it past 24, or the installed "unknown" tag. Language 0 has no tag. A
heard line is garbled once, as it arrives, as the original garbles it (reads
R49-4 and R49-4b):

- **By the speaker's skill.** Short words may be dropped or said plainer,
  then letters may change. The word list is this client's own, written fresh.
- **By your own skill**, but only where the server turns it on. EQEmu leaves
  it off; TAKP and Quarm turn it on.

Lines from a game master, lines to a game master, broadcast lines and item
links are never changed. Neither are your own lines (inferred).

While you are AFK, a tell gets the installed away reply back, in your window's
language, unless the tell is itself an away reply. A sender on your ignore
list gets none.

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
invisibility values remain hidden. Titanium zones without a sky are lit as the
original lights them: by each vertex's baked light, the zone's ambient, and up
to three nearby placed or carried lights (see [Zone lighting](zone-lighting.md)).
Known race and buff effects can change low-light vision. **See the World Clearly** and **Show Invisible Entities** are local
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
On Titanium servers a click on a NO DROP item asks before looting it, as the
original does, unless the corpse is your own. `/lootnodrop always` always asks
(the default when the installed `eqclient.ini` keeps no `LootNoDrop`),
`/lootnodrop sometimes` asks only about items your class or race can't use,
and `/lootnodrop never` never asks. The installed client's own words for
these choices work too. A question left unanswered for 20 seconds loots
nothing. Loot all leaves an item that would ask for its own click.
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

Interact with an in-reach guildmaster of your class to open Training. Ordinary
skills and languages share its list. Language rows use received proficiency
and the same estimated price formula as skills. EQEmu permits language IDs
0..25 up to proficiency 100 when the current offer enables the language;
P99 follows Titanium's racial level table and
class exceptions. Native race/class/level rules do not restrict EQEmu's separate
server policy. Unknown proficiency stays blank and cannot be practiced.

On P99, select a skill or language from the received Training list and press Train once. The listed price
estimates the cost in copper denominations. The skill value follows server
updates. The point count goes blank as soon as the request is sent. A skill
increase leaves the practice outcome unknown. Paid practice leaves the
purse unknown until a new balance arrives. Re-enter the zone before
practicing again or asking the same guildmaster again. Done closes the
trainer. This path awaits an attended P99 check.

EQEmu retains its existing point and purse projection after a matching received
skill or language increase. The update contains no point or cost receipt;
re-entering the zone supplies fresh profile balances for verification. A language
practice that times out or loses its trainer leaves points unknown and also
withdraws the purse when coins may have been spent. Free practice uncertainty
preserves known coins. Expired or invalidated unanswered openings require a
fresh admission before that trainer can be asked again. An already answered
offer can reopen after invalidation through a fresh request and received offer.
Silence never reports success.

Grouping, raids, pets, training, reading and consent dialogs have separate
capabilities by server. Pet Info can hold an early buff snapshot while the
spawn table identifies your pet. It keeps an unresolved snapshot for up to one
second from local receipt. It shows buffs once there is one living
owned pet with the matching spawn ID. A despawn, replacement, death or
ownership conflict invalidates the old snapshot for that ID.

Camp uses timed preparation and acknowledgement where offered. The local
`--demo-trade` mode previews loot, merchant and trade UI.
See the [policy matrix](../SUPPORT.md).

On EQEmu, take a race-141 boat you stand on by right-clicking it, as the
Titanium program does; right-click it again to let it go. A right-click on a
boat you are not standing on does nothing, and considers nothing. `/boat
acquire` and `/boat release` are this client's own shortcut to the same
request, and a script's `interact` step on the boat makes it too. Both need
the current accepted floor link to identify the vessel.
An acquire remains pending until its unambiguous server echo. An unresolved
same-ID acquisition needs a release echo before retrying; completed ordered
releases permit ordinary reacquisition. A late acquire cannot undo release
intent or timeout. Walking off requests release. P99,
EQMac, steering and control across zone admissions remain unverified gaps.

## Zone music and ambient sound

Each zone plays its own ambient sounds near you, such as water, wind and
birds, by day or night, and the music of the region you stand in, which
fades when you leave it. The ambient sounds follow the original program's
reading of the zone's files (item 6.503, read R49-7 first pass and its
engine follow-up), and so does the music: the smallest region you stand in
plays its track, by day or night, and a change fades the old track out.
Music needs a SoundFont you name; see
[Display and audio](settings.md#display-and-audio) for it and the sound
options. `/combatmusic` turns combat music on or off and keeps the choice,
but this client plays no combat music yet.
