# Changelog

## Unreleased

- Follow the network-owned spell-table requirement for provider lifecycle and
  initial readiness instead of matching provider policy names in the host.
- Post automatic-food preferences only to admissions offering `Capability::Meals`,
  keeping the saved choice on servers without meal support. Inventory support
  alone no longer enables the meal option or sends an unsupported zone-in update.
- Add explicit `--script-background` input ownership and a non-activating
  initial window request. Returning to the client cancels held script input;
  default launches, local-only commands and gameplay guards retain their rules.
  API: `script::InputMode`, `Script::input_mode`, `Script::for_viewer`,
  `ViewerConfig::script_input_mode` and `Phase::WaitingForInput`.
- Add opt-in `--script-control` local file requests for explicit scenario starts,
  targeted Stop and retained status, including recovery after terminal errors.
  Runs share the workbench executor and limits; active runs reject replacement.
  API: `ViewerConfig::script_control` and `Script::for_viewer`.
- Track admission-scoped death-bind knowledge separately from death-packet
  details. Show the existing death notice and action/pose lifecycle without
  manufacturing a corpse; keep duplicate notifications quiet and clear state
  on the next admission.

- Start fresh Mac-era bindings with the requested Q attack, C consider and
  Z duck keys, with arrow-key turning and no duplicate default chords. Saved
  character bindings keep precedence. Q/Z are requested defaults; complete
  original key-table/settings parity remains partially read.
- Right-click world characters through the existing interaction paths,
  considering non-interactable or known out-of-range NPCs without changing
  the current target. Keep the existing capability and reach checks.
- Offer a quantity picker on a plain stack click with an empty cursor.
  Shift-click takes the whole stack and Ctrl-click takes one; these controls
  are requested UX, with official modifier parity still unverified. Cursor
  placement, single items and plain merchant selection keep their behavior.
- Cover right-click note/book requests and visible outbox refusals without
  opening a reading window before server text arrives. The shared outbox
  remains the sole owner of posting-refusal notices. Render classic book
  backticks as line breaks in notes and paginated books.
- **Breaking:** renderer `Session` now supplies `inventory_move_rules` from the
  network producer. Configure `ClientWorld::plan_inventory_as` before queuing
  moves; missing policy refuses planning. `Reply::InventoryMove` adds
  `present_error` while preserving typed dependent refusal errors.
- Compute movement weight, equipment and food from one ordered worker replica
  (`worker_inventory`), keeping queued display/planning projections separate.
  Same-admission resets retain pending result identities for late submissions;
  new admissions and terminal resets discard them. Dependent refusals drain
  without repeating the root chat notice.
- Supply the network-owned optional `EqMacFatigueInputs` from the same proven
  worker inventory. Signed strength stays uncapped until the session applies
  fatigue; known stamina is capped to 1..255. Keep a stable conservative scalar
  fallback so fatigue-only ticks leave the exact posted report unchanged, and
  display current capacity through the network formula. Missing prerequisites
  stay unavailable; Titanium capacity and zero-sentinel behavior are unchanged.
- Offer the door hint and F key for supported ordinary EQMac doors with a
  current empty cursor, generation-specific reach and settled hinge. Animate
  received actions and native local return timing; sending a click does not
  open the door. Specialized, static and unknown mechanisms stay visible
  without use. Additive `doors::DoorTable` APIs: `apply_for`, `supports_use`,
  `use_distance` and `hinge_fraction`; re-export `doors::eqmac`.
- Keep autorun during a continuously positive speed recalibration. Actual
  withdrawal, admission and unsafe readiness resets still clear it.
- Add a bounded, versioned character-lifecycle JSON field to opt-in script
  reports: complete offered lists and policies, uncorrelated creation history,
  and current versus retained admitted identity. Preserve unknown scope and
  source values; malformed/oversized output explicitly forbids consumer reliance.

- Add ordinary named character deletion with an explicit Yes/No confirmation,
  safe Back/Disconnect while uncertain, and shared `delete NAME confirm NAME`
  scripting. Reports distinguish pending, unconfirmed, local refusal and absence
  observed in a specific refreshed list; absence never claims permanent deletion.
  Breaking API migration: character lists/events require connection generation
  and deletion capability; commands/events/replies add typed deletion variants.
  Keep actual session/list scope, including across reconnects and late replies.

- Apply item moves to a local projection and send them in order without
  waiting for the worker between clicks; scribing waits for queued moves. A
  refusal rolls the projection back to the server's state and refuses the
  moves queued behind it. API: `ClientWorld::inventory()` returns the projection
  and is no longer `const`; new `queue_inventory_move` and
  `inventory_moves_pending`; `InventoryState::settled()` removed; `click_slot`
  returns `bool` and takes `&mut OnlineState`.
- Show the Skills window by the generation's own list, rank and value rules
  from per-generation tables shipped as data (numbers only), list languages,
  and name language zero from the received selector; the Mac-era selector is
  unread, so its row is withheld while its received proficiency is retained.
  API: new `eq_client_assets::skills`,
  `InstalledClient::skill_rules()`, `ClientWorld::language_zero_selector()`;
  `skills_revision` also advances on level and language changes.
- Honour authored material sidedness for placed objects, mirroring one-sided
  materials of reflected placements. API: `ZoneModel` gains a public
  `materials: Vec<ObjectMaterial>` field; new `ObjectMaterial`.
- Select an available helmet texture family instead of treating every head
  material as the face. API: new `outfit::material()`; `dressed()` swaps only
  the `00` head materials.
- Start a script's menu-entry wait when the opener's click is released, not when
  the pointer wait begins.
- Drop held network events when a session restarts, so an old session's events
  cannot enter its replacement. `select_spots::BUILT_IN` is now public.
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
- Preserve the released display-only status constructor. For the P99 provider,
  a Connecting status without a typed run ID leaves spell mechanics unavailable
  rather than inventing identity or aborting the connection. The unreleased
  exhaustive bridge `Fact` enum adds `RunUnavailable` for this case.
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
