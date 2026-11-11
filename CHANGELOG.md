# Changelog

## Unreleased

- Buffs that land on the player take the slot the Titanium program gives
  them (program reads R3 and R20), so the Effects window keeps the
  program's order, received and modelled buffs together, and a right-click
  cancels a buff the server has not slotted yet. A held spell keeps its
  slot; otherwise it takes the lowest empty slot of its band, songs in the
  short window, and after a fade a re-landing takes the lowest empty slot
  again, as the program does. The server's own slot updates win: a fade
  removes its spell wherever it is held, a duration update changes the
  buff in place, and a slot write overwrites the slot (and ends an
  unslotted copy of its spell, inferred). Each contradiction of a modelled
  slot is counted (`buff_slot_mismatches` in the Script report, with
  `modelled_slots` and `unslotted_reasons`) and logged; a type-4 record a
  type-2 fade leaves held is one, as in the program (recording checklist
  6.459). A landing whose slot is unknown, such as one that may replace
  another buff, stays unslotted, and while one is held no new landing is
  given a slot until the next zone-in; its click-off is still refused as
  its slot is unknown. Only Titanium places landings; the Mac-era client
  keeps them unslotted, its program unread. A spell counts as landed on
  the program's test (an action of kind 231 whose flag carries 4). The
  speed report leaves a second copy of a buff's strength out and marks
  itself inexact. Live rows are recording checklist 6.457 (local EQEmu)
  and 6.458 (attended).
- A root no longer takes the player's input away. The session grants a
  rooted player at no speed (read R18: the program zeroes the speed once and
  blocks nothing else; read R22). The keys, the mouse and a script's
  facing still turn them, nothing carries them, a jump is refused as the
  Titanium program refuses it, and held keys, a crouch and taps go on. On
  Titanium the autorun latch survives the root, so the player runs again by
  themselves when it ends (R22 b). The Mac-era program is unread, and there
  the root still drops autorun. A sample under way when the root or any
  speed change lands is no longer refused, and the model keeps playing to
  where it was sent, so a snare no longer holds the player at its landing
  and fade. The sample under way when a speed changes asks the lower of
  each earlier and new speed, and the new speeds apply from the next one,
  so a fade's raise is no longer refused as faster than its time allows.
  The log line for a root is "Movement rooted". Mouse-look pitch, which the
  program holds while rooted in camera modes 0-2, is not modelled
  (recording checklist 6.451). Networking pinned to 3daa711 (eq-network
  #296).

- A Feign Death spell landing on you lies you down, as the Titanium
  program does (program read R21): the session sends one lying posture and
  you are held still as for the ability, with auto attack left as it is and
  no feign request. The client reads effect 74 from the installed spell
  data; a spell whose effects are unknown lies no one (checklist 6.455).
  Move While Feigned covers the spell too: its landing sends nothing, and
  your next stand ends the feign (checklist 6.454). Networking pinned to
  93faf1f (eq-network #295).
- Test scripts may set a test character's PvP flag on a local EQEmu or TAKP
  server (`gm pvp on`, `gm pvp off`), as other `gm` steps are allowed, so a
  GM's `castspell` can land a root or snare on another flagged test
  character. The server keeps the flag until `gm pvp off`. This lets the
  owed live rows for a root and snares landing while moving run (recording
  checklist 6.451).

- Feign Death and lying on the ground as the Titanium program handles them
  (program reads R19, R19b and R19cd). The press lies the player down
  before the feign request, so others see the body fall, and turns auto
  attack off without a line (the program's second line is unread,
  checklist 6.455). A lying player's own forward, back, strafe, keyboard
  turning, autorun and jump move nothing and stand no one; mouse look still
  turns them in the chase views. Casting, an item click or a knight's
  special while lying is refused with the program's line. A hit stands a
  sitting or looting player; a lying one stays down. The sit toggle and
  `/sit` stand a lying or looting player, and the toggle refuses under a
  mesmerize, where the player's buffs' effects are known (checklist
  6.455). Auto attack now stays on through a sit, a duck or a lie, the
  server deciding the swings (read R19cd 3); the attack key still turns it
  off and still refuses to turn it on while the player is not standing.
  The Mac-era client keeps the earlier rules, its program unread. Two
  Dangerous options, where the server type offers gameplay departures:
  Stand When Hit, on by default as the original client behaves, whose off
  keeps a hit sitter seated (checklist 6.453), and Move While Feigned, off
  by default, where the press sends only the feign so the player stays
  standing, is seen standing and moves while the server keeps the feign;
  their next stand ends it, as a jump or a duck does (checklist 6.454).
  Autorun pressed while sitting neither stands the player nor runs them
  once they stand (read R19c), and any posture change ends autorun.
  Networking pinned to 5248ca5 (eq-network #293).
- Stop needless movement blocking. A Bard's song pulse, any buff or spell
  landing on the player, a level, eating, looting or moving items, and a
  server's move no longer stop the player, cancel autorun or drop a jump:
  the original client stops input only for a stun, a freeze or not standing
  (reads R7, R18). Run 1d's Bard, frozen for the whole song, moves freely. A
  speed change for a player already moving replaces only the speeds, so held
  keys, taps, a queued or crouched jump and autorun go on, and the sideways
  carry of a jump under way is limited to the new speed. A server's move
  starts prediction afresh with the speeds kept, and autorun, held keys, a
  jump request and a crouch go on from the new place, as the original
  client's own-spawn update and warp touch no input (read R18). A root
  anywhere among a buff's effects roots and the strongest snare the client
  can evaluate applies, even where another effect cannot be evaluated. Where
  the client cannot prove every term, it reports the terms it can prove as a
  bound. A term it can evaluate now is taken as it is now, slower or faster,
  and one it cannot evaluate is left out; a term comes from its last report
  only where the client no longer knows it (buff membership or belongings
  not known), never with a root or snare in it, so a speed buff that ends,
  ends at once. The session takes the bound as reported and starts a grant
  from it where none is in force, so a zone-in, or a root or snare fading
  under a song, moves at once. With no earlier report and nothing provable,
  input waits; there is no "Movement paused" line while the player can move.
  By Adam's decision of 11-10 this can be faster than the original client
  would allow until the inputs are proven (recording checklist 6.451).
  Networking pinned to a1296f6 (eq-network #292).

- Open the Runbook window, in front, whenever the local harness starts a
  Runbook, so whoever watches can see a run is in use. Only a start opens
  it: once closed, it stays closed until the next start. It takes no
  keyboard focus, and as the newest window Escape closes it first. Zoning
  was checked and keeps the window open.
- Choose what each Actions window ability button holds, as the Titanium
  program does (reads R17 and R17b). A left click on an empty button, or a
  right click on any, opens the skills picker for it: the skin's
  `SkillsSelectWindow` where the skin has one, the client's own panel
  otherwise. The Abilities page's picker lists the program's ability skills
  then its combat skills, and the Combat page's its combat skills only,
  each the player has, in the program's order, with the player's value.
  Then both list a knight's Harm Touch (105) or Lay on Hands (107) by the
  program's special numbers, offered by class and level where the program
  reads a profile byte (checklist 6.450), and then the disciplines the
  player knows (6.450). A click on a row puts it on the button and closes
  the picker. Its close box or Escape only closes it, as the program's
  does; so do a zone change, a new session and the Actions window closing.
  Until the first choice nothing is saved, and the buttons follow the
  installed client's saved choice where it has one, else the client's own
  order, worked out again on each entry. The first choice keeps all ten
  and saves them in the character's own settings file under the program's
  `Ability1`-`Ability6` and `CombatSkill1`-`CombatSkill4` keys, with a
  discipline in the client's own section (6.448). The installed client's
  file is only read. A button holding a number the client cannot use (the
  specials 111, 114 and 115, or skills 16, 71 and 72) keeps it, says "Not
  available" and does nothing on a left click (6.449). Scripts choose a row
  with `click skill_select N`, and the `Script ability report` line names
  each button's choice.
- Use the class tools as the Titanium program does (Batch B2). On EQEmu and
  P99, Fishing, Pick Pockets, Begging, Sense Traps, Disarm Traps, Pick Lock
  and Tracking are abilities the player can use where they have the skill,
  on the Actions window's buttons the player chooses (above). The client
  tells the session whether the player stands in liquid, which Fishing
  refuses on. A press while an ability's wait runs sends nothing and says
  the client's own line, where the program says nothing (a chat-only
  departure, checklist 6.447). Between a beg or a pick and its answer the
  program holds casting and some window actions until the server replies,
  and the client does not yet (checklist 6.452). Fishing's cast line, and
  the answer to a beg or a pick (nothing, coins of one kind, or an item by
  name), are the installed string table's lines by id. Tracking opens the
  track window, the skin's `TrackingWnd` where the skin has one and the
  client's own panel otherwise, emptied until the server's list comes. The
  list keeps the server's order and leaves out spawns the zone does not
  know, each name in the colour of its level against the player's by the
  track window's own rule, which is not Consider's; the program's light
  blue near the green edge is drawn blue, and the green threshold's
  rounding and edge are inferred (checklist 6.444). A click chooses an
  entry, kept across a new list while it is listed; Track follows it and
  closes the window. Cancel stops tracking and closes the window, as
  `/stoptracking` does, since the program binds the two to one routine;
  the close box only closes it (inferred, checklist 6.446), as Escape does.
  A bearing's side word is put in by its words from the string table.
  While following, the session's bearing lines say where the spawn lies,
  and its end lines say when the spawn is gone or the six minutes pass
  (checklist 6.441). The Ranger's colour filters are drawn greyed and the
  sort and players comboboxes are not drawn, and another class's
  `/tracksort` or `/trackfilter` gets the program's Rangers-only line; a
  Ranger's own, and `/trackplayers`, are answered as before (checklist
  6.446). The host's auto-attack follows the session's at
  every zone change, so a new zone starts with it off on both sides
  (checklist 6.445), and an off the client sends on its own, when the
  player stops standing or dies, goes again until it leaves a full queue.
  Fishing's liquid test is made at the press, from the zone's collision and
  the player's place then; with no zone collision loaded it answers dry and
  the server decides. Scripts gain `click tracking N` and `click track`, and
  the script report a tracking line.
- Ride lifts as the Titanium program does (Batch B1). On EQEmu and P99,
  vertical doors (lifts, sinking floors and platforms) rise and sink at
  their type's travel and speed, counted from the server's action, and
  slides go their type's distance and direction. Each door returns on its
  own after its type's delay from its arrival: 10 s for ordinary doors, 20 s
  for most others, never for a lift whose field is over 170; types 40 and 45
  go back by the click the networking sends. A door holds the player up or
  stops them only as the program's rule says, so a hinged door is no floor,
  and route searches use the same rule. A player on a rising lift, still or
  walking, stunned or not, is carried up by the lift's own rise since the
  last sample, however long the frame took, and each move on a lift names
  it so the movement guard allows its rise. Nothing above the rider stops
  the carry, as the program tests nothing overhead while a lift carries
  (read R1d; checklist 6.427). A player who steps off a rising lift is
  carried no further,
  and the step still names the lift that raised it. Between samples the
  model is drawn on the rising floor. A door is in click reach, in 3-D within 20,
  of its origin where it stands now, so a lift resting raised is used from
  its top; the program measures from the clicked point on the door as drawn,
  within the player's size and 20, so ours is short of it (read R16b, R16e;
  checklist 6.436). A door that moves into the
  player, such as a lift landing around someone beside it, no longer holds
  them in place: its walls let them walk out, and its floor still holds them
  up (checklist 6.439). The movement report names the door whose floor is
  under the feet, if any. A lift that sinks drops its
  rider at once, as the program's floor sweep does, rather than leaving
  them standing on air until the gap passes a step. The script report lists nearby
  doors with their mechanism, field, offset and solidity. EQMac servers keep
  their doors solid both ways (checklist 6.425). At a lift whose rest floor
  lies inside zone geometry, the rider is carried up and rests on the lift's
  floor under the deck; the ordinary floor sweep stands them on the deck with
  their first step, and the lift does not carry them down from there: when it
  leaves, they stand on the deck. A rider at such a lift's origin rides into
  the zone's own column. What the program does at the top, and how its rider
  goes down, is unread (checklist 6.440).
- Look and swim as the Titanium program does (Batch M slice 3). Look Up
  and Look Down (keypad 9 and Page Up, keypad 3 and Page Down) tilt the
  first-person view while held, ramping as the turn keys do up to straight
  up or down, and Center View (keypad 5 and Home) levels it; in the chase
  view the keys do not tilt it, and Center View returns it to its opening
  pitch behind the player (checklist 6.422). The new Look Spring option,
  off by default and read from `eqclient.ini`, brings the first-person view
  back level when no look key or mouse look holds it. Backing up in water,
  unless ducking or sneaking, goes only at the buffs' speed modifier, so
  not at all without one, as the program's does.
- Move and look as the Titanium program does (Batch M slice 2). A
  jump takes a tenth of the maximum endurance, at most 50, off the shown
  bar at take-off, until the server's next report. The mouse turns the
  program's (0.5 + 1.5 x (s - 1) / 7) x 512 heading units per screen width,
  the sensitivity setting spanning the program's 1 to 8 (checklist 6.418).
  The chase camera sits 0 to 30 above the eyes and 2 to 200
  behind (farther on the long leash, at the same steepest slope), looking
  down at the player, never below eye level; other views are the free
  camera's. A strafe goes at the run speed running and the walk speed
  walking, halved while ducking. The duck key's toggle gains a test. The
  air still slows a fall once a 50 ms step: the program does it once a
  frame, and taking 60 frames a second would about halve the damage of
  mid-height falls, so it waits on a recording (checklist 6.419).
- Jump and turn as the Titanium program does. A hurt or overloaded
  player's jump starts at the same health and weight share that slows
  running. A jump from standing still with no forward input crouches for
  half a second first, and backing up drops it, on Titanium as on the
  Mac-era client; a strafe-only jump still goes at once (checklist 6.417). A jump is refused with the official line when endurance
  is at or under 50 and at or under a tenth of the maximum (the client's
  estimate; with no estimate the jump goes as before, checklist 6.415). The
  turn keys ramp up from 20 to 240 heading units a second over a fifth of a
  second, and to 160 while walking (the ramp steps at 60 a second,
  checklist 6.416).
- Add the General page's Allow Auto-Stand option, on by default as in the
  official client. Off, a sitting player stays put when they move forward
  until they stand.
- Typed slash commands now resolve as the Titanium client resolves them.
  The word is matched against the start of each official command name, in
  the official order, and the first match wins. So `/s` is `/split`, `/l` is
  `/location`, `/m` and `/se` are `/tell`, `/me` is `/emote` and `/raid` is
  the Raid window. A game master's command does nothing for a player. Only
  a word that matches no official name reaches this client's own commands
  (`/social`, `/ranged`, `/harmtouch`, `/layonhands`). Any other word is
  passed over without a line (recording checklist 6.413), and so is a line
  starting `//`. `/rsay` sends raid chat, and `/group` and `/raid` no longer
  send chat; `/raid` opens or closes the Raid window, whatever follows it.
  A bare `/` is `/who` because an empty word starts the table's first name,
  which retires the inference recorded as checklist item 6.328.
- Add the Titanium client's contacts and guild commands. `/reply` and
  `/rtarget` use the last 50 players who sent a tell. `/friends` and
  `/ignore` keep 100 names each per character. The first lists come from
  the installed client's character file, or its defaults. Ignored players'
  chat is hidden, except from a game master. `/who all friends` lists the
  friends online, and says when the list is empty. A guild message of the
  day is shown only when it is new and not empty; the one `/getguildmotd`
  asks for always shows, in the guild colour and log line.
- Answer guild invitations as the official client does. The confirmation
  dialog asks about joining or a higher rank. Every other case is answered
  at once, in the official lines. Add `/guildinvite` (with officer offers),
  `/guildremove`, `/guildmotd`, `/guildleader` and `/getguildmotd`, each with
  the official checks. A guild's roster is kept as the server sends it, and
  `/guildremove` and `/guildleader` name a member of it before they say
  their line.
- Add `/split` and `/autosplit`, and the bank window's Change button, which
  is offered only where the server takes a banker's change.
- `/follow` now follows the targeted group member on foot, with the
  official client's turn steps, walk and run distances, and stopping rules.
  The group window's Follow button still joins the group the player was
  invited to. A stun holds the follower until it lifts, autorun ends
  following like the other movement keys, and a mouse turn made meanwhile
  is not kept. Inferences (the dialog's timeout, the clear-path test,
  letter case in `/autosplit`, and others) are recorded as checklist items
  6.410 to 6.412.
- Keys typed into the chat line no longer reach the game when Enter closes
  the line in the same frame, so a digit at the end of a line no longer
  presses a hotbar button.
- Build zone navigation meshes by default. Script walks plan over the mesh and
  warn in the log and the Execution tab when they fall back to grid search.
  `--no-navmesh` turns meshes off; the old `--navmesh` flag is still accepted.

- Look up and swim forward to use Titanium's shoreline boost near a
  reachable supported bank. The renderer sweeps the capsule and the worker
  replays the bounded frame trace through ordinary position updates. Frames
  longer than 50 ms count as 50 ms, which only lowers an arc; the profile's
  frame bound, lifetime and damping are inferred (recording checklist 6.407).

- Correct the Titanium floor-sweep radius to 0.1 times the model's scaled
  standing reference (this client's standing-height estimate, inferred,
  6.347), which slightly raises how high one step can reach.
- Let opt-in Fly pass through mapped liquid while retaining solid collision.
  Settle permission changes with the session worker and report the result in
  script movement diagnostics.

- Resize the Runbook from an edge or corner. Wrap checklist rows to fit
  and remember the size with the character's window layout.

- Keep Titanium sliding door pose, picking and collision together. A script's
  right-click press on a spell gem or buff is dropped when native input takes
  over, so a native release cannot complete it. Background scripts can
  minimize and restore the Runbook through the ordinary window controls.
- Right-click a spell gem to forget its current spell (inferred, recording
  checklist 6.405). Fit Effects and Songs icons to their skinned layout
  (inferred, 6.406) and explain when a cancellable buff's server slot is
  unknown.

- Expose the Project 1999 session's ordinary assist, dice, emote and
  listing commands through chat and script dispatch. Presence requests and
  received listing changes are shown separately. Script `slash emote` and
  `slash random` steps, which reach everyone nearby, are refused outside a
  network-constrained local test session, as `chat` steps are.

- Keep identical scoped Runbook retries idempotent after an admission
  change. Track harness and window exports with separate files and status.
- Keep the full Bind Wound recovery visible. Distinguish an unanswered
  bandaging result from uncertain bandage stock.
- Bound inspection note and landmark file reads, protect oversized
  landmark files from replacement, and share immutable installed spell
  definitions across sessions.

- Pin networking ebdff963 (Batch B2 landing): the Titanium class tools,
  Fishing, Pick Pockets, Begging, the trap skills, Pick Lock, Tracking and
  `/stopsong`, as the program uses them, with the session's own line for a
  press during a class tool's wait (checklist 6.447).

- Pin networking 0e45bfcc (Batch B1 landing): Titanium lifts and doors as
  the program moves them (vertical travel and speeds by open type, slides,
  floor and wall solidity, the client's own returns, and the return click
  for types 40 and 45), the movement guard's allowance for a lift's rise
  while a sample names it, and a door's click reach to where it stands now.
  It also brings the zone-in admission of a player with an empty inventory,
  such as after a death (checklist 6.437).

- Pin networking 9d9d0ee1 (Batch M slice 3 landing): Titanium's own position
  update carries the program's movement-history entry counter, goes out by
  the program's flag rule (a straight run once a second), repeats while
  frozen and stops while stunned; the session grants the rate for backing
  up in water; and a moving player stops only on this client's own stop,
  so a frame stall no longer sends one.

- Pin networking 835c95d2 (Batch M slice 2 landing): Titanium strafes on the
  selected base speed, and the grant carries the walking strafe rate.

- Pin networking 4ef182e4 (kept-grant landing): a change in the player's belongings (an item
  eaten, drunk, looted or moved, or coins) keeps the movement grant until
  the new speed inputs land, so a jump, a crouch or autorun under way is no
  longer dropped.

- Pin networking 51c94173 (Batch M slice 1 landing): a jump starts at the
  health and weight factor that slows running, and the movement guard bounds
  its rise by the factor it launched with.

- Pin networking 462baf69 (Batch A landing): friends who, guild requests,
  splitting and a banker's change on the Titanium wire, offered on `EQEmu`
  and Project 1999; a waiting guild invitation and the member list do not
  outlive the zone connection.

- Pin networking 0d93a466 (P99 repairs landing): the bounded Titanium
  shoreline impulse, with each replayed frame counting for at most 50 ms.

- Pin networking ec34373e (cycle 10 landing): ordinary P99 assist, dice,
  emote and presence commands; an accepted assist also moves the server's
  stored target. Station placements, combines and NPC gives no longer leave
  carried weight unknown, which paused movement until zoning.
- A landmark file reached through a link is treated as unreadable, as its
  save already refused it, instead of loading as editable and failing every
  save.

- Pin networking 97212d8d (cycle 09 landing): Bind Wound's button time runs
  out in full after every use, one bandage decrease settles every carried
  stack, and an unanswered player inspection holds the next request to that
  player for up to five seconds (inferred, recording checklist 6.400). The
  installed zone boundaries are always offered to the networking layer, which
  decides where same-zone pads use them. The P99 knight reuse estimate is
  labelled inferred (recording checklist 6.399).
- Resolve supported P99 same-zone pads from installed geometry and current
  zone points. Script reports distinguish the local landing from received
  server placement. Attended verification is pending.
- Show received Titanium server dialogs and inspect a nearby selected player
  on EQEmu and Project 1999. The inspection panel displays received equipment
  fields and note text. Item selections, received inspection details, and
  clicks bring the corresponding inspector to the front.
- Render installed lights in supported Titanium interiors and apply supported
  low-light vision. Ordinary invisibility and target eligibility use the
  observer's current See Invisible effect when the inputs are known.
- Offer bounded swimming in mapped classic water on Project 1999.
  Titanium wet movement samples the actor origin, with the native head
  probe used separately for the air meter. Live P99 wet movement and
  liquid damage remain to be verified.
- Play installed MP3 cues named by `sound name.mp3` script steps and received
  sound requests. Only installed files are read; two MP3 voices play at once,
  a cold cue may wait up to eight seconds and a cue stops after 300 seconds
  (inferred, recording checklist 6.398).
- Show received marquee banners above the scene with their hold time and
  packed opacity (inferred, recording checklist 6.397). A death keeps queued
  banners and sounds; other world resets clear them.
- Save named map landmarks per world, character and zone, shown as teal
  diamonds with range and bearing from the character.
- Outline tracked names' models through scenery with the map's **3D**
  toggle. It departs from the official client, so it starts off and needs
  gameplay departures.
- Compose owned item links from scripts with `compose_owned_link`. A
  structured script run reports `total_steps` as null in `status.json`, and a
  control-flow run takes no appended lines from `--script-follow`.
- Hold the knight and discipline buttons back from the Combat page where
  the session does not offer them, so the ordinary abilities keep their
  places and script indices.
- Take the chat-link length budget from the networking server policy rather
  than a protocol comparison.

- Add typed variables, read-only client observations, conditions and bounded
  loops to eqscript. The workbench shows branch decisions, active loop counts
  and variable values. Existing action scripts use the same controls.

- Use Ctrl+right-click on a displayed Titanium inventory item to add its
  link when the record includes the needed metadata, even during an item move.
- Keep an unchanged book or note selection usable across unrelated inventory
  updates while its text request is queued.
- Open cached item details while a different inspection request is pending.
  A late reply leaves the selected panel in place.
- Offer the ordinary Project 1999 forge through the nearby object controls
  and show the container only after the server opens it. Keep local item
  move projections distinct from received station and inventory contents.
- Show Project 1999 self-targeted Bind Wound requests separately from server
  results. After the request ends, affected slots show unconfirmed stock
  while unrelated inventory actions resume.

- Allow `gm traindisc MAX [MIN]` on network-constrained local EQEmu sessions
  to prepare learned disciplines for test characters.

- Apply a single-stepped social control. A single-stepped play runs its first
  line and retires its remaining lines at the step boundary.

- Scope social definitions and bindings to exact character profiles. Keep
  ordinary legacy hotbar bindings and require rebinding legacy socials.
- Stop script-owned socials when their executor yields or ends. Use named
  local script controls and keep deferred target changes out of socials.
- Keep linked chat drafts when the routed text would exceed EQEmu's expanded
  limit or the tell recipient exceeds its field size.

- Save up to 120 named socials per character and bind them to a hotbar page.
  A social runs up to five ordinary slash commands with optional
  tenths-of-a-second pauses and a cancel command.

- Shift-click a retained received chat item link to insert it into a draft.
  Linked drafts survive zoning on the same connection and known profile.

- Draw received arrows and thrown items in flight using installed item models.
  The script report separates received flights from rendered models.

- Apply received same-zone moves at the absolute landing supplied by the server.

- Follow local EQEmu server-owned same-zone pad handoffs through ordinary
  walking and received relocation. Reset movement and boundary detection at
  landing, and expose a bounded script wait for the received instruction.

- Added Evasive and Defensive on Titanium through Actions, the hotbar and
  named commands. Received shared discipline reuse, linked spell timers and
  active discipline slot updates are tracked per admission. Exact empty
  slot replacements clear the active state. Staged discipline state preserves
  the cancellation revision.

- Added the Titanium knight actions Lay on Hands and Harm Touch through
  Actions, the hotbar, and ordinary slash and script commands. The hotbar
  distinguishes the local recovery estimate from received profile recovery.
  A matching instant class refresh consumes its cast request even when the
  server sends no cast start. A new admission clears the previous knight,
  discipline, and linked spell reuse timers.
- Open the ordinary resurrection and translocate confirmation dialogs
  for supported P99 offers. Answers identify the displayed offer, and
  repeated wire-identical offers are withheld during that admission.
  Movement and recovery follow received server updates.
- Offer beneficial-buff cancellation on P99 through the same Buffs and Songs
  controls. Keep received slot membership and show an unknown outcome when
  the two-record request was only partly submitted.
- Open P99 skill training through the ordinary trainer interaction and
  Training window. Show received skill changes and estimated prices.
  The practice point count becomes unknown after a request. Re-enter the
  zone before practicing again or reopening the same guildmaster.

- Group Interface scale with ordinary QoL settings. Place **Run a Little
  Faster**, **Walk While Winded** and **Memorize Without Sitting** in the
  **Dangerous** section with the other flagged options.
  When space allows, either slider's caption, track and value share one
  line.
- Confirm an ordinary Titanium cursor item drop from Inventory on local-only
  EQEmu. Escape cancels the pending confirmation. Scripts can open it with
  `click drop` and use the ordinary Yes/No buttons. A fresh inventory snapshot
  resolves the cursor hold. Ground objects follow their own received updates.
- Report completed falls when their cost can be established while a drop
  awaits inventory evidence. Active
  movement can resume once the worker accepts a current upper bound on
  carried weight. A speed recalibration preserves the vertical history of
  an airborne jump or fall and any landing report waiting for queue space.
  A cursor resync can preserve the fall bound while movement is paused.
  If the load and landing cost are unknown, the client sends no report and
  says so in chat. Ground drops are local-only EQEmu, so this case does not
  reach public servers.
  `ClientWorld::fall_weight` continues to return `Option<f32>`.

- Set **Run a Little Faster** from 1.00x to 1.40x on the QoL page. Existing
  enabled checkboxes load at 1.10x, and new profiles start at ordinary speed.
- Keep moving while a speed change settles: new movement uses the lower of
  the applied and requested speeds until the worker's receipt arrives, one
  option request is in flight at a time, and a zone reset resyncs the
  choices. Without gameplay departures the defaults are retained.
- Consider replies map the received faction code to its standing phrase in
  wire order. Codes 6 to 9 were reversed.
- Consider chat uses the fixed Titanium palette, including its even-level
  white, and a native light blue summary. The renderer uses one representative
  phrase per color. The network policy keeps the received color on every
  server. An optional level estimate falls back to the received color when
  either level is unavailable.
- Use the networking game's instrument scaling for movement-related buff
  effects. Signed snares and the existing bounds and unknowns keep their
  behavior.
- Add a hidden, movable Runbook checklist for a local script harness. Rows
  show only results the harness reported, stale IDs and conflicting terminal
  results are rejected, and the state survives closing and reopening.
- Offer **EQ → Runbook** only in launches with `--script-control`. Checklist
  results accept an explicit null note as well as an omitted note.
- Keep the next air and lava checks due while a movement result is pending.
  Hide the air gauge after 250 ms and evaluate one due check against the
  next usable worker position. A pending Move survives a same-admission
  reset and requires a result with its admission and request stamp.
  **Breaking:** `Moved` and `WorldEvent::MotionSent` now include `created`.
- A cast request times out after five seconds through a typed event. A
  compatible delayed own start retains its request spell through
  `CastLateBegin`. Ordinary item casts use the matching spell-bearing
  Mana result. Exhaustive event matches now include `CastLateBegin`.
- Show Air Remaining during ordinary mapped Titanium immersion, including
  WLD lava that restricts breathing at the head. Local-only EQEmu sessions
  report drowning after depletion and lava heat from mapped body contact.
  Turning off Take Environmental Damage withholds the reports while air
  still runs. **Gills for Days** starts off, needs the session's gameplay
  departures, and disables the local air counter and meter. Lava heat
  continues to follow the damage setting. A due liquid report uses the
  worker's last observed position before the next move is queued.
- An unconfirmed spellbook edit releases the book for other edits when the
  session reports its timeout. A retry with the same possible reply requires
  a fresh admission. Received gem clears update the slot independently of
  which request they answer.
- Release a superseded cast while keeping a different pending spell.
- Keep a newer cast's request alias when an older cast is released. Preserve
  skinned spellbook edit receipts through a temporary link interruption and
  report worker timeouts. Check known installed Titanium class and level
  requirements before sending a scribe request; whether the official client
  makes that check locally is unread (inferred, recording checklist 6.394).
- Retain an early pet buff snapshot for up to one second until the owned pet
  resolves. Discard it when the spawn is replaced or its ownership conflicts.
- Scripts can `interact` with the exact selected merchant, banker or corpse
  through the same path as the UI, `inspect_link` a received chat item link,
  and drive the spellbook Search field and its Clear button. Reports carry
  the interaction tuple, an unconfirmed merchant opening and received item
  links. A merchant request the server never answers no longer pins the
  interaction fence to that merchant: a later request or any opened shop
  settles it.
- Hold Titanium travel after a fresh placement below a nearby installed floor
  with neither a lower surface nor mapped liquid below. Allow turning at the
  received XYZ and warn once for the held location.
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
- Use `/stopsong` as a Bard on Titanium EQEmu. A tracked gem cast has one
  second for a matching update before the client releases its hold and
  reports the stop as unconfirmed. If no cast is held, the client reports
  unconfirmed song state on submission. Song effects follow server updates.
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
