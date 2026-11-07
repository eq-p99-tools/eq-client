# Movement inputs and deferred compatibility checks

The renderer reports explicit ready/unavailable movement inputs for the current
session revision. Missing inventory, attributes, coin state, buff membership or
affecting spell data withdraws player input. A confirmed empty inventory is a
valid zero load. Corrections and new admissions cannot reuse an earlier report.
Queue saturation retries rather than leaving an old grant indefinitely.
Purse and cursor coins require receipt in this admission. Bank balances do not
contribute to carried weight, and an unknown bank balance alone does not block it.

Carried weight uses the networking library's generation-specific bag, stack and
coin rules. Effective STR includes eligible, level-scaled equipment and stat
food, supported worn spell effects, active spell effects and EQMac fatigue.
Spell values use caster level and supported formula caps, not the raw base.
Movement modifiers choose the strongest snare, otherwise the strongest positive (the Titanium and EQMac programs' own rule, read 2026-11-03: each buff's effect evaluated at the buff's caster level, the most negative value or else the largest positive one; it is not a sum)
modifier; an actual root dominates. STR is clamped to 1..255. Known EQMac fatigue
subtracts `clamp(fatigue - effective_STA, 0, 10)`; absent fatigue conservatively
subtracts its source-defined maximum 10.

Weight-preserving ordinary inventory predictions can keep movement ready.
Pending equipment/stat-food changes or changes to carried weight cannot. An
invalidation remains unavailable until authoritative recovery. This calculation
does not turn a timeout into a confirmed inventory update.

After a ground drop, the server may already hold a queued arrival as its
current cursor item. Take the heaviest possible current cursor group and
count the rest of the carried inventory once. The worker requires a fresh
input report meeting that bound. If a correction makes movement inputs
unavailable, the readiness notice asks the player to re-enter for a fresh
inventory snapshot. An ordinary cursor resync can leave the landing bound
usable during that pause.
Recalibration preserves an in-progress descent and pending fall report.

The session owns HP/sneak speed adjustments, movement caps and permission. When
it withdraws input, the renderer clears taps, autorun and queued jumps but still
simulates passive descent where supported. The session independently rejects
horizontal drift, turns and ascent during withdrawal. Death and transfer still
suspend all movement.

## Buff membership and diagnostic reports

Script reports keep `slots` (spells in known server slots) separate from `effects`
(spell landings awaiting a server slot). The Effects window reads both. A new
profile replaces the slot table and clears the unslotted entries, so `effects=[]`
does not mean that the player has no buffs; check `slots` as well.

Known instant spell landings do not become unslotted buffs. EQMac's spell-file
layout defines only the primary duration, so explicit zero primary duration
fields establish an instant spell. Titanium also defines alternate duration
fields: missing, malformed or nonzero values do not establish an instant spell.
Unknown mechanics stay unresolved. Explicit server buff slots remain
authoritative regardless of local instant classification.

On Titanium, movement and its strength and stamina inputs use the modifier
carried by a landed effect or a received buff slot. The native action handler
at 0x414913 stores the modifier on the buff, and the cache at 0x4102a6 scales
the level-adjusted, capped magnitude in tenths. Supported boosts are 11 through
45. EQEmu also captures the instrument modifier at application and sends it
with the action and slot records. [Application and action](https://github.com/EQEmu/EQEmu/blob/4aceae18b94ffaafc08e2b17bc41cd72c77f795d/zone/spells.cpp#L3486), [Effect evaluation](https://github.com/EQEmu/EQEmu/blob/4aceae18b94ffaafc08e2b17bc41cd72c77f795d/zone/spell_effects.cpp#L3252)

Equipment changes follow the existing inventory and movement revision path.
A fresh landing can carry a different modifier after a gear change. The
active effect keeps its received modifier until a refresh, replacement,
server fade or new snapshot. The displayed duration is an estimate.

## Deliberate limits

- Unknown affecting formulas, formulas 60/70 with differing semantics, missing
  spell records, unsupported worn effects and unresolved instrument modifiers
  withdraw input. Unslotted additive STR/STA effects withdraw until membership is
  known, because they might replace a slotted effect rather than add to it.
- Positive AA/stat-cap, alcohol and berserk STR bonuses are not granted. The
  255 cap and omitted positive bonuses produce a conservative capacity. This is
  not a complete replacement for all server stat calculations.
- Coin transfers follow the existing validated protocol ledger; individual
  transfers have no server acknowledgement. Cursor coin inclusion and P99 use of
  public Titanium/EQEmu rules remain inferences requiring compatibility checks.
- `ClientWorld::carried_weight` estimates the displayed load.
  `ClientWorld::fall_weight` uses the retained cursor bound during a drop.
  It returns `None` if an unbounded item change or unknown coins leave the
  load uncertain. The renderer can still determine a weight-independent
  landing. If the landing needs a load it cannot determine, the renderer
  records that uncertainty in chat.
  Safe Fall's skill notice requires that the fall would use the skill at
  both zero weight and the upper bound.
- Source evidence and decoder details are in the companion network document
  `docs/movement-input-readiness.md`. No live session was used for these changes.

## Single deferred live-check batch

Run only after final inventory/abilities/spell integration, using the executable
built from the final client head and its exact network pin. The coordinator owns
the dedicated live-check chat. None of the following has been run by this task.

Prerequisites: authorized disposable test character; user-provided installed
assets and matching spell data; a safe flat area with room to walk; a harmless
ledge over a safe landing; known equipment, food, bags and coins; sufficient HP;
no active combat. Keep account/character identifiers, captures and assets private.

| Check | Servers | Steps | Expected | Risk / cleanup |
| --- | --- | --- | --- | --- |
| Admission and recovery | Quarm first; TAKP, EQEmu, P99 separately | Enter zone, wait for profile/inventory/buffs, walk; zone again | Input starts only after complete state; no old grant crosses admission | Use safe zone; stop autorun before zoning |
| Weight and STR | Same server matrix | Walk a fixed safe distance; equip/remove known STR gear; compare load with coins, a stack and a reduction bag | Reported weight/STR change by supported rules; speed responds at encumbrance boundary | Use disposable gear; restore original layout |
| Ordinary cursor round trip | Quarm first | Move an ordinary non-stat item to cursor and back without changing total load | No invented authority; unchanged load remains usable while prediction origins persist | Do not use consumable/valuable items |
| Uncertain weight/equipment | Isolated Quarm first | Change bag reduction/carried load or worn STR; exercise a server-controlled refused move/resync on disposable state | Input withdraws while uncertain; only covering updates/snapshot recover; no timer grant | No deliberate public-server refusal; obtain authoritative resync before proceeding |
| Buff values and fade | Quarm; others independently | Apply known level-scaled speed buff, snare, root and STR buff/debuff; let each fade | Correct supported magnitude; strongest snare wins; root blocks input; fade recomputes | Safe consenting setup; remove all effects afterward |
| Correction and passive fall | Quarm/EQEmu controlled test server first | Cause a supported safe correction; repeat above a harmless ledge while inputs unavailable | Old queued input rejected; bounded descent/landing persists; no horizontal/turn/jump input | Fall damage possible; avoid dangerous drops |
| Fatigue | Quarm/TAKP | Compare known fatigue/STA state and its subsequent stamina updates | At most 10 STR penalty, recomputed on changed fatigue | Avoid exhaustion/combat; recover before leaving |

Record each server, exact source heads, steps, expected versus observed behavior
and any correction/refusal. A pass on one server does not establish another's
compatibility. Stop on unexpected displacement or inventory divergence.


## Coherent inventory and fatigue prerequisites

Host move selection/display uses `ClientWorld::inventory()`, the bounded queued
projection. Movement weight, equipment and food use `worker_inventory()`, the
ordered submitted replica. Neither view acknowledges server delivery. Bind
`Session::inventory_move_rules` from the network Client before queuing moves;
`plan_inventory_as(None)` refuses planning instead of choosing EqEmu rules.

A same-admission death/transfer/nonterminal-link reset discards the display
projection while pending results drain. Matching late submissions update the
worker replica once; duplicates, old admissions and inconsistent payloads cannot
overwrite newer authoritative state. Typed dependent refusals remain available,
while `Reply::InventoryMove { present_error, .. }` controls repeated UI notices.

For EQMac, ready reports include the network-owned `EqMacFatigueInputs`: signed
effective strength before fatigue/cap, and known effective stamina capped to
1..255. All metadata, effects, inventory and coin prerequisites still apply.
The scalar capacity is a stable final capped maximum-penalty fallback; the typed
component is the sole basis for current fatigue-adjusted capacity. The session
applies authoritative fatigue before the final cap. The display calls the same
network formula without changing the report or weakening receipt equality.
Fatigue alone therefore leaves the exact posted payload and report identity
unchanged. Missing prerequisites remain unavailable; absent components retain
legacy handling and Titanium capacity/zero-sentinel semantics are unchanged.

These are source and synthetic regression contracts. Final combined dependency
pins, client execution and separately coordinated live validation remain required.
