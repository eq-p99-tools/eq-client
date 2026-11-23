# Changelog

## Unreleased

- A swimmer climbs out of a pool onto a rim up to about 3.3 above the water, as the Titanium client does (Temple of Life, North Qeynos; checklists 6.482, 6.483b, 6.483c):
  - Stroking forward with the view pitched up toward dry ground just ahead hops, at most once in 1500 ms: three times the jump strength added to the rise and a push along the heading (0x48a793, 0x48a026, read R33b).
  - The wet step ends on the frame that takes the origin out of the water, so the fall that follows keeps the velocity it left with instead of losing it to the water's damping for the rest of the sample.
  - The walk loses its edge climb only with the origin in water or lava, the program's deep liquid (+0xad of 5 or more, R40), not whenever the feet are wet. Wading with the origin dry climbs edges.
  - A fall whose origin has just left the water, the soles still in it, passes over what the walk would climb (origin + 1.25) and lands on it, inside the announced climb reach.
  - A body falling back into water keeps the fall's velocity, so a wall that stopped the move no longer stops the next stroke's forward speed.
  - A swimmer that surfaces under a ramp or deck over the water, its head inside it, drops back into the water instead of freezing in place (Temple of Life's north deck). The fall's downward sweep is led by the lower body when the whole body's is refused where it starts. Ours: the program's sphere differs (R40).
- On TAKP and Quarm, a Tracking press now fills the track window itself, as the Mac-era program does (TAKP read 10c, checklist 6.619 client half): group members in the zone first, at any range, then every other player or NPC within the program's range. The range is skill × 10 + 500 in zone types 1, 2 and 5, and skill / 2 + 500 elsewhere, from the zone header's time type, with the read's above-100 term; before a header has given the type, the smaller range. Corpses, races 0x8f, 0x8d, 0x48, 0x49, 0x72 and 0x8e, invisible spawns and the player are left out, up to 1000 entries. No Mac server sends a list. The distance (a straight line), the visibility test (the invisible flag) and the list order (by spawn ID) are inferred; the actor +0x198 skip is unread. Titanium's press still waits for the server's list.
- On TAKP and Quarm, `/decline` refuses a waiting duel challenge, as the Mac-era program does (TAKP read 11h, checklist 6.622 client half). The session says the program's line for each state: dueling (0x33bd), declined (0x33be) or nothing to decline (0x33bf). A plain `/duel` there now sends the target as it stands, as the program does (read 11g), and the session gives the program's refusals for no target, the player or a corpse. Words after `/duel` are still refused on the Mac era, since the program's handling of them is unread. Titanium is unchanged, and RoF2 keeps this client's checks.
- `/toggleinspect` and the Inspect Others option as the Titanium program
  keeps them (read R49-16, checklist 6.574). `/toggleinspect on` and `off`
  set it; any other word or none says the usage lines 0x339a and 0x339b;
  each ends with 0x339c or 0x339d, in colour 0x111. Nothing is sent. The
  option is on by default and seeded from `InspectOthers` in `eqclient.ini`
  (first letter t, T or 1; missing is on), then kept per character, which
  is this client's choice (the original keeps one per installation).
  - **Inspections are always answered now.** The option gates only the
    player's own right-click inspects (6.644, next), not answering others,
    so the earlier session-only switch that stopped answering is gone.
  - Scripts can run it with `slash()`, and their reports list
    `inspect_others` with the other options.
- `/ttell` and `/whotarget` as the Titanium program takes them (read R49-18,
  checklist 6.528). `/ttell` says nothing with no words or no target; a
  target is taken by the name the server sent, refused (line 0x3322) when
  it has a `#` or `_` unless it is a creature's corpse, and cut at its
  apostrophe, so a player's corpse's tell goes to its owner; a tell to
  yourself or your own corpse gets line 0x3023 and is not sent.
  `/whotarget` lists any target as a typed `/who` of its name, ignoring any
  words, and says nothing with no target. Labelled: the program counts a
  creature that dies in view as a player's corpse and refuses it, where
  this client sends the tell. Mac-era and RoF2 keep this client's own
  rules. The code moves to `targeted.rs`.
- `/shield` follows the Titanium program's handler (checklist 6.517, read R49-22d). A Warrior of level 30 or more can name the player to shield, as `/shield Name`: the name is matched as `/assist Name` matches it, the shortest name it begins winning, and the target is left as it is. A name that matches no one says nothing. With no name, the target is shielded. The lines for another class or a lower level, for no target, and for naming or targeting yourself are the installation's own (strings 0xfc1, 0xfbf and 0xfc2, 0xcce and 0xfbe), with this client's words where the installation lacks them. As in the program, the client checks no distance, kind or recast; the server answers those. The Mac-era client's handler is unread there, so it keeps the earlier checks and words. The program's general "can act" test and its silence while a trade or certain windows are open are not modelled yet (row 6.517a), as for `/assist`.
- **The Use key now uses what is at the centre of the view, not the nearest door** (read R49-19, checklist 6.609). On Titanium (and RoF2, inferred) it casts the program's ray from the camera through the centre of the 3D view, the same pick a mouse click makes:
  - The first character the ray hits becomes the target. An NPC, or one the target setter refuses while no target stands, also gets what a right click on it does (a merchant, banker or guildmaster opens, a boat asks to pilot, else it is considered; no hail). A player or corpse that becomes the target is only targeted, as the program's key handler reads (0x462519).
  - With no character, a door on the same ray gets one click at the point the ray met it, and the session judges its reach from there, as for a mouse click. Nothing else is reached: an item on the ground or a world container opens only by its own click (read).
  - With neither, nothing happens and nothing is said.
  - **This changes scripts.** Use no longer finds the nearest door or item without facing it. A script's `press("U")` is the real key, so it goes the same way. Turn on the new quality-of-life setting **Use Picks Nearest** (off by default) to keep the old rule: the nearest door, item on the ground or world container within the same reach.
  - Inferred and labelled: the program's pick skips its own actor, its mount and actors with a "not pickable" bit; this skips what a mouse click skips.
  - EQMac keeps the nearest rule until its key is read. The status line's "use the door" hint shows only where Use takes the nearest thing.
- A Bard's song stop resets the gem timers as the Titanium program's stop does (0x40f06d, read R49-21 c; checklist 6.542). On each `WorldEvent::SongStopped`, which the session sends only under the Titanium song rules (P99, Titanium EQEmu), the timers of the gems whose spells have no recast end, and so does the shared recovery before any spell. A spell whose timing the installed data lacks keeps its timer. The stop is applied in order with the gem refreshes around it, so a refresh after it starts its timers again. Before, a press just after `/stopsong` could be refused with "Spell available in N s" where the program casts.
- A zone line that leads back into the same zone places the player at its landing on stock EQEmu too, as the Titanium program does (0x48aaa6, checklist 6.640, repin to eq-network 231d67d with #430). Before, the client waited for a server move that never came, so in qeynos2 the player fell down the Temple of Life shaft. The installed zone lines already checked P99's crossings, and now check stock EQEmu's. Quarm, TAKP and RoF2 refuse a same-zone line, as before.
  - The repin carries every eq-network change since 614d1c0. An item's cold resistance is scaled with the other bonuses (#418). A training refusal keeps this client's words; the program's line is the language-training client half's (#427). Movement sends no vertical speed of its own, so the session sends 0 as before (#423, the 6.617 client half's).
  - On Quarm and TAKP: another player's inspection says the program's line 0x57c naming them (#426), and the player's inspect note is now sent wherever the session answers inspections. The Mac era's `/decline` answers say the program's lines 0x33bd to 0x33bf (#429), though this client offers no `/decline` yet.
  - P99's expansion mask now arrives (#437, 0x3). Only character creation's race and class lists read it, and on P99 they are unchanged.
- Zones without a sky are lit as the original lights them, so the Erudin palace and other interiors are no longer too dark (Adam, 11-23). See [Zone lighting](docs/zone-lighting.md).
  - **Baked light:** each terrain vertex's colour from the zone file, and each placed object's colour track, is now drawn. Before, both were dropped at load.
  - **The engine's single-pass formula:** each vertex sums the ambient by its alpha past the 0.12 offset, its own light at full weight, and up to three point lights. The sum saturates once, then multiplies the texture.
  - **The ambient** comes from the zone header's time type, as in eqgame.exe 0x441295. Type 4 (the palace) gets 0.5 grey and other types 0.1. Types 1, 2 and 5 follow the day table, read from the installed eqgame.exe. Vision raises the ambient to the floor.
  - **Point lights:** the zone's placed lights and carried torches light zone geometry per vertex, worked out on the CPU, in place of Bevy's lights. Which lights each mesh takes is inferred until the engine's light lists are read.
  - **Tone curve:** the scene camera no longer applies one, inside or out, as the original applies none. Colour grading stays.
  - **Known departure:** the floor's fade while you carry a light isn't modelled.
- Petitions and reports as the Titanium program takes them (read R49-23,
  checklist 6.511, C1a), on eq-network #414.
  - **`/petition`** sends its words and echoes them in the installed line
    0x3025; with no words it prints 0x3332 and sends nothing. Our own "Your
    petition was sent." line stays for Mac-era servers, whose program is
    unread.
  - **`/guidehelp`** is the same text form, with lines 0xde8 and 0xde3.
  - **The petition window:** where the world turns it on, the program
    toggles a window and ignores the words. Which skin screen that is is
    unread, so this client takes the text form there: a departure, labelled,
    with a TODO.
  - **Typed Reports** (new setting, a departure, on until the report
    windows are built in C1b, then off): `/bugreport` and `/feedback` send
    the typed words, as before. The program ignores the words and opens its
    windows; with the setting off this client ignores them too and says it
    has no window yet. A bug report prints 0x22ae, or 0x22b0 with no words;
    feedback prints nothing, as the program's window does.
  - Feedback now carries the player's place and heading, which Titanium's
    feedback sends. Stock EQEmu now offers feedback and drops it, as it
    drops the program's. Project 1999 offers none of the three.
  - No repin of its own: it uses main's eq-network pin (614d1c0, #635),
    which carries `reports::petition_form` and the feedback's place.
- A Bard's song stop says the Titanium program's line (0x40f06d, read R49-21), from eq-network #421's `WorldEvent::SongStopped`. It shows in the program's colour 0x108, in the installation's words (string 0x318f), or this client's where the installation lacks them. A gem click that stops the song now shows that line, not a cast refusal. As read, the program hides the line when the Bard songs filter is "mine only" (filter 9 holding 1), and so does this client, through the local filter gate (`chat_filters::hidden_locally`). Read R49-48 e asks to confirm that compare. On Titanium, where the program's line is the only one, this client's own "Song stop requested" and "Song stop unconfirmed" lines no longer show beside it. The Mac-era servers, which send no program line, keep them.
- A Bard's `/melody`, as the Titanium program's (0x472156, read R49-33 c; gap 3.5, plan approved with Y1 to Y4). It is offered where the session offers `Capability::Melody`, on P99 (by Adam's word) and Titanium EQEmu. Elsewhere it answers as an unknown game command does.
  - **`/melody 1 3 2`** clears the list, then, unless a song is sung, plays gems 1, 3 and 2 in turn. It presses the first at once through the same gem press as a click, with the same refusals and lines.
  - **Each refresh of the player's own gem** sends the song stop, then presses the next gem, cycling. Nothing else moves the melody (no timer, no retry), so with no refresh after a refused press it stalls ([I]; read R49-48 f is filed for the program's index on a refusal).
  - **`/melody` while a song is sung** sends only the stop. With no numbers it only clears the list.
  - **Gems 1 to 8** are taken. The program takes 9, which this client has no gem for. How the program splits the numbers isn't in the read, so a run of digits is one number here ([I]).
  - **Only a Bard's `/melody` does anything.** Anyone else's sends nothing and says nothing, as the program's.
  - **A gem click while singing** clears the list, the user's and a script's alike (0x47e8e0). A new admission starts with none. Scripts run it as typed: `slash("melody 1 2")`.
- `/pet target` and unknown `/pet` words, as read R49-22a reads the Titanium program (checklist 6.610, gap 3.5).
  - **`/pet target`** stays local and sends no pet order.
    - It targets the player's pet through the same selection a click on the pet uses.
    - It then says the program's line (string 0x3330). With no pet it says string 0x3323.
    - Both lines come from the installation, in this client's own words where the installation lacks them.
    - Before, no pet gave a refusal in our words. That the word is "target" is inferred (row 22a-iii).
  - **A `/pet` word that names no order** sends nothing and says nothing, as the program's sender skips it (0x49975b). Before, this client said "Unknown pet command".
  - **The Pet Info window:** the Options window's Pet Window Popup already decides whether a new pet shows the window (default on). That matches the program's auto setting; its default is row 22a-iv.
  - The network side, which sends an order with no pet, is eq-network #411. This client has no pet refusal of its own.
- Buffs landing on the player take the slot the Titanium program's whole slot finder gives them (checklist 6.612, read R49-22e, repin to eq-network #417). That includes step 4's replace in place, worked out from each effect's base, formula and max as 0x40f339 does, and step 6's eviction. The facts come from the installed spell data: effects, the Bard level and the band, with each held buff's stored level from its record.
  - A buff the program removes for the landing is dropped as a prediction. If the server's word still holds it afterwards (a record for it, or the next zone-in's table), its word stands, and the disagreement is logged with the rule and both spells (6.612 T1).
  - A value the client can't work out still leaves the landing unslotted. That covers a random formula, one that reads hit points, or one that needs a level we don't know. The old guess for a possible step-4 replace is gone.
- Seven local-settings slash commands work as the Titanium program's do (checklist 6.525, read R49-22c). They change only this client: nothing goes to a server, on any server type, and scripts can run each with `slash()`.
  - `/clearchat` clears the active chat window's view, silently. The history keeps its lines, so scripts, the log and other windows still have them.
  - `/showspelleffects` and `/combatmusic`: "off" turns the option off and any other word turns it on; with no word they give their usage. Each ends by saying where the option stands. `/combatmusic` now takes the read's words instead of toggling, and says the installation's lines. This client plays no combat music yet, so it changes nothing audible.
  - `/particledensity` takes 0 to 3, or a word starting OFF, LOW, MED or HIG, for the session only. This client draws spell particles fully or not at all, so level 0 hides them and 1 to 3 show them. It is its own flag: `/showspelleffects` still says what it said.
  - `/mousespeed N` keeps and reports the pointer speed as typed. This client leaves the pointer to the system, so it moves nothing.
  - `/chatfontsize 0` to `6` sets the chat's font. Labelled departure: the program sets only the active chat window's size, and this client sets one size for the whole chat. `/usercolor <kind> <red> <green> <blue>` sets a text colour (a word that is not a number keeps that channel), `/usercolor <kind> reset` puts it back, and `/usercolor help` or `page1` to `page7` list the kinds.
  - Lines with string ids come from the installation, with this client's words where it lacks them. The program's English-only lines are in this client's words. Choices are saved in this client's per-character settings, never in `eqclient.ini` or the UI ini.
- A Bard's song holds nothing back, as the Titanium program's doesn't (read R49-21; gap 3.5, checklist 6.442, 6.542). This repins eq-network to 33057d9 (#413, and #407's server filters), on P99 and Titanium EQEmu. The session now tells the client which song a Bard is singing (`WorldEvent::Singing`), and the client's own gates follow it:
  - **A gem press while singing** goes to the session, whatever the gem's timer or the mana, and the session stops the song and casts nothing, as the program's gem click does. A forget goes too.
  - **An item use while singing** isn't refused by the client; the session lets it go alongside the song.
  - **Memorizing while singing** isn't held back by the song ([I], read R49-48 a).
  - Any other cast holds as before, including an item cast started alongside the song.
  - The gem timers the program resets when a song stops are not reset yet (a labelled follow-up).
- The four new server filter kinds from eq-network #407 get this client's words on the filters page: Item speech, Strikethrough, Stuns and Swarm pet deaths. The rest of #407's client side comes with #624.

- Answer `/played` as the Titanium program does (read R49-8 a, checklist
  6.490b): the account's entitled time in years (line 0xe24), when the
  character was made (0x32a8) and its played days, hours and minutes
  (0x32af), from the profile's fields (eq-network #410), adding a minute
  each minute after the profile as the program does. The creation time's
  wording follows the program's `%#c` in the C locale (inferred). The
  Mac-era and RoF2 clients' `/played` is unread, so it's refused there
  for now.
- A Bard's item use now carries the song test from the installed spell data (gap 3.5, checklist 6.542, read R49-21b): the item's spell counts as the class's when the player's class level for it is neither 0 nor 255. The session uses it to choose, for an item's song it keeps, between sending the stop and ending it here. Without installed spell data, or without that spell's class levels, the session keeps its memorized test as before. Labelled: the program also needs the spell's byte +0x1c4 to be clear. That byte isn't mapped to a spells file field yet (read R49-48c), so a class spell with it set counts as a song here and not in the program.
- Keep the server filters as the Titanium program keeps them (read R49-2,
  checklist 6.494a to c), on eq-network #407.
  - **Rows and choices:** the Filters page lists the program's 28 rows in
    its order but swarm pets' deaths, which has no row there either, each
    stepping through its own choices in the program's order. Bad words start
    filtered, as there. The page takes the place of the skin's own Filters
    page, whose controls are not wired, so the window shows one Filters tab.
  - **Keys:** the options file keeps each row under the original's
    `eqclient.ini` key (`Channel00`, `BadWord`, `PCSpells` and the rest) as
    its choice's index; an index outside the row's choices gives its
    default. They stay per character, this client's choice (the original
    keeps one set per install). The `filter_<kind>` lines of the first
    filters change are no longer read, so **every saved filter choice
    falls back to its default once** after this update; set them again on
    the Filters page.
  - **`/filter`** shows the Options window on the Filters page, reading no
    words and sending nothing, as the program does.
  - **At once on Titanium:** a change goes to the server at once on EQEmu
    and P99 alike, as the program sends it again on every change; Mac-era
    servers take it at the next zone-in. P99's first use is attended.
  - **Scripts:** `filter.<kind>` reads a row and `click("filter", kind)`
    steps it.
  - **Page note:** shorter, and wrapped within the page (6.494d's look
    found it ran past the window's edge).
  - A hidden row hides only where the server honours it; the program's own
    local hiding (stuns, others' environmental lines, spell and damage
    lines) is the next change.
  - Uses main's eq-network pin, which carries #407; no repin of its own.
- Hide locally what the Titanium program hides by its filters, for the lines
  this client prints (read R49-2 d, checklist 6.494, C2). With **Missed me**
  hidden, another's melee attack on the player that does not land is not
  shown (the damage display's test at 0x4a6631; which outcomes it covers is
  inferred). The program's other local tests gate lines this client does not
  print yet (stuns, others' environmental damage, others' fights, others'
  spells, the bad-word mask): each is a named TODO at
  `chat_filters::hidden_locally`. Mac-era and RoF2 programs are unread and
  hide nothing locally.
- Show the session's chat pace (checklist 6.523, eq-network #354). Its
  refusal, "You are sending chat too fast. Wait N seconds.", is printed as
  the player's own line. A script fails at a step's line with those words
  when the pace holds chat that step sent: a `chat`, `social` or `gm` step,
  or a `slash` step whose typed line sent chat (`say`, `ooc` and the rest).
  The pace is this client's own: the Titanium program paces no chat (read
  R49-17), so it is a departure, not parity.
- Follow program read R49-6 on Titanium (checklist 6.492). Each duel outcome the session tells is the program's line in its colour, and `/duel` sends the target as it stands, with the session wording the result. The **Duel Answer Card** is a quality-of-life setting, on by default: Yes accepts as `/duel` does, No sends nothing, and off there is no card, as in the program. Scripts can answer it. A quest's coins are said one line per coin with the giver's name, then the reward sound plays (also for EQEmu's fixed sound record). A yell for help names where the yeller stands from your heading. A level lost is said, and a level gained plays its sound. Another spawn's stat increase has the program's line. `/lfg on` and `off` say the program's lines, and any other form is its usage line. `/motd` says the kept message in the program's line, and `/charinfo` names the bind zone in it. The Mac-era and RoF2 programs are unread and keep this client's words. Repins eq-network to bf7a4db.
- The Interface scale slider on the QoL page no longer swings between its ends while dragged.
  - **Cause:** each frame of a drag set the scale, which moved the Options window and the slider under a pointer that held still. The pointer then read near one end and then the other, so the scale flipped back and forth and could not be set back to 100%.
  - **Fix:** a setting that rescales the layout (`Level::rescales_layout`, the interface scale alone) is held on its slider while pressed. The thumb and value label show the held size, and the setting changes once as the press ends, by the mouse's release or a script's one-frame press. Closing the window mid-drag discards the held size. The other sliders still set as they move.
  - **Recovery:** right-click the slider for 100%, or type `/uiscale` (100%) or `/uiscale N` with N a 5% step from 50 to 200. Scripts can use `slash("uiscale 100")` or the slider as before. The options file's `ui_scale` key still works too.

- Mouse look in the chase view tilts as far as straight up or straight down again, at any zoom (checklist 6.606, Adam 11-23). The chase camera had kept the Titanium program's height model, from eye level to 30 units above the eyes, which at the ordinary zoom tilts only about 15 degrees and never looks up.
  - **Free Chase Tilt**, a new quality-of-life setting, on by default: a drag turns the chase camera around the eyes up to 89 degrees above or below level, and the wheel moves it along its sightline. Looking up, the camera swings below the eyes, never under the feet, and floors and walls still pull it in. An almost overhead view still zooms before it snaps into first person, and the far plane follows the camera out.
  - Turned off, the chase camera keeps the program's height model as before. In the Titanium program (read of 11-23, checklists 6.348 and 6.422), mouse look Y in that camera only zooms or moves its height, only the orbit cameras tilt, from about 45 degrees up to 84 down, and no third-person camera follows the character's look pitch. Straight up or down in every view is Adam's choice. First person looks straight up or down either way.
  - A script's `camera` step takes a pitch from -89 to 89 degrees, so a script can look up as a player can.

- `/loadskin <name>` switches the UI skin in game (checklist 6.605), as the official client's command does. The windows redraw from the new skin at once, and the choice stays the character's for later runs. A name matches the installation's `uifiles` folder whatever its letter case. A skin that isn't installed changes nothing and says so, and an unreadable line gives the usage. The official client's trailing `0` or `1` is accepted, and this client keeps window placements either way. The choice is saved in this client's settings folder, not in the installation's files. A skin loaded in game wins over `--ui-skin` until the player camps or plays another character or world, when `--ui-skin` applies again. What the official client says in reply isn't read, so the replies are this client's own words. Why: the Velious skin's inventory shows the bags alone, with no paperdoll, statistics or hit point and mana numbers, and its equipment sits in Hot Button window 1. `/loadskin default` brings the full inventory back without restarting.

- `/loadskin <name>` switches the UI skin in game (checklist 6.605), as the official client's command does. The windows redraw from the new skin at once, and the choice stays the character's for later runs. A name matches the installation's `uifiles` folder whatever its letter case. A skin that isn't installed changes nothing and says so, and an unreadable line gives the usage. The official client's trailing `0` or `1` is accepted, and this client keeps window placements either way. The choice is saved in this client's settings folder, not in the installation's files. A skin loaded in game wins over `--ui-skin` until the player camps or plays another character or world, when `--ui-skin` applies again. What the official client says in reply isn't read, so the replies are this client's own words. Why: the Velious skin's inventory shows the bags alone, with no paperdoll, statistics or hit point and mana numbers, and its equipment sits in Hot Button window 1. `/loadskin default` brings the full inventory back without restarting.
- Every other spawn's helm follows its own show-helm flag, as read R49-13 reads the Titanium program (checklist 6.513). This rebuilds #567 on main.
  - **The flag** comes from the spawn record's byte 139 and appearance kind 43, and the head slot's model is nothing while it is off. Players, NPCs and corpses alike: the program's head code has no distinction. `EQEmu` sends a player's own option, and for an NPC whether its helm texture is set.
  - **When it shows:** a kind 43 for another spawn only sets the flag, with no redraw, so its helm changes at that spawn's next head wear change (inferred, as the read gives).
  - **Your own** follows the Show My Helm option at once, as before.
  - The flag counts only where the session offers Show My Helm (`Capability::ShowingHelm`, stock `EQEmu`). Project 1999's records aren't checked, and `EQMac` and `RoF2` records never say it, so every helm shows there as before.
- Keep the latest title list (checklist 6.516), lifted onto main from #574. Where the session offers titles (stock EQEmu only), the core keeps the list the server last sent in this zone (`ClientWorld::titles`), forgotten on a zone change; elsewhere a list is ignored. Nothing asks for the list or chooses a title yet. Read R49-14 has now read the program's title window, but titles are outside Project 1999 and Quarm, so the window waits under Adam's 11-23 priority rule.

- Scripts can use the map's own controls (checklist 6.589), since a script may do whatever the player can (Adam, 11-23): the search boxes, the watch controls and rows, a spawn's marker, the inspector's Clear and Target, the landmarks and the presentation controls, as `map_search`, `map_watch`, `map_marker`, `map_inspector`, `map_target`, `map_landmarks` and `map_style` targets. Each click runs the map's own handler, so the Target button keeps its capability and range checks. A marker takes the spawn's admission, id and revision, which the report's new `map_markers` field lists for the 64 nearest spawns the map can place (a spawn with no finite position is left out, as the map leaves it out); a stale one is refused. Watches, landmarks and presentation are saved settings, and these clicks change them as a person's do.
- Destroy the coins on the cursor (gap 3.8, checklist 6.512), as the
  Titanium program's Destroy button does (read R49-11). With coins on the
  cursor and no item, **Destroy cursor item** asks first, with the
  installation's own question (string 0x80d), or our words where the
  installation lacks it. With no answer in 10 s the question closes as a No.
  On Yes it sends one `DestroyCoins` (eq-network #342) for the first kind on
  the cursor, the most valuable first, whole, as the program sends one move.
  - **Departure, labelled (6.512c):** the program then clears every kind on
    its cursor, including kinds it never told the server about. This client
    keeps those shown, as the server still holds them. Its own coin controls
    hold one kind on the cursor at a time, so two kinds come only from what
    the server says the cursor holds.
  - The program skips the question when its ini `Destroy` key is set. That
    option is row 6.512b, not built here.
  - Where the session doesn't offer `Capability::DestroyingCoins` (all but
    stock EQEmu for now) it says "This server does not take destroying
    coins." and asks nothing. An item on the cursor is destroyed as before;
    its question in the program's own string (0x7fb) and timeout is row
    6.512d.
  - Any change to the cursor's coins cancels the question. Scripts click the
    same control and dialog, so they destroy coins as they destroy items,
    and a script that answers nothing gets the timeout's No.
- Send petitions, bug reports and feedback from a typed line (gap list 3.6,
  recording checklist 6.511). `/petition`, `/bugreport` (also `/bug`) and
  `/feedback` with their words send `SendPetition`, `ReportBug` and
  `SendFeedback` where the session offers them, which today is stock EQEmu
  for petitions and bug reports. A bug report's leading `word:` names its
  category where it is one, else the line is the text under Other; it
  carries the player's place and heading and the selected entity. A typed
  line sends a report, whoever types it: the keyboard, a social's line or a
  script's `slash()` step, since a script may do whatever the player can.
  A button's game command answers as before.
  Stock EQEmu says nothing on success, so this client's own line says the
  petition or bug report was sent; feedback gets none. Refusals from the
  session show in chat. Repins eq-network to 66900af (#339), which also
  brings the duel offer and `/surname` answers and the session's report of
  each server message it could not read (none of them shown or logged yet).

- Keep server filters per character and give them to the session (parity
  item 6, recording checklist 6.494). Options gains a Filters page with one
  row per kind the generation's packet has a word for (24 on Titanium, 16 on
  Mac-era servers): a button steps through the kind's modes and a label says
  the current one. The modes are kept as `filter_<kind>` lines in the
  character's options file; a mode no generation can say keeps the default,
  which is the packet sent before. The character list gives the session the
  chosen character's saved set, so the first zone-in carries it; a change in
  play is sent at once where the session offers it (EQEmu) and otherwise
  goes with the next zone-in, as the page's note says. The kinds, words and
  page are this client's own until the official options, ini keys and
  `/filter` forms are read (6.494a to c); `/filter` answers that it is not
  taken that way yet.
- Drowning and lava now hurt on every Titanium server, public EQEmu and
  P99 included, as the Titanium program reports them on any server
  (eq-network's liquid change; read R25). **Characters on public servers
  can now drown and burn with this client.** Take Environmental Damage,
  on by default, still withholds every report; a test shows a P99-style
  session reports neither liquid with it off. `docs/settings.md` says so.
  Inferred parts are under checklist 6.491.
  - **The program's lines (reads R49-3 e and R49-3b).** After each of its
    own reports on a Titanium session, the client prints the program's
    amount line, then the record's own line for the type, in the
    program's colours: falls, drowning, lava, zone 200's lava and cold.
    The death path sends no record, so it prints the amount line alone.
  - **A record the server sends** for the player (a trap's, or `EQEmu`'s
    echo of the player's own non-fall record) prints the program's line
    for its type in place of a melee line. A trap's line is chosen by the
    record's spell field, as the program does. `EQEmu`'s echo prints the
    line a second time, as the official client does there, so log
    parsers read the same lines.
  - **Departure, labelled (6.491f):** the program takes an echoed
    record's amount off its local HP a second time. This client keeps HP
    the server's, so it does not.
  - **A drowning record from the server empties the air counter** while
    the head is under, as the program's does.
  - **The tick** runs strictly more than a second after the last, as the
    program's `> 1000 ms` (read R49-3 a).
  - **Lava takes any resist,** a negative one included, as the program's
    arithmetic does. Zone 200's type-244 heat and cold (type 246) have
    their formulas in `environment` but nothing samples them: zone 200
    needs disease resistance, and cold's regions wait on read R49-26.
  - Other players' environmental lines, which the program builds with
    their name under chat filter 0x11, are row 6.491g, not built here.
  - Repin eq-network to c52bc91, its main with #326 merged. It brings
    `WorldEvent::HazardReported` and `TRAP_DAMAGE_KIND`.
- `/guildstatus` is offered, as read R49-6k reads it (checklist 6.575).
  - The Titanium program answers it itself and sends nothing: for the player a typed name finds (`/guildstatus Name`, ignoring case), or else for the target.
  - Lines 12256, 12257 and 12258 give the rank (member, officer, leader) with the name and the guild. 12259 gives the name alone for any other rank, and 12260 refuses a missing spawn, one that isn't a player, or a game master. All come in message type 0x111.
  - A player in no guild gets this client's own words, because the program's are its own and not in the strings.
  - Still inferred: that +0x256 is the GM flag (read R49-4 takes it so), and the name lookup's exact rule. Anonymity isn't tested, so an anonymous player's guild shows as the server sent it. The Mac client's answer wasn't read, so it is refused there for now.

- Scripts can use the map's own controls (checklist 6.589), since a script may do whatever the player can (Adam, 11-23): the search boxes, the watch controls and rows, a spawn's marker, the inspector's Clear and Target, the landmarks and the presentation controls, as `map_search`, `map_watch`, `map_marker`, `map_inspector`, `map_target`, `map_landmarks` and `map_style` targets. Each click runs the map's own handler, so the Target button keeps its capability and range checks. A marker takes the spawn's admission, id and revision, which the report's new `map_markers` field lists for the 64 nearest spawns; a stale one is refused. Watches, landmarks and presentation are saved settings, and these clicks change them as a person's do.

- Speak and hear languages as the Titanium program does (checklist 6.495, reads R49-4, R49-4b and R49-4m). This replaces the unmerged design of #558.
  - **The choice.** Each chat window speaks a language of its own, an index into the languages the character knows, for the session only. It is never saved, and there is no setting for it (Adam, 11-23).
  - **`/language`.** The installed lines 0x33cc, 0x33cd and 0x33ce, in message type 0x111. The help word lists the languages, nothing says the current one, and a number chooses one. Scripts use `slash("language ...")`.
  - **The send.** Every channel carries the active window's language. The skill is worded by eq-network's `Tongue::spoken`: your skill less intoxication >> 2, or 100 for a game master. A hail goes out in it too.
  - **Hearing.** Lines are tagged with 0x583 (the name) or 0x584 (unknown), and language 0 has no tag. Then two garble stages run:
    - the speaker's stage: this client's own short-word list, then the program's per-letter rule;
    - the listener's stage, only where the world's log-server switch is on.
  - **Not garbled:** channels 6 and 14, and any line when a game master is on either side. A game master sender is found by name in the zone (inferred).
  - **The away reply.** While AFK, a tell that got through the ignore list gets 0x58b back.
  - **Inferred or left out:**
    - on the Mac client, `/language`, the send and the speaker stage follow the Titanium read;
    - an unlabelled program flag at the gate is taken as 0;
    - the program's bad-word mask is left out (no profanity filter here);
    - your own lines are never garbled;
    - the pet speaking as you on channel 8 is not built yet.
  - **Repin:** eq-network to #405, for `Tongue::spoken`'s `Speaker`, `PlayerState::intoxication` and `WorldRules::listener_garble`.
- The window's title bar says **Marginal Advantage** (and **Marginal
  Advantage offline viewer** for the offline viewer) in place of
  `eq-client` (eq-client #447).
- **Zone Anywhere** (`zone_anywhere`, off) in the QoL page's Dangerous
  section, a gameplay departure (eq-client #448). It would let the player
  cross a zone line whatever level the zone asks for. That the official
  client turns players away below a zone's level is inferred, not read; this
  client blocks no crossing today, so the setting changes nothing yet.
- Say the server's message of the day again with `/motd` (checklist
  6.520). The message the world sent on this connection is said again as it
  came, to the same tab and colour, and is not kept as a new one; a new
  connection, to the same server or another, never says the old one.
  Without one, this client's line says "There is no message of the day."
  It sends nothing, on any server. The program's handler (0x46f698) is
  unread, so that it says the world's message and not the guild's, and
  that words after it are ignored, is inferred (pending R49-6j). A script
  may run `slash("motd")`, as it may `/loc` and `/time`.

- Say the player looks for a group with `/lfg` (checklist 6.515), where
  the session offers it (stock `EQEmu`, eq-network #344). Bare `/lfg`
  turns the flag over; `/lfg on` and `/lfg off` set it. Those two words
  are this client's forms, inferred until the program's handler is read
  (R49-6i), and any other word is refused in this client's words with
  nothing sent. No level range or comment is sent. The session tells each
  change, and the chat says "You are now looking for a group." or "You
  are no longer looking for a group." (this client's words, pending
  R49-6i). A social, a hotbutton or a script's `slash()` can send it, as
  typing does (6.584). Repins eq-network to #344's head; its chat channel events wait in
  the ignore arm for that thread's PR.

- Challenge the player targeted to a duel with `/duel` (checklist 6.492b),
  where the session offers it (stock `EQEmu`, eq-network #338). It sends
  `ChallengeDuel` only when another player in the zone is targeted, and
  says this client's line otherwise ("You must first target another player
  to challenge to a duel."). Words after `/duel` send nothing and say so,
  since the program may challenge by name and the target could be someone
  else. No line is said on sending: whether the program says one is
  unread. Every line is pending R49-6h. A social, a hotbutton or a script's
  `slash()` can send it, as typing does (6.584). Repins eq-network to
  e2e9a02 (#338); its surname and staff report events wait in the ignore
  arm for those threads' PRs, and the viewer drops its gap log events
  untraced (a gap's sample can hold other players' chat or names) until
  eq-client #563 takes them.

- Answer another player's challenge to a duel, and tell the news the
  client used to drop (checklist 6.492). Where the session asks (stock
  `EQEmu`, eq-network #328), the skin's confirmation dialog asks "<name>
  challenges you to a duel. Do you accept?" after a resurrection and a
  translocate. Only the player's own click on Yes accepts: a script step
  that reaches the dialog's Yes, by any name and by click, right-click or
  held click, is refused in this client's words and stops the script,
  while a scripted No still declines. No setting or key answers it. The
  question has no local deadline. A challenger not in the spawn list is
  "Someone". `InstalledClient::duel_offer()` names the installed client's
  string where it is read; it is None for every generation until R49-6a.
  The chat now tells, in this client's words until the program's strings
  and colours are read (pending R49-6a, 6d, 6f and 6g):
  - a challenge with no question to answer;
  - the player's own duel beginning;
  - a TAKP client turning the player's challenge down;
  - a nearby player's yell for help;
  - each stat a quest changed, with no number, since the record does not
    say whether it is the new value or the rise;
  - the coins a quest or task gave.
  The server sends no line of its own for any of these. Becoming a corpse
  stays unhandled until R49-6e. Repins eq-network to c7ac7ea (#328); its
  Apply Poison and server filter events are ignored here until eq-client
  #556 and #560 take them, and outgoing chat names `Tongue::COMMON`.

- Add RoF2 as a placeholder generation, following eq-network's
  (`GameDialect::Rof2`, `ServerProtocol::EqEmuRof2`). `InstalledClient::Rof2`
  is in `InstalledClient::ALL` but has no marker files, so no folder is ever
  detected as it, and the installation warnings name only generations with
  markers, as before. No preset offers or names `EqEmuRof2`, and eq-network
  refuses a configuration naming it. Each rule without a read of RoF2's
  program is absent, or Titanium's on its own arm marked
  `inferred for RoF2 (6.489)`. The skill rules read through a private
  `Program` (Titanium, `EQMac`). Repins eq-network for the placeholder.
  Nothing a player can reach changes.
- Take `/autoinventory`, `/hidecorpses` and a plain `/assist` as the
  Titanium handlers do (read R46). `/autoinventory` puts the cursor item in
  the first place the program's order finds, worn slots first, then the
  general slots and bags, in one move; with no room it prints the installed
  line and the item stays. `/hidecorpses` hides the corpses present but
  yours, or but yours and your group's, once; any other word shows them
  all again, and a new zone forgets what was hidden. The third word, whose
  flag is unread, is answered as not taken yet. A hidden corpse isn't
  drawn or picked by the mouse, and a name still finds it. A plain
  `/assist` with no target prints the installed two lines and sends
  nothing; a server that takes no assist keeps its earlier answers. The
  Mac-era client keeps its earlier answers. Inferred parts are
  under checklist 6.490. Networking pinned to 126b2c7 (eq-network #324).
  - After read R49-8, on a Titanium installation:
    - `/assist Name` looks the word up as the program does (0x47981d): any
      spawn whose name starts with it, the shortest first. It sends the
      assist for that spawn without changing the target. No match is
      silent; the player's own name prints line 0x57b and sends nothing.
      The main-assist words keep this client's answer.
    - Attack On Assist (`attack_on_assist`, default off, inferred): when on,
      the assist answer that sets the target starts auto attack through the
      attack key's own refusals. `/assist on` and `/assist off` set it.
    - `/autoinventory` with coins on the cursor puts them in the purse, one
      move of that kind, whole, and is silent past a billion of the kind.
      The inventory window's put-away button runs the same routine.
    - `/hidecorpses`' third word sets the looted-corpse mode: when the loot
      window closes, the corpse just looted is hidden unless it is yours.
      It doesn't hide corpses as they arrive. The show-all word and a new
      zone clear it (both inferred).

- Make every client-generation decision exhaustive, so adding a third
  generation (RoF2 is planned) breaks each place that must decide what it
  does instead of taking another generation's branch. The 31 one-variant
  checks on `InstalledClient` and `GameDialect` are `match`es naming every
  variant, and a test refuses new ones. Installation detection checks every
  generation in `InstalledClient::ALL`, and every installed archive is found
  through one module (`eq-client-assets/src/archives.rs`), still S3D only.
  Repins eq-network for its exhaustive `GameDialect` and `ServerProtocol`.
  No behaviour changes.

- Take the 88 social emotes and disciplines as the Titanium client does
  (reads R47 to R48c). An emote's first word picks its target: any spawn
  whose name it begins, the shortest name winning, or the current target
  with no word or the installed "my target" word. A target past 60 units
  (flat, origin to origin) is refused, as is a still emote (`/wave`,
  `/cheer`, `/frown`, `/gesture`) while moving or not standing. The text
  goes out as OP_Emote, the player's own line prints in the emote colour,
  and the animation is sent and played while standing still. Two or more
  words after the target replace the emote's own words. `/discipline` (and
  `/disc`) with no word lists the known disciplines whose timers run; with a
  word it uses the first known discipline the word begins. The new Combat
  Abilities window, opened from the EQ menu, lists the known disciplines with their time left: a click
  uses one, and a hold or Ctrl and a number puts it on the action bar. The
  typed command, the window, a hotkey and an Actions window button share one
  use: a discipline you don't know, can't use at your level or class, or
  that isn't flagged in the spell file is refused silently; one on its timer
  prints the time left; a timed self discipline while one is running prints
  its line; otherwise the cast goes out from slot 10 at the current target,
  or none, and like a gem cast it waits while a cast is under way or not yet
  acknowledged. Any discipline the server lists works, not only Evasive and
  Defensive. The Actions window's picker no longer offers disciplines, which
  have their own window now; a button that held one keeps it. Inferred parts
  are under checklist 6.488. This client's words where the installation has
  no line: "uses /NAME", "You use /NAME" (and "on TARGET"), "Stand still to
  do that.", "Nobody by that name is near.", "Your target is too far away
  for that.", "Your class has no disciplines.", "NAME can be used again in
  Mm Ss." (with hours, "Hh Mm Ss"), "You can't use that discipline right
  now." The Mac-era client keeps its earlier answers. Networking pinned to
  2865334 (eq-network #319, #320, #321, #322), which also settles a
  destroyed item on servers that confirm by silence; an emote goes out as
  the program built it, spaces kept, and its own line prints once. A social may now run `/target`,
  `/assist`, `/camp` and `/quit`, as the original's socials do; `/exit` and
  `/social` stay refused inside one.

- Take the Titanium slash forms socials and hotbuttons use (reads R45,
  R45b). Social lines run a tenth of a second apart, as the original's
  runner steps them, and `/pause` takes 1 to 600 tenths, larger or smaller
  numbers clamped. New commands, in a social or the chat: `/yell` (sends the
  empty 0x61ef and says "You yell for help."), `/stopcast` (sends the empty
  0x4839, as `/stopsong` does; refused while mounted, where any buff with
  the summon-mount effect, or with effects the spell data can't name, stands
  in for the program's own mount test), `/doability 1` to `10` (the Actions
  window's buttons, 1 to 6 Abilities and 7 to 10 Combat), `/memspellset`
  (a saved set by number or name start; it opens the Spellbook) and
  `/hotbutton NAME [COLOUR] TEXT` (writes the social and puts it on the
  cursor; the name is cut to 15 characters). The inferred parts are under
  checklist 6.487. Lines the installation has no string for are in this
  client's words: "You yell for help.", "You can't stop casting now.",
  "Use /doability 1 through 10.", "No spell set has that name or number.",
  "Made hotbutton NAME.", "Every hotbutton is taken." and the four
  `/hotbutton` help lines. Networking pinned to 57a87f6 (eq-network #318).

- Ask before looting a NO DROP item, as the Titanium client does (read
  R45b). This is a visible change to everyday looting: a click on a NO DROP
  item from a corpse that isn't yours now opens the confirmation dialog, and
  only Yes loots it. A question left 20 seconds loots nothing. `/lootnodrop`
  picks when it asks, by the words of the installed client's three lines or
  this client's `always` (always asks), `sometimes` (asks only about an item
  your class or race can't use) and `never` (never asks); with no word, or
  a word that names none, it lists the three. The choice is saved with this
  client's options for the character. It starts from the installed
  `eqclient.ini`'s `LootNoDrop`, and where there is none from `always`, which
  the program read doesn't give (inferred, checklist 6.487). Loot all takes
  what doesn't ask and leaves the rest for their own clicks. Whether the
  item is NO DROP is read from its flag, and whether the corpse is yours
  from its name, where the program's own tests are unread (6.487). The
Mac-era servers loot as before, as that client's loot click is unread, and
  `/lootnodrop` answers there as it did.

- Refuse untargetable spawns as the Titanium client does (read R44). On
  EQEmu and P99, a spawn whose body type the original's target setter
  refuses (11, 60, 65 and 67: EQEmu's zone controller and other utility
  spawns) is never chosen. A click on one keeps the current target, with no
  line and nothing sent, and doesn't pick the spawn behind it. Target Next
  and `/target` pass over it, and a script's `target()` finds nothing, as
  for a spawn that isn't there (inferred for these paths until their
  handlers are read; checklist 6.486). There is no interaction with body
  types 11, 65 and 67. A target the server sets is kept whatever its body
  type. Body types above 64 are neither drawn nor named, as the original's
  per-spawn update skips them; See Invisible doesn't reveal them, and only
  the QoL Show Invisible Entities does (whether the original draws their
  model is inferred, 6.486). The Mac-era servers target as before.
  Networking pinned to d0333db (eq-network #317).

- Swim as the Titanium client does (reads R33, R33b, R39, R41). Water now
  keeps the body's velocity from frame to frame and damps it, gravity still
  pulls in water so an idle swimmer slowly sinks (about 0.67 units a
  second), and the Swimming skill adds to swim speed: at run 0.7, level
  swimming goes from 4.7 units a second to 7 untrained and 17 at skill 100.
  Looking up or down adds a climb or a dive without slowing forward travel,
  a held jump pushes up at the original's rate, and rising out of the water
  falls back in as an ordinary fall. Water is stepped at a fixed 60 times a
  second (checklist 6.483). In thegrey a head out of water can't breathe,
  as in the original. Gills for Days still turns the air counter off.
  Characters in water now swim when moving and tread water when still,
  sinking or on the bottom, instead of walking and standing (read R43;
  checklist 6.485). Needs eq-network's per-axis swim bound.
- A jump never stands a seated player or ends a feign held standing (checklist 6.600, Adam 11-23: "jumping does not stand, just as jumping from sitting does not stand").
  - **Seated jump:** a jump press while sitting or kneeling to loot does nothing and stands no one, as the original client's jump waits on the can-move flag (read R19 c). The Mac-era client is inferred the same. With Allow Auto-Stand on, forward still stands a sitter.
  - **Feign held standing:** with Move While Feigned on, a jump from standing still keeps its crouch's wait but sends no duck or stand, so the feign holds. The session's new `FeignHeld` report says when one is held. Without the option, a feigned player lies, and a lying jump stays blocked.
  - The Move While Feigned text in the settings and the guide now says a jump keeps the feign.
- Repin eq-network to 86970af, its main with #398 merged (6.599). The repin brings `WorldEvent::FeignHeld`, the seated-jump refusal, and eq-network main through b9a5668. That includes the Quarm and TAKP offers 6.580 to 6.598 and the 6.602 boat-control read. It also brings `TrainingOffer::price_scale`, which is None in every literal here.
- Play each zone's music and ambient sound from the installation's own files (item 6.503).
  - **Emitters** follow the Titanium program's loader (read R49-7 first pass and its engine follow-up). `eq-client-assets` reads `<zone>_sounds.eff`'s 84-byte records:
    - one emitter for a type-2 record or matching day and night fields, else a day one and a night one, each with its own type, sound, delay and volume;
    - a radius below 0 reaches 10,000;
    - positions are in the files' own frame, whose x and y are the server's y and x (inferred from the local look, where qeytoqrg's regions met the ground only swapped);
    - a delay above 0 repeats after delay + 500 plus a random share;
    - a positive volume is a millibel attenuation (2000 is a tenth, above 10000 silent); otherwise 0.2 for type 0 and 1.0 for the rest.
    - a music record's sound number of 0 or more is the `<zone>.xmi` sequence it plays, counted from 0 (read on the music sign: 0x44bc19 to 0x4c6a99). A negative number names an `mp3index.txt` entry, which this client does not play yet; it is logged once a zone.
  - **The bank.** `<zone>_sndbnk.eff`'s EMIT list is numbered from 1 and LOOP from 162, with CR and spaces stripped. Inferred and labelled: the linear falloff, a bank name's `.wav`, the ms unit, a delay of 0 looping, and ids 32 to 161.
  - **Voices.** Emitters play on at most eight voices of their own, so cues always keep theirs.
  - **Music.** The music region the player stands in plays through `rustysynth` 1.3.8 (MIT) on a SoundFont the player names with `--soundfont`. There is no music without one, a stated departure. The `.sf2` files an installation may ship are refused by `rustysynth` as they stand, so the player names another. Regions follow the program's chooser (read on zone music tracks): the smallest sphere the player is in wins, night is the hour below 5 or above 18, a switch fades over 1500 ms and leaving every region over the record's +0x44 / +0x48 (at most 20000 ms), and EnvSounds off silences music regions too. A new track's fade-in (+0x4c) is inferred and not applied.
  - **Options.** New per-character options: Music, Music Volume (wired to the skin's slider), Environment Sounds and Combat Music. A character without its own choice seeds them from `eqclient.ini`'s `Music` (0 is off), `EnvSounds` and `CombatMusic`.
  - `/combatmusic` toggles and saves combat music, saying in this client's words that it plays none yet.
  - A `nosound.txt` in the installation turns all sound off.
  - Unknown record types and negative (mp3-list) music ids are logged once a zone and not played. Everything stops on a zone change, camp, logout or lost connection.


- Scripts can click spawns, ground objects and doors in the world by id (checklist 6.587), since a script may do whatever the player can (Adam, 11-23). `click("spawn", admission, id, revision)` targets a spawn (two of one name included), `right_click` loots, trades, boards or considers it without changing the target, `click("object", admission, drop_id)` picks up an item or opens a world container, and `click("door", admission, id)` makes a door's two clicks. Each click goes through the scene's own pick, with the script's pointer as the cursor: it acts only where the pick names that same thing right now, so a thing behind a wall, off screen or under a window is refused and nothing is sent. The report's surroundings line gives the admission and each nearby spawn's click tuple. A run that stops mid-click hands the scene back the real pointer the next frame.
- Apply a poison by right-clicking it (item type 42) in a top-level slot or on the cursor, as the Titanium program does it (6.493, read R49-1). eq-network's session sits the player, waits 10 s seated and sends; this client prints the program's lines: the start (0x305e), the refusals that carry one (0x305f no skill, 0x305d not sitting) and a posture change's cancel (0x30d1), all in colour 0x111, and the server's answer (0x3269 applied, 0x326a failed) in colour 0x10e. A poison in a bag falls to the ordinary item use. The sit is the session's posture event; scripts reach the same right-click path and notices. The wait is effect 234's total (6.493b, read R49-1f): this client sums the effect from buffs (type-4 records negated, as 0x41002f) and usable AAs (0x40feaa), not worn items, and tells the session (`ClientCommand::PoisonWait`, eq-network #404) whenever it changes; where any part is unknown it tells None and the session keeps 10 s. No script, slash line or social can post the total. Repin eq-network to bf7a4db (its main with #402 and #404), bringing `WorldEvent::PoisonStarted` / `PoisonInterrupted`, `ClientCommand::PoisonWait`, `DuelNews`, the player's `intoxication` and `ToggleStance`.

- Scripts can read the group window, the effects window and the last tell (checklist 6.593, O2).
  - **Group:** `group.present`, `group.count`, and `group.member1.name` to `group.member5.hp_percent`.
  - **Effects:** `buffs.count`, and `buff1.spell` to `buff50.spell` in the window's order.
  - **Tells:** `chat.last_tell` and `chat.last_tell_from`. Like every field, each reads only what a window already shows.
  - **Kept private:** a run that reads the tells writes neither to a log, a `report()` line, its events (which the control status and Runbook export carry) or a gap record, and a test checks it.

- Escape on the login screen quits the game when nothing is logging in, as the official client's does (checklist 6.601, Adam 11-23; the Mac-era client's the same, inferred). Anything else on the screen takes the press first: an open window or the corner menu closes, and a login under way stops. The Escape that brings character select back to the login screen never also quits; the next one does.

- The player chooses how long a script run may go, on every server (checklist 6.594; Adam, 11-23: "If people want to script indefinitely, let them").
  - **Choices:** the default keeps today's bounds (15 minutes, 5,000 statements, 500 actions, loops of 100). A number of minutes from 1 to 1440 caps the time and lifts those counts, and no limit lifts them all.
  - **Where it's set:** only the workbench's length selector, which starts at the default on each launch, and the new `--script-run-length=default|MINUTES|none`. No statement or file line can set or raise it.
  - **What stays:** every length keeps the same action, chat and server paces, per-step waits, input rules and local-only `gm` fixtures. Stop always ends the run on the next frame and releases held input. The history stays at 64 entries.
  - **Loops:** `while condition:` may leave out `limit=`. Under the default length its 5,000 statements still bound it. `range()` takes counts past 100 under a longer length.
  - **Execution tab:** the status line shows the time so far and the length chosen.

- Scripts can read the gem bar and the time (checklist 6.592, O1b). `gem1.spell` to `gem8.spell` name each gem's spell, and `gem1.ready` to `gem8.ready` say whether the gem is lit by the gem bar's own rule. `time.hour`, `time.minute`, `time.day`, `time.month` and `time.year` read what `/time` tells, with the installed client's hour, and are unknown where `/time` says the time can't be told.

- Scripts can order and delete spellbook entries (checklist 6.588): `click("book_edit", "earlier" | "later" | "delete" | "confirm" | "cancel")` presses the spellbook's own buttons for the chosen row. A button the session doesn't offer (deletion on P99) is refused with the greyed button's reason, and nothing is sent.

- Scripts can read more of what the player sees (checklist 6.592, O1), since a script may do whatever the player can (Adam, 11-23).
  - The new fields are `player.hp_percent`, `player.posture`, `player.casting`, `player.stunned`, `player.loc_x`, `player.loc_y` and `player.loc_z` (as `/loc` gives them), `player.exp_percent`, and the purse's `player.platinum`, `player.gold`, `player.silver` and `player.copper`.
  - The target adds `target.hp_percent`, `target.kind` and `target.con` (after a consider), and the pet adds `pet.present`, `pet.name` and `pet.hp_percent`.
  - Each field reads only what a window or typed command already shows. A target's distance and level, and the player's weight and heading, have no field, because nothing on screen gives them. Each field keeps the existing unknown and stale rules.

- Scripts can click with Shift, Ctrl or Alt held (checklist 6.586). `click_with(keys, target...)` and `right_click_with(keys, target...)` take `"shift"`, `"ctrl"`, `"alt"` or a chord of them. The keys go down with the button's press and up the frame after its release, through the same input a person's keys use, so every window reads them unchanged. That reaches an item's click effect (Alt + right-click), whole-stack and single-item moves, all-coin moves and link composing, which no script could do before (Adam 11-23: scripts may do whatever a player can). A run stopped mid-click releases the keys.

- A script's `slash(...)` line runs exactly as a typed one (checklist 6.584), since a script may do whatever the player can (Adam, 11-23).
  - The step takes any line the player could type, up to the chat line's 512 bytes and without control characters, instead of a list of allowed commands.
  - The line goes through the chat line's own path: abbreviations first, then `/reply`, `/rtarget`, `/friend`, `/ignore`, `/who all friends`, sharing, `/follow`, `/quit` and `/exit`, the client's own commands, the game commands and the chat words. The same rules, paces and refusals apply, and the line carries the script's admission.
  - A social, however it is spelled, still goes through the named `social` step.

- Scripts can read the AA facts (checklist 6.582). `report([label])` logs a new "Script alternate advancement report" line with an `alternate_advancement` object, numbers only: Titanium's definition count, selections, trainable ranks with their cost, activatable ranks, points, progress, unspent points and timers, and the Mac client's ranks, selections, points, progress and timers. Each list gives at most 64 entries and its full count. The core gains `definition_count`, `selections` and `activatable`, for display only.
- Repin eq-network to 2c89198, keeping today's behaviour apart from the AA requests (checklist 6.581). A local EQEmu or local TAKP session now offers the AA requests, so `train_aa`, `activate_aa` and `aa_split` go there; everywhere else they are refused as before. On the Mac-era client only the split goes. The repin also brings Quarm giving, groups, listing and translocate from Quarm's own handlers (6.576), TAKP looting from its own corpse handlers (6.577), and the hosts' script holds lifted in the docs (6.578). These add no event: the client already handles each one.
- Scripts may chat, emote, roll, run socials and send `gm` lines on any server (checklist 6.579), since a script may do whatever the player can (Adam, 11-23). `chat(...)`, `slash("emote ...")`, `slash("random ...")`, `social(...)` and plain `gm(...)` steps no longer need a local test session: a `gm` line goes as a typed `#` line would. They still need a character in the world, and the session's chat pace holds whatever sends a line. Only the `gm` fixture forms (spawn, depop, guild, stun, setlanguage, traindisc) keep the local test session. A synthetic chat submission or hotbar press of a social still points to the named `social` step.

- Repin eq-network to b631816, keeping today's behaviour (checklist 6.572). The repin brings the bandolier sets, the swap and belt use (all unoffered), and augments read from Titanium item records (6.570).
  - **Augments:** an augmented item no longer makes the inventory unreadable. `InventoryItem::augments` is filled in every literal.
  - **Weight:** the session's new `WeightUnavailable::Augments` reads exactly as `Container` does: an unknown load, never a lighter one. A worn augmented item makes equipped stats unconfirmed, so the speed inputs stay unproven.
  - **Ignored for now:** `WorldEvent::BandoliersUnknown` and `BandolierRefused` go in the ignore arm with the bandolier sets, whose windows are parked (P99 and Quarm features first).

- `/toggleinspect` built, held for read (checklist 6.574).
  - The client keeps an InspectOthers option, on by default, which is today's behaviour: it answers others' inspections whenever the character's note is known.
  - While the option is off, the session is told no note, so it answers no one. eq-network already treats that as no answer.
  - The command stays unoffered behind `TOGGLE_INSPECT_READ`, so it answers "This client does not take /toggleinspect that way yet" as before. Read R49-16 gives its lines, its words and where the official client keeps the option, and nothing is saved until then. `/inspect` (LDoN's treasure inspect) is post-era and not built.
- `/guildstatus` built, held for read (checklist 6.575).
  - Titanium's patch maps no `OP_GuildStatus`, so the client answers from its target. It uses EQEmu's `Handle_OP_GuildStatus` lines in the installed client's strings: 12260 (target a player), 12259 (no guild), and 12258, 12257 or 12256 (leader, officer, member, with the guild's name from the world's list).
  - It gives no line for an anonymous target, for a guild the list doesn't name, for a rank not yet known, or for a string the installation lacks.
  - That the program answers locally, from its target, and in these strings is inferred. The command stays unoffered behind `GUILD_STATUS_READ` until read R49-6k. `/sheathe` and `/aggressive` have no EQEmu handler and wait for R49-6n.

- Scripts can ask for AA requests (checklist 6.569). The new steps are `train_aa(RANK_ID)`, `activate_aa(RANK_ID)` and `aa_split(PERCENT)`, in this client's own words: the official `/alternateadv` subwords are unread (R49-40b), so they wait.
  - Each step posts one `TrainAlternateAdvancement`, `ActivateAlternateAdvancement` (no target) or `SetAlternateAdvancementSplit` command for the current admission, then moves on. It never waits for the server and never retries.
  - The client checks nothing about the rank or the points: the session decides, and its refusal is said in chat.
  - Every server type still refuses them as not offered until the AA live rows pass. Adam said scripts may train.

- Keep every received AA fact for display (checklist 6.565). The client's
  `alternate_advancement::State` now holds, besides Titanium's selections
  and definitions, the profile's points, the latest AA experience update,
  the unspent points from whichever came last, and each reuse timer, and on
  `EQMac` the `OP_RespondAA` ranks, the profile's AA array and points, the
  experience update and the reuse timers, all as sent. The world applies
  them on `EQMac` as on Titanium; RoF2 sends none yet. `owned` and
  `trainable` read ownership from the table's shape, as eq-network's train
  gate does, but are for display only: the session alone decides what is
  sent, and nothing here sends. `ClientWorld::environmental_aa` is renamed
  `alternate_advancement`.
- An AA request the session would not send is now news:
  `Notice::AlternateAdvancementRefused` carries the session's words, shown
  in chat, and its kind of reason for the AA window to come. A refusal for
  an earlier admission is ignored. The client sends no AA request yet.
- Read AA names and descriptions from the installed Titanium client's
  `dbstr_us.txt`: `eq_client_assets::db_strings::DbStrings` reads the file
  with a 32 MiB bound and finds a text by its type and id, so a rank's
  `title_sid` and `desc_sid` name it. The `id^type^text` layout and the
  types (1 for AA titles, 4 for descriptions) are inferred from `EQEmu`'s
  string tables until the client is read (checklist 6.565); a missing
  string is none. The Mac-era client installs no such file. Nothing loads
  it yet: the AA window comes later.
- CI: the Lint workflow gains a Docs job, `cargo doc --workspace --no-deps --locked` with `RUSTDOCFLAGS=-D warnings`, on every pull request and push to main beside Format (Adam's yes, 11-23). A broken doc link now fails the PR it lands in.
- A second click on the boat you stand on, while its take still waits for
  the server's echo, asks for it again, as the program's does (read R35
  (c), (e); checklist 6.533). The click was already a take here, since a
  waiting take holds nothing; a test now pins that. From eq-network
  `4946ab9` on, the session sends that take again and keeps the first
  take's wait instead of refusing it as already active, so the change
  reaches play when this client's pin passes that commit. A release
  already ends local control at once, as the program's does: the session
  drops control before it sends (6.477, which needed only its docs fixed).

- Draw players with the Luclin models where the viewer chose them
  (checklist 6.554, Luclin slice L1). The choice is one per-character
  option, `luclin_models` in the options file (a list of model codes such as
  `HUM,DAF`), none by default. Where the official client's `eqclient.ini`
  sets `[Defaults] UseLuclin<Race><Gender>`, that seeds it. Those key names
  are recalled, not read (R49-44a): they live in one table,
  `eq_client_assets::ui::LUCLIN_MODEL_KEYS`, and a file that names none
  leaves every model classic. The server sends nothing that chooses, so the
  choice is the viewer's alone on every server, P99 included, as the
  official client's `eqclient.ini` is. The choice covers players and
  their corpses (the corpse inferred); non-players of player races and
  illusions stay classic until the read. It is fixed per zone, so a change
  shows from the next one (inferred, R49-44b).
  - `eq_client_assets::characters::load_luclin_character` reads a race's
    Luclin model in its base look, through the archive and donor table the
    creation preview uses (`luclin_source`). A model that can't be read draws
    the classic one, said once a run.
  - Departure until L2 and L3: a Luclin player shows the base face, hair and
    eyes, and no worn armour, tints or helm; held items still show.
  - The look changes only what is drawn (and the name, which rides on the
    drawn head): the size, feet, collision height, reach and breath come
    from the race, size and classic model as before, with tests. The zone's classic scale flag
    applies as Titanium's race rule reads (R49-44d asks whether the program
    skips Luclin models).
- A session that ends leaves no ask for the next (gap 3.11, checklist
  6.535). The chat state's pending asks (a `/target`, `/who`, `/loc` or
  `/time`, a guild or split command, a boat take, `/follow`, a Ranger's
  tracking option, a `/reply` with no tell, and the script scopes for them)
  were cleared only in part by `/quit` and not at all by a link loss or a
  camp, so one made just before could act after the next login.
  `ChatState::forget_admission` clears them all at a link loss, a camp and
  `/quit`, naming every field so a new one has to be sorted into kept or
  cleared. A typed ask still survives a zone change within the session, as
  before, and nothing is said for a cleared one.
- The Player window's "%" signs show only with their numbers (gap 3.9,
  checklist 6.529). While the mana or endurance estimate has no maximum, the
  default skin showed its "%" with no number over an empty bar; now the sign
  hides until the number shows, as the Group window's already does, and the
  hit point sign follows the same rule. The program always knows its
  maximum, so what shows until ours is known is our choice (inferred). The
  sign names (`PW_HPPercLabel`, `PW_ManaPercLabel`, `PW_FatiguePercLabel`,
  also `Endurance` and `Stamina`) are inferred from the Group window's until
  the default skin is looked at; a skin naming them otherwise keeps its own
  text. The estimate itself is unchanged.
- `/ttell message` and `/whotarget` (checklist 6.528, inferred). `/ttell`
  becomes the `/tell` to the targeted player and goes as a typed one, item
  links included; `/whotarget` lists the target's name in the zone's
  `/who`. A tell goes only to another player whose name is letters alone;
  any other target gets this client's own line and nothing is sent.
  `/ttell` counts as chat for scripts, which cannot send it.

- Read zones in the EQG format newer clients install, for RoF2 later:
  `eq_client_assets::eqg::load_eqg_zone` turns `<zone>.eqg`'s descriptor,
  terrain and model meshes, placements and diffuse textures into the same
  `ZoneAsset` the S3D loader gives. It reads through David Henry's MIT
  `libeq_eqg` crate (djhenry/libeq at `686b74a`, credited in the
  third-party notices). The coordinate frame, model file names, triangle
  flags and texture property are inferred from the format (checklist
  6.498); regions and lights aren't read. Nothing calls it yet:
  `load_zone` still reads S3D archives alone.

- Jump onto what the floor sweep reaches as the Titanium client does: a
  standing jump or fall that meets an edge whose top is within 3.375 of
  its feet (for a human of size 6) now lands on it instead of stopping and
  dropping, so a running jump from the Erud's Crossing planks lands on a
  docked ship's deck. A standing jump mounts a ledge up to its rise plus
  that reach, about 8.3 for a human, where before it mounted nothing it
  couldn't walk up. The landing leaves no vertical speed, as the
  program's put-back does (read R42; inferred bound, checklist 6.484).
  Ducking, wet feet and the Mac-era generation are unchanged, and the
  landing sample stays inside the session's guard.
- Script `slash()` steps take the Titanium forms socials and hotbuttons use:
  `/yell`, `/stopcast`, `/autoinventory`, `/played`, `/doability`,
  `/memspellset`, `/lootnodrop` and `/hidecorpses`, each through the same handler as the
  typed line, and `/hotbutton` as a social step. `/yell` and `/hotbutton` need a
  local test session (checklist 6.505).

- Drop an item on the ground without the question with `/fastdrop`
  (checklist 6.536), where ground drop is offered (local `EQEmu` today).
  Each `/fastdrop` turns it over and says the state it lands in, in this
  client's words. With it on, **Drop cursor item** sends at once through the
  same checks and the same one-at-a-time hold as a Yes; a destroy and every
  other question still ask. It is off at start, never saved, and off again
  at a world entry, a camp, a return to login or a lost link (a zone change
  keeps it). A social, a hotbutton or a script's `slash("fastdrop")` turns it
  over as typing does (checklist 6.584). The program's handler (0x4685ba) is unread, so
  all of this is inferred (pending R49-6n). It sends nothing itself.

- Say where the player is bound with `/charinfo` (checklist 6.524), from
  the session's new `BoundIn` (eq-network #353): "Your bind point is in
  <zone>.", in this client's words, as the program's handler (0x46f6ec) is
  unread (pending R49-6m). The zone's long name is used, else its short
  name, else its number; before the profile says it, "Your bind point is
  not known yet." The bind is the zone-in profile's, so one that lands
  mid-session shows from the next zone-in (inferred). It sends nothing, on
  any server. A script may run `slash("charinfo")`, as `/loc` and `/time`.
  Repins eq-network to #353; the title list waits in the ignore arm for its
  thread's PR.

- Play each zone's music and ambient sound from the installation's own
  files (item 6.503; how the original program does it is pending read
  R49-7). `eq-client-assets` reads `<zone>_sounds.eff` emitters (84-byte
  records, layout inferred), the `<zone>_sndbnk.eff` EMIT and LOOP lists, and
  converts `<zone>.xmi` sequences to standard MIDI, all bounded and tested
  with inputs built in code. The render crate plays emitters by distance and
  day or night on at most eight voices of their own, so cues always keep
  theirs, and plays the music region the player stands in through
  `rustysynth` 1.3.8 (MIT) on a SoundFont the player names with
  `--soundfont`; there is no music without one (a stated departure). New
  per-character options Music, Music Volume (wired to the skin's slider) and
  Environment Sounds. Unknown record kinds and negative music ids are logged
  once a zone and not played. Everything stops on a zone change, camp,
  logout or lost connection. Combat music is not built.
- `/shield` (and `/sh`, socials and scripts): a Warrior's shield of the
  targeted player, on stock EQEmu (6.517). Before it sends, the client
  refuses a character that is not a Warrior or is under level 30, no
  target or one gone from the spawn table, and the player themself, in its
  own words, inferred until the program's handler is read. The server
  answers NPC targets, distance, the recast and its rule with its own
  lines. eq-network is repinned to f408f2e; the news its new events bring
  (surname answers, chat channels, reports) is counted in the gap file
  until each front end lands.
- Record every server message a session drops or cannot read, and the
  world news the client hears but does not show yet, so live runs become a
  list of gaps to close (6.500). Each session writes one
  `gaps/gaps-<UTC start>.json` in the settings directory, at most every
  10 s and when the session ends: each gap by server type, generation,
  stage, zone, opcode and kind, with its count, first and last seen, body
  lengths and up to three 256-byte samples. Unshown world news is counted
  by its kind's name alone, never by what it holds. Only the newest 50
  files are kept. The files stay on this machine and never belong in a
  repository: a zone sample may hold what other players said or their
  names. `--gap-report [PATH...]` prints a Markdown report over the saved
  files, gaps to close first; `--samples` adds the samples, which it leaves
  out by default. Repins eq-network to the merge of #333 (72b0ccd), which
  also brings the duel answer, Rogue poison, server filters and a chosen
  chat language: their events are counted as unshown news here until their
  own client changes land, and chat keeps to the common tongue as before.

- Another player's helm follows their own Show My Helm (gap 3.10, checklist
  6.513), as their spawn record and each appearance kind 43 change tell of
  it, where the session offers `Capability::ShowingHelm` (stock EQEmu).
  Elsewhere, and for NPCs and corpses, every helm shows as before. Inferred,
  pending program read R49-13.
- Join chat server channels on stock EQEmu (checklist 6.509). `/join`,
  `/leave`, `/leaveall`, `/list`, `/announce`, `/1` to `/10` and `/chat`
  send the chat server's own words as `ClientCommand::ChatChannel` where
  the session offers `Capability::ChatChannels`. Elsewhere they say "You are
  not connected to the chat server." They work typed, in socials and on
  hotbuttons. Channel messages print through the ordinary chat path with
  their item links, as `Name tells channel:N, 'text'`. The slot comes from
  the server's last joined list, kept across zones, and before a list names
  the channel the line shows the channel's name alone. Member joins and
  leaves, the server's own lines, a lost link and refusals print too.
  Every wording and form is this client's, inferred until read R49-12.
  `/autojoin` is not built. `/chat` passes only the chat server's channel
  words, never its mail, friends, ignore or saved-channel words. Channel and
  player names print without control characters or link markers. A
  script's `slash()` does not reach these commands.
  Repins eq-network to d0c75e2 (#341 with #327 to #340). Their poison,
  filters, duel, surname, report and gap events are passed over here until
  their own client changes land. Outgoing chat names `Tongue::COMMON`.
- Reach a Titanium door from the point clicked on it (checklist 6.530,
  read R16b and R16e). A mouse click on a door sends where the pointer's
  ray met the door as drawn (`ClickDoor.point`, eq-network #358), and the
  session admits it within the player's size and 20 in 3-D, as the
  program does, so a lift resting above can be called from its deck by the
  part clicked. A click out of reach is no longer dropped without a word:
  the session refuses it and the chat shows the program's line 1462. The
  Use key and scripts send no point and keep the stricter reach to the
  door's origin; the Use key's own ray is unread (R49-19). `EQMac` doors
  are unchanged.
- Name a strafe's pace (checklist 6.540). A strafe sample says whether Walk
  or Run is selected (`MovementMode::Strafe(Pace)`, eq-network #365), the
  same choice that already picks its rate, so the session bounds a walking
  strafe by the walking strafe rate rather than the running one. Nothing a
  player sees or the server receives changes.
- eq-network is repinned to 266039c. The events other changes show (titles,
  a surname's answer, the bind zone, a poison's answer, chat channels, the
  chat pace refusal, filters, a duel's offer, reports, RoF2 buff icons and
  pools) are left to those changes; a command dropped before a fresh login
  is logged by its kind.

- Keep the latest title list (gap 3.6 titles half, checklist 6.516). Where
  the session offers titles (stock EQEmu only, for now), the core keeps the
  list the server last sent in this zone (`ClientWorld::titles`), forgotten
  on a zone change; elsewhere a list is ignored. Nothing asks for the list
  or chooses a title yet: the program's title window is unread (R49-14), so
  no command, script step or key reaches it. Repins eq-network to the titles
  change (#346); the chat server's channels (6.509) and the petition and bug
  report answers (6.511) are ignored until their own client PRs land.
- Walk onto low edges as the Titanium client does: a standing walk now
  climbs anything it runs into up to 1.25 above the body's origin, so a
  human of size 6 steps over Erudin's pier-end rail and onto its posts
  (5 high) like stairs instead of stopping dead, as the official client was
  seen to on 11-19. The horizontal move sweeps the body raised over its
  origin (read R40); the bound is the measured height, scaled to each
  body's origin (inferred, checklist 6.482). A ducking or wading walk, and
  feet that fall or are placed under a floor, keep the floor sweep's climb.
  The session's guard hears the same climb. The Mac-era generation is
  unchanged.
- Scripts now use a Python-style syntax, and only that syntax. Every action
  is a call with literal arguments, such as `press("I")`, `wait(500)` or
  `click("slot", 22)`; free-text commands such as `report`, `slash` and `gm`
  take one quoted string. Variables are declared `name: int = 0` and
  assigned `name = 1`, `+=`, `-=` or `*=`. Blocks follow indentation under
  `if`, `elif`, `else`, `for i in range(n):` and `while c, limit=n:` headers,
  with `break`, `continue` and `pass`; conditions use `and`, `or`, `not`,
  `True` and `False`, and text takes single or double quotes with `\"`,
  `\'` and `\\` escapes.
  Help, the editor highlighter and the Vim plugin (now with indenting after
  `:`) follow the new syntax, and a thin line divides the editor's line
  numbers from the text.
- Script editor: breakpoints and stepping. Click a line number to set a
  dot; a run pauses before that statement each time it reaches it, and
  dots follow their lines through edits. Run to cursor, Step over, Step
  into and Step out join Run, Pause, Resume and Stop as icons above the
  script; while paused, a click on a line of the Execution source chooses
  where Run to cursor stops. The Execution source scrolls with the wheel,
  and during a run the Editor tab shows the script read-only; while paused,
  its caret and dots work and the run takes the new dots. See Run controls
  and troubleshooting in Help.
- Script functions: `def name(parameter: type, ...):` defines one, called
  as `name(arguments)` before or after its definition, with `return` to
  leave early. Parameters and inner variables belong to each call; top-level
  variables are shared. Up to 32 functions, 8 parameters, 32 variables each
  and calls 8 deep. The Execution tab lists the calls in progress, and the
  editor, Help and the Vim plugin colour functions. See Functions in Help.
- Scripts window: it resizes from any edge or corner and remembers its size,
  and the editor, the Execution source and Help grow with it. Run, Pause,
  Resume, Step and Stop are icons at the right end of the row above the
  script, on the Editor and Execution tabs, named by their tooltips. The
  gutter's line reaches the bottom of the editor, with a little more room
  before the text, and a click below the last line lands in the editor.
- Swim up and down as the Titanium program does (reads R13, R33, R39): Look
  Up and Look Down now tilt the character's pitch in every camera view, and a
  swim stroke follows that pitch instead of the camera's. In the chase views
  the mouse moves only the camera, so chase mouse look no longer steers a
  swimmer. Jump in water strokes up for 500 ms, and a held jump keeps rising.
  Strokes stay within the level swim budget. The character's pitch levels at
  each zone entry and on a wheel change of view (partly inferred, checklist
  6.480). Script reports carry `look_pitch`. Nothing new is sent.
- Other players and NPCs walk, run or stand by their latest report (its
  forward input or its velocity), as the Titanium program picks from the
  spawn's own velocity (read R38), not by how far they are drawn moving. A
  player standing on a moving ferry now stands; one the world has no record
  of still goes by its drawn motion. A report moving only up or down stands
  (inferred, checklist 6.481).
- A player standing still on a moving boat stands, rather than playing the
  run: the animation measures the player's own motion across the boat it is
  drawn on, not the boat carrying it (read R38).
- Uses eq-network 0d9570be: a rider arriving aboard is restored where the
  program's profile loader puts them (read R36c).
- Arrive aboard a boat as the Titanium program does (read R36): when the
  session says the boat the profile names is here, the client stands the
  player at the profile's place in the boat's frame, sweeps for the boat's
  deck from one unit above (down to 30 below, inferred 6.478), retries at
  (0, 0, 25) in the frame, and answers once; a floor that is not that boat's
  counts as none. Movement is held until the session places the player, and
  the boat's collider loads wherever the player seems to be. Script movement
  reports carry the arrival. Uses eq-network bb17371f (boat riding, the boat save checksum and the safe point after a refused carry-on).
- Doors of open type 54, 55 and 56 are no longer clicked on Titanium: the
  program's click routine returns at once for them, sending and saying
  nothing (read R37). They stay solid at their placed pose, as before.
- Debug logs name what stopped a movement proposal that resolved short: the
  zone, a door id or a carrier spawn id, with the contact normal and no world
  coordinates. Diagnostic only; movement is unchanged.
- Script editor: one highlighter, driven by the parser's own command list,
  colours the editor, the Execution view and help examples alike, now
  including statement keywords, types, quoted text, client fields and
  declared variables. Errors are checked as you type and underlined under
  the exact word, with every error (whole-script limits too) listed under
  the editor; click one to jump to its line. Unknown commands and
  malformed steps now have separate messages. The Vim plugin knows every
  command.

- Recall sent chat lines with Shift+Up and Shift+Down while typing, as the
  Titanium program does (read R34): 40 lines per chat window, kept at submit,
  never the newest line twice, an empty line or the installed password
  command; Shift+Down past the newest clears the line. The lines live in
  memory only and go with their window or the character. Held repeats step
  once and the caret lands at the end (inferred, checklist 6.476).

- Take a boat by right-clicking it while standing on it, and let it go with a
  second right-click, as the Titanium program does (read R35). The click asks
  the existing boat request, so its floor-link, incarnation and echo guards
  all apply; a boat not underfoot gets nothing, not a consider. A script's
  `interact` step on the boat does the same. The program's release resets
  local control at once; this client still waits for the echo (checklist
  6.477). Only where boat control is offered; elsewhere a right-click
  considers as before.

- Fold ordered environmental AA admission batches without discarding facts,
  reducing event amplification while preserving unknown and withdrawal states.

- Accept the documented bare boat script steps and add `--check-script` for
  complete syntax checks before settings, assets or sessions are opened.

- Use received local EQEmu AA definitions for supported breathing effects;
  unresolved capacity modifiers keep unprotected capacity unavailable.

- Submit chat at each Enter event before routing later keys in the frame,
  retaining main/detached channel ownership and suppressing held Enter repeats.

- Add EQEmu boat acquire/release through `/boat` and scripts, bound to the
  current accepted floor link. Echoes, pending requests and unconfirmed
  outcomes stay distinct; vessel steering remains unavailable.

- Level swimming can cross into known air above continuous water and use
  ordinary falling to reenter, preserving unknown-region and dry-gap guards.

- Script reports expose raw received buff durations and observation ages,
  with local remaining-time estimates separate from server ticks.

- Script reports expose current chat composition and its main or detached
  window without recording draft text or typed keys.

- Route `slash follow` through ordinary deferred follow input while retaining
  the script's admission and zone guard.
- Use received own-guild membership changes for network requests without
  requiring a relog.

- Add a read-only guild roster using received lists and member changes, with
  explicit unknown fields and retained history across matched zone transfers.
- Horizontal follow keeps moving in mapped water while chat or another app
  owns keyboard input; the near-target stop cap follows swim-rate selection.
- Add bounded `wheel world` and `look world` script gestures through the
  ordinary camera path, with ownership and lifecycle cancellation.

- Guildmaster Training lists include offered languages with received proficiency
  and estimated prices. Scripts can select `click training_language ID` and
  inspect `training_languages`; local EQEmu fixtures can use bounded
  `gm setlanguage ID VALUE`.
- Unresolved language practices withdraw practice-point authority and, when
  coins may have been spent, purse authority. Free uncertainty preserves coins;
  late proficiency updates do not restore balances. Expired or invalidated
  unanswered trainer openings require fresh admission before that ID reopens.
- **Breaking:** re-exported `training::TrainingOffer` and `TrainingReply` require
  `language_caps` and `language_flags` fields. Exhaustive `TrainingRequest` and
  `PracticeUpdate` matches must handle `PracticeAt` and `OutcomeUnknown`.

- Movement speed changes use exact clock boundaries, preventing rounding from
  delaying root fades or applying a higher speed to a sample that began
  before the new grant.

- Positive local fall, drowning and lava reports stand a sitting or kneeling
  Titanium character through the same policy as received damage. The
  stay-seated option and lying posture are preserved; local reports do not
  fabricate a received damage event or change authoritative hit points.

- Script-control starts can bind to an observed admission and zone with
  `expected_world`. A transfer or re-entry stops the scoped run, withdraws its
  synthetic input and deferred gameplay intent, and rejects queued chat from
  an earlier admission. Unscoped control runs remain available.
- Item icons follow the selected skin's declared `A_DragItem` atlas rather
  than fixed sheet names and cell dimensions. Invalid layouts retain the
  existing placeholder or label.
- Version output and startup logs include the compiled Git revision and
  tracked-tree state. Archives and build overrides identify unavailable or
  unknown provenance explicitly.
- Replace unmaintained `chrono` with `jiff`, preserving local chat/log/time
  formatting. Client crate timestamp APIs now use `jiff::civil::DateTime`.

- A forward-plus-strafe step on a moving boat keeps both movement components.
  The boat's own travel no longer makes the diagonal collision trial fall back
  to a single direction, including the final step onto ordinary ground.

- Carrier actors and their missing-model markers share the ordinary spawn
  root, nameplate and targeting rules. Loaded actors replace only the marker;
  missing geometry supplies no floor. Moving collision now poses a shared
  authored mesh rather than rebuilding its triangles and BVH each frame.

- Bare slash commands this client does not implement keep their unsupported-command
  explanation. Only the seven chat channel commands ask for a message when empty.
  The ability picker's fallback uses the installed default skin's "Choose a skill"
  title and opening position; a skin that omits its title gets the same title.
  Authored skin placement, including overlap with Actions, is preserved.

- Imported friends and ignore lists are saved to this client's per-character file
  immediately, without waiting for the first edit. An unreadable existing file
  does not trigger an import write. Camping or ending a session clears guild MOTD
  deduplication so the next character can see the same notice; zoning keeps it.

- Fixed buff rows can expand the Effects window vertically as fixed columns
  already expand it horizontally. The extent includes borders and a title bar;
  anchored buttons and skin-hidden windows keep their authored sizes. Expansion
  rather than clipping remains an inferred display policy (recording checklist
  6.474 for height, 6.406 for width), pending official-client comparison.
- Releasing a turn key between movement samples keeps the unsampled turn
  for the next request. Focus, stun and permission resets discard it.
  Shoreline arcs retire immediately when their continuation cannot be
  resolved or has expired, and share the ordinary coordinate conversions.
- Titanium carrier actors use their installed model's authored collision, so
  a floor sweep can stand on the model and carry its rider through received
  translation and yaw. The client and session share the report-driven pose
  and budget only the rider's relative motion. Geometry source, between-report
  motion, rider reach and still-rider wire details remain labelled inferences
  (6.467-6.470). A prior local EQEmu run observed canopy support, translation,
  turning and short relative movement. Ordinary deck boarding, dock walk-off
  and attended P99 remain unverified; pitch and boat zoning remain separate.

- A buff's and a pet buff's icon background no longer shows magenta at its
  corners. The installed client's skins draw it from CS_Buttons.bmp
  (BlueIconBackground, RedIconBackground and the pet ones), whose
  transparent parts are painted pure magenta, and skin pictures were never
  keyed. Every skin picture, in any format, now reads exact opaque
  (255, 0, 255) as transparent black, as the Titanium program does
  (program read R30); magenta below full alpha is kept, and cursors are
  not keyed. One reader does it for all skinned windows. The installed
  velious skin's logo on the Actions window is keyed the same way.

- A bag's window and a world container's window show the container's icon
  on the skin's recessed box alone, as the Titanium program does (program
  read R29). They used to draw the skin's whole first item-icon sheet
  squashed under it, since the installed client's skins name their icon
  grid (A_DragItem) as that button's decal. Two rules from the read now
  cover every window: a grid animation draws one cell or nothing, never
  its sheet, so a button's grid decal is no picture of its own; and where
  a window sets a button's picture (buffs, pet buffs, the item display,
  the merchant's chosen item, the spellbook's places and now the bag's
  icon) the skin's decal is never drawn under it. That second rule was a
  list of five windows missing the bag; it is now one rule. An icon below
  500, such as the 0 servers may send for a world container, shows the box
  alone, and a bag's icon no longer stays after its bag loses it. A still
  picture (StaticAnimation) naming a grid keeps its place with no picture,
  read for the cursor attachment and inferred elsewhere (checklist 6.464).

- On Titanium a ducked strafe now goes on the selected base, halved: the
  run speed when running and the walk speed when walking (program reads
  R28 and R28b; checklist 6.460). It used to take half the walk speed
  either way, which was slower than the program. A ducked forward-plus-
  strafe now moves diagonally as standing, its sideways part unhalved and
  its forward part the ducked one; it used to go forward only. Sneaking
  strafes the same way; neither sneaking nor ducking changes the run flag
  (read R28b). Networking pinned to 9b2ab94 (eq-network #300).

- The Titanium scribe press refuses as the Titanium program does (program
  read R27, checklist 6.463), in its order and by its strings: an item that
  is not a scroll, or one your class or race cannot use (181; the class and
  race test is inferred), a place that holds a spell (12051), a spell the
  book has (12050), a spell with no level for your class (12047) or above
  your level (12048, with the level and the spell's name, replacing this
  client's own wording), and a stack of two or more (6192). Nothing is sent
  and the scroll stays on the cursor. A press while a spell action is in
  flight now returns without a line, as the program's does; the cast test
  in it is this client's own. The 12049 line and the worker's sit and
  5-second wait before the request are unchanged (the program's countdown
  step is unread). The Mac-era client keeps its earlier press, its program
  unread; the level check stays Titanium-only, as before.

- On Titanium a ducking player moves ducked, as its program does (program
  read R23; checklist 6.460). Forward, back, strafe and autorun no longer
  stand you first: you move at the sneaking speeds, half the walk speed
  with a run buff added in full, and a strafe halved once (the session's
  grant now carries it, so the client no longer halves it again). Autorun
  pressed while ducked runs you ducked; ducking, and the stand a ducked
  jump sends before it launches, still end autorun as posture changes. A
  duck doesn't end a Bard song in the program (read R25). Ducked headroom
  (a clearance of your size plus one) and the refused stand under a low
  ceiling aren't modelled yet, and the ducked walk animation is unread, so
  the duck pose is drawn. The Mac-era client is unread and still stands you
  to move. Networking pinned to 50c1662 (eq-network #299).

- The Script report's `effects` field is now `unslotted_effects`, as it
  only ever held landings the server gave no slot, and a new `held_spells`
  field lists every spell the player holds, slotted or not. A buff held at
  zone entry is in the server's slot list, so it was missing from
  `effects`, which read as a missed buff.

- Scripts on a local test session may read the server's own view of a test
  character with `gm show`: exactly `buffs`, `currencies`, `timers`,
  `distance` or `line_of_sight`, or `inventory SCOPE` with SCOPE one of
  `all`, `allbank`, `bank`, `cursor`, `curlimbo`, `equip`, `gen`, `limbo`,
  `poss`, `shbank`, `trade`, `trib` or `world`. These read only. Any other
  subcommand, scope, case or extra word is refused when the script is read,
  and like every `gm` step they run only in a local test session. Their
  output names characters and items, so it stays in private run notes.

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
  and 6.458 (attended). Networking pinned to ba1ac5c (eq-network #294 and
  #298).

- A scribe or memorization the server refuses for your level now ends at
  once, with the server's line in chat, instead of holding the cursor
  ("Wait for scribing to finish") until you zone; the same scroll can be
  tried again. A scribe the server never answers frees the cursor after 30
  seconds (inferred, checklist 6.462). Networking pinned to ba1ac5c
  (eq-network #298).

- After a zone-in your first /stand no longer sends a standing report the
  Titanium program would skip: the session now stands you at entry, as the
  program does, and sends one stand only when the server's record of you
  wasn't standing (program reads R24 and R24b; checklist 6.461). A lone
  first standing is the posture you enter with, not a change, so it leaves
  autorun alone; a stand at entry that changed your posture ends autorun,
  as the program's does. The rule is the same on every client generation,
  so on the Mac-era client (whose entry is unread) a first /stand after a
  zone-in no longer ends autorun either. Networking pinned to d35b10b
  (eq-network #297).
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
