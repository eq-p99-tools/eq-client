# Terrain materials

Terrain reads the sidedness flag in WLD material fragment `0x30` and the
texture frame sequence in `0x04`, in addition to the existing render method.
These are separate properties: water does not have one universal material
type. No zone or bitmap name selects a rendering rule.

A material with the two-sided flag clear culls its back face. Opposite-wound
faces at the same position remain in the mesh, so the authored upper and
lower surfaces can draw from their own sides without competing for the same
pixels. A material with the flag set still draws both sides. Collision faces
are unchanged.

Animated terrain textures keep their complete ordered frame references,
including repeats, and advance by the stored nonzero frame interval. A delayed
update advances one frame unless the asset enables skipping. Each
frame keeps its own color key and the material's existing alpha mode.
Different materials sharing the first bitmap can have different sequences
and timing. Animation belongs to the terrain entity and ends when that
entity leaves the scene.

The loader keeps the existing still texture when timing is absent or zero,
a frame is missing, a bitmap reference contains multiple image layers, or
the texture specifies a current frame that the pinned parser cannot retain.
It does not manufacture a rate or flatten image layers into animation.

## Evidence and limits

- Sidedness and frame layout are source-derived from libeq at
  `e0e5a4e65c170272035a8beb65bda048f7a60a01`,
  `material_def.rs` (`MaterialFlags::is_two_sided`) and
  `simple_sprite_def.rs` (`frame_references`, `sleep`).
- Program reads of the WLD material loaders at Titanium `EQGfx_Dx8.dll`
  `0x1001d810` and EQMac `eqgfx_dx8.dll` `0x1002fc90` confirm that bit0 selects
  the material's two-sided state.
- The texture loaders at Titanium `EQGfx_Dx8.dll` `0x1001a280` and EQMac
  `eqgfx_dx8.dll` `0x1002b4e0` read the skip permission from bit6. Their terrain
  material ticks at `0x10040270` and `0x100508d0`, respectively, require time
  strictly beyond the deadline. Without skip permission they advance once and
  schedule the next deadline from that update; the skip path catches up against
  the previous deadline. The code uses this raw flag rather than the pinned
  parser's differently named convenience bit.
- The interval's millisecond unit follows
  [LanternExtractor's BitmapInfo reader](https://github.com/LanternEQ/LanternExtractor/blob/bb4cdba64c1f0453568f3a11f158a38ee1659d0c/LanternExtractor/EQ/Wld/Fragments/BitmapInfo.cs).
  This is extractor-source evidence, not an official program timing read.
- Installed-zone inspection found opposite-wound coincident water faces
  with the two-sided flag clear, and multi-frame water textures, in South
  Qeynos, Qeynos Hills and Lake of Ill Omen. Asset data and visual comparisons
  remain private.
- Blend values and render-method mappings are retained. Unlisted methods
  keep the existing opaque fallback. Bevy 0.19.1's alpha-blend pass already
  tests depth without writing it and draws after the opaque world; opaque
  and masked materials retain their normal depth behavior. No depth offset
  or water-specific draw pass is added.

Exact official blend equations, equal-depth transparent draw ordering and
runtime animation start phase remain unverified. The cited timing reads cover
the two DX8 programs; Titanium DX9 timing and draw ordering remain unverified.
The change
does not claim full official rendering parity or alter movement policy.
