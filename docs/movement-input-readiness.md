# Movement inputs and deferred compatibility checks

The renderer reports explicit ready, bounded or unavailable movement inputs for
the current admission and session revision. A first grant needs a usable report:
missing inventory, attributes, carried coins or buff membership can prevent
calculation, while an unsupported effect can leave the calculable terms usable
as `Bounded`. The session can start a grant from a bound without an earlier one.

Once a grant is in force, ordinary belongings changes, buffs, spell landings and
level changes ask for fresh inputs while the existing speeds continue (reads R7,
R18). A usable report replaces the speeds in place. An `Unavailable` report
preserves the active grant without a timeout. A root stops travel but keeps
turning input; stun, freeze, posture, death and zoning still apply their holds.

Each term the client can evaluate now is taken as it is now, slower or faster;
one it cannot evaluate is left out. Positive equipment/food or buff terms come
from the earlier same-admission report where the corresponding state is
unknown, with any known current root or snare applied. A known ended speed buff
ends at once. This policy can be faster than the original client would allow
until all inputs are proven (recording checklist 6.451). A confirmed empty
inventory is a valid zero load; missing inventory never becomes zero.

Reports carry `session_id`, `revision`, `report_id` and state. A changed state or
revision gets a new report identity; queue retries reuse the identity of that
logical report. Corrections request a fresh revision while keeping an active
grant and controls at the corrected position. A new admission discards the old
report history. Queue saturation retries; a late host report does not expire an
active grant. Local input acceptance is not a server acknowledgement.
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

Weight-preserving ordinary predictions can keep the calculated load usable
without proving their placements. Pending equipment/stat-food changes or
unbounded carried-weight changes can prevent a complete report until covering
updates or a fresh snapshot restore the required authority. That calculation
limit does not withdraw an active grant: the renderer reports usable bounded
terms from the same admission, or the session retains its inputs when the
report is unavailable. A known current weight or slowing effect still applies.
Inventory actions retain their own guards, and neither silence nor a movement
grant confirms an inventory update.

After a local EQEmu ground-drop submission, cursor removal is a local projection,
not a received inventory update. Keep each distinct submitted instance's whole
mass in addition to the projected inventory, with coins counted once. A later
cursor group can still be queued on the server; the bound includes the heaviest
possible group and retains locally removed instances until covering received
records or a full snapshot replaces them. This can deliberately read heavy.
Ordinary silence settlement does not retire drop mass. The worker checks the
bound independently and raises an understated report to it. Without a current
bound it refuses a new grant; an active grant can continue with its weight
retained as a floor while usable reported terms apply. A slot-only reduction
can leave the last proved landing bound usable while inventory readiness is
withdrawn. An unbounded update withdraws that landing bound too.
Recalibration preserves an in-progress descent and pending fall report.

The session owns HP/sneak speed adjustments, movement caps and permission. A new
positive speed for a player already moving replaces only the speeds; held keys,
taps, a queued or crouched jump, autorun and a jump under way go on, the jump's
sideways carry limited to the new speed. A server's move or a same-zone landing
starts prediction afresh with the speeds kept; autorun, held keys, a jump
request and a crouch go on from the new place (read R18). A root grants the
player at no speed instead of withdrawing input (reads R18, R22): the keys,
the mouse and a script's facing still turn them, nothing carries them, a jump
is refused, and on Titanium the autorun latch survives so they run again by
themselves when it ends; the Mac-era program is unread and the root drops
autorun there. When the session withdraws input (death, zoning), the
renderer clears taps, autorun and queued jumps but still simulates passive
descent where supported. The session independently rejects horizontal drift,
turns and ascent during withdrawal.
Death and transfer still suspend all movement.

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

- Unknown affecting formulas, formulas 60/70 with differing semantics, effect
  ids above 254, missing spell records, unsupported worn and food worn effects,
  unresolved instrument modifiers (a Bard song's STR term, run 1d), unslotted
  additive STR/STA effects (they might replace a slotted effect rather than add
  to it) and the inventory states that end only with a full refresh leave the
  inputs unproven. Calculable terms can form a bounded report even before the
  first grant; otherwise a missing prerequisite waits without a grant and
  retains the inputs where a grant is already active (6.451). Each is closed
  as its rule is read. Every effect is still scanned, so none hides an
  evaluable root or snare.
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

## Deferred scenario checklist

Use an executable built from the recorded client head and its exact network pin.
Check the shared scenarios on local Titanium EQEmu before public-server checks;
EQMac behavior needs its own policy-specific validation. P99 checks remain
attended. This table is a checklist, not a record that a scenario passed.

Prerequisites: authorized disposable test character; user-provided installed
assets and matching spell data; a safe flat area with room to walk; a harmless
ledge over a safe landing; known equipment, food, bags and coins; sufficient HP;
no active combat. Keep account/character identifiers, captures and assets private.

| Check | Servers | Steps | Expected | Risk / cleanup |
| --- | --- | --- | --- | --- |
| Admission and recovery | Local EQEmu first; Quarm, TAKP and attended P99 separately | Enter zone, wait for usable profile/inventory/buff inputs, walk; zone again | A ready or usable bounded report establishes the grant; no old grant or report history crosses admission | Use safe zone; stop autorun before zoning |
| Weight and STR | Same server matrix | Walk a fixed safe distance; equip/remove known STR gear; compare load with coins, a stack and a reduction bag | Reported weight/STR change by supported rules; speed responds at encumbrance boundary | Use disposable gear; restore original layout |
| Ordinary cursor round trip | Local EQEmu first; Quarm separately | Move an ordinary non-stat item to cursor and back without changing total load | No invented authority; unchanged load remains usable while prediction origins persist | Do not use consumable/valuable items |
| Uncertain weight/equipment | Controlled local EQEmu first; Quarm separately | Change bag reduction/carried load or worn STR; exercise a server-controlled refused move/resync on disposable state | An active grant continues while usable current terms replace stale ones; inventory recovery follows that policy's rules and silence never proves delivery (6.451) | No deliberate public-server refusal; obtain authoritative resync before proceeding |
| Buff values and fade | Local EQEmu first; other policies independently | Apply known level-scaled speed buff, snare, root and STR buff/debuff; let each fade | Correct supported magnitude; strongest snare wins; root stops travel while preserving turning; fade recomputes | Safe consenting setup; remove all effects afterward |
| Correction and passive fall | Controlled local EQEmu first; Quarm separately | Cause a supported safe correction; repeat above a harmless ledge | Old queued input rejected; held input continues from the corrected position at the speeds in force; bounded descent/landing persists | Fall damage possible; avoid dangerous drops |
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

For EQMac, ready and bounded reports can include `EqMacFatigueInputs`: signed
effective strength before fatigue/cap, and effective stamina capped to 1..255
when known. This host-calculated component lets the network session own fatigue
application. Complete reports still require the metadata, effects, inventory
and coin prerequisites; bounded reports keep their unresolved category.
The scalar capacity is a stable final capped maximum-penalty fallback; the typed
component is the sole basis for current fatigue-adjusted capacity. The session
applies authoritative fatigue before the final cap. The display calls the same
network formula without changing the report or weakening receipt equality.
Fatigue alone therefore leaves the exact posted payload and report identity
unchanged. Missing prerequisites do not become proven: usable retained terms
can form a bounded report, while an unavailable report keeps an active grant
or waits where there is none. Absent components retain legacy handling and
Titanium capacity/zero-sentinel semantics are unchanged.

These are source and synthetic regression contracts. Final combined dependency
pins, client execution and separately coordinated live validation remain required.
