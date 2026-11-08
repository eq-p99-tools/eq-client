# Variables, conditions and loops

[Help home](README.md)
[Command reference](reference.md)
[Run controls](running.md)

Write one statement per line. Indentation is optional. Use the same action commands inside a block as you would on their own. The parser checks every action in the file, including actions in branches that may be skipped. Runtime action checks still use the current client and its ordinary controls.

## Variables and expressions

Declare a variable with `let name: type = expression` before using it. Declare at the top level and change its value with `set name = expression`, including inside blocks. There are up to 32 variables per run. Names are case-sensitive and use up to 32 ASCII letters, digits or underscores, starting with a letter or underscore.

- `int` holds a signed 64-bit integer. Operators `+ - * / %` use checked integer arithmetic. Division rounds toward zero.
- `bool` holds `true` or `false`. Use `!`, `&&` and `||` for negation, conjunction and disjunction. `false && …` and `true || …` skip the right operand.
- `text` holds case-sensitive text. Use double quotes, with `\"` for a quote and `\\` for a backslash. Text literals may contain `#` and are limited to 120 UTF-8 bytes.
- `==` and `!=` compare matching types. `<`, `<=`, `>` and `>=` compare integers. Use parentheses to group expressions.

From tightest to loosest, precedence is unary `!` and `-`, then `* / %`, then `+ -`, then ordering, equality, `&&` and `||`.

Variables hold the value read at the assignment line. To refresh an observation, read the field again. Action arguments keep their own syntax and take literal text.

## Current client observations

Fields read the local client projection on the statement's frame. Values give the latest available observation from the current admission.

- `client.ready` is `bool`. It is true when the client has a connected, admitted, living character that can act outside a zone transfer.
- `zone.name` is the received short name and `player.level` is the admitted character's level.
- `player.reported_hp` and `player.reported_hp_max` are raw integers from the latest HP report. Depending on the provider they can omit equipment modifiers.
- `player.mana` and `player.endurance` are the last reported integer values, when available.
- `target.selected` is a `bool` from the reconciled target state. A pending or unresolved selection is unavailable. `target.name` is the display name of the known selected spawn, when available.

An observation can be `unknown` before a usable value arrives, or `stale` when the client can identify that its retained context no longer applies, such as a stale target or a retained admission during a transfer. Both stay distinct from `false`, zero and an empty string. `known(value)` is true for a usable value. `stale(value)` is true for a stale value. Ordinary operators preserve an unavailable operand when they read it. A condition that remains unavailable stops the run at that source line.

## Branches and bounded loops

`if condition` runs its body when the condition is true. Add `else` for an optional second body and close with `end`.

```eqscript
let mana: int = player.mana
if known(mana) && mana > 0
    report mana_observed
else
    report mana_empty_or_unavailable
end
```

`repeat count` evaluates its integer count once and runs up to 100 iterations. Zero skips the block. `while condition limit N` checks a boolean at the start of each iteration, with a required literal cap `N` from 1 to 100. Both use `end`. The Execution tab records when a while loop reaches its cap. Add a `wait` when polling for an incoming change.

```eqscript
let attempts: int = 0
while !client.ready limit 5
    wait 500
    set attempts = attempts + 1
end
if client.ready
    report admitted
else
    report admission_unavailable
end
```

Blocks can nest up to 16 levels. Execution advances one statement per frame or waits for its current action. Stop remains available during loops. The Execution tab shows which branch was taken, active iteration counts, and the latest values assigned to variables.
