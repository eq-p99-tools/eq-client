# Changelog

## Unreleased

- Keep movement input calculation available for provably stat-neutral equipment
  histories without confirming their placement. Affecting or unknown equipment
  histories remain pending even after a local round trip. Report pending equipment
  as prediction uncertainty, and log typed movement/equipment inputs separately
  from the combined load display for script diagnostics. Unversioned partial
  updates cannot retire affecting history; non-neutral readiness remains
  conservative until a full baseline or the existing settlement policy.
- Add `--spots` to the selection proposal example for explicit full-document
  output preserving audit headers, comments and existing reviewed rows. Default
  output remains row snippets; every generated row stays unreviewed.
- Forward typed local loot results with their original admission, action and
  available creation time. Release only the matching refused opening or item
  request, show its reason once, and retain unlooted rows and authoritative
  holdings. Local submission continues to await separate server loot news.
- Keep close refusals informational: existing close commands have no creation
  time and cannot identify a newer window or pending item. Diagnostics remain
  log-only. `world::Reply` gains an exhaustive `LootRequest` variant for consumers
  of the matching network API.

- Offer no door where the server policy only receives door state: the HUD
  hint and F skip doors on such servers (TAKP) instead of offering a use the
  server refuses.
- Send a script's `create` starting city as the selection's generation sends
  it: the Titanium choice number named in the script is translated through the
  city's name in the generation's own list, so a Mac-era server receives its
  start-zone ID instead of a refusal.
- Select spell-provider semantics through the typed network accessor and bind
  actual attempt IDs from numeric status fields, without protocol-name checks
  or parsing display labels. Core's existing `spell_tables::Policy` name now
  reexports the network-owned provider policy.
- Preserve the released display-only status constructor. A Connecting status
  without a typed run ID leaves spell mechanics unavailable rather than
  inventing identity or aborting the connection. The unreleased exhaustive
  bridge `Fact` enum adds `RunUnavailable` for this case.
- Emit one chat notice when a provider first becomes unavailable. Repeated
  failures and reconnects in that failed lifetime do not spam notices; explicit
  provider replacement rearms the transition. Ordinary synchronization waits
  do not report failure.

- Correct character-select proposals against drawn floors, upward triangle
  winding, dry supports and camera limits; review zone placements while keeping
  unresolved cases explicit.
- **API change:** `select_spots::propose` takes separate drawn and upward-floor
  worlds. Add `propose_select_spot`, `Preview::selection`, `SelectionPreview` and
  `SelectionCapture` for offline selection audits.
- Apply P99 spell-table registration and zone replay to a separate installed
  spell provider, with immutable source snapshots, atomic batches and explicit
  readiness. Preserve destination text and keep other server policies isolated.
- Add `spells::ReplayProjection` and the producer-bound `spell_tables` bridge.
  **API change:** `WorldUpdate::SpellTable` adds an exhaustive enum variant and
  `eq_client_render::Session` now requires a `spell_provider` binding allocated
  with its producer. Route these facts before admission/loading filters; see
  [the provider contract](docs/spell-providers.md).
