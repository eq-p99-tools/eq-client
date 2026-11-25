# Movement parity

`[R]` identifies a static program read. `[I]` identifies an inference or
offline comparison. Offline results do not establish live compatibility.

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

[I] The separate upward headroom query retains the client's body dimensions
and authored facing; its shape remains an approximation. The collider and
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
