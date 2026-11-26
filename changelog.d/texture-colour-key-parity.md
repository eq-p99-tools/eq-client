- Pick a zone texture's transparency key as the Titanium program does: only an
  8-bit BMP has one (palette entry 0, when it is magenta-like or index 0 fills
  a pixel pair at a corner), and every other texture, DDS included, masks by
  its own alpha. Pure black in a DDS no longer turns transparent on cut-out
  surfaces.
- Make a masked copy of a zone texture only when a masked or additive terrain
  or object draw shows it and its key clears a visible texel. With the
  Highres 4X HD pack that halves zone texture memory (Qeynos: 967 to about
  484 MB of base levels).
