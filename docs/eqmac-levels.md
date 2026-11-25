# EqMac character and displayed levels

The Mac program keeps character level at character +0x90 and spawn level at
spawn +0xad. Its level appearance handler (0x4df52a, type 1) changes a spawn's
level, including the player's own. It never changes character level.

The 0x9841 handler (0x4e06ef) only raises character level, then copies it to the
own spawn. Equal/lower updates change nothing. The experience handler
(0x4e4458) changes experience alone. A new profile replaces the character
level. That no live handler lowers character level is read; retaining the old
character gate after a loss until a profile reload is the resulting inference.

`ClientWorld::player().level` is character level. AA eligibility, spells,
skills, abilities, item usability, resource/speed/breath calculations and
script `player.level` observations continue to read it. The session's own AA
handler gates the request too; a locally queued request is never confirmation
that the server accepted or applied it.

`ClientWorld::displayed_level()` is the own spawn's shown level on EqMac.
Inventory/skin level labels (EQType 2), local `/who` and the map's own-player
details use it. Lifecycle reports retain `level` for character level and add
`displayed_level` explicitly. Other actors' appearance levels still update
their spawn records. Other client generations keep their existing projection.

On EqMac, a qualifying level rise adds five practice points once when the
packet's prior high-water level is lower than the new level. It is not five
per level crossed. A fresh profile replaces that balance. The networking
library retains its existing bounds rather than the program's byte cast for
out-of-range level fields, and diagnoses malformed records. The Mac level
effect/sound remains an inferred gap; this change adds no sound or chat line.
