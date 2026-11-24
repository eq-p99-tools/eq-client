# Zone lighting

How the client lights a zone without a sky (a palace, a dungeon or a cave)
on a Titanium installation. Outdoor zones still follow the time of day, as
the module notes of `daylight.rs` describe. The Mac-era client's engine
isn't read, so its zones are drawn as before.

The original draws zone geometry in its graphics engine, EQGraphicsDX9.dll.
eqgame.exe chooses the ambient light and hands it over. The engine then
lights each vertex in its effects.

## The path the original takes

The engine has two lighting paths. The eqclient.ini `[Defaults]` key
`MultiPassLighting` chooses between them; it is read at 0x1007fb50.

- By default the key is off. The constructor sets 0, and a missing key is
  written back as FALSE.
- The multi-pass target is built only when the option is on (0x1007bee0).

So the original draws zones with its **single-pass** effects. This client
follows that path. The multi-pass path isn't built here. The effect of the
separate `AccurateLighting` key isn't read.

## The ambient (eqgame.exe 0x441295)

The zone ambient is chosen by the zone header's time type, at header offset
424 ([0x923b88]):

| Time type | Ambient |
| --- | --- |
| 1, 2, 5 | The program's day table, one RGB row an hour from 12 AM, each hour's row moving toward the next by the minute |
| 4 | 0.5 grey |
| Any other | 0.1 grey |

- **The day table is read from the installed eqgame.exe** when a zone loads.
  Its three columns are at 0x78f708, 0x78f768 and 0x78f7c8. The client
  holds no copy of the values. An installation whose program doesn't hold
  the table there gives types 1, 2 and 5 the 0.1 grey.
- Which hour indexes the table is inferred: the client takes the hour the
  sky follows.
- The ambient is then raised, channel by channel, to a floor. The floor
  comes from the observer's vision (set by 0x4917dd): 0.08 grey, or
  65/50/50 (/255) with infravision, or 80/80/120 (/255) with ultravision.
- eqgame.exe hands the engine the ambient at scene vtable +0xa4 and the
  floor at +0xa8.

**Known permissive departure.** 0x441295 scales the floor by (1 - fade).
The fade follows the player's own carried light ([0x698a08]), 0.01 a call.
The light value's scale isn't read, so the floor applies unscaled here.
That is brighter than the original only while the player carries a light,
and only where the floor clears the offset below: with infravision or
ultravision.

## Each vertex (the single-pass RegionC1 effect)

Every frame, the engine's frame routine (0x10076f20) halves the ambient
(giving Amb) and the floor (giving Spec). The scene's third colour is meant
to be added to the floor first; its setter isn't read, so it counts as none.

Each zone vertex carries its own baked light from the zone file: one colour
per vertex of a terrain mesh (WLD 0x36), and for a placed object, one colour
per vertex of its model from the placement's colour track (0x15 → 0x33 →
0x32). Write V for the colour's RGB and A for its alpha. The vertex then
sums:

- **the ambient:** max(0, (Amb × A + Spec − 0.12) × 1.111);
- **its own light:** V, at full weight;
- **point lights 0 and 1:** each light's colour × min(0.05 R² / d², 2) ×
  N·L, with no cutoff. R is the light's radius and d its distance.
  0x100756e0 sends the 0.05 R² strength for ordinary meshes;
- **point light 2:** its colour × (N·L)², with no distance term.

The sum is saturated to 0-1 once. The texture is then multiplied by it
(texture stage 0 MODULATE), with no doubling.

In a no-sky zone the ambient term is usually 0: a 0.1 zone's
0.05 + 0.04 stays under the 0.12 offset. In a type-4 zone like the Erudin
palace, a vertex's alpha is what lets the 0.5 ambient in, and the palace's
vertices have none. So indoor walls are lit by their own baked light and up
to three lights.

The directional term (scaled by A) is left out, because a zone without a sky
has no scene light (inferred).

## Which lights a draw takes

[R] A draw takes the first three lights of its list, in list order
(0x100756e0). [I] How the engine builds those lists isn't read. Here, each
mesh's list holds the lights within their radius of the mesh's box, nearest
first:

- the zone's placed lights, with the colour scaled by the light's level;
- the light the player carries, and those of the eight nearest others who
  carry one (ShowDynamicLights is on by default, eqgame.exe 0x4a3341).
  Each takes the Titanium light type's colour, with its range as R.

A mesh's colours are rewritten only when the lights its draw takes change,
for example when a torch moves near it.

## How this client draws it

- The engine's sum is worked out per vertex on the CPU, as the engine's
  vertex shader does it. The result goes into the mesh's vertex colours on
  unlit materials, which keep the zone's distance fog.
- The engine multiplies in gamma space. This renderer multiplies linear
  values, so each colour is converted by the sRGB curve first. A vertex's
  own product stays within 0.06 of the engine's on the 0-1 display scale.
  Between vertices the blend differs slightly. A material that multiplies
  in gamma space would close both.
- The scene camera has no tone curve, as the original has none. Colour
  grading, such as the night's exposure outdoors, still applies.

## What is inferred

These meshes take the base light alone, with no point lights:

- doors;
- placed objects drawn apart (blended ones) without a colour track of their
  own.

So a door beside a lamp looks darker than the wall around it.

Further inferences:

- A placement or mesh without colours takes no light of its own, with the
  full ambient weight.
- Blended surfaces take the same colours as opaque ones. Additive surfaces
  (glows) are drawn as before. The engine's alpha path isn't read.
- Boats in a no-sky zone are still lit by the renderer's own ambient light,
  set from the same zone ambient.
