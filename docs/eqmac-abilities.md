# EQMac ability menus and recovery

The client pins networking commit
[`988e193b368a09d9aeb3f2b7e43857e09b2c61b3`](https://github.com/eq-p99-tools/eq-network/pull/176),
which adds 15 supported abilities for TAKP and Quarm on top of the spell
workstream. The existing spell behavior and documentation remain in place.

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

The [networking evidence and live-check matrix](https://github.com/eq-p99-tools/eq-network/blob/988e193b368a09d9aeb3f2b7e43857e09b2c61b3/docs/eqmac-abilities-parity.md)
records pinned Quarm/TAKP sources, exact packets, recovery assumptions and
remaining gaps. All public behavior remains unverified in this workstream.
The coordinator's dedicated live-check batch must cover Mend/reset, feign
posture and failure, stealth state/cancellation, forage delivery, each strike,
Taunt and training transitions on each server separately.

Fishing, Bind Wound, active stealth toggles, knight epic shieldless Bash,
larger-model reach, haste/slowing/AA/focus recovery and complete persistent
reuse remain gaps. Frenzy is not dispatched by the inspected EQMac servers.
No installed assets, character data, captures or credentials are fixtures.
