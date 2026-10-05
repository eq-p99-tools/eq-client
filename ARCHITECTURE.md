# Client extension contracts

## Ownership

Keep packet decoding and server policy in `eq-network`. Put engine-independent
world reduction, validation and gameplay calculations in `eq-client-core`, with
narrow data/lookup interfaces where installed assets provide inputs.
`eq-client-assets` returns owned decoded data; it must not require Bevy. Rendering
owns entities, GPU assets, layout, input arbitration and presentation. The app
owns configuration, worker lifetime and the event bridge.

Session validation state and frontend presentation state serve different owners.
Share their rules and typed records rather than letting the UI mutate session
state directly. Route commands through `Outbox`; do not add feature-specific
network channels or raw opcode dispatch to widgets.

What a control needs is a capability the session offers (`outbox::offered`),
never a flag the client keeps per server. A control the server type lacks says
"Not available on this server", and one this client lacks says "Not in this
client yet". Official wording comes from the player's installed client by its
eqstr id, with this client's own words only as the fallback.

## Ordered work and asynchronous outcomes

The frame stages run receive, scene, typing, routing, gameplay input, then
presentation. Text entry must consume keys before gameplay acts on them; scene
reconciliation must see the reduced world. Existing chains also order focus and
deferred entity changes. Preserve these edges when extracting feature systems.
Do not claim that removing a chain improves performance without measuring it.

Archive I/O and CPU decoding should run outside the frame consumer. Scope every
result to its admission, discard obsolete work, and apply the latest authoritative
placement when installing a scene. Keep graphics-resource mutation on the engine
thread. Cache owners need a documented lifetime or budget, including failed-load
entries; a process-wide map of strong handles is not an eviction policy.

An enqueued command is not a successful action. `Outbox` refuses what it cannot
send, such as a command while the queue is full; what it sends settles only when
the session reports the answer. The session holds what an operation uses until
the answer settles it, and where a server refuses with silence, as `EQEmu`
refuses a merchant offer, the hold ends after a timeout while a late answer
still settles the operation it answers and nothing else. A timeout never
authorizes resending a non-idempotent action. Admission reset invalidates
pending work, input prediction and asynchronous loads.

Keep the event callback bounded and fast. Do not block transport work on asset
loading or silently drop ordered inventory, cast and lifecycle events. Queue
overflow currently ends the app session explicitly; a future coalescing or
resynchronization design needs a coherent ordering boundary and tests first.

Use `Instant` for elapsed-time decisions and wall time for logs. Today the UI
reducer timestamps an update when it consumes it, which is not when the network
received it, so do not treat existing duration fields as network timings.

## Errors and compatibility

Use typed reasons for decisions the caller must make (queue busy, invalid
admission, missing resource inputs). Keep readable contextual errors at I/O and
process boundaries. Callers must not parse diagnostic strings to decide retries.
Unknown data should carry a reason when it affects behavior; ordinary absence
does not need an elaborate state machine.

Keep typed action enums and exhaustive internal matches. Public breaking changes
must have an explicit dependency migration and version policy; do not introduce a
generic action registry solely to reduce source lines. Networking's
[API contract](https://github.com/eq-p99-tools/eq-network/blob/main/API-CONTRACT.md)
says how its features, commands and their answers behave.

Regression tests should assert externally meaningful state: a delayed result
cannot affect a replacement item, queue recovery cannot lose movement grants,
formatted links keep their payload and UTF-8 ranges, and obsolete asset jobs
cannot install a scene. Keep live validation distinct from those synthetic proofs.
