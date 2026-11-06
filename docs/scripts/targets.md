# UI target reference

[Help home](README.md)
[Command reference](reference.md)
[Examples](examples.md)

Use these targets after `click`, `right_click` or `hover`. For a held click, put the duration first: `hold_click 500 hot_button 1`. A syntactically valid target still needs its window and control to exist. Slots use the client's protocol slot numbering; other ordinals are noted below.

## Inventory, spells and money

- `slot number`: an inventory slot; carried top-level slots start at 22.
- `store`: auto-store the item on the cursor.
- `scribe`: scribe the cursor scroll.
- `book row`: a spellbook list row, counted from zero.
- `book_place place`: a skin spellbook place, from 0 to 15.
- `book_page next` or `book_page back`: turn a spellbook page.
- `memorize gem`: choose gem 1 to 8 when memorizing.
- `spell_gem gem`: a gem from 1 to 8.
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

- `window key`: click a window toggle, such as `inventory`, `spellbook`, `options` or `scripts`. The toggles live in the EQ menu; while it is closed the step first clicks the menu's own button (`button selector EQButton`), waits for the menu to open (up to a second), then clicks the entry; the script ends if the menu stays closed. Keyboard shortcuts such as the map's starting `press M` do not require the menu; saved bindings still apply.
- `close_box key` or `minimize_box key`: the window's titlebar control.
- `done key`: a window's Done button.
- `button key ScreenID`: a named button in the installed skin's window.
- `scroll_up key` or `scroll_down key`: a window's scrollbar.
- `tab number`: an options tab counted from 1.
- `option name`: an option toggle by its settings-file name.
- `slider name percent`: `clip_plane`, `max_fps`, `mouse_sensitivity` or `quantity`, from 0 to 100 percent.
- `dropdown key_filter`: the options key-binding filter.
- `choice number`: a dropdown choice counted from 1.
- `chat_tab label`: a chat tab by its displayed label, ignoring case.

## Characters and actions

- `character number`: character slot 1 to 8.
- `enter_world` or `quit_game`: character selection buttons.
- `actions`: open the Actions window through its EQ menu entry, opening the menu first when it is closed, as `window key` does.
- `attack`: the attack control.
- `ability combat number` or `ability abilities number`: a button counted from 1 on an Actions page.
- `training row`: a training row counted from 1.
- `train` or `training_done`: the training window's buttons.
- `buff number` or `song number`: a buff button counted from 1. `right_click`
  uses ordinary beneficial-buff cancellation, including current slot, spell-data
  and admission checks. Unslotted inferred effects never supply a cancel slot.
- `answer yes` or `answer no`: the confirmation dialog.
- `page next` or `page back`: the reading window's page controls.

## Groups, raids and pets

- `group invite`, `group follow`, `group disband` or `group decline`: a group button.
- `raid invite`, `raid accept`, `raid decline`, `raid disband`, `raid lock`, `raid unlock`, `raid nogroup` or `raid makeleader`: a raid control.
- `raid group number`: a raid group from 1 to 12.
- `raid_row grouped row` or `raid_row ungrouped row`: a raid row counted from 1.
- `pet command`: a supported Pet Info button, using the same words as its `/pet` command.

## Map controls

- `map zoom_in`, `map zoom_out`, `map reset` or `map labels`: a map toolbar button.
- `map up`, `map down`, `map left` or `map right`: pan the map.

The map is available on every server and can show known entities without local map files. Search, watches and the inspector are interactive controls; they do not yet have dedicated named script targets. Map inspection is local, while its explicit Target button uses the ordinary gameplay capability and range checks.

Targets are resolved through the normal client UI and session capabilities. No target bypasses a server's unsupported feature or conjures a closed merchant, bag or spellbook into existence.
