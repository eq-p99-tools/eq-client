# Language and command reference

[Help home](README.md)
[Examples](examples.md)
[Run controls](running.md)
[UI target reference](targets.md)
[Variables, conditions and loops](language.md)

## Language

One statement per line. Blank lines are ignored. `#` begins a comment, including after an action. Action commands are lowercase and their arguments use whitespace and command-specific names. Expressions in typed assignments and control flow accept quoted text. See [Variables, conditions and loops](language.md) for that syntax.

Durations are milliseconds. Headings use EQ's 0 to 512 units; positions and distances use EQ world units unless described as screen pixels. The first invalid line prevents the entire buffer from running.

## Your current session

Run a script from the workbench while logged in to act on your current character. Commands use your existing target and UI state. Login and character-selection commands are unnecessary for this workflow.

- `wait_zone shortname`: wait for admission to the named zone.
- `wait_dead milliseconds`: wait for the targeted NPC to die or leave.
- `wait milliseconds`: wait without sending input.

## Keys and UI

- `press keys`: press a key or chord for one frame, such as `I` or `CTRL+I`.
- `hold keys milliseconds`: hold a key or chord, then release it.
- `click target`: left-click a named control.
- `right_click target`: right-click a named control.
- `hover target`: rest the synthetic pointer over a control.
- `hold_click milliseconds target`: hold the left button on a control.
- `drag window x y`: move a named window to logical screen pixels.

Click targets include `slot 22`, `spell_gem 1`, `hot_button 1`, and `character 1`. They refer to controls, not arbitrary desktop coordinates. An invalid target produces a source-line error.

`click destroy` selects the whole cursor item or stack for destruction on P99
and Titanium EQEmu. It opens the ordinary confirmation; `click answer yes`
confirms that exact admission, inventory revision, item and quantity, while
`click answer no` cancels. Any inventory change cancels consent. This first
slice refuses populated containers, queued cursor items and unresolved inventory
predictions. A sent destruction remains a local prediction until received
inventory evidence arrives; scripts must not repeat it on an uncertain result.
Coin destruction has no script action.

`click drop` selects an eligible whole cursor item or stack in Inventory
on local-only Titanium EQEmu. It opens the ordinary drop confirmation.
`click answer yes` on that question submits the exact selection, while
`click answer no` cancels. The cursor stays held until a fresh received
inventory snapshot. Treat submission and any ground-object appearance as
separate from inventory confirmation.

Key names are case-insensitive. Combine them with `+`, such as `alt+1`. Supported names include letters, digits, `f1` to `f12`, `space`, `escape`, `enter`, `backspace`, `delete`, `tab`, `slash`, `up`, `down`, `left`, `right`, `shift`, `ctrl`, `alt`, `numlock`, `insert`, `home`, `end`, `pageup`, `pagedown`, `numpad0` to `numpad9`, `numpad_add`, `numpad_subtract`, `numpad_multiply`, `numpad_divide`, `numpad_decimal` and `numpad_enter`.

## Camera and movement

- `camera heading pitch [distance]`: position the camera; pitch is -89 to 0 degrees. Use `camera player offset pitch [distance]` for an offset from the player's heading.
- `face`: turn the camera toward the target.
- `face heading`: turn the character to an absolute heading at the ordinary turn rate, waiting up to 5 seconds for server confirmation.
- `move command milliseconds`: hold `forward`, `back`, `camera_forward`,
  `camera_back`, `strafe_left`, `strafe_right`, `turn_left` or `turn_right`,
  independent of key bindings. In mapped water on Titanium EQEmu, a negative
  camera pitch makes `camera_forward` dive and `camera_back` rise. Set pitch
  to zero for level movement.
- `approach range milliseconds`: approach the current target.
- `approach object kind range milliseconds`: approach a tradeskill container type.
- `walk range milliseconds`: follow a searched path toward the target.
- `walk object kind range milliseconds`: path toward a tradeskill container type.
- `chase range milliseconds`: follow the target until it dies, leaves or times out.
- `give`: queue an offer of the cursor item or coins to the current NPC or
  player target. The run stops at this line if the offer cannot be queued,
  including when the cursor is empty, the target is unsuitable, or an exchange
  is already pending or open. The script result carries the local refusal.
  The step completes when the local queue accepts the offer. The server's
  answer separately determines whether an exchange opens.
- `interact admission spawn revision` opens the service of the exact selected
  merchant, banker, guildmaster or corpse. Copy the `target_interaction` tuple from a fresh
  `report`. A stale target, a known reach or capability refusal, or another
  open or pending transaction stops the run. The bank opens locally. For
  merchants and corpses, check a subsequent economy report for the matching
  received shop or loot listing before using its controls.

Movement uses the client's ordinary movement and collision behavior. It is not a teleport. Walk uses a ready navigation mesh when `--navmesh` is enabled and falls back to grid search otherwise. A reachable destination and the session's movement grant are still required. Keep the camera attached when using movement scripts; free-camera mode consumes movement input for the camera.

Approach, walk and chase hold the camera-relative forward command, independent of key bindings. Object commands choose the nearest matching tradeskill container when they start. Missing targets, unreachable routes and timeouts stop the run. `wait_dead` only recognizes NPC corpses or a target leaving the zone; a player's death does not satisfy it while their corpse remains.

After walking through a server-driven pad, `wait_relocation 1500` waits for
a received same-zone movement instruction. Each observation is consumed once.
It distinguishes this instruction from ordinary position samples, and
the report labels its source `same_zone_relocation`. The instruction does not
identify which server trigger produced it.

Walk stall logs use `waiting` for an outstanding Move or crossing. For a
Move, the worker's local result releases that wait. Server placement is
reported separately.

## Diagnostics and commands

The projectile line from `report [label]` includes the current admission, the
number of received flight records since the last world reset, the flights still
tracked, the number of scene spawns queued, and the scene instances from the
latest completed projectile update, normally the preceding frame. Missing assets or unsupported flight
types can produce a receipt without a model. Scene instances may be off-camera
or occluded. The counters are independent of damage and inventory.

- `report [label]` logs a state summary with an optional label. Its `option_levels` field includes slider settings, with `run_boost` in hundredths of ordinary running speed. For a selected target, `considered` shows the decoded received color and `display_con_color` shows the presentation color. The report includes `selected_target_level` and `own_level` when known.
  `air_remaining` gives the local air reserve and capacity in ticks while
  Air Remaining is active.
  `breath_probe` reports the last sampled head medium, when resolved, and the
  local breath sample or its first unavailable input. The companion model
  fields give race, gender and raw size, and `breath_zone_scale` gives the
  received classic model scale flag.
  `MovementPending` and `MovementUnconfirmed` defer the due liquid check
  until the worker returns a usable position. After 250 ms the air gauge
  is hidden while the result is outstanding. `Effects` means a relevant spell
  or worn input could not be evaluated from the available mechanics. An
  affecting instrument modifier or unsupported scaling formula can withhold
  the meter.
  `pet_ownership` reports the current pet and owner IDs, zone admission ID and
  pet identity revision as JSON. It is null when the session has no resolved
  pet identity.
  The companion `Script economy report` event contains versioned JSON with
  the admission, merchant identity and catalog revision, up to 80 stock rows, the current
  selection and displayed single-unit quote, and up to 32 corpse rows.
  `merchant_opening_unconfirmed` identifies a locally queued shop opening
  still awaiting a matching answer. Wait for an answer or a new admission
  before attempting another scripted interaction. Check `loot.listed` for a
  received corpse listing.
  Total row counts reveal truncation. The report also shows the coins offered
  on each side of an open exchange. Check the ordinary report's item and
  purse fields for delivery and final balances.
  The `Script movement report` logs a small `movement_state` object. Its
  `session_position` is the working position used for collision (including
  local proposals); `rendered_position` is the drawn avatar;
  `last_server_placement` is the last admission or received placement in
  the current admission. None of these identifies a server acknowledgement
  for a particular proposal. `liquid_contact_at_session_position` is local
  geometry: `unknown`, `unmarked`, `wading`, `surface`, `submerged` or
  `lava`. `unmarked` means no liquid at the sampled points, and
  `submerged` is not a breath report.
  `mapped_liquid_at_session_origin` reports water or lava at the character
  origin. `mapped_liquid_surface_at_session_feet` reports a mapped liquid
  top at standing height in the feet's center column. Both fields use local
  geometry and are null when their query finds no mapped liquid.
  `local_geometry` shows the reported admission size when supplied, the loaded
  body's resolved size and feet offset, and whether collision is ready.
  `support_z_at_session_feet` is the EQ Z returned by the local movement
  support probe, or null when it finds no support or cannot run.
  `selected_target_los` is null when the ray cannot be evaluated. Otherwise it
  identifies the selected spawn and its incarnation revision and says whether
  solid client collision leaves the segment from the session origin to the
  spawn's reported origin clear. Server spell range, line of sight, and
  acceptance require separate evidence.
- `trace milliseconds`: log movement and frame timing for a bounded duration.
- `screenshot filename.png`: capture beside the script; the argument must be a filename.
- `sound filename.wav` or `sound filename.mp3` plays a named cue from the installed game assets. Use the basename with its extension.
- `inspect_link admission message ordinal` opens an item link using the
  `admission` and `(message, ordinal, label)` from the current `Script item
  links` report. Ordinals start at zero. The report lists up to 32 links
  from 16 recent received messages in the admission. The run stops if the
  link is unavailable or another inspection is pending. The `inspection`
  tuple gives the selected item ID and whether a received definition is
  available, including a definition already cached in the admission.
- `slash command [arguments]`: run one of the supported game slash commands, without a leading slash.
  `slash ranged` requests one shot or throw against the current target on Titanium.
  `slash layonhands` and `slash harmtouch` request the class ability against the
  settled selection. Select yourself before using Lay on Hands on yourself.
  `slash discipline evasive` and `slash discipline defensive` use the learned
  Warrior discipline on yourself, with the same reuse checks as its button.
  Watch the server's combat and inventory updates for the result.
  `slash pet target` selects your visible pet. Pet orders complete the script
  step when queued. Read subsequent server observations to check their effects.
  `slash camp desktop` uses the ordinary camp destination and current QoL exit choice.
  `slash stopsong` queues the Bard's stop request on Titanium EQEmu.
  The immediate disconnect aliases typed in chat are not script slash commands.
- `gm command [arguments]`: an allowed GM command on a network-constrained local test session only.
  On local EQEmu, `gm traindisc MAX [MIN]` requests all eligible disciplines
  in that inclusive level range for a disposable fixture. Each level is a number
  from 1 to 126 written with up to three decimal digits. Give MAX first. MIN
  defaults to 1. For example, `gm traindisc 52 52` requests only level 52.
  In GM mode, EQEmu can apply the command to the selected player. Check the
  received learned disciplines before using them.
  `gm stun 8000` requests an 8,000-millisecond server stun for a local fixture.
  Check received `own_stun` state; command submission alone does not establish the effect.
  On local EQEmu, use
  `gm spawn NAME RACE LEVEL TEXTURE HEALTH GENDER CLASS PRIMARY_MODEL SECONDARY_MODEL 0 BODY_TYPE`
  for a temporary NPC with explicit health and body type. Use a fresh alphabetic
  name, positive health, and decimal fields. The merchant ID is fixed at zero.
  The short `gm spawn NAME [RACE [LEVEL [TEXTURE]]]` form uses server defaults
  for the remaining fields.
  To remove a temporary NPC, confirm that its current incarnation is selected
  and run `gm depop` without arguments. The zone worker rechecks the same
  admission and target revision before sending. Keep that NPC selected until
  its removal is observed, since EQEmu acts on its current target and starts
  no respawn timer. A queued command does not establish creation or removal.
- `chat channel message`: chat on a network-constrained local test session only.
- `quit`: close the client.

The typed parser and normal client capability checks decide which commands and controls are available. A valid action can still fail at runtime when its target, session or expected result is unavailable.

- `compose_link admission message ordinal` inserts a recent received item link
  using the IDs from the item-link report. It edits the normal draft without
  changing keyboard focus. Use the usual chat input to edit or send the
  draft. An expired, mismatched or unknown link stops the script.

- `social set Name | /command | /pause 10,/command`, `social bind 1 Name`,
  `social play Name` and `social cancel` use the same social controls as
  chat on a local session. `social list`, `social show Name` and
  `social delete Name` also use the ordinary editor. Use the named step
  for scripts. Synthetic chat submission and hotbar activation refuse socials.
  A step queues the control request. Keep the script running with a wait if
  the social has more lines to dispatch. Stop, failure, completion, pause,
  or loss of script input ownership cancels its unsent lines. The controller's
  single-step action applies a named social control once. A single-stepped play
  processes its first line and retires the rest. A manually started social
  has independent ownership. Use a later `report [label]`
  to observe the social
  runner's current admission, definition ID, progress and remaining delay.
  The report describes dispatch and scheduling, independently of server results.

## Optional login automation

These commands support scripts that deliberately start before world entry. They are separate from the usual workflow of editing and running scripts while playing.

- `login`: use the configured environment credentials at the login screen.
- `wait_servers`: wait for the world-server list.
- `server name`: choose a listed server by its full name.
- `wait_select`: wait for character selection.
- `select name`: choose a character and enter the world.
- `create name race class gender deity start_zone primary_stat`: request character creation from selection where the server policy supports it, using numeric identifiers; the final stat receives the free stat points. `start_zone` is the city's Titanium choice number (1 Qeynos, 2 Halas, and so on); a generation that sends its own value for the same city, such as the Mac-era start-zone IDs, has it translated before sending. This command is not restricted to local servers by the script parser.
- `create_refused name race class gender deity start_zone primary_stat`: the same request where the server is expected to refuse it (a name already taken, for one); the step passes on the refusal and ends the script if the character is created.
- `delete NAME confirm NAME`: request deletion of the exact listed character,
  repeating its spelling and case. It uses the ordinary named confirmation/sender
  and never chooses a character by row position. The step waits for an explicit
  absence observation in the current connection's refreshed list. A refusal,
  unconfirmed outcome, session change or bounded timeout stops the script before
  following actions, including `quit`; it never retries or disconnects by itself.
  Ordinary Back / Disconnect remains available. Absence is not a server deletion
  acknowledgment or proof of database purge.
- `wait_online`: wait for a zone admission and player before continuing.

## Recovery consent reports

On a server that offers resurrection or translocate, including P99 for
supported offers, wait for
`consent_presented` to match the current offer before using
`click answer yes` or `click answer no`. These actions use the ordinary
confirmation buttons. `resurrection_consent` reports the offer token and
milliseconds remaining. `translocate_consent` also reports the spell and
offered zone IDs. Each token includes the admission, kind and sequence.

`consent_closure` reports the last token and worker disposition. An
`AnswerSent` records the submitted Yes or No. An `Expired` records a local
deadline and submitted decline. An `Interrupted` records a lifecycle
boundary. `consent_refusal` identifies the last refused answer in the current
admission. Compare tokens when reading these fields after a replacement or
zone admission. Read the ordinary position, zone, admission and state reports
for server movement or recovery.

## Character-deletion report

The `character_deletion` field in `Script report` and `Character deletion
observation` log events is JSON, version 1:

```json
{"version":1,"current_scope":{"connection_id":7,"selection_id":12},"update":{"request":{"scope":{"connection_id":7,"selection_id":11},"request_id":3,"name":"Disposable"},"progress":{"ObservedAbsent":{"scope":{"connection_id":7,"selection_id":12}}}}}
```

`current_scope` and `update` can be null. The request scope is the confirmed
original list. `progress` is the ordinary typed enum: `{"NotSent":"Reason"}`,
`{"Pending":null}` or `{"Pending":{"scope":{...},"presence":"Present"}}`,
`{"Unconfirmed":{"reason":"TimedOut","observation":null}}`
(with a scoped observation when available), or `{"ObservedAbsent":{"scope":{...}}}`.
Other unconfirmed reason labels are `ConnectionEnded` and `SendFailed`.
Local refusal labels are `Unsupported`, `StaleList`, `StaleConnection`, `NotListed`,
`Busy`, `Duplicate`, `NotAtSelection`, and `QueueUnavailable`.

Consumers must match the complete original request and require the observed scope
to equal `current_scope` on the same connection. An older absence remains a
historical snapshot. There is no deleted/success boolean, and prose is not a
machine contract. These reports may contain character identities; retain them as
private evidence.

## Character-lifecycle report

Opt-in `Script report` events add `character_lifecycle`, JSON version 1. It is a
bounded snapshot of existing model facts; it does not establish disposable
ownership or causal creation/deletion success. Reports and fixture identities
belong in private evidence.

| Field | Meaning |
| --- | --- |
| `complete`, `incomplete_reason`, `consumer_reliance`, `limits` | Completeness of this field; incomplete output forbids reliance. |
| `lifecycle` | Online-enabled, connected, ended, admission ID, has-player, in-world, dead and transfer-pending facts. The model does not retain active world scope or last Link; both are null. |
| `selection` | Separate selection-view scope/entering flag and full received `snapshot`: scope, all entries (slot, exact name, source level/zone ID), actual creation/deletion rules, policy capacity and free slots. |
| `creation_answer` | Last count/name/outcome, labeled `history_unscoped`; request ID and scope are null because the model provides no correlation. |
| `player` | Exact admitted name, spawn/admission IDs, level/race/gender/class and zone, labeled `current_admission` or `retained_admission`. |

Selection states are `selection_view`, `entering_view`, `cached_snapshot`,
`view_without_snapshot` and `absent`. View matching does not prove a currently
connected world socket. Nonterminal progress can retain an older list; terminal
loss/entry/camp clears it. Creation history can survive those resets and a new
world connection; restart clears it. Unknown source levels/zones stay null;
reported zero values stay zero. An absent creation offer has null capacity/free slots,
not zero capacity. Capacity comes only from the offered creation rules and is
not a promise of server acceptance. Empty admitted-zone text is unknown/null.

The field permits at most 16 entries, 63 ASCII bytes per nonempty name, 128 ASCII
zone bytes and 8,192 serialized JSON bytes. Control characters, ambiguous names
or slots, policy-capacity mismatch and exceeded bounds yield `complete:false`,
`consumer_reliance:"forbidden"`, a static reason and null data sections. No partial
list is labeled complete. Otherwise reliance still requires exclusive runner/log
binding and fresh-observation provenance; no account identity, transport generation
or creation-request correlation is invented from cached facts. The existing
`character_deletion` field and its observation-only contract remain separate.
