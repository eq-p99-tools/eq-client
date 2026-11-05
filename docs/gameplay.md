# Gameplay

[Documentation index](README.md) · [Server support](../SUPPORT.md)

These are implemented client paths, not a claim that every action is available
on every server. Disabled controls explain whether the server policy lacks a
feature or this client lacks the UI. The session validates actions again before
sending them; queueing a request is not proof of a successful game action.

## Movement, falling and zoning

Movement is available when the admitted session grants it; there is no separate
P99 opt-in switch. Local movement uses collision checks and bounded simulation
steps. The display interpolates accepted movement instead of waiting for each
remote position update. Speed, elapsed time and admission checks constrain the
outgoing requests; corrections replace the predicted position.

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
when those data cannot be read. Swimming is incomplete: the water/lava guard
prevents entering deep water or lava while allowing escape from an existing
hazard. The server remains authoritative over corrections and zone admission.

Installed WLD boundary regions supply zone-line detection. Numbered boundaries
resolve through the server's destination table; older absolute boundaries carry
their own destination. Crossing queues a request and the server decides whether
to accept it. Arriving inside a boundary does not immediately retrigger it, and
standing in a rejected boundary does not continuously resend requests.

Server-directed transfers and death/bind offers suspend old-zone actions, use
the world/zone handoff, and install a new admission with its own placement and
state. Same-zone server relocations and rejected-zone rewinds update the local
controller without unnecessarily reconnecting. Client detection of same-zone
teleport pads remains incomplete. Initial login, boundary crossing, teleport,
death and respawn are separate validation scenarios; success in one does not
establish the others.

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
click another box with coins on the cursor to place/convert them. Shared banking,
dropping carried items and destroying coins have no client action yet.

On P99 and Titanium EQEmu, **Destroy cursor item** asks before destroying the
whole cursor item or stack. **No**, Escape, closing inventory, or any inventory
or admission change cancels that confirmation. Place queued cursor items and
empty containers first; populated containers are outside this initial slice.
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

Select a spell and destination gem to memorize; click a populated gem or use its
binding to cast. Shift-click forgetting is separate from casting. Casts use the
selected target or self when none is selected. Movement and posture changes can
interrupt a cast; estimated completion time alone does not report success.
Spellbook operations serialize outstanding edits and wait for matching answers.
Deletion confirmation is invalidated when its selection or admission changes.

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

## Targeting and interaction

Click a rendered entity, use Tab/Shift+Tab to cycle the nearby rendered set, or
F1 for self. Escape clears the target after higher-priority UI actions. The target
panel shows identity and known HP. The map's explicit Target button shares these
checks; selecting alone does not attack or cast.

Considering, hailing, auto-attack and class abilities use the target and session
rules. Loot lists received corpse contents; Loot all takes items sequentially.
Merchants support selection and buy/sell requests. Giving to NPCs and trading
with players have separate interaction state; a trade is not complete merely
because its window opened. Ground pickups and doors use reach checks. World
tradeskill containers and combines exist where `Tradeskills` is offered, rather
than being universally unsupported.

With Titanium, Buy and Sell open the existing quantity picker when more than
one unit is available; a single unit goes directly through the same checked
submission. The picker starts at one and is bounded by the received stack,
stock and known purse. The selected name and quote follow its count. Cancel,
closing the merchant or changing its catalog, inventory or admission discards
the selection. Only one merchant action can be pending. An unanswered action
shows an unknown outcome and is never retried automatically; received items
and coin updates remain the session's responsibility.

Grouping, raids, pets, training, reading and resurrection/translocate dialogs have
client paths with distinct capabilities. See the [policy matrix](../SUPPORT.md)
before assuming a visible window can act on P99, Quarm or another server. Camp
uses a timed preparation and acknowledgement flow where offered. Local
`--demo-trade` previews loot/merchant/trade UI without confirming a live protocol.
