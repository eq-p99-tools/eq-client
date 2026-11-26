- Draw DDS textures in S3D zones, objects and characters the right way up. WLD texture
  coordinates address a DDS file's rows bottom-up, so HD texture packs (which store
  their DDS under the stock BMP names, upside down against them) drew every texture
  flipped: torch flames hung down and signs read upside down. The flip is now done
  by content when a texture loads, including the compressed blocks, instead of by a
  `.dds` name for Luclin characters only. EQG textures are unchanged.
