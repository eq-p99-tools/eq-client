# EQMac loot receipts

An EQMac loot-item echo is receipt of a request, not confirmation that an
item was delivered. The client retires the matching corpse-view row and
clears its manual pending request through `Reply::LootReceived { place }`.
It does not emit `LootTaken { accepted: true }` for that echo, add an item,
change coins, or infer delivery from another same-item inventory gain.
Only the session's ordinary inventory and coin updates establish holdings.
A refusal can follow receipt and close the window with nothing delivered.

The Windows EQMac program's request routine is `0x426b02`; receipt processing
at `0x4e2ca2` calls the view-retirement routine `0x426c53`. The window's
periodic method `0x426708` checks reach. These reads establish manual
request/view behavior, without establishing a safe automatic loot-all loop.
The server-side ordering is documented in eq-network's Quarm loot contract.
No official program text or captured packets are fixtures.

`LootContinuation` selects the supported reply boundary by client generation.
Titanium retains the existing `TakenReply` loop. EQMac uses `Manual`: the
generic window omits Loot all, the action entry refuses a bulk request, and
the actual automatic command loop independently refuses it. Skinned or
scripted UI actions pass through the same input guard. Individual item
requests continue after a matching receipt, with delivery still unconfirmed
until actual inventory updates arrive. Closing a window does not roll back
an inventory update that arrives later.

Automatic EQMac loot-all remains unsupported. There is no transaction key
linking the corpse listing to inventory updates, and same-item gains can
have another cause. This implementation supplies no heuristic `Delivered`
event. Later source evidence can extend the generation policy without
changing the receipt/delivery distinction.

Validation must cover receipt followed by refusal, receipt-driven manual
readiness, no inventory/success from receipt, late real inventory delivery,
and both bulk-entry and already-armed-loop refusal. These synthetic checks
do not certify live server behavior.
