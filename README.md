# KYNEX

Robot and factory simulation. KYNEX reads its scenes live from Kreate.

## Kreate link

KYNEX joins Kreate's shared session as a participant. It receives every edit
as soon as the relay forwards it: objects, transforms, meshes, and robot
links, joints, limits, inertia and rigid-body settings. The link is
read-only: KYNEX sends no patches, so it cannot change the scene in Kreate.

```
Kreate ──patch──▶ relay (kreate-collab, TCP 7878) ──patch──▶ KYNEX (KreateLink → Model)
```

1. In Kreate, open **File ▸ 공동 편집 세션… ▸ 이 장면으로 세션 열기** (set a key if you want one).
2. Run KYNEX:

   ```
   cargo run -p kynex -- link 127.0.0.1:7878 --key <session key>
   cargo run -p kynex -- link 192.168.0.12 --json      # one JSON object per line
   cargo run -p kynex -- inspect model.kreate          # a saved file, offline
   ```

If KYNEX starts before Kreate opens the session, it waits and connects once
the scene is there. It never seeds the session with an empty scene.

## 3D viewer

```
cargo run -p kynex -- view 127.0.0.1:7878 --key <session key> --open
```

This opens `http://localhost:8090` in a browser. The viewer is a web page
that KYNEX serves itself. It works on Windows, macOS, Linux and tablets, in
any browser with WebGL, and there is nothing to install. The page and
three.js are built into the `kynex` binary, so the viewer also works on a
network with no internet.

- Updates are pushed to the browser over Server-Sent Events (`/events`).
  KYNEX sends a full scene when a viewer connects, then only what changed:
  world matrices of objects that moved, meshes only when their geometry
  changes.
- Panels: scene tree with search, a joint panel per robot (value, limits,
  out-of-limit warning), an inspector for the selected object, and a view
  toolbar (fit, ISO / top / front / side, joint gizmos, wireframe, grid).
  Keys: `F` fit, `0`/`7`/`1`/`3` views, `J` `W` `G` toggles, `Esc` deselect.
- `--lan` lets other devices on the network open the viewer. The viewer
  shows the whole scene to anyone who can reach the port, so the default is
  this machine only.
- The server only answers requests addressed to `localhost` or to an IP
  address. This blocks DNS rebinding, where a web page elsewhere points its
  own name at this machine to read the scene. On the LAN, open the viewer by
  IP (`http://192.168.0.12:8090`), not by machine name.
- `--file model.kreate` shows a saved file instead of a session.
- On screens narrower than 960 px, the side panels become drawers.

To try the viewer without Kreate, start a demo session. It hosts a
Kreate-compatible session with a robot on a linear track that keeps moving:

```
cargo run -p kynex-link --example demo_session -- --key demo
cargo run -p kynex -- view 127.0.0.1:7878 --key demo --open
```

## Benchmark

```
cargo run --release -p kynex -- bench              # this side only
cargo run --release -p kynex -- bench --browser    # also the viewer, on this machine's GPU
```

The benchmark builds synthetic factories in four tiers: `cell` (500 parts),
`line` (5k), `factory` (20k) and `factory-xl` (50k). It measures model
rebuilds, the viewer's scene and updates, and the shared session's snapshot
and patches. With `--browser`, the viewer opens in the default browser and
reports its load time, frame rate and draw calls while it orbits the scene.
Results are written to `bench-results/`.

## Kreate dependency

Kreate is pinned to one commit (`rev` in `Cargo.toml`) and fetched with the
git CLI, because the repository is private. To change Kreate and KYNEX
together, build against the checkout beside this one:

```
cargo --config .cargo/kreate-local.toml test --workspace
```

The changes KYNEX needs from Kreate are in
[docs/kreate-change-requests.md](docs/kreate-change-requests.md).

## Layout

| Path | What |
|---|---|
| `crates/kynex-link` | `KreateLink`: the session connection, the mirrored `Document`, reconnection |
| `crates/kynex-model` | `Model`: world poses, robots as joint trees, and `Diff` between models |
| `crates/kynex-view` | The viewer: scene → JSON (`ViewState`), HTTP/SSE server, and the web page (`web/`) |
| `apps/kynex` | The command line (`view`, `link`, `inspect`) |

The Kreate crates (`kreate-scene`, `kreate-io`, `kreate-collab`) are path
dependencies on `../Kreate`. Both sides use the same scene types, so nothing
is converted between them. `Cargo.lock` started as a copy of Kreate's.

## Tests

```
cargo test --workspace
```

`crates/kynex-link/tests/live.rs` runs a real relay and a real Kreate
`Session`. It checks that a joint turned in Kreate arrives in KYNEX within
500 ms, with the child link's world pose correct to 1e-9 m. It also checks
limit changes, adding and deleting objects, KYNEX connecting first, and a
wrong session key.
