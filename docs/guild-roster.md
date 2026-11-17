# Guild roster

Open **Guild** from the EQ menu. The read-only list shows received names,
levels, classes, ranks, online status, zones and public notes. The installed
`EQUI_GuildManagementWnd.xml` supplies its frame, list and scrollbar when
available; otherwise the client supplies a fallback. Movement, minimization,
Escape, stacking and per-character placement use the shared window behavior.

The display follows the server's full member list and later deltas. No list
received means unknown membership, with a waiting message. A received empty
list means zero members. Only a received location establishes Online or Offline;
silence never changes status. Status describes the last received roster, rather
than a fresh presence check. Numeric ranks are displayed as received, including
the server's rank mapping. Unknown class/rank numbers remain visible. A newly
added member's note remains Unknown until a note update or full list supplies it.

The last received list survives a session-reported zone or bind transfer only
when the next admission matches the destination, exact character name and
guild in the same known world. Location/status remain last-received values;
the transfer does not refresh them. Membership changes, disband, removal of the
player, world changes, character selection, terminal or unoffered connection
loss, camping and other admissions discard roster evidence. Death retains it.
There are no moderation controls, note edits, polling or refresh request.
The current network API has no established Titanium roster-refresh operation;
opening the window sends nothing. A session without guild roster capability
reports that availability separately from a guilded session awaiting its list.

EQEmu supplies the full list on an ordinary fresh character login and guild
join. Its initial-login marker is server lifecycle state, separate from the
native window's local offline-member filter. Ordinary zoning can rely on the
retained list and received member changes rather than resending the full list.

The deterministic synthetic text snapshot and UI/lifecycle regressions are in
`guild/roster/tests.rs` and `world/tests/guild_roster.rs`. Run them through the
project's allocated build launcher; synthetic checks establish local projection
and lifecycle behavior, not server compatibility. Live admission delivery,
skin visuals, wheel scrolling and placement persistence still require runtime
checks on the exact integrated build.
