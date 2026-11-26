- Keep one copy of a zone, character or item texture when its masked version
  would be identical (no visible texel matches the transparency key), instead
  of two. Nothing draws differently; with the Highres 4X HD pack this saves
  10 to 14% of texture memory in Qeynos, Neriak and Greater Faydark.
