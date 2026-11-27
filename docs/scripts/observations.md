# Script observation limits

[Command reference](reference.md)

## Chat input

The `chat_input` field in a labeled `Script report` reads the local UI state;
it does not change input routing. Its JSON value is null if the `Typing`
resource is absent. Otherwise it has:

- `composing`: the actual boolean chat-composition flag.
- `composition_window`: null when not composing or no active chat group is
  resolved; otherwise `{"kind":"main"}` or
  `{"kind":"detached","slot":N}`, where N is the local u8 group slot.
- `other_text_input_active`: a boolean covering quantity/login/map/spellbook
  text entry, spell-set naming, rebinding, script editing and character creation.

A report with composing=true, a resolved composition window and no other text
input active establishes the local chat composition owner at that report. It
does not establish native OS window focus, server receipt, or uninterrupted
ownership between reports. An Enter/Escape script step records an input request;
inspect a later report after normal UI processing to observe its result. No
draft contents, typed keys or clipboard data are included.

## Breathing during movement

`breath_probe` is the last air-counter evaluation, including its first
unavailable input; it is not a new position-stamped sampling API. `air_remaining`
is a local counter/gauge, not independent proof of a fresh head observation.
Neither value grants movement or proves a server movement acknowledgement.

The script driver runs in PreUpdate. World updates are received in Update;
breathing then samples before a new movement request can advance the worker.
Continuous follow can queue the next request before the next script report.
Thus a moving report can legitimately encounter `MovementPending` even though
the previous Update evaluated breathing. The worker may already have advanced
beyond the renderer's latest projection. Querying that older position again,
or retaining a prior Air sample, would not establish its current head medium.

`MovementPending` leaves the due breath check for a matching worker result.
After 250 ms, `MovementUnconfirmed` also hides the stale air gauge; it does not
release the request or manufacture a position. Only matching movement results
resolve the in-flight basis. The existing air reserve, damage policy and
admission/placement invalidation remain in force. A positive older gauge must
not override either unavailable reason.

For a bounded settled observation:

1. Use ordinary cancellation of follow/held movement and allow its matching
   worker result to be received. Script Stop alone does not establish that
   ordinary follow or an already queued worker request has stopped.
2. Take fresh reports after normal receive and breathing updates, within an
   explicit finite settling budget. Keep the actual current admission,
   lifecycle, supported body/scene and stable pose evidence. Pose stability
   alone does not establish request completion or head medium.
3. Require an actually resolved breath probe for that settled interval. Any
   pending, overdue, unknown or changed-admission state remains unavailable;
   an expired settling budget is inconclusive and follows the allocated
   cleanup path. Never replay an uncertain move or extend an allocated test
   merely to obtain a report.

This contract supports the narrower settled check. It leaves continuous
mid-motion breathing evidence unverified when the probe is unavailable. No
cached sample is relabeled current and no schedule or safety guard is relaxed.

Body wetness and head breathing are different observations. Movement reports
classify contact from the collision body at its feet; breathing uses the
native model head reference and resolved zone region. A wet body can have a
head in air. Camera position, water near the body, and missing region data do
not supply the head medium.

Unavailable reasons remain explicit:

- `OutsideAdmission`, `OutOfWorld`, `Gills`: no eligible counter evaluation.
- `MovementPending`, `MovementUnconfirmed`: worker position is unresolved.
- `SceneLoading`, `MissingHeadBasis`, `ModelChanged`, `ModelMismatch`,
  `MissingZoneScale`, `InvalidHeadOffset`, `UnresolvedMedium`: head/scene inputs
  cannot resolve the sampled medium.
- `MissingProfile`, `AlternateAdvancement`, `Equipment`, `Food`, `BuffSlots`,
  `Effects`, `UnslottedModifier`, `Capacity`: required submerged breathing
  inputs are unavailable. A known medium alone does not resolve these inputs.

No unavailable value is converted to Air. Admission changes discard the old
counter scope; placement changes break timing continuity without refilling
spent air. Reporting does not alter breath ticks, drowning/lava reports,
movement eligibility or native input cancellation.

## Melee positioning controller

`melee.state` reads `disabled`, `approaching`, `locked`, `suspended`, `released`
or `invalidated`. `melee.reason` distinguishes the local cause; deliberate
release differs from temporary input/context suspension and invalid-target
teardown. `melee.target_id` and `melee.admission_id` are known only while a scoped
identity is retained. The admission is a decimal text token, preserving its
full 64-bit identity. `melee.facing_owner` is `melee` or `ordinary`.
`melee.motion_proposed` says whether the last Input frame queued an ordinary
movement request for this controller. It does not establish transport, server
movement admission, reach or a successful swing.

These fields read the same controller for native and script-authored actions.
The script driver runs before the next Input frame, so its observation describes
the preceding Input frame. `report` includes the companion `melee_positioning`
object with the local radius and these identity/state fields. State/reason are
readable outside an admitted world when the local controller exists; absent
controller/identity stays unknown or stale. No script command acquires a privileged
orbit speed, selects a target or enables attack on behalf of this option.
