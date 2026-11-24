# Guild roster

Open **Guild** from the EQ menu. The list shows received names,
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
There are no note edits, polling or refresh request.
The current network API has no established Titanium roster-refresh operation;
opening the window sends nothing. A session without guild roster capability
reports that availability separately from a guilded session awaiting its list.

Select a named roster row to manage another member. Officers and the Guild
Master can request removal of a Member or Officer. Only the Guild Master can
offer a Member the Officer rank, or demote an Officer to Member. Promotion
requires that member to be in the zone and accept the existing guild invitation.
Unknown ranks remain readable but do not grant management authority. These
controls do not remove oneself, remove a Guild Master or transfer leadership.
The server remains responsible for its guild permission settings and acceptance.

Every action asks Yes or No with the exact canonical member name and proposed
change. No cancels without sending. The ticket binds the character, admission,
guild, actor rank and received membership revision: replacement, removal/rejoin,
rank changes, leaving the window or another admission withdraw stale intent.
Sending queues one existing command and leaves the roster unchanged. A later
received matching rank or removal establishes the displayed state; it is not a
request acknowledgement. A guild refusal is shown without claiming which
request it answered. After 30 seconds without a matching change the result is
unconfirmed, with no automatic retry. Closing the window cancels an unanswered
question but does not recall an already sent request.

Scripts click the same controls with `click button guild Member_Name`, where
`Name` is the roster's exact character name, then `click button guild Remove`,
`Promote` or `Demote` and the ordinary confirmation dialog's Yes or No. No
script path bypasses member identity, rank, capability or stale-ticket checks.

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
