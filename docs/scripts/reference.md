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
- `give`: give the cursor item to the current target.

Movement uses the client's ordinary movement and collision behavior. It is not a teleport. Walk uses a ready navigation mesh when `--navmesh` is enabled and falls back to grid search otherwise. A reachable destination and the session's movement grant are still required. Keep the camera attached when using movement scripts; free-camera mode consumes movement input for the camera.

Approach, walk and chase hold the camera-relative forward command, independent of key bindings. Object commands choose the nearest matching tradeskill container when they start. Missing targets, unreachable routes and timeouts stop the run. `wait_dead` only recognizes NPC corpses or a target leaving the zone; a player's death does not satisfy it while their corpse remains.

## Diagnostics and commands

- `report [label]`: log a state summary with an optional label.
- `trace milliseconds`: log movement and frame timing for a bounded duration.
- `screenshot filename.png`: capture beside the script; the argument must be a filename.
- `sound filename.wav`: play a sound from the installed game assets.
- `slash command [arguments]`: run one of the supported game slash commands, without a leading slash.
- `gm command [arguments]`: an allowed GM command on a network-constrained local test session only.
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
- `create name race class gender deity start_zone primary_stat`: request character creation from selection where the server policy supports it, using numeric identifiers; the final stat receives the free stat points. This command is not restricted to local servers by the script parser.
- `wait_online`: wait for a zone admission and player before continuing.
