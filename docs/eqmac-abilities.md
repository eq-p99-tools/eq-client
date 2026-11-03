# EQMac ability menus and recovery

The client pins networking commit
[`4edb2a04ca62a3f68230e191555945026e149039`](https://github.com/eq-p99-tools/eq-network/pull/180).
It includes the 15 supported TAKP/Quarm abilities introduced in
[#176](https://github.com/eq-p99-tools/eq-network/pull/176), the prior spell
and movement work, and the idle world-session reliability fix in
[#179](https://github.com/eq-p99-tools/eq-network/pull/179).

The profile decoder now supplies the 100 raw EQMac skill words that were
previously absent at admission. This lets the existing Skills window and
ability list consume server-reported ranks. Ordinary skill updates use the
same state path, and re-admission replaces the previous table. No skill
ranks are inferred from class or level. The positive-rank Skills display
is unchanged; eligible zero-rank abilities can still appear in the ability
list. EQMac off-hand weapon checks also reject untrained/unavailable skill
sentinels instead of treating them as positive Dual Wield ranks.

The ability list uses the networking library's EQMac eligibility rules:
profile skill sentinels, class requirements and racial Bash exceptions.
Untrained/unavailable skills are omitted; zero-rank skills that can be used
without training remain available. Existing Titanium eligibility is unchanged.
The session still owns admission, equipment, target, range and reuse refusals.

Server `SkillReady` events release the displayed recovery group. For example,
a Mend reset clears only Mend; a Bash reset clears the shared strike group.
Unknown IDs and unsupported EQMac skills do not release unrelated abilities.
Raw `SkillReset` records preserve their fields in the network event stream but
do not themselves imply readiness, HP changes, or successful ability use.
The UI does not invent an `AbilityUsed` event or success notice for a reset.

Synthetic client tests cover reset grouping, nonzero raw timers, unknown IDs,
no player-state or success-notice changes, late resets after admission is cleared,
skill updates, class restrictions, and the unchanged Titanium fallback.
An encoded synthetic profile also covers decoding through admission,
ability listing, a decoded ordinary update, re-admission and an absent table.

The [networking evidence and live-check matrix](https://github.com/eq-p99-tools/eq-network/blob/988e193b368a09d9aeb3f2b7e43857e09b2c61b3/docs/eqmac-abilities-parity.md)
records pinned Quarm/TAKP sources, exact packets, recovery assumptions and
remaining gaps. The [profile skill follow-up](https://github.com/eq-p99-tools/eq-network/blob/4edb2a04ca62a3f68230e191555945026e149039/docs/eqmac-profile-skills.md)
records the missing-state defect and source layout; the separate silent
`SkillUpdate2` remains unsupported. Private live findings motivated these
fixes, but corrected live behavior remains unverified. The coordinated
retest must cover idle world selection/creation and profile skills, then
Mend/reset, feign posture and failure, stealth state/cancellation, forage delivery, each strike,
Taunt and training transitions on each server separately.

Fishing, Bind Wound, active stealth toggles, knight epic shieldless Bash,
larger-model reach, haste/slowing/AA/focus recovery and complete persistent
reuse remain gaps. Frenzy is not dispatched by the inspected EQMac servers.
No installed assets, character data, captures or credentials are fixtures.
