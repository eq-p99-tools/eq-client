- Draw zone textures that come as DXT (BC) blocks, as HD texture packs ship
  them, straight from those blocks on GPUs that support BC, instead of decoding
  them to full colour first: a quarter of the memory with mipmaps (Highres 4X
  HD Qeynos: 645 to 161 MB). Smaller mip levels are built and encoded on load,
  and a zone's texture images are now built on all cores.
