# Variables, conditions and loops

[Help home](README.md)
[Command reference](reference.md)
[Run controls](running.md)

Scripts read like a small subset of Python. Write one statement per line. Every action is a call such as `press("I")` or `wait(500)`, and a block is the indented lines under a header that ends in `:`. The parser checks every action in the file, including actions in branches that may be skipped. Runtime action checks still use the current client and its ordinary controls.

## Actions

Each action takes literal arguments, separated by commas: quoted text, such as `"I"` or `'CTRL+I'`, and numbers, such as `500` or `-55`. An argument names one word, so `click("button", "spellbook", "Spellbook_Search")` takes three. The commands that carry free text (`report`, `slash`, `social`, `server`, `gm` and `chat`) take one quoted string, such as `report("camera check complete")`. Use `wait_select()` with empty parentheses for a command with no arguments. Action arguments cannot read variables yet. [The command reference](reference.md) lists each action.

## Variables and expressions

Declare a variable with `name: type = expression` before using it. Declare at the top level, then change its value with `name = expression`, including inside blocks. `name += expression`, `-=` and `*=` update an integer in place. There are up to 32 variables per run. Names are case-sensitive and use up to 32 ASCII letters, digits or underscores, starting with a letter or underscore. A name cannot be a command or a keyword.

- `int` holds a signed 64-bit integer. Operators `+ - * / %` use checked integer arithmetic. Division rounds toward zero.
- `bool` holds `True` or `False`. Use `not`, `and` and `or` for negation, conjunction and disjunction. `False and …` and `True or …` skip the right operand.
- `text` holds case-sensitive text in single or double quotes, with `\"` and `\'` for either quote in either style and `\\` for a backslash, such as `'it\'s'` or `"say \"hi\""`. Text literals may contain `#` and are limited to 120 UTF-8 bytes.
- `==` and `!=` compare matching types. `<`, `<=`, `>` and `>=` compare integers. Use parentheses to group expressions.

From tightest to loosest, precedence is unary `-`, then `* / %`, then `+ -`, then ordering and equality, then `not`, `and` and `or`. As in Python, `not a == b` means `not (a == b)`.

Variables hold the value read at the assignment line. To refresh an observation, read the field again.

## Current client observations

Fields read the local client projection on the statement's frame. Values give the latest available observation from the current admission.

- `client.ready` is `bool`. It is true when the client has a connected, admitted, living character that can act outside a zone transfer.
- `zone.name` is the received short name and `player.level` is the admitted character's level.
- `player.reported_hp` and `player.reported_hp_max` are raw integers from the latest HP report. Depending on the provider they can omit equipment modifiers.
- `player.mana` and `player.endurance` are the last reported integer values, when available.
- `target.selected` is a `bool` from the reconciled target state. A pending or unresolved selection is unavailable. `target.name` is the display name of the known selected spawn, when available.

Each field below reads only what one of the client's windows or typed commands already shows the player at that moment.

- `player.hp_percent` is the HP gauge's percent. `player.posture` is `"standing"`, `"sitting"`, `"ducking"`, `"looting"`, `"lying"` or `"frozen"`. `player.casting` is true while the cast bar runs, and `player.stunned` while the player is stunned.
- `player.loc_x`, `player.loc_y` and `player.loc_z` are the numbers `/loc` gives, to the nearest whole number. `/loc` prints them as y, x, z.
- `player.exp_percent` is the experience bar's fill, from 0 to 100, rounded down.
- `player.platinum`, `player.gold`, `player.silver` and `player.copper` are the coins in the purse, as the inventory window shows them.
- `target.hp_percent` is the target window's percent. `target.kind` is `"npc"`, `"player"` or `"corpse"`. `target.con` is the color a consider gave, as the target's name shows it: `"gray"`, `"green"`, `"light_blue"`, `"blue"`, `"white"`, `"yellow"` or `"red"`. It is unknown until the player considers the target.
- `pet.present` is whether the player has a pet. `pet.name` and `pet.hp_percent` read the pet window, and are unknown without a pet.
- `gem1.spell` to `gem8.spell` are the names of the spells in the gem bar, unknown for an empty gem. `gem1.ready` to `gem8.ready` are true when the gem is lit: nothing is being cast or awaiting the server, and the spell's reuse and any linked timer have run out. An empty gem is never ready.
- `time.hour`, `time.minute`, `time.day`, `time.month` and `time.year` are the time in Norrath as `/time` tells it. The hour runs from 0 for 12 AM to 23 for 11 PM, named as the installed client names it. They are unknown before the server gives the time, and in a zone where `/time` says the time can't be told.

Nothing on screen gives a target's distance, the player's carried weight or heading, or the target's level, so no field reads them.

An observation can be `unknown` before a usable value arrives, or `stale` when the client can identify that its retained context no longer applies, such as a stale target or a retained admission during a transfer. Both stay distinct from `False`, zero and an empty string. `known(value)` is true for a usable value. `stale(value)` is true for a stale value. Ordinary operators preserve an unavailable operand when they read it. A condition that remains unavailable stops the run at that source line.

## Branches and loops

`if condition:` runs the indented body under it when the condition is true. Add `elif condition:` for further tests and `else:` for the remaining case, each at the same indentation as its `if`. A block ends where the indentation returns to an outer level. Indent with spaces or with tabs, consistently within a script; four spaces is conventional. A block with nothing to do yet holds `pass`.

```eqscript
mana: int = player.mana
if known(mana) and mana > 0:
    report("mana_observed")
else:
    report("mana_empty_or_unavailable")
```

`for i in range(count):` evaluates its integer count once and runs that many iterations: up to 100 under the default run length, any count under a longer one. Zero skips the block. The loop variable counts from 0, can only be read, and exists only inside its loop; write `for _ in range(count):` when the body doesn't need it. `while condition:` checks a boolean at the start of each iteration. `while condition, limit=N:` also stops after `N` iterations, a literal from 1 to 100, and the Execution tab records when it reaches that cap. A while loop without a cap runs until its condition is false, a `break`, or the run's length ends it: under the default length its 5,000 statements do. Add a `wait` when polling for an incoming change. `break` leaves the innermost loop and `continue` starts its next iteration.

```eqscript
attempts: int = 0
while not client.ready, limit=5:
    wait(500)
    attempts = attempts + 1
if client.ready:
    report("admitted")
else:
    report("admission_unavailable")
```

```eqscript
for i in range(3):
    if i == 1:
        continue
    press("I")
    wait(500)
```

Blocks can nest up to 16 levels. Execution advances one statement per frame or waits for its current action. Stop remains available during loops. The Execution tab shows which branch was taken, active iteration counts, and the latest values assigned to variables.

## Functions

`def name(parameter: type, ...):` at the start of a line, outside any block, defines a function; its indented body runs when a line calls it as `name(arguments)`. A function can be called before or after its definition, and from other functions, including itself. Each parameter has a type, `int`, `bool` or `text`, and each call passes one value of that type per parameter.

```eqscript
def open_bags(times: int, quiet: bool):
    for _ in range(times):
        press("CTRL+B")
        wait(250)
    if not quiet:
        report("bags_toggled")

open_bags(2, False)
```

Parameters and variables declared inside a function belong to that call: each call starts them fresh, and the caller's values are back when it returns. Variables declared at the top level are shared, and a function can read and assign them once their declaration has run, and doing either sooner stops the run with an error; a function's own variable can't reuse a top-level name or a function's name. `return` leaves the call at once, from inside any of its loops; functions give no values back yet, so `return` stands alone. `break` and `continue` stay inside the function's own loops.

A script holds up to 32 functions with up to 8 parameters and 32 variables each, and calls nest at most 8 deep. A call counts as one statement. An error inside a function names it and the line that called it, such as `(in open_bags, called from line 8)`.

In the editor, a breakpoint on a `def` line pauses each time the function is called. Step over runs a call to its end, Step into pauses on the first line inside it, and Step out runs to the end of the current call. The Execution tab lists the calls in progress, innermost first, with the function's own variables above the shared ones.
