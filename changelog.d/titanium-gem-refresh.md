### Fixed

- P99 gem clicks, hotkeys and script input use an inclusive early-start reuse
  boundary: a spell with 10 seconds of reuse and a 4-second effective cast
  can start again 6 seconds after its previous completion. Another spell
  starts no artificial global recovery or ten-second reuse wait for it;
  actual pending/casting ownership and received category restrictions remain.
  An eligible gem immediately looks ready in both spell bars.
- Titanium gem artwork dims during its applicable wait. The installed skin
  keeps its background, holder, highlight and icon geometry; fallback gems
  also dim. Clicking an unready gem keeps its usual warning. Other server
  policies retain their existing timing rules, separately from cast progress.
