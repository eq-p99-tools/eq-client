# Changelog

## Unreleased

- Retain an early pet buff snapshot until the owned pet resolves. Discard it
  when the spawn is replaced or its ownership conflicts.
- Hold Titanium movement after a fresh placement below a nearby installed
  floor with neither a lower surface nor mapped liquid below. Preserve the
  received position and resume when a correction or collision change permits it.
- Include the admitted size, local body, floor support, and target sightline
  in script movement reports.
- Refresh the player's held models and paperdoll after a submitted Titanium
  equipment move when the item definition supplies a base model. Received
  wear and inventory corrections take precedence.
- Add the off-by-default Jesus Mode option. Walk on mapped water and lava
  reached from above at ordinary ground speed where gameplay departures
  are offered.
- Center the target name and draw it in bold where the face is available.
- Keep detached chat windows in their previous stack order when the HUD is
  rebuilt. The skinned dock stays bare when empty, and chat links remain
  clickable when their lines are nested within the viewport.
- Give spellbook search and set names the same input focus rule as other
  controls, including when a local script runs with the window unfocused.
- Apply received helmet dyes to numbered helmet textures while preserving the
  exposed face. Draw received one-shot animations at their playback rate.
- Fire a bow or throw a weapon once from the Titanium Actions window, a
  hotbar binding, `/ranged`, or the script step `slash ranged`. The client
  rechecks the target and equipment before sending and shows server-reported
  combat and inventory updates.
- Use `/stopsong` as a Bard on Titanium EQEmu. If a starting song has no
  matching cast update within one second of the stop, the client marks the
  stop unconfirmed and lets you try another gem. Song effects follow the
  server's updates.
- Stop spell-set apply with the actual reset reason and keep the panel's
  selection and draft through temporary interruptions. An unanswered book edit
  remains unconfirmed after a same-admission link interruption.
- Keep Titanium mana and endurance estimates available for confirmed effects
  whose mechanics cannot alter those capacities, including instrument-scaled
  effects in that category.
- Use `/pet target` to select your visible pet, including a charmed NPC.
  Pet orders use the current pet identity, and attack uses the current
  target selection. Ownership loss, death and spawn replacement clear stale
  pet details. `/pet guard me` sends its own Titanium command.
- Script reports include bounded merchant stock, selected quote, corpse listing
  and exchange coin observations for local transaction checks.
- Show Titanium EQEmu translocate consent and bind each resurrection or
  translocate answer to the question displayed. A held click cannot answer a
  replacement. Death, zoning and link loss dismiss the offer; an offer left
  unanswered is declined after the session's deadline. Script reports
  distinguish the current and displayed tokens from a worker answer, expiry,
  interruption or refusal.
- Scripts on a local EQEmu session can `gm spawn` a temporary NPC fixture
  with explicit fields and `gm depop` the selected one; `move` accepts
  `camera_forward` and `camera_back`.
- Wade and swim through mapped water in Titanium EQEmu sessions. Look up
  or down while swimming to surface or dive, and follow a supported bank
  back onto land. The bounded swim rate does not model swimming skill,
  breath, or drowning. Script reports separate local liquid contact,
  rendered position, and the last server placement.

- Report a local scripted `give` refusal through the script result once.
  Native click refusals still appear in chat.
- Draw server-reported rain and snow in zones with a sky. See the World
  Clearly hides precipitation locally. Try the offline sequence with
  `--demo-weather`.
- Save, rename, delete and apply named spell gem layouts from the spellbook.
  Sets are stored per character and world. Spellbook changes wait for their
  individual results: a memorize, forget, delete or swap made while the
  previous one is unanswered is refused. An interrupted set reports the last
  observed layout.
- Drag chat tabs to reorder them, open a separate chat window, or dock them into
  another tab strip. Closing a detached window returns its tabs to the main chat.
  Each character keeps the tab arrangement and window placements.
- Stop a scripted `give` at the failing line when the target or cursor is
  unsuitable, an exchange is in progress, or the local session queue refuses
  the offer. A queued offer still awaits the server's answer.

- Search learned spells in the spellbook by name. Results follow book-slot
  order and preserve the source spell and slot for inspection and memorizing.
  Clearing search restores normal browsing.

- Draw the larger local player arrow above other map markers and labels. Show
  travel direction with a line and colored tip, and limit ground item squares
  to the same range as nearby item models.

- Cover the chase camera's 89-degree pitch bound in both directions on the
  classic and long leash, including the view's heading and eye-centered zoom.

- Explain movement pauses in the System chat while the client applies movement
  options, once per wait in the current admission.
- Use networking policy for mouse-heading snapshots, movement fatigue, door
  cursor checks and note/book routing. Describe unresolved buff-cancellation
  requests in terms of the missing fade.
- Document the current server availability of buff cancellation, cursor-item
  destruction, merchant quantities, movement and book options, and list polls.
- Breaking API: removed the unused `qol::Kind::Unlocks` variant,
  `Fix::unlocks`, `Fix::unlocking` and `Settings::unlocked`.
- API: `ClientWorld::reading_window` returns the pinned networking's
  `books::ReadingWindow` for the open text; buff cancellation adopts
  `Progress::FadeObserved` and `Progress::Changed` from the pinned networking.
- Show the networking's player-admission failure on the login screen before
  the session ends, instead of a bare disconnect.

- The Quality of Life page's gameplay departures (Walk While Winded, Memorize
  Without Sitting, Run a Little Faster, Fly) are offered only where the session
  lists `Capability::GameplayDepartures`. Elsewhere they are greyed out and the
  session keeps the official client's movement and memorization whatever the
  saved settings say; movement input is never held waiting for a choice the
  server type refuses. The pinned networking lists the capability on every
  checked server type while the departures are under test (2026-11-06).
- API: `movement::offered_options` is the movement choice a session takes;
  `qol::Fix::needs` names `Capability::GameplayDepartures` for the four
  departures.
- Quality of Life page: See the World Clearly, Walk While Winded, Loot Corpses
  Automatically, Floating Combat Text, Read the Spellbook Standing, Memorize
  Without Sitting, Show Invisible Entities, Answer List Presence Polls, and the
  unsafe Run a Little Faster, Fly and Exit at Once, all off by default; the
  Particles page's Show Spell Effects. First-person view at minimum chase zoom
  and head-focused zoom; chase pitch to 89°. Titanium `[KeyMaps]` movement and
  posture bindings are read from the installation. Simultaneous forward/back
  and strafe ground movement, mouse-look steering with immediate Titanium
  turns, door double-click, cursor-item destruction with confirmation, the
  merchant quantity picker, right-click beneficial-buff cancellation,
  `/exit` and `/quit` aliases, memorized spell-gem artwork, and settled
  animation transitions.
- Resize explicitly sizable installed chat screens by their edges and corners,
  retaining widgets, anchored content, clipping and full size across rebuilds
  and restarts. Fixed Inventory and Spellbook remain fixed. The 160×100 logical
  chat minimum is client usability policy; native numeric limits remain unknown.
  Window-layout v2 retains sizes and reads legacy position-only records.
- API: `sidl::Screen::sizable` preserves `Sizable::{Omitted, Invalid, Fixed,
  Enabled}`. Screen struct literals must supply the authored declaration.
- Return to login with a specific retry message when the server completes a
  zone handshake but the client cannot read the character data. This also
  applies when entering a transfer destination.
- Distinguish a received camp logout reply from the normal local close deadline.
  Both preserve the requested selection or desktop destination. Retain the
  departing admission and evidence in lifecycle reports; a deadline alone does
  not confirm server logout or cleanup.

- Keep explicit background scripts running while their window is focused or
  passively observed with the pointer. Genuine keyboard, button and wheel input
  still takes ownership; Stop and scripted-click target checks remain available.

- Follow admission-scoped server target changes and clears without echoing
  selection packets. Preserve pending local choices and refuse ambiguous ability
  recipients; unknown IDs and replaced spawns remain unavailable until resolved.

- Read P99 notes and books through the selected worker inventory revision and
  enqueue time. Refuse outstanding item projections or stale holdings before
  submission. Only supported server text opens a note/book window; death,
  transfer and disconnect clear it, and suspended arrivals cannot reopen it.
  Preserve EQMac's nonzero book-kind behavior separately from Titanium.

- Separate carried Combine controls from world-station availability using
  `Capability::WorldContainers`. P99's carried offer enables no station hint,
  open/close/world-combine or station script approach. Combine waits for queued
  item-move results and keeps products/consumption separate from its reset.
  Additive `ground::nearest_usable_for` selects only offered object kinds.

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

- Start every generation's bindings from its program's own key table
  (`eq_client_assets::keys::EQMAC` added beside `TITANIUM`; Titanium gains
  Spellbook Ctrl+B, Sit/Stand Ctrl+S, Walk/Run Ctrl+R and Map Backspace/M).
  Native commands this client does not support are `Command::Reserved` and
  stay unbound; target previous, strafe, loot, trade, free camera and hotbar
  key editing start unbound. Saved character bindings keep precedence.
- Bind group invitations to their consent identity: the invitation shown is the
  one `/follow` accepts and `/decline` (or `/disband` while one is pending)
  declines, and a formed group names its leader. API:
  `ClientWorld::group_invitation()`, `group_invitation_id()`.
- Keep the session's own-player stun state out of movement and camping:
  API `ClientWorld::stun_state()`, `is_stunned()`, `stun_revision()` and the
  `eq_client_core::control` re-export.
- Add ten retained hotbar pages (`eq_client_core::hotbar::{Pages, PAGES}`);
  the shortcut hold that carries a button lasts 750 ms (was 500 ms).
- Preserve complete skin border geometry: `sidl::Border` carries the four
  side-cap pieces (`right_top`, `right_bottom`, `left_top`, `left_bottom`).
- Show unanswered merchant outcomes: `Notice::MerchantUnanswered` reports a
  purchase or sale with no result, and a `PurseUncertain` event leaves the
  purse unknown until the server supplies it. Classic merchant catalog cells
  bind by their `EQTypes` 6000..6079.
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
- Offer the door hint and the Use key (U by default) for supported ordinary
  EQMac doors with a current empty cursor, generation-specific reach and
  settled hinge. Animate received actions and native local return timing;
  sending a click does not open the door. Specialized, static and unknown
  mechanisms stay visible without use. Additive `doors::DoorTable` APIs: `apply_for`, `supports_use`,
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
