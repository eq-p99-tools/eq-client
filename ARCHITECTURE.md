# Client extension contracts

[Documentation index](docs/README.md) · [Development](docs/development.md) ·
[Server policy and evidence](SUPPORT.md)

## Named character deletion

`eq-network` owns the native codec, selected server capability, checked world
connection generation, current-list validation, single submission and list
observations. `CharacterSelection` and `world::CharacterList` require actual
`connection_id` and `deletion` fields. `ClientCommand::DeleteCharacter` and
`WorldEvent::CharacterDeletion` add exhaustive-match cases; core forwards the
latter as `world::Reply::CharacterDeletion` without optimistically changing names.
Do not fill missing scope with a fake connection ID. Offline preview fixtures
explicitly offer no deletion capability.

The renderer's one `character_delete::State` owns frozen named confirmations,
request IDs, queue submission and lifecycle presentation. Ordinary Yes/No and
`delete NAME confirm NAME` share it and the normal Outbox. New lists void unsent
confirmation, connection changes void old intents, and uncertain submissions
hold creation/entry with Back/Disconnect available. A result can establish only
presence/absence in its explicit received list scope, never causal deletion or
backend purge. Script reports serialize that typed evidence with its current
scope; they do not parse display wording or invent a success flag.

## Ownership

| Layer | Owns | Must not own |
| --- | --- | --- |
| `eq-client-axes` | EQ/file/render coordinate conversions | Protocol or gameplay policy |
| `eq-network` (external) | Wire codecs, connection lifecycle, server policy, command validation | Bevy UI or assets |
| `eq-client-core` | Reduced world state, calculations and renderer-independent rules | Socket I/O or widgets |
| `eq-client-assets` | Owned decoded installation data | Bevy entities or session commands |
| `eq-client-render` | Scene, UI, input, presentation and command intent | Raw packet encoding |
| `apps/eq-client` | Configuration, network-worker lifetime and event bridge | Duplicated server policy |

Keep packet decoding and server policy in `eq-network`. Put engine-independent
world reduction, validation and gameplay calculations in `eq-client-core`, with
narrow data/lookup interfaces where installed assets provide inputs.
`eq-client-assets` returns owned decoded data; it must not require Bevy. Rendering
owns entities, GPU assets, layout, input arbitration and presentation. The app
owns configuration, worker lifetime and the event bridge.

Session validation state and frontend presentation state serve different owners.
Share their rules and typed records rather than letting the UI mutate session
state directly. Route commands through `Outbox`; do not add feature-specific
network channels or raw opcode dispatch to widgets.

What a gameplay command needs is a capability the session offers (`outbox::offered`),
never a flag the client keeps per server. A control the server type lacks says
"Not available on this server", and one this client lacks says "Not in this
client yet". Official wording comes from the player's installed client by its
eqstr id, with this client's own words only as the fallback.
Capabilities authorize command paths; they are not a record of exhaustive live
validation. Keep those claims in SUPPORT and scenario evidence.

Local presentation needs no server capability. The map reads installed geometry
and `ClientWorld` directly, including spawns culled from the 3D scene. Its local
inspection selection is admission-scoped and distinct from the gameplay target;
hovering, pinning or tracking an entity must not send a command. The explicit
`map::target` button shares `target::select` and the existing outbox, and uses the
same visibility and zone-range checks as scene targeting. `map` owns projection and
geometry, `map::entities` owns markers and inspection, and `map::window` supplies
a standard frame when the installed skin has no usable map window. Search and
watch controls live in `map::controls`; `map::watch` owns profile persistence and
appearance alerts. Name matching, watch-file encoding and con-level estimates
are engine-independent in `eq-client-core::maps`. Watch identity survives spawn
ID changes; alert deduplication is scoped to each admission's spawn revisions.
Search claims the shared `keys::Typing` state before chat and gameplay input.
`map::motion` samples the displayed player pose after movement interpolation;
remote travel arrows use the existing dialect movement calculation and expire
when reports go stale. This presentation state never writes to `ClientWorld`.

Character creation is scoped to the world server's character list, before any
zone admission. `eq-network` offers `creation::Rules` on that list and owns name
approval, encoding, completion and timeout recovery. `eq-client-core::character_creation`
keeps the editable draft, point budget and dependent choices. The renderer's
`character_create` owns pages, focus and the pending presentation; only final
submission sends `CreateCharacter` through `Outbox`. Refusals retain the draft,
new lists invalidate it, and success selects the confirmed character without
entering. Creation claims `keys::Typing` before chat, Escape and gameplay input.

## Consider presentation

`eq-network` selects the presentation policy. Every current server policy
uses the received color. An optional classic NPC estimate falls back to that
color if either level is unavailable. The world retains the received color,
and the notice carries the reply. The chat line pairs the wire faction phrase
with a representative assessment for the displayed color. Unknown faction
and color keep their own fallbacks.

Unpatched Titanium resolves the NPC at `eqgame.exe` 0x45ec2a and reads the
wire faction at 0x49b9c0. It calculates its ordinary NPC category from
local and target levels at 0x4bc20b and converts that category to a chat
type at 0x491e30. The reply's color field does not enter that calculation.
The chat type reaches the output at 0x49be6f. The fixed palette at 0x4f2bbe
returns the following values. The renderer uses these as chat defaults.

| Color | Chat type | RGB | Phrase IDs at player levels 1–14 | Levels 15–24 | Levels 25+ |
| --- | --- | --- | --- | --- | --- |
| Green | 2 | `#008000` | 12226 | 12226 for delta below −5, otherwise 12232 | 12236 for delta below −10, otherwise 12237 |
| Blue | 4 | `#0040FF` | 12228 at level 7+ with delta at least −3, otherwise 12227 | 12233 for delta above −3, otherwise 12234 | 12235 |
| White | 10 | `#E0E0E0` | 12229 below level 8, otherwise 12230 | 12235 | 12231 |
| Red | 13 | `#F00000` | 12225 | 12225 | 12225 |
| Yellow | 15 | `#F0F000` | 12231 | 12231 | 12238 |
| Light blue | 18 | `#00F0F0` | 12227 | 12232 | 12232 |

The phrase branches at 0x49bc39–0x49be16 use target level minus player level
for delta. This table covers the zero dword at reply offset 24. The nonzero
branch at 0x49bbe8 selects IDs 12223, 12224 and 12225 for green, white and
red respectively. The renderer uses the fixed type 6 gray (`#808080`) for
a reported gray, which is outside the native ordinary NPC category map.
Wire white aliases normalize to the same displayed color and use chat
type 10. These program reads establish the unpatched Titanium path only.
Project 1999's patched thresholds and the renderer's representative phrases
still need comparison in the official clients.

## Ordered work and asynchronous outcomes

The renderer keeps its resource registration and frame graph in `systems`,
camera input and occlusion in `camera`, GPU conversion in `gpu_assets`, and
zone geometry/region adapters in `scene_collision`. Skin action bindings live
in `skinned::bindings`, recursive container construction in `skinned::tree`,
and world-to-label and gauge updates in
`skinned::presentation`. The crate root composes startup and the shared scene.
These modules keep their helpers internal rather than expanding the public API.

`camera` owns attached/free view state. Detaching consumes movement input for the
camera and clears character input, but does not suspend authoritative position
updates or gravity. Zone/session changes reset the view. `keys::bindings` owns
keyboard reassignment and per-character persistence; all input consumers use the
shared key map and typing gate.

`chat::links` recognizes validated HTTP(S) ranges and opens browsers in background
work. Item and web links share text-span hit testing before the shared click
tracker, so only the released link acts. Item definitions remain in the normal
session inspection path; web links never become game commands.

The script workbench is a client feature over the existing scenario executor.
`script::Program` owns validated source and line locations; `Script` owns the
active run; `RunStatus` retains progress and the terminal outcome after that run
is removed. CLI and UI launches share these types. Pause and single-step operate
at action boundaries; Stop releases synthetic input without committing a pending
click. The source snapshot never changes during a workbench run.

The opt-in `script::control` file bridge submits snapshots through the same
`Program`, `Script::for_viewer` and `RunControl` paths. It owns bounded request
IDs, receipts and status publication; the executor still owns action completion,
input release and terminal outcomes. An active run rejects replacement. Fixed
file exchanges run off the frame thread and keep their process lifetime across
game admissions and runner retirement. Local receipts never certify server
acceptance; shutdown status never certifies a clean camp.

`script_workbench::document` owns editable text, disk snapshots and bounded file
operations. File jobs run off the frame thread, use document generations rather
than game admissions, and preserve the buffer on failure. `view` owns chrome,
`syntax` owns presentation-only token colors for execution/help, and `help` renders the Markdown
pages in `docs/scripts` that GitHub also displays. Executable help examples are
tested with the real parser. The editable source uses `egui_code_editor` through
`bevy_egui`, rendered once into a texture in the native workbench. `editor::buffer`
enforces the source byte limit; `editor::input` maps pointer coordinates and
ordered keyboard events into its isolated context; `editor::surface` handles
size, scale and IME placement. Error markers use the editor's own line geometry
and `Program::diagnostics`, which shares the executor's parser. `input` adapts
the native path field's keyboard/clipboard events.
The workbench clears text focus before injected
input and uses the shared typing gate to keep editor input out of gameplay and
login. The existing local-session restrictions and pointer ownership still apply.

The frame stages run receive, scene, typing, routing, gameplay input, then
presentation. Text entry must consume keys before gameplay acts on them; scene
reconciliation must see the reduced world. Existing chains also order focus and
deferred entity changes. Preserve these edges when extracting feature systems.
Do not claim that removing a chain improves performance without measuring it.

Archive I/O and CPU decoding should run outside the frame consumer. Scope
session-related results to their admission, discard obsolete work, and apply the latest authoritative
placement when installing a scene. Keep graphics-resource mutation on the engine
thread. Cache owners need a documented lifetime or budget, including failed-load
entries; a process-wide map of strong handles is not an eviction policy.
Independent file/document jobs use their own generation or profile identity;
they should not be invalidated merely because the player zones.

An enqueued command is not a successful action. `Outbox` refuses what it cannot
send, such as a command while the queue is full; what it sends settles only when
the session reports the answer. The session holds what an operation uses until
the answer settles it, and where a server refuses with silence, as `EQEmu`
refuses a merchant offer, the hold ends after a timeout while a late answer
still settles the operation it answers and nothing else. A timeout never
authorizes resending a non-idempotent action. Admission reset invalidates
pending work, input prediction and asynchronous loads.

Keep the event callback bounded and fast. Do not block transport work on asset
loading or silently drop ordered inventory, cast and lifecycle events. Queue
overflow currently ends the app session explicitly; a future coalescing or
resynchronization design needs a coherent ordering boundary and tests first.

Use `Instant` for elapsed-time decisions and wall time for logs. Today the UI
reducer timestamps an update when it consumes it, which is not when the network
received it, so do not treat existing duration fields as network timings.

## Errors and compatibility

Use typed reasons for decisions the caller must make (queue busy, invalid
admission, missing resource inputs). Keep readable contextual errors at I/O and
process boundaries. Callers must not parse diagnostic strings to decide retries.
Unknown data should carry a reason when it affects behavior; ordinary absence
does not need an elaborate state machine.

Keep typed action enums and exhaustive internal matches. Public breaking changes
must have an explicit dependency migration and version policy; do not introduce a
generic action registry solely to reduce source lines. Networking's
[API contract](https://github.com/eq-p99-tools/eq-network/blob/main/API-CONTRACT.md)
says how its features, commands and their answers behave.

Regression tests should assert externally meaningful state: a delayed result
cannot affect a replacement item, queue recovery cannot lose movement grants,
formatted links keep their payload and UTF-8 ranges, and obsolete asset jobs
cannot install a scene. Keep live validation distinct from those synthetic proofs.
