# Movement parity

`[R]` identifies a static program read. `[I]` identifies an inference or
offline comparison. Offline results do not establish live compatibility.

## 6.670 — ordinary forward run and release glide

[R] Titanium samples input before its motion step. On release it clears
Forward immediately, while owned horizontal velocity decays by 0.625 per
frame; input contributes 0.8 while held (`eqgame.exe` `0x4a1d74`,
`0x4bfe0f`). The following pass's ordinary report block samples current
position and velocity, subject to its existing more-than-280 ms gate
(`0x4aba14`). A gated start and stop can coalesce. There is no special final
endpoint or stop report.

The client owns these completed frames separately from local admission. It
shows each collision-resolved step immediately, publishes the previous
completed state before advancing the next input, and never replays an admitted
prefix. Local refusal or server placement retires prediction. A refusal keeps
the existing mouse heading; a genuine placement retains its existing reset
semantics. Unsupported handback waits for a genuinely admitted prefix rather
than rewinding accepted movement or catching up missing time.

[I] The host's precise observed frame duration replaces the native whole-ms
clock. Input is normalized to the existing independently calibrated steady
speed; the 150 ms cap, movement guard and packet pacing remain unchanged.
The per-move stationary-delta test is not inferred from distance. Current
scope is Titanium ordinary Forward running on positively known, static,
dry geometry with ordinary grounded support. Static walkable slopes and
risers use the existing Titanium walking body physics and body-specific reach;
the horizontal recurrence stays the same through keyboard turns. Per-sample
rise/drop budgets are checked against both the received basis and every posted
observation that could already have been admitted, without assuming acceptance.
Walk, calibrated Strafe and GroundedDiagonal, backward, unsupported edges,
unknown liquid metadata, moving support, water, flight, crouch, jump, boosted run,
routes and follow retain their existing paths. Slippery-region friction is
not separately modeled. Zero vertical wire velocity at this horizontal-only
seam is inferred, not a native slope-velocity claim.

Camera-relative actions which select Forward and face their direction can
also use this Forward path; they are distinct from calibrated body-relative
Strafe. No new lateral normalization or rate is introduced. Uncertain or moving
support still uses the admitted-prefix handback above; a static riser replay
does not establish a moving-carrier transfer.

[R] Key-command handler `0x4619cd` writes the forward state; input `0x4a1d74`
reads it once per frame, before `0x445243` calls `0x4bfe0f` and `0x48ba76`.
A held state observed in several frames can therefore move before a 100 ms
legacy sample opportunity. [I] A complete press/release handled before any
input pass is expected to read up, but the original handler's possible latch
remains unread. This producer conservatively uses current held state only;
it does not manufacture a frame from an unobserved press edge. An observed
60 ms pulse and an unobserved same-pass tap are separate cases.

Offline release/re-press and whole-ms-reference comparisons are distinct
from gameplay validation. The native target includes a short decreasing
glide after release; an instant stop is not the selected behavior. The local
look remains required before merge, and the first public-server movement
check is attended.

## 6.660 — ship rigging, sail and rail as walking walls

[R] Titanium's ordinary walking move (`eqgame.exe` `0x48acef`) uses object
filter `0x42cf6e`, which skips NPC spawns before testing their faces. The
ship is therefore not a lateral body wall; floor filter `0x42d05f` retains
it. Ordinary boarding uses actor `+0x158`; it does not set `+0x160`,
the mount/rider link managed by `0x483958`. Walking off the deck edge is
expected once floor support ends.

[R] The ceiling probe (`0x43a0a0`) uses `0x42d123` and `0x42cf5c`, which
also admit the NPC carrier and its down-facing faces. Unlike the floor
filter, these do not exclude mount/rider links. They exclude doors,
collision-mode-2 objects and one model identity; the identity being the
own model is [I]. This carrier change does not add those general ceiling
rules to the client's other object classes.

The client applies this distinction to received NPC carrier instances in
Titanium dry walking. Deck support, vertical collision, headroom, viewing
rays and other movement modes retain the geometry. Doors and placements
are unchanged. Player and unknown-kind carriers retain their prior body
collision; their qualification is a separate gap below.

[R] The separate upward ceiling query starts at the actor origin, so low
rigging below that origin does not become a wall again. [I] Its radius and
top retain the client's body estimates and authored facing; the shape
remains an approximation. The collider and
drawing share the current rest pose and rigid carrier transform. Native
membership of every authored face, and animated rigging
floor poses, remain unconfirmed. Rope or sail support still depends on
floor winding and step reach. EqMac and RoF2 retain their previous behavior;
the Titanium walking rule has not been read for those programs. A local
ride remains the before-merge visual check; the first public-server ferry
check requires attended validation.

## 6.657 — jump arc at the assumed frame rate

[R] `eqgame.exe` `0x4bfe0f`, called from `0x445243`, scales gravity and travel
by elapsed time but applies the 0.99 air factor once per rendered frame.

[I, harness] At 60 frames per second, the client's physical apex is 5.245
units against 5.243 for the read's operation order; both are unsupported
for 500 ms. This matches the assumed 60 fps arc without changing launch
strength or gravity. At 30 and 100 fps the read's apex is 5.169 and 5.046,
while the client stays at 5.245: a labelled frame-rate difference, up to
0.2 units in these rows. Drawn and reported sampling remain separate from
this physical arc comparison.

## 6.657 and 6.659 — drawn and recorded jump positions

Ordinary unsupported dry samples retain their collision-resolved position
slices with the same relative times as the local velocity/pose snapshots.
Drawing follows those vertices instead of the single chord to the last
endpoint. Queued samples keep their own paths; refusal/reset discards them.
Clipped zone-line, water, flight and moving-support samples keep their existing
paths. The release translation cursor cannot accelerate an airborne path.

The same bounded actor-origin trace accompanies the ordinary movement request.
After the existing endpoint guard accepts it, the session checks its anchors,
span and vertices against the proposal's existing budgets. Accepted cause-1
history frames sample the trace on the existing counter clock. No higher arc,
extra airborne hold, packet cause, update counter rate or extra apex update is
introduced. The final actor position, `0x14cb` packet fields and eligibility
gate stay unchanged; `0x6a1b` frame positions and duplicate suppression may
change because the missing intermediate positions are now present. A transport
failure or rejected request commits neither movement nor its path.

[R] Native motion runs once per rendered frame using whole elapsed milliseconds,
with per-step drag and a capped timestep; this does not establish a fixed
native 60 Hz tick. [I] The current port still groups airborne physics in 50 ms
steps with three assumed damping frames. This change records/draws that existing
trajectory; it does not change launch, gravity or the shared controller.

## 6.666 — grouped-clock diagnostic retained

[I, offline diagnostic] Three 1/60-second dry slices produced a 0.000424-unit
horizontal lead over the per-slice reference. A separate steep-exit handoff
after a 217 ms prefix produced a 1.7815865-unit endpoint 3D displacement over
the following 100 ms, versus a 1.5383435-unit continuous checked path. The
approximately 0.24324-unit difference is mostly vertical; it is not a measured
standing-jump height or complete-jump result. The endpoint-versus-full-path
metric distinction and inferred reference rate matter. Neither short interval
alone establishes a whole-move tolerance result. These are known controller
diagnostics; presentation/history changes do not tune or conceal them.

## H2 — fall start across a same-zone pad

[R] Pad placement keeps actor `+0x4c`, the fall-start height; landing clears
it (`eqgame.exe` `0x48aaa6`, `0x48b6c5`). The client keeps the old peak through
placement and clears it when the first-step landing is consumed. A floor
at or below the soles keeps the previous fall until an actual landing. This
matches the original; no new jump or fall-height adjustment is implied.

## 6.664 — door types 50 and 51

[R] Walking object filter `eqgame.exe` `0x42cf6e` skips door types 50 and 51.
Difference: the client's current Titanium door classification keeps their
walls. This remains a separate gap; the carrier filter does not change
doors or lifts.

## 6.665 — other players as walls

[R] `eqgame.exe` `0x42cf6e` includes a player only when model slot `+0x1c`
passes and actor `+0xebc` is within 12,000 ms. Difference: the client has no
general player collider registration or that qualification. A player using
a carrier race can enter the existing carrier collider path; player and
unknown-kind carrier instances retain their previous behavior here rather
than receiving the NPC exclusion. This remains a separate gap.

## 6.663 — surface swimming

The clear-water surface continuation uses the existing native swim calibration and frame clock. It uses ordinary calibrated movement input, supported Titanium calibration, known immutable ordinary-water regions and a clear unsupported body. Forward with keyboard turning, mouselook turning, strafing, pitch changes, camera-relative input and autorun share the ordinary wet input rates. Walking and backward input use their existing calibration; a zero-rate stroke does not create a new continuation. Water, W2 and W3 keep their exact region kind. Each physical frame move and final partial is checked by the original collision world; a proved numerical no-update tail can share the preceding move in the same request at the same velocity and stored medium.

| Evidence | Behavior | Program location |
|---|---|---|
| [R] | One dynamics update per frame: gravity, select the stored medium, drag, negligible clear, input, then move. Horizontal forward input is not multiplied by the pitch cosine. | `0x4bfe0f` |
| [R] | Unsupported dry surface frames keep 0.99 on both axes, add gravity and no new input. | `0x4bfe0f` |
| [R] | Move first, then refresh liquid for the next frame; a medium crossing does not discard elapsed frame time. | `0x48ba76`, `0x489b0a` |
| [I] | The client keeps its fixed 1/60 second water clock (6.483), across both media and eligible accepted requests. Rendering FPS and network cadence do not select another clock. | Clock is a client inference from the per-frame `0x4bfe0f` step. |
| [I] | The conditional dry animation clear is omitted because its relevant pose mapping is not established. | Conditional clear in `0x4bfe0f`. |
| [I] | The existing conservative ordinary-entry cap remains at ownership seams; internal wet/air refreshes do not add a clamp or a new update. | Client modeling choice alongside `0x4bfe0f`. |

A request ending inside a frame integrates its current velocity and carries the actual frame phase. Liquid refresh occurs only after a physical frame's checked movement completes. The continuation starts at a known ordinary wet boundary, including normal entry or admission. A carried ordinary partial frame can reach that boundary inside the request: the unchanged API must complete it without a new update, and the original collision and continuous-liquid queries must prove its entire path remains in the same water. A crossing or unknown phase declines instead of guessing a medium or resetting the clock. Passive all-wet tracking keeps the actual ordinary `WetVelocity` snapshot after local acceptance, so it can retain phase provenance while ordinary whole-medium commands remain unchanged. Phase is never reconstructed from position or elapsed request time.

The two false frame-event flags are not enough to identify a numeric tail: a genuine interior partial can have that pair too. The planner captures the actual pre-call boundary state and uses the untouched API result. At a known boundary, a successful no-start/no-complete result implies the network clock's own bound. There is no client copy of its private tolerance and no displacement threshold for classifying tails. A tail-only request declines. The combined path still goes to the collision owner, and genuine tiny or shortened owner results decline the entire trial. The next medium comes from the frame-completion point on that checked segment; its answer and the returned tail endpoint's answer must be known and equal, including when the completed frame crossed the surface.

Dry surface frames also require the same authored water-region identity directly beneath the origin, not just the same WT/W2/W3 kind. The original immutable BSP provider must prove that uninterrupted vertical column at the frame's start and end, and the original solid query must find no obstruction before the water. Another pool, a route, unknown data or an unsupported provider declines the entire interval to ordinary movement. The original terrain domain bounds the query; no water height or maximum surface height is guessed.

Planning runs on a clone. A queued request does not commit the clock. Only its exact local movement result can commit it once; duplicate and stale results do not. Refusal, correction, zoning, changed context and ineligible or idle input invalidate continuation ownership. A late accepted queued move may transfer its raw velocity once to ordinary movement, without resuming the old continuation or integrating an idle gap. No server acknowledgment is inferred from local acceptance.

Wet handback transfers the actual velocity/phase object unchanged. With the same outer call partition, the next ordinary result is exact; different outer partitions retain the existing clock's accumulation window. Dry handback uses the existing full water-exit carry without an extra update and preserves the highest original-owner feet point since the last wet frame, so a later fall does not lose its peak. Required handbacks are immediate. The ordinary airborne controller's grouped clock remains unchanged (6.666); its steep-exit diagnostic is separate from any distance added by a seam.

The renderer follows accepted endpoints with its existing playback. It adds no submitted preview or release catch-up travel for the continuation. The existing character picker retains an already-playing swim/tread while the drawn origin is dry and the soles remain wet. Camera orientation is unchanged.

Stutter remains on ordinary fallback paths: idle or cancelled input, follow/route movement, jump or bank/shoreline interactions, carriers/lifts, focus or posture changes, stun, pending server placement, flight, conflicting movement departures, missing native calibration, unknown phase, an uncompleted ordinary entry clamp, changing geometry, unsupported or mutable region providers, other liquid kinds, zone lines, unknown regions, floor/support/contact and shortened/tiny physical moves. An interval outside zero to 250 ms also declines. Ordinary direction or pitch changes do not invalidate the clock: the existing API applies the changed input at its next full-frame update. A fallback does not retry the same request under a second mode or increase a movement budget. Ordinary jumps/falls keep their current controller.

The exact fixed-60 no-contact reference and a whole-millisecond 60 FPS reference are different. For forward input 1.7, start 0.1 below the surface at rest, 15 degrees up, no animation clear and ten matched seconds: fixed 60 gives about 91.508 forward units; the 17/17/16 ms trace gives about 92.435. The client model is about one percent lower in that comparison. A practical outcome band is reported separately; it never changes validation limits or licenses unexplained excess.

Validation separates synthetic/reference checks, the real local movement guard, installed-zone CPU cost, local visible behavior and attended public compatibility. Local visual checks remain a premerge gate. The first public P99 surface swim is attended.

## 6.666 — grouped ordinary airborne clock

[I, diagnostic] After two eligible surface requests (100 and 117 ms) at an
85-degree upward pitch, a 100 ms ordinary dry handback traveled 1.7815865
resolved 3D units versus 1.5383435447 for uninterrupted frame-clock swimming
(+0.243242955, about 15.81%). This is the unchanged ordinary airborne
controller's grouped-clock behavior. The handback itself is compared with
the direct ordinary path seeded with the exact same carried state; it does
not reset phase, add an update or alter the ordinary fall controller. This
diagnostic is distinct from the surface planner's no-gain bounds.
