# Language and command reference

[Help home](README.md)
[Examples](examples.md)
[Run controls](running.md)
[UI target reference](targets.md)

## Language

One action per line. Blank lines are ignored. `#` begins a comment, including after an action. Commands are lowercase. Arguments are separated by whitespace; there is no quoted-string or escape syntax. Multiword names are accepted only by the commands that explicitly take them. There are no variables, loops, branches or functions.

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
Coin destruction and ground dropping have no script action.

Key names are case-insensitive. Combine them with `+`, such as `alt+1`. Supported names include letters, digits, `f1` to `f12`, `space`, `escape`, `enter`, `backspace`, `delete`, `tab`, `slash`, `up`, `down`, `left`, `right`, `shift`, `ctrl`, `alt`, `numlock`, `insert`, `home`, `end`, `pageup`, `pagedown`, `numpad0` to `numpad9`, `numpad_add`, `numpad_subtract`, `numpad_multiply`, `numpad_divide`, `numpad_decimal` and `numpad_enter`.

## Camera and movement

- `camera heading pitch [distance]`: position the camera; pitch is -89 to 0 degrees. Use `camera player offset pitch [distance]` for an offset from the player's heading.
- `face`: turn the camera toward the target.
- `face heading`: turn the character to an absolute heading at the ordinary turn rate, waiting up to 5 seconds for server confirmation.
- `move command milliseconds`: hold `forward`, `back`, `strafe_left`, `strafe_right`, `turn_left` or `turn_right`, independent of key bindings.
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

Movement uses the client's ordinary movement and collision behavior. It is not a teleport. Walk uses a ready navigation mesh when `--navmesh` is enabled and falls back to grid search otherwise. A reachable destination and the session's movement grant are still required. Keep the camera attached when using movement scripts; free-camera mode consumes movement input for the camera.

Approach, walk and chase hold the camera-relative forward command, independent of key bindings. Object commands choose the nearest matching tradeskill container when they start. Missing targets, unreachable routes and timeouts stop the run. `wait_dead` only recognizes NPC corpses or a target leaving the zone; a player's death does not satisfy it while their corpse remains.

## Diagnostics and commands

- `report [label]`: log a state summary with an optional label.
- `trace milliseconds`: log movement and frame timing for a bounded duration.
- `screenshot filename.png`: capture beside the script; the argument must be a filename.
- `sound filename.wav`: play a sound from the installed game assets.
- `slash command [arguments]`: run one of the supported game slash commands, without a leading slash.
  `slash ranged` requests one shot or throw against the current target on Titanium.
  Watch the server's combat and inventory updates for the result.
  `slash camp desktop` uses the ordinary camp destination and current QoL exit choice.
  `slash stopsong` queues the Bard's stop request on Titanium EQEmu.
  The immediate disconnect aliases typed in chat are not script slash commands.
- `gm command [arguments]`: an allowed GM command on a network-constrained local test session only.
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
