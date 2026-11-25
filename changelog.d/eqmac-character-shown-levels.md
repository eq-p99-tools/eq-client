- Keep EqMac's own displayed spawn level separate from its character level.
  Inventory level labels, the local `/who` list and the map's own-player
  details follow the displayed level; AA, spell, skill and other character
  gates keep the profile level, raised only by an accepted level update.
  Equal/lower level messages do nothing, and a new profile replaces both.
  This preserves the Mac program's stale-high character level after a loss
  until profile reload (inferred from character +0x90 and spawn +0xad writes).
  The server still decides requests; no extra AA request or capability is added.
