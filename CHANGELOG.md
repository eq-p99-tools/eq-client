# Changelog

## Unreleased

- Offer no door where the server policy only receives door state: the HUD
  hint and F skip doors on such servers (TAKP) instead of offering a use the
  server refuses.
- Send a script's `create` starting city as the selection's generation sends
  it: the Titanium choice number named in the script is translated through the
  city's name in the generation's own list, so a Mac-era server receives its
  start-zone ID instead of a refusal.
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
