# Installed spell providers

The installation selects the generation's spell file and layout. Its base
definitions remain immutable. P99's separate provider caches explicit numeric
source projections and applies complete destination/source replay batches;
other server policies continue to use their own installed definitions.

A first registration snapshots the source's **current effective** values.
Duplicate registration never refreshes that snapshot. Replay resolves immutable
cached sources, preserves wire order (the last duplicate destination wins),
and retains destination names and fade text. Only explicitly supported numeric
fields are copied, including icon and beneficial/harmful classification. Unknown
optional source values remain unknown. No native pointers are represented.

Every source and destination must exist before a batch commits. A malformed
message, missing record, order gap or invalid stage makes mechanics unavailable
for that provider. A valid prefix never applies. This host policy deliberately
latches failure until explicit provider replacement followed by genuine server
registration and replay; it does not claim native cache-reset parity or send
an unsupported recovery request.

Readiness distinguishes awaiting registration, awaiting the current zone's
replay, ready and unavailable. Registration alone does not certify the current
zone. Zero-count registration and replay are valid facts; an empty replay is
ready only after registration and does not restore omitted destinations.
Destination labels remain available while mechanics wait. Casting is refused
while spell data is unavailable; buff-dependent movement uses the existing
missing-mechanics report. Chat and logging continue.

The host allocates `spell_tables::Binding` when creating a producer. Each
`WorldUpdate::SpellTable` envelope retains that epoch and policy **before
enqueue**. A replacement `Session` carries its new binding to the renderer;
old queued Connecting and table facts cannot be relabeled as current. A real
Connecting lifecycle notification establishes a network run, with a monotonic
producer attempt number. A new run ID appearing only in table data cannot
reset ordering. Overload of the bounded event bridge fails the session.

Cached source snapshots and effective values survive zone/world transport
changes and automatic retry within the same producer/provider lifetime.
Changing the provider, installation or policy requires a new binding and real
registration/replay. The network run/revision and zone admission IDs establish
freshness; they do not define provider lifetime. Provider facts route before
profile/admission and presentation loading gates. They do not fabricate movement
acceptance, clear receipt identity, or alter another policy's definitions.

The implementation and synthetic tests establish these source-level contracts.
They do not establish attended live compatibility.
