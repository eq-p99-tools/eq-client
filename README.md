# eq-client

`eq-client` is an experimental native, top-down EverQuest client. The first
milestone is an offline viewer for classic S3D/WLD zones. It loads game data
from an EverQuest installation supplied by the user; this repository and its
build artifacts do not contain or redistribute EverQuest assets.

The code is intentionally split at stable boundaries:

- `eq-client-core` owns engine-independent world coordinates and updates.
- `eq-client-assets` turns local EQ archives into owned, renderer-independent
  meshes and textures.
- `eq-client-render` is the reusable Bevy presentation layer.
- `eq-client` is the command-line application and offline demo.

Future online support will translate typed events and commands from the
[`eq-network`](https://github.com/eq-p99-tools/eq-network) crates into
`eq-client-core`. Packet parsing, connection state, game state, and rendering
will remain separate, so movement, zoning, inventory, combat, and other systems
can be added without coupling the UI to a specific server protocol.

## Offline East Commonlands demo

Install current stable Rust, then point the viewer at your own EQ directory:

```console
cargo run -p eq-client --release -- \
  --eq-dir "/path/to/EverQuest" \
  --zone ecommons
```

On Windows PowerShell:

```powershell
cargo run -p eq-client --release -- `
  --eq-dir "C:\Program Files (x86)\Sony\EverQuest" `
  --zone ecommons
```

Drag with the right mouse button to orbit and use the wheel to zoom. Add
`--camera orthographic` for a locked isometric-style projection.

To validate asset loading without opening a graphics window:

```console
cargo run -p eq-client -- --eq-dir "/path/to/EverQuest" --inspect-only
```

For a repeatable visual smoke test, save a frame after the scene loads:

```console
cargo run -p eq-client --release -- \
  --eq-dir "/path/to/EverQuest" \
  --screenshot offline-demo.png
```

`EQ_CLIENT_DIR` may be used instead of `--eq-dir`. Zone archives, extracted
files, caches, credentials, and packet captures must remain outside the
repository. On Windows, the viewer also detects the standard
`Program Files (x86)\Sony\EverQuest` installation when neither setting is
provided.

## Status

This milestone loads base zone geometry and diffuse textures. The next
rendering milestone adds placed objects, sky, transparent and animated
materials, and water. It does not connect to a server yet.

## License

The original code in this repository is available under the [MIT License](LICENSE).
The loader currently uses the MIT-licensed
[`libeq`](https://github.com/cjab/libeq) project at a pinned revision.
