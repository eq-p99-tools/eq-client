# UI target reference

[Help home](README.md)
[Command reference](reference.md)
[Examples](examples.md)

Use these targets after `click`, `right_click` or `hover`. Each word of a target is one argument, so the target `slot 22` is written `click("slot", 22)`. For a held click, put the duration first: `hold_click(500, "hot_button", 1)`. A syntactically valid target still needs its window and control to exist. Slots use the client's protocol slot numbering; other ordinals are noted below.

## Inventory, spells and money

- `slot number`: an inventory slot; carried top-level slots start at 22.
- `store`: auto-store the item on the cursor.
- `drop`: open Inventory's ordinary ground-drop confirmation for the cursor
  item on local-only Titanium EQEmu. Answer the drop question separately.
- `destroy` opens Inventory's confirmation to destroy the entire cursor item
  or stack where the session offers destruction. Answer separately.
- `scribe`: scribe the cursor scroll.
- `book row`: a spellbook list row, counted from zero.
- `book_place place`: a skin spellbook place, from 0 to 15.
- `book_page next` or `book_page back`: turn a spellbook page.
- `book_edit earlier`, `book_edit later`, `book_edit delete`, `book_edit confirm` or `book_edit cancel`: the spellbook's ordering and deletion buttons, for the spell chosen with `book row`. `delete` asks first, and `confirm` or `cancel` answers. A button the session doesn't offer is refused with its reason, as its greyed look tells a person: P99 holds deletion back.
- `memorize gem`: choose gem 1 to 8 when memorizing.
- `spell_gem gem`: a gem from 1 to 8. `click` casts its spell and
  `right_click` forgets it.
- `button spellbook Spellbook_Search`: focus the Search field in either
  spellbook. Use `press` to type and `press("enter")` to finish. Filtered entries
  retain their received book slots. Use `button spellbook Spellbook_SearchClear`
  to clear the query and restore the ordinary page.
- `button spellbook SpellSets_Toggle`: open the saved layouts panel. While it is
  open, use the same prefix with `SpellSets_Name`, `SpellSets_Save`,
  `SpellSets_Previous`, `SpellSets_Next`, `SpellSets_Rename`,
  `SpellSets_Delete`, `SpellSets_Apply` or `SpellSets_Cancel`. After focusing the
  name field, `press` sends its usual key input.
- `hot_button number`: a hotbar button from 1 to 10.
- `tint slot [color]`: open a bag's color picker, or choose a palette color.
- `coins place kind`: a coin box; place is `purse`, `bank` or `give`; kind is `platinum`, `gold`, `silver` or `copper`.
- `pick control`: the quantity picker's `less`, `more`, `min`, `max`, `confirm`, `cancel` or `amount` control.
- `slider quantity percent`: the quantity slider, from 0 to 100 percent.

## Loot, merchants and tradeskills

- `loot place`: take the corpse item at its server place number.
- `loot_all`: take all corpse items.
- `loot_done`: end looting.
- `merchant_row slot`: select a merchant row.
- `buy slot` or `sell slot`: buy the merchant slot or sell the inventory slot.
- `buy_chosen` or `sell_chosen`: act on the selected merchant item.
- `shop_done`: leave the merchant.
- `give`: the give window's button.
- `combine slot`: combine in the tradeskill container in a pack slot.

Titanium purchases and sales of multiple available units use the existing
quantity picker. After `buy_chosen` or `sell_chosen`, use the quantity slider
or `pick amount` number box, then `pick confirm` (the skin's Accept button).
Closing the quantity window leaves holdings unchanged. The unskinned picker
also has `min`, `max`, `more`, `less` and `cancel` buttons; selectors require
the corresponding control to exist. A single available unit submits directly.
A changed selection must be chosen again; scripts do not retry an unanswered
merchant action.

## Windows and lists

- `window key`: click a window toggle, such as `inventory`, `spellbook`, `options` or `scripts`. The toggles live in the EQ menu; while it is closed the step first clicks the menu's own button (`button selector EQButton`), waits for the menu to open (up to a second), then clicks the entry; the script ends if the menu stays closed. Keyboard shortcuts such as the map's starting `press("M")` do not require the menu; saved bindings still apply.
- `close_box key` or `minimize_box key`: the window's titlebar control.
- `done key`: a window's Done button.
- `button key ScreenID`: a named button in the installed skin's window.
- `scroll_up key` or `scroll_down key`: a window's scrollbar.
- `tab number`: an options tab counted from 1.
- `option name`: an option toggle by its settings-file name.
- `filter kind`: a Filters page row's button by its kind's stable name, such as `ooc` or `bard_songs`: a click steps the row to its next choice, as the player's does.
- `slider name percent` sets a slider by its position from 0 to 100 percent. Names include `clip_plane`, `max_fps`, `mouse_sensitivity`, `ui_scale`, `run_boost`, `sound_volume`, and `quantity`. On the QoL page, `click("slider", "run_boost", 50)` selects 1.20x and `click("slider", "run_boost", 100)` selects 1.40x.
- `dropdown key_filter`: the options key-binding filter.
- `dropdown bug_type`: the bug report window's category drop-down.
- `report bug_text` or `report feedback_text`: the bug report or feedback window's text box, which then takes `press(...)` keys as a player's click does. The windows' buttons are `button bug-report ScreenID` (`LocationButton`, `DupButton`, `CrashButton`, `FlagsButton`, `TargetButton`, `CancelButton`, `SendButton`) and `button feedback FBKW_SendButton` or `FBKW_CancelButton`.
- `choice number`: a dropdown choice counted from 1.
- `chat_tab label`: a chat tab by its displayed label, ignoring case.

## Characters and actions

- `character number`: character slot 1 to 8.
- `enter_world` or `quit_game`: character selection buttons.
- `actions`: open the Actions window through its EQ menu entry, opening the menu first when it is closed, as `window key` does.
- `attack`: the attack control.
- `ability combat number` or `ability abilities number`: a button counted from 1 on an Actions page.
  `right_click` on one, or `click` on an empty one, opens the skills
  picker for it.
- `skill_select row`: a skills picker row counted from 1, in the program's
  order; it puts that ability on the button and closes the picker.
  `close_box skill-select` or Escape closes it with no change. The
  `Script ability report` line names each button's choice and, while the
  picker is open, its rows.
- `training row`: a training row counted from 1; ordinary skills and languages
  share the installed trainer list.
- `training_language id`: the currently offered language row by its zero-based
  language ID (0..25). This selects a visible ordinary row; it sends no training
  request. An unavailable row is refused. `report` includes `training_languages`
  as `(id, received_value, cap)`; a missing value remains unknown.
- `train` or `training_done`: the training window's buttons. Use
  `interact(admission, spawn, revision)` with the selected guildmaster's
  current `target_interaction` tuple to open training. On P99, opening the
  same trainer or submitting another practice requires a fresh admission.
- `tracking row`: a track window row counted from 1, in the server's order.
- `track`: the track window's Track button, which follows the chosen row.
  The Tracking ability opens the window; `done tracking` (the skin's Cancel)
  or `close_box tracking` closes it.
- `buff number` or `song number`: a buff button counted from 1. `right_click`
  uses ordinary beneficial-buff cancellation, including current slot, spell-data
  and admission checks. Unslotted inferred effects never supply a cancel slot.
  P99 uses the two-record request and waits for received membership changes.
- `answer yes` or `answer no`: the confirmation dialog.
- `page next` or `page back`: the reading window's page controls.

## Groups, raids and pets

- `group invite`, `group follow`, `group disband` or `group decline`: a group button.
- `raid invite`, `raid accept`, `raid decline`, `raid disband`, `raid lock`, `raid unlock`, `raid nogroup` or `raid makeleader`: a raid control.
- `raid group number`: a raid group from 1 to 12.
- `raid_row grouped row` or `raid_row ungrouped row`: a raid row counted from 1.
- `pet command`: a supported Pet Info button, using the same words as its `/pet` command.

## World

These click things in the scene, as a person clicks them where they show. Take the ids from a fresh `report()`: the "Script surroundings" line gives `admission`, the nearest spawns as `spawn_clicks` (admission, id, revision), the ground objects and stations by drop id, and the doors by id.

- `spawn admission id revision`: a spawn. `click` targets it, and offers it the item on the cursor if it is an NPC, as a person's click does. `right_click` uses it without changing the target: loot a corpse, open a merchant or banker, take a boat, or consider. `click_with` holds keys as for any click.
- `object admission drop_id`: an item or world container on the ground. `click` picks it up, or opens the forge, oven or other container.
- `door admission id`: a door or lift. `click` makes the two clicks a person makes, on the same point, to open, close or call it.

The script pointer glides to the thing's middle on screen, and the click acts only where the scene's own pick names that same thing then. A thing behind a wall, behind the camera, off screen, out of drawing range, under a window or covered by another spawn refuses the step, and nothing is sent. A stale admission, a spawn that is gone or has a new revision, or an object or door not in the zone's list refuses it before the pointer moves. These targets take `click`, `right_click` and `click_with` only, not `hover` or `hold_click`. Reach, capability and pace are checked as for a person's click.

## Map controls

- `map zoom_in`, `map zoom_out`, `map reset` or `map labels`: a map toolbar button.
- `map up`, `map down`, `map left` or `map right`: pan the map.

- `map_search names` or `map_search landmark`: focus the name search or the landmark box. Then `press(...)` types into it, as for any box.
- `map_watch clear`, `map_watch track`, `map_watch track_selected` or `map_watch list`: clear the search, track the searched name, track (or stop tracking) the inspected entity's name, or open the watch list.
- `map_watch remove NAME`: the remove control of the watch row showing NAME. A name of several words is given as several arguments, such as `click("map_watch", "remove", "a", "rare", "guard")`.
- `map_marker admission id revision`: a spawn's marker, which pins it in the inspector. The report's surroundings line lists the tuple for the 64 nearest spawns as `map_markers` (with how many there are), and the map shows every known spawn. A stale admission or revision is refused, as for a click in the world. A marker that isn't drawn can't be clicked.
- `map_inspector clear`: the inspector's Clear.
- `map_target`: the inspector's Target button. It uses the ordinary target capability and range checks, and its refusal is the chat line a person gets.
- `map_landmarks toggle`, `map_landmarks save` or `map_landmarks remove`, and `map_landmarks locate NAME`: the landmark controls and a landmark's row.
- `map_style less`, `map_style more`, `map_style style`, `map_style lock` or `map_style outlines`: the presentation controls.

The map is available on every server and can show known entities without local map files. Map inspection is local, while its explicit Target button uses the ordinary gameplay capability and range checks. Watches and landmarks are saved per character, and presentation beside the window placements, so these clicks change saved settings as a person's do.

Targets are resolved through the normal client UI and session capabilities. No target bypasses a server's unsupported feature or conjures a closed merchant, bag or spellbook into existence.
