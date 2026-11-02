# Development and maintenance

[Documentation index](README.md) · [Architecture contracts](../ARCHITECTURE.md)

## Build

The workspace uses Rust 2024 with minimum Rust 1.99 and Bevy 0.19.1. Use a native
Windows Rust toolchain to test native Windows behavior. Linux builds need
`pkg-config` and ALSA development headers (`libasound2-dev` on Debian/Ubuntu).
Windowed tests also require a working graphics/display environment; compiling
inside WSL does not establish desktop input or rendering compatibility.

```console
cargo build -p eq-client --release --locked
cargo run -p eq-client --locked -- --help
```

`Cargo.toml` pins the reviewed `eq-network` integration by Git revision;
`Cargo.lock` resolves the same commit. [Support](../SUPPORT.md) identifies that
revision and its server policies and validation coverage.
The asset decoder uses a pinned fork of `libeq`. A normal build needs neither a
sibling networking checkout nor unpublished local patches. Inspect resolved
sources with `cargo metadata --locked --format-version 1`. `--offline` is useful
only once all required dependencies have been fetched.

Use separate worktrees and `CARGO_TARGET_DIR` directories for concurrent feature
work. Do not reset or clean another session's worktree or build output.

## Validation

```console
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo doc --workspace --no-deps --locked
```

The automatic [Lint workflow](../.github/workflows/lint.yml) runs formatting on
PRs and pushes to main. The [CI workflow](../.github/workflows/ci.yml) is manual:
Actions → CI → Run workflow, choosing Linux, Windows or both. It runs Linux
Clippy/tests and native Windows tests. It does not log into public servers or
exercise a physical desktop/GPU. Neither workflow currently tests Rust 1.99 as
a separate minimum-version job; use that compiler explicitly when changing the
MSRV or dependencies.

Most tests use synthetic data. Ignored tests that need installed assets or a
rendering environment are opt-in; read each test's prerequisites before running
it. Some probes save screenshots or inspect local files, and are not portable
unit tests. A passing test count must name the revision and whether ignored
probes actually ran.

For live evidence, record exact client/network revisions, server policy,
scenario, duration and observed outcome. Initial admission is not a death/zone
recovery test. Report local tests, remote CI and live compatibility separately.
Keep proprietary assets, raw captures, credentials, account/character identifiers
and official-client text out of public fixtures and reports. Read installed text
at runtime by its string identifier instead of copying it into tests.

## Dependency changes

Update networking intentionally: record old/new commits, review lockfile changes,
run applicable checks, and exercise affected scenarios with authorization. A
networking feature is not integrated merely because its upstream README changed.
The resolved policy and runtime capabilities are the authority; see
[Support](../SUPPORT.md).

## Documentation ownership

Keep the root README a short starting point. User workflows belong in the linked
guides, policy/evidence distinctions in SUPPORT, and implementation contracts in
ARCHITECTURE. Avoid duplicating a long command reference across these pages.

When behavior changes, check:

1. Flags and environment inputs against `apps/eq-client/src/main.rs`, `logins.rs`
   and `presets.rs`; keep example combinations valid.
2. Key bindings against `eq-client-render/src/keys.rs`, including installed-client
   defaults and per-character overrides.
3. Settings against their definitions and serialization, especially
   `eq-client-core/src/qol.rs`.
4. Server claims against the networking commit in Cargo.lock, separating wire
   parsing, offered commands and actual scenario evidence.
5. Relative links, anchors and examples after moving a section or renaming a file.

The six `docs/scripts` pages are embedded by
[`script_workbench::help`](../crates/eq-client-render/src/script_workbench/help.rs).
Its lightweight renderer supports headings, paragraphs, bullets, inline code,
fences and standalone links to those help pages. Do not assume it is a full
Markdown browser. Keep shared help within that subset; use `eqscript` fences
for executable examples, which the real parser tests. Update parser/targets,
syntax highlighting and reference pages together when extending the language.

## Character creation render check

The offline example loads a user-owned Titanium installation and captures a
hidden-window form with a synthetic name. It contacts no server. Run once per
page (0 identity, 1 appearance, 2 attributes, 3 home/review), keeping output outside Git:

```console
cargo run -p eq-client-render --example character_creation -- "/path/to/EverQuest" "/private/output/creation.png" 0
```

Append `classic`, `titanium` or `eqmac`, a race ID, gender (0/1), and optionally
`last` to inspect the last offered cosmetic choices. For example, append
`eqmac 8 1 last` for a female dwarf. The synthetic draft opens in face close-up.

The review page deliberately retains unspent points so its validation and
disabled Create button can be inspected. State tests cover valid submission,
refusal/retry, success, full accounts, cancellation and stale lists separately.

The asset probe checks all 30 playable race/gender models, their standing
animations, standing bounds, layered materials and creation attachments without
opening a window:

```console
cargo run -p eq-client-assets --example inspect_creation -- "/path/to/EverQuest"
```

Appearance policy and packet tests exercise each offered cosmetic value for
Classic, Titanium and EQMac. Name tests cover stale identities, single-flight
requests, late replies, malformed responses and local generation. Offline
render checks and synthetic wire tests do not establish live-server acceptance.
