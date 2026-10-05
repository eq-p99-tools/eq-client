# Support and validation

Three separate questions determine support:

1. **Implemented:** a codec, reducer or UI path exists.
2. **Offered:** the connected session advertises the capability for its server policy.
3. **Verified:** a named scenario passed against a specified server and revision.

The networking library lists a feature for a server type only once it has been
checked against that kind of server (P99's earliest listings wait for their check
on P99), so what a session offers was checked there; one scenario's check still
does not cover the others. A visible panel or passing synthetic test does not
establish live server support.
Frontends must use the session's offered capabilities and explicit player choices,
not infer them from a server name or protocol generation.

| Area | Client implementation | Availability and evidence boundary |
| --- | --- | --- |
| Offline world and characters | S3D/WLD loading, scene rendering and local input | Requires a user-owned installation; not a server test |
| Admission and chat | Character selection, entry, channel tabs and structured item links | Offered on every server type; recheck each server type when changing these paths |
| Movement, targeting and combat | Typed commands, projection, correction and input paths | Only offered capabilities are actionable; shared Titanium/EQMac layouts do not imply equal policy |
| Inventory, spells and interaction | Bags/equipment, casting/book, merchants, loot, doors and other panels | Individual actions may be absent even when a related panel exists |
| Zoning and recovery | Admission-scoped state, reset handling and server transfer paths | Test each transfer/death scenario separately; initial admission is not proof of recovery |
| Character creation | Networking library has generation-specific support | Attended scripts can create a character; the selection screen does not yet create or delete one |

The exact policy lives in the resolved networking revision's
`crates/eq-network/src/client/session/servers.rs`. Runtime capabilities remain the
authority. The networking repository's current README can describe features newer
than this application's lockfile.

## Dependency integration

`Cargo.toml` follows networking's `main`, but `Cargo.lock` chooses the actual commit.
Use `cargo metadata --locked --format-version 1` to inspect resolved package sources.
Do not call a newer networking fix integrated until the lockfile advances and the
client passes validation against that revision.

For an intentional networking update:

1. Record the current and proposed network commit, including related open PRs.
2. Update the dependency and review all lockfile changes.
3. Run format, workspace Clippy, tests, and minimum-version/Windows checks.
4. Exercise affected server scenarios with permission; record the exact client and
   network revisions, server policy, steps, duration and result. Keep credentials,
   game assets, raw captures, account identifiers and the official client's text
   out of public evidence; the client reads that text from the installation at
   runtime by its string id.

CI runs synthetic tests without proprietary assets or live logins. Ignored tests
requiring an installation remain a separate, opt-in validation step. Windows CI
checks native compilation and tests, not GPU rendering or desktop input. The
minimum supported compiler is Rust 1.95; stable Clippy is checked separately.
