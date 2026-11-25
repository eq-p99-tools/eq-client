# Own-player jump presentation

Titanium's per-frame picker at `0x4868ee` selects the own-player pose from
horizontal and vertical speed and the actor's unscaled jump strength. It
does not start a separate jump animation event. Pose codes map through
`0x488128`, `0x693310` and `0x8ffbc8` (static read 157).

In native units per 50 ms step:

| Motion | Rise | Intermediate | Fall |
| --- | --- | --- | --- |
| Either horizontal component has magnitude >= 0.001 | L03 at vertical speed >= strength | ordinary walk/run | L05 below -3.2 |
| Both horizontal components are below 0.001 | L04 at vertical speed >= half strength | P01 | L05 below -2.25 |

The comparisons use the unscaled actor strength, even when a reduced jump
factor lowers launch speed. The local controller uses units per second,
so native speeds are multiplied by 20. The apex and landing return to an
ordinary pose; a short arc need not reach the falling threshold.

Velocity snapshots travel with each drawn movement segment. Reading the
latest simulated endpoint instead can skip the brief L03 interval when a
sample covers 100 ms or more. Refusal, correction and lifecycle resets
discard those snapshots with the position they described. This adds no
minimum animation hold and changes no outgoing action. Death, held
postures, gestures, water and carrier presentation retain precedence.
Missing airborne clips use the existing standing fallback.

## Remaining reads and observations

| Gap | Current evidence and next check |
| --- | --- |
| Own jump selector on EqMac | Titanium's rule is inferred for EqMac; read the own-player picker. |
| Own jump selector on RoF2 | Titanium's rule and unscaled strength are inferred for RoF2; read its picker and actor strength. |
| Remote running jump | The remote picker is unchanged and still treats horizontal reports as locomotion. The historical #680 observer result is separate from this own-player change; read and validate remote selection independently. |

The after-merge visual check is one standing and one running jump on each
of two races, plus a walk off a dry ledge. Code tests and installed clip
metadata do not establish that visual result or live server compatibility.
