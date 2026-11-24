# Language and command reference

[Help home](README.md)
[Examples](examples.md)
[Run controls](running.md)
[UI target reference](targets.md)
[Variables, conditions and loops](language.md)

## Language

One statement per line. Blank lines are ignored. `#` begins a comment, including after an action. Every action is a call with literal arguments separated by commas, such as `hold("CTRL+I", 500)`: quote text and leave numbers bare. The forms below name each argument; a quoted word must be written as shown, square brackets mark an optional argument, and `target...` stands for the words of a [UI target](targets.md), each quoted. Commands that carry free text, such as `report`, `slash` and `gm`, take one quoted string. See [Variables, conditions and loops](language.md) for variables, blocks and the rest of the syntax.

Durations are milliseconds. Headings use EQ's 0 to 512 units; positions and distances use EQ world units unless described as screen pixels. The first invalid line prevents the entire buffer from running.

## Boat acquire/release

`boat("acquire")` and `boat("release")` use the same deferred actions as `/boat acquire`
and `/boat release`. EQEmu offers this first slice for a supported race-141
vessel under the player's current accepted floor link. There is no steering
command. P99 and EQMac are unverified gaps. Script step completion means input
submission; inspect `report([label])`'s movement `boat_control` for correlated state and
`boat_control_notice` for the separate raw received packet. An unresolved acquisition needs an observed release echo before a same-ID
retry; a completed ordered release permits ordinary reacquisition. Walk-off, camp and admission changes withdraw local control. An unanswered
floor-link withdrawal is retried until the worker reports changed state; queue
acceptance alone does not settle it.

## Alternate advancement

These three steps are this client's own words, not the official client's. The official `/alternateadv` subwords haven't been read yet (R49-40b), so no slash form is offered for them.

- `train_aa(rank_id)`: asks to buy the AA rank with this definition id (on the Mac-era client, the Mac AA id from 1 to 226). It spends points, which nothing gives back.
- `activate_aa(rank_id)`: asks to use an owned rank's ability. It takes no target.
- `aa_split(percent)`: asks to send this share of experience, from 0 (off) to 100, to AAs.

Each step posts one request for the current admission and moves on. It never waits for the server and never tries again. The client checks nothing about the rank, the points or the level: the session alone decides, and when it refuses, the reason is said in chat. Only a local EQEmu or local TAKP session takes them until the AA live rows pass (checklist 6.581). Every other server type refuses them as not offered, and on the Mac-era client only `aa_split` goes for now. The steps need a character in the world.

`report([label])` also logs a "Script alternate advancement report" line with an `alternate_advancement` object (checklist 6.582). It holds numbers only, as the server sent them:

- `titanium`: the count of `definitions`; the `selections` ids; the `trainable` ranks as `{id, cost}`; the `activatable` rank ids; the profile's `points`; the latest `progress`; `unspent`; and the `timers` as `{ability, begin, end}`.
- `mac`: the `ranks` from `OP_RespondAA` as `{id, value}` with `spent`; the profile's `selections`; `points`; `progress`; and `timers`.

A fact not yet received is `null`. Each list is `{count, listed}`, with at most 64 entries listed. Trainable and activatable are for reading only: the session judges every request again.

## Your current session

Run a script from the workbench while logged in to act on your current character. Commands use your existing target and UI state. Login and character-selection commands are unnecessary for this workflow.

- `wait_zone(shortname)`: wait for admission to the named zone.
- `wait_dead(milliseconds)`: wait for the targeted NPC to die or leave.
- `wait(milliseconds)`: wait without sending input.

## Keys and UI

- `press(keys)`: press a key or chord for one frame, such as `I` or `CTRL+I`.
- `hold(keys, milliseconds)`: hold a key or chord, then release it.
- `click(target...)`: left-click a named control.
- `right_click(target...)`: right-click a named control.
- `click_with(keys, target...)` and `right_click_with(keys, target...)`: click a named control with modifier keys held, as a person holds them. `keys` is `"shift"`, `"ctrl"`, `"alt"` or a chord of them such as `"ctrl+shift"`. The keys go down with the button and come up the frame after it, so each window reads them as it reads yours. For example:
  - `right_click_with("alt", "slot", 22)` uses the item's click effect;
  - `click_with("shift", "slot", 23)` moves the whole stack, and `click_with("ctrl", "slot", 23)` moves one;
  - `click_with("shift", "coins", "purse", "platinum")` takes all of that coin;
  - `right_click_with("ctrl", "slot", 22)` puts the item's link in the chat line.

  A run that stops mid-click lets the keys go.
- `hover(target...)`: rest the synthetic pointer over a control.
- `click("spawn", admission, id, revision)`, `click("object", admission, drop_id)` and `click("door", admission, id)`, with `right_click` and `click_with` for a spawn: click a thing in the world, by the ids a fresh `report()` gives. The click obeys what is on screen: it acts only where the scene's own pick names that thing right now, so a thing a person couldn't click from here refuses the step and nothing is sent. A door takes a person's two clicks. See [World](targets.md#world).
- `hold_click(milliseconds, target...)`: hold the left button on a control.
- `drag(window, x, y)`: move a named window to logical screen pixels.
- `wheel("world", lines)`: turn the wheel over visible world space, from -64 to
  64 lines; positive zooms inward, negative outward.
- `look("world", dx, dy, milliseconds)`: hold right over visible world space and
  spread this relative mouse displacement over 1 to 10000 milliseconds, then
  release after the final input frame. Each axis is limited to +/-4096 physical
  pixels. `look("world", 0, 0, 500)` holds the view still for half a second.

World gestures require `--script-background` and an attached camera with a
player present. The drawn script pointer chooses an uncovered world point;
the step refuses when no sampled point is clear. Wheel Zoom, Mouse Sensitivity,
Invert Y, attached camera limits and Look Spring retain their ordinary effects.
The wheel uses the normal chase/first-person transition; `camera` sets a chase
frame directly and therefore cannot test that transition. The look gesture
uses the right-button look ownership and ordinary character/camera turn path.
It never presses the OS mouse button or moves the physical cursor.

Stop, failed scope checks and session/zone changes release world gestures.
Pause finishes the bounded gesture before pausing. Genuine keyboard, button or
wheel input cancels an active gesture; mouse motion does so while the client
has focus. Desktop motion while another app has focus is ignored. A gesture
binds the admission it starts in even when the caller omits `expected_world`;
controllers should still supply the current `expected_world` for the whole run.
Script gesture results establish semantic input behavior; physical mouse
hit-testing, capture and feel need their own attended observation.

Click targets include `click("slot", 22)`, `click("spell_gem", 1)`, `click("hot_button", 1)` and `click("character", 1)`. They refer to controls, not arbitrary desktop coordinates. An invalid target produces a source-line error.

`click("destroy")` selects the whole cursor item or stack for destruction on P99
and Titanium EQEmu. It opens the ordinary confirmation; `click("answer", "yes")`
confirms that exact admission, inventory revision, item and quantity, while
`click("answer", "no")` cancels. Any inventory change cancels consent. This first
slice refuses populated containers, queued cursor items and unresolved inventory
predictions. A sent destruction remains a local prediction until received
inventory evidence arrives; scripts must not repeat it on an uncertain result.
Coin destruction has no script action.

`click("drop")` selects an eligible whole cursor item or stack in Inventory
on local-only Titanium EQEmu. It opens the ordinary drop confirmation.
`click("answer", "yes")` on that question submits the exact selection, while
`click("answer", "no")` cancels. The cursor stays held until a fresh received
inventory snapshot. Treat submission and any ground-object appearance as
separate from inventory confirmation.

The map's controls are targets too (see [Map controls](targets.md#map-controls)). Watches and landmarks are saved per character, and the map's presentation beside the window placements, so a script's clicks on them change saved settings, as a person's do. The script does not put them back.

Key names are case-insensitive. Combine them with `+`, such as `alt+1`. Supported names include letters, digits, `f1` to `f12`, `space`, `escape`, `enter`, `backspace`, `delete`, `tab`, `slash`, `up`, `down`, `left`, `right`, `shift`, `ctrl`, `alt`, `numlock`, `insert`, `home`, `end`, `pageup`, `pagedown`, `numpad0` to `numpad9`, `numpad_add`, `numpad_subtract`, `numpad_multiply`, `numpad_divide`, `numpad_decimal` and `numpad_enter`.

## Camera and movement

- `camera(heading, pitch[, distance])`: position the camera; pitch is -89 (straight down) to 89 (straight up) degrees, which the chase view holds within its own range: with the Free Chase Tilt setting off it stays between eye level and 30 units above the eyes, so it never looks up. Use `camera("player", offset, pitch[, distance])` for an offset from the player's heading.
- `face()`: turn the camera toward the target.
- `face(heading)`: turn the character to an absolute heading at the ordinary turn rate, waiting up to 5 seconds for server confirmation.
- `move(command, milliseconds)`: hold `forward`, `back`, `camera_forward`,
  `camera_back`, `strafe_left`, `strafe_right`, `turn_left` or `turn_right`,
  independent of key bindings. In mapped water on Titanium, strokes follow
  the character's look pitch, not the camera's: hold the Look Up or Look Down
  key (`PageUp`, `PageDown`, such as `hold("PageUp", 500)`) to tilt it, and
  Center View in first person levels it. Holding `space` strokes up. The `camera` command's
  pitch moves only the camera.
- `approach(range, milliseconds)`: approach the current target.
- `approach("object", kind, range, milliseconds)`: approach a tradeskill container type.
- `walk(range, milliseconds)`: follow a searched path toward the target.
- `walk("object", kind, range, milliseconds)`: path toward a tradeskill container type.
- `chase(range, milliseconds)`: follow the target until it dies, leaves or times out.
- `give()`: queue an offer of the cursor item or coins to the current NPC or
  player target. The run stops at this line if the offer cannot be queued,
  including when the cursor is empty, the target is unsuitable, or an exchange
  is already pending or open. The script result carries the local refusal.
  The step completes when the local queue accepts the offer. The server's
  answer separately determines whether an exchange opens.
- `interact(admission, spawn, revision)` opens the service of the exact selected
  merchant, banker, guildmaster or corpse. Copy the `target_interaction` tuple from a fresh
  `report`. A stale target, a known reach or capability refusal, or another
  open or pending transaction stops the run. The bank opens locally. For
  merchants and corpses, check a subsequent economy report for the matching
  received shop or loot listing before using its controls.

Movement uses the client's ordinary movement and collision behavior. It is not a teleport. Walk uses the zone's navigation mesh once it is ready, which is the default. Without one, or with `--no-navmesh`, it falls back to grid search and the Execution tab and log say why. A reachable destination and the session's movement grant are still required. Keep the camera attached when using movement scripts; free-camera mode consumes movement input for the camera.

Approach, walk and chase hold the camera-relative forward command, independent of key bindings. Object commands choose the nearest matching tradeskill container when they start. Missing targets, unreachable routes and timeouts stop the run. `wait_dead` only recognizes NPC corpses or a target leaving the zone; a player's death does not satisfy it while their corpse remains.

After walking through a server-driven pad, `wait_relocation(1500)` waits for
a received same-zone movement instruction. Each observation is consumed once.
It distinguishes this instruction from ordinary position samples, and
the report labels its source `same_zone_relocation`. The instruction does not
identify which server trigger produced it.

Walk stall logs use `waiting` for an outstanding Move or crossing. For a
Move, the worker's local result releases that wait. Server placement is
reported separately.

## Diagnostics and commands

Use `popup(admission, serial, "ok"|"yes"|"no"|"close")` for the matching server
dialog and `inspect_player(admission, spawn, target_revision)` to request
the selected player's equipment report. Use the values from `report([label])`.
A queued step records submission to the client queue. A later report
includes the inspection spawn and slot count when received.

The projectile line from `report([label])` includes the current admission, the
number of received flight records since the last world reset, the flights still
tracked, the number of scene spawns queued, and the scene instances from the
latest completed projectile update, normally the preceding frame. Missing assets or unsupported flight
types can produce a receipt without a model. Scene instances may be off-camera
or occluded. The counters are independent of damage and inventory.

- `report([label])` logs a state summary with an optional label. Its `option_levels` field includes slider settings, with `run_boost` in hundredths of ordinary running speed. For a selected target, `considered` shows the decoded received color and `display_con_color` shows the presentation color. The report includes `selected_target_level` and `own_level` when known.
  `guild_state` is a read-only JSON snapshot of the known guild ID/name/rank
  and last-received member fields. `members: null` means no retained list;
  `members: []` means a received empty list. A member's `note: null` remains
  unknown after an add without a note field; `note: ""` is a received empty
  note. At most 80 rows are included, with the total in `member_count` and
  explicit `members_truncated`. Current admission/connection fields describe
  report context, without establishing when the list was received or refreshed.
  Opt-in debug logging also records actual `GuildUpdate` events as
  `Guild update received`. Use these events to distinguish a newly received
  full list from retained history.
  `chat_input` is a read-only JSON observation of local chat composition:
  `composing`, `composition_window` (main/detached or null), and
  `other_text_input_active`. Missing input state is null, not false. It never
  includes the draft or typed keys. See [observation limits](observations.md)
  for ownership, timing and settled breathing checks.
  `air_remaining` gives the local air reserve and capacity in ticks while
  Air Remaining is active.
  `breath_probe` reports the last sampled head medium, when resolved, and the
  local breath sample or its first unavailable input. The companion model
  fields give race, gender and raw size, and `breath_zone_scale` gives the
  received classic model scale flag.
  For buffs, `buff_slot_records` is the server's own slot list (the buffs
  held at zone entry and any slot it writes since), `modelled_slots` the
  slots the client gives landings as the program would, `unslotted_effects`
  the landings the server gave no slot, and `held_spells` every spell the
  player holds, slotted or not, which is what speed and the other buff
  effects are read from. A buff held at zone entry shows in
  `buff_slot_records` and `held_spells`, not in `unslotted_effects`.
  The additive `buff_slot_durations` field is JSON: null before a received
  snapshot, otherwise an array of objects with `slot`, `spell_id`, signed
  `duration_ticks`, `duration_kind`, `observation_age_ms` and
  `estimated_remaining_ms`. Existing buff identity fields keep their format.
  `duration_ticks` is the unchanged value in the last received record; one tick
  is six seconds. `duration_kind` is `finite` for nonnegative ticks and
  `negative_special` for negative values, which have no finite estimate.
  The age measures time since the core applied the record, excluding unknown
  transport, queue and pre-admission staging delays. Reading a report does not
  renew that age; a newly received snapshot or applicable update does.
  `estimated_remaining_ms` subtracts that age from the finite tick duration,
  saturating at zero. It is a local elapsed estimate, can differ from the
  server's tick phase, and is never current authoritative remaining ticks.
  Missing age or estimate is null. Modelled and unslotted effects supply no
  received-duration evidence; spell metadata is not used by this report.
  Bind the records to the same report's current admission and connection state;
  retained records during a transfer or disconnection are not current evidence.
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
  for a particular proposal. `settings` reports the requested, applied and
  usable movement policy, along with any pending request and last settlement.
  Its ages describe renderer observations. `camera_detached` reports whether
  the orbit camera is detached when the renderer can identify it.
  `liquid_contact_at_session_position` is local
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
- `trace(milliseconds)`: log movement and frame timing for a bounded duration.
- `screenshot("filename.png")`: capture beside the script; the argument must be a filename.
- `sound("filename.wav")` or `sound("filename.mp3")` plays a named cue from the installed game assets. Use the basename with its extension.
- `inspect_link(admission, message, ordinal)` opens an item link using the
  `admission` and `(message, ordinal, label)` from the current `Script item
  links` report. Ordinals start at zero. The report lists up to 32 links
  from 16 recent received messages in the admission. The run stops if the
  link is unavailable or another inspection is pending. The `inspection`
  tuple gives the selected item ID and whether a received definition is
  available, including a definition already cached in the admission.
- `slash("command [arguments]")`: run any line the player could type, without its leading slash,
  exactly as the chat line runs it: the same abbreviations, handlers, paces and refusals,
  stamped with the script's admission. Chat words (`say`, `tell`, `ooc`, `shout`, `gsay`,
  `auction`), `reply`, `friend`, `ignore`, `quit` and `exit` work as typed. The line is at
  most 512 bytes counting the slash, as the chat line is, and takes no control characters.
  A social goes through the named `social` step instead. `petition`, `guidehelp`,
  `bugreport` (`bug`) and `feedback` run as typed, with the same lines and the Typed
  Reports setting: on Titanium they open their windows unless that setting is on (see
  `report` and `button bug-report ...` in [targets](targets.md)). A person on the staff
  reads each report.
  `slash("follow")` queues the ordinary local follow request for the targeted group
  member. Its deferred request keeps the script's expected admission and zone;
  a changed world withdraws it before movement handles it. Completion of the
  step does not establish movement. The ordinary standing, movement and group
  checks still apply, and a movement key ends following.
  `slash("ranged")` requests one shot or throw against the current target on Titanium.
  `slash("layonhands")` and `slash("harmtouch")` request the class ability against the
  settled selection. Select yourself before using Lay on Hands on yourself.
  `slash("discipline evasive")` and `slash("discipline defensive")` use the learned
  Warrior discipline on yourself, with the same reuse checks as its button.
  Watch the server's combat and inventory updates for the result.
  `slash("pet target")` selects your visible pet. Pet orders complete the script
  step when queued. Read subsequent server observations to check their effects.
  `slash("camp desktop")` uses the ordinary camp destination and current QoL exit choice.
  `slash("language")` says the language the active chat window speaks, `slash("language help")` lists the languages your character knows, and `slash("language N")` chooses the Nth of them for that window, as the typed `/language` does; other words get the same line as typed.
  `slash("guildinvite [Name] [o]")`, `slash("guildremove [Name]")`,
  `slash("guildmotd [text]")` and `slash("getguildmotd")` use the typed guild
  handlers. Omit a name to use the current target; `o` offers officer rank.
  An empty MOTD requests clearing it. Membership, rank and listed-member checks
  remain with the ordinary handlers and session. Queueing a step proves no
  invitation acceptance, roster change or MOTD update; check received evidence.
  `slash("stopsong")` queues the Bard's stop request on Titanium EQEmu.
  `slash("quit")` and `slash("exit")` leave as typing them does, by the same route;
  `quit()` closes the client.
  `slash("emote")` and `slash("random")` reach everyone nearby, and run on any
  server, as the player's own would.
- `gm("command [arguments]")`: an allowed GM command, sent as a typed `#` line on any
  server. Its fixture forms (`spawn`, `depop`, `guild`, `stun`, `setlanguage` and
  `traindisc`) run only on a network-constrained local test session.
  Local Titanium EQEmu also supports `gm("guild create CHARACTER_ID_OR_NAME GUILD_NAME")`,
  `gm("guild delete GUILD_ID")` and `gm("guild set CHARACTER_ID_OR_NAME GUILD_ID")`.
  These are narrowly bounded fixture setup/cleanup forms: character names use
  ASCII letters, IDs use unsigned decimal, and guild names contain at most
  63 ASCII letters/digits/spaces. Create/delete IDs must be positive; `set`
  accepts guild ID zero to remove membership. The reserved maximum ID is
  refused. Use the guild ID actually returned by the server for cleanup;
  request submission alone does not establish fixture creation or deletion.
  Other GM guild subcommands are unavailable to scripts.
  On local EQEmu, `gm("setlanguage ID VALUE")` uses the supported SetLanguage
  alias for a staged fixture: language ID 0..27 and proficiency 0..100.
  It retains the local-session and Titanium gates. General `gm("set …")` commands
  remain unavailable. Training itself permits only offered IDs 0..25.
  On local EQEmu, `gm("traindisc MAX [MIN]")` requests all eligible disciplines
  in that inclusive level range for a disposable fixture. Each level is a number
  from 1 to 126 written with up to three decimal digits. Give MAX first. MIN
  defaults to 1. For example, `gm("traindisc 52 52")` requests only level 52.
  In GM mode, EQEmu can apply the command to the selected player. Check the
  received learned disciplines before using them.
  `gm("stun 8000")` requests an 8,000-millisecond server stun for a local fixture.
  Check received `own_stun` state; command submission alone does not establish the effect.
  On local EQEmu, use
  `gm("spawn NAME RACE LEVEL TEXTURE HEALTH GENDER CLASS PRIMARY_MODEL SECONDARY_MODEL 0 BODY_TYPE")`
  for a temporary NPC with explicit health and body type. Use a fresh alphabetic
  name, positive health, and decimal fields. The merchant ID is fixed at zero.
  The short `gm("spawn NAME [RACE [LEVEL [TEXTURE]]]")` form uses server defaults
  for the remaining fields.
  To remove a temporary NPC, confirm that its current incarnation is selected
  and run `gm("depop")` without arguments. The zone worker rechecks the same
  admission and target revision before sending. Keep that NPC selected until
  its removal is observed, since EQEmu acts on its current target and starts
  no respawn timer. A queued command does not establish creation or removal.
  `gm("show")` reads the server's own view of the target (or the GM) and changes
  nothing. The parser takes exactly `gm("show buffs")`, `gm("show currencies")`,
  `gm("show timers")`, `gm("show distance")` and `gm("show line_of_sight")` (these two
  need a target other than yourself), and `gm("show inventory SCOPE")` with SCOPE
  one of `all`, `allbank`, `bank`, `cursor`, `curlimbo`, `equip`, `gen`,
  `limbo`, `poss`, `shbank`, `trade`, `trib` or `world`. Words are matched
  exactly, in lower case; every other subcommand, scope or extra word is
  refused when the script is read. What these print names characters and
  items, so their output stays in private run notes and never goes into the
  repository, a pull request or a published page.
- `chat("channel message")`: chat on any server, under the session's chat pace,
  with a character in the world.
  The session paces all chat (10 lines in any 5 seconds, 12 far-reaching ones
  in any minute): a line it holds fails the run at that `chat` or `social`
  step's line, with the session's wait.
- `quit()`: close the client.

The typed parser and normal client capability checks decide which commands and controls are available. A valid action can still fail at runtime when its target, session or expected result is unavailable.

- `compose_link(admission, message, ordinal)` inserts a recent received item link
  using the IDs from the item-link report. It edits the normal draft without
  changing keyboard focus. Use the usual chat input to edit or send the
  draft. An expired, mismatched or unknown link stops the script.
- `compose_owned_link(admission, slot)` inserts a link for the displayed Titanium
  item at the given protocol slot. Use the current admission
  and slot from a report. The action edits the draft without changing focus
  or sending chat. A stale admission, empty slot or incomplete item metadata
  stops the script.

- `social("set Name | /command | /pause 10,/command")`, `social("bind 1 Name")`,
  `social("play Name")` and `social("cancel")` use the same social controls as
  chat, on any server. `social("list")`, `social("show Name")` and
  `social("delete Name")` also use the ordinary editor. Use the named step
  for scripts. Synthetic chat submission and hotbar activation refuse socials.
  A step queues the control request. Keep the script running with a wait if
  the social has more lines to dispatch. Stop, failure, completion, pause,
  or loss of script input ownership cancels its unsent lines. The controller's
  single-step action applies a named social control once. A single-stepped play
  processes its first line and retires the rest. A manually started social
  has independent ownership. Use a later `report([label])`
  to observe the social
  runner's current admission, definition ID, progress and remaining delay.
  The report describes dispatch and scheduling, independently of server results.

## Optional login automation

These commands support scripts that deliberately start before world entry. They are separate from the usual workflow of editing and running scripts while playing.

- `login()`: use the configured environment credentials at the login screen.
- `wait_servers()`: wait for the world-server list.
- `server("name")`: choose a listed server by its full name.
- `wait_select()`: wait for character selection.
- `select(name)`: choose a character and enter the world.
- `create(name, race, class, gender, deity, start_zone, primary_stat)`: request character creation from selection where the server policy supports it, using numeric identifiers; the final stat receives the free stat points. `start_zone` is the city's Titanium choice number (1 Qeynos, 2 Halas, and so on); a generation that sends its own value for the same city, such as the Mac-era start-zone IDs, has it translated before sending. This command is not restricted to local servers by the script parser.
- `create_refused(name, race, class, gender, deity, start_zone, primary_stat)`: the same request where the server is expected to refuse it (a name already taken, for one); the step passes on the refusal and ends the script if the character is created.
- `delete(name, "confirm", name)`: request deletion of the exact listed character,
  repeating its spelling and case. It uses the ordinary named confirmation/sender
  and never chooses a character by row position. The step waits for an explicit
  absence observation in the current connection's refreshed list. A refusal,
  unconfirmed outcome, session change or bounded timeout stops the script before
  following actions, including `quit()`; it never retries or disconnects by itself.
  Ordinary Back / Disconnect remains available. Absence is not a server deletion
  acknowledgment or proof of database purge.
- `wait_online()`: wait for a zone admission and player before continuing.

## Recovery consent reports

On a server that offers resurrection or translocate, including P99 for
supported offers, wait for
`consent_presented` to match the current offer before using
`click("answer", "yes")` or `click("answer", "no")`. These actions use the ordinary
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
