### Added

- Saved `.eqscript` files can be placed on a character's normal hotbar from
  the Scripts window's Hotbutton control. Existing slot drag, swap, pages and
  assigned hotkeys apply. Click or press the assigned key to start, pause or
  resume; right-click to stop while keeping the binding.
- Script hotbuttons show their actual runner state, including a distinct
  paused label and ordinary-button color. Completion, failure and external
  stop clear the latch. Controls are scoped to the execution they started,
  and loading a file cannot replace an unrelated run. Existing script input
  policies and pause/stop cleanup are preserved.
