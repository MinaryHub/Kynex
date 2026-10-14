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
cargo run -p kynex -- view 127.0.0.1:7878 --programs cell-a.json   # programs kept elsewhere
```

This opens `http://localhost:8090` in a browser. The viewer is a web page
that KYNEX serves itself. It works on Windows, macOS, Linux and tablets, in
any browser with WebGL, and there is nothing to install. The page and
three.js are built into the `kynex` binary, so the viewer also works on a
network with no internet.

- When a viewer connects, the stream (`/events`, Server-Sent Events) tells
  it a scene exists. The page then fetches `/scene.bin`: a JSON header
  followed by f32 matrices and vertices and u32 indices, read straight into
  typed arrays. After that, only changes travel, as JSON on the stream:
  matrices of moved objects, and meshes only when their geometry changes.
  Each change is derived only from the objects the session patch names,
  plus everything below them.
- Drawing: every part lives in one `THREE.BatchedMesh`, with three levels
  of detail per mesh (full, 25%, 6%, made with meshoptimizer). Each part is
  drawn at the level its size on screen calls for. The scene is rendered
  once into a target; edges come from that pass's depth in the composite.
  The shadow map is redrawn only when the scene changes.
- The scene tree is virtualized: only the rows in view are in the page.
- Coordinates stay in f32 world space. At 500 m from the origin, f32 still
  resolves about 0.03 mm, which is enough for a factory. Camera-relative
  rendering is left until a site is kilometres across.
- Panels: scene tree with search, a joint panel per robot (value, limits,
  out-of-limit warning, and posing: see below), an inspector for the selected object, and a view
  toolbar (fit, ISO / top / front / side, joint gizmos, wireframe, grid).
  Keys: `F` fit, `0`/`7`/`1`/`3` views, `J` `W` `G` toggles, `Esc` deselect.
- `--lan` lets other devices on the network open the viewer. The viewer
  shows the whole scene to anyone who can reach the port, so the default is
  this machine only. Anyone who can open the viewer can also pose its robots. That
  pose stays in KYNEX and never reaches Kreate.
- The server only answers requests addressed to `localhost` or to an IP
  address. This blocks DNS rebinding, where a web page elsewhere points its
  own name at this machine to read the scene. On the LAN, open the viewer by
  IP (`http://192.168.0.12:8090`), not by machine name.
- `--file model.kreate` shows a saved file instead of a session.
- On screens narrower than 960 px, the side panels become drawers.

### Posing robots

The robot panel can pose a robot in the viewer. KYNEX works out the pose
(`crates/kynex-kin`) and shows it. Nothing is sent to Kreate: the robot
is marked **로컬 자세** and keeps that pose while the session moves on.
**세션 자세로** lets go of it, and the robot takes the session's pose again.

- **Joints**: drag a joint's bar. The joint stays within its limits.
- **TCP pendant**: X/Y/Z and Rx/Ry/Rz jog buttons, in world or tool
  axes. Hold a button to repeat. Steps are 1 mm · 1°, 10 mm · 5° or
  50 mm · 15°. **이동할 때 자세 유지** keeps the tool's orientation while it
  moves. It starts on for arms with six or more joints. Arms with fewer
  cannot keep an orientation everywhere, so for them it starts off.
- **Handle**: **핸들** puts a move/rotate handle on the TCP in the scene.
  Dragging it sends the tool after the handle.
- The TCP is the tool link plus a Z offset. The tool link is the link
  named `tcp`, `tool0`, `tool`, `flange`, `ee_link` or `end_effector`. If no
  link has one of those names, it is the link furthest out along the
  moving joints.
- When a pose cannot be reached, the arm goes as near as it can. The
  panel then shows how far off it is, in mm and degrees.

The inverse kinematics is damped least squares on the Jacobian, with the
joint limits enforced. It takes any tree Kreate holds: prismatic axes,
robots on tracks, and mimic joints. A drag solves from the current pose,
so the arm does not jump to another configuration. A request can ask for
random restarts (`"restarts": n`) when the start is far from the goal.

### Programs

Each robot card has a **프로그램**: a list of steps.

1. Pose the robot with the controls above.
2. Add the pose with **+ MoveJ** or **+ MoveL**.
3. Between moves, add an action from **+ 동작**:
   - 그리퍼 잡기 / 놓기 (grip, release).
   - 장비 명령: open or close a CNC machine's door, clamp or unclamp its
     chuck, or start its cycle.
   - 신호 대기: wait until a machine says its door is open or closed, its
     chuck is clamped, it is idle, or its cycle is done.
   - 시간 대기: wait a set number of seconds.

   An action's machine, grip point, command or signal is changed in its
   row. Clicking a step's row picks it, and the next step added goes
   after it.
4. **▶ 실행** runs the program (`apps/kynex/src/run.rs`).

KYNEX times the moves (`kynex_kin::path`):

- **MoveJ**: all joints move at once and arrive together. The speed is a
  share of each joint's top speed (its URDF `velocity`).
- **MoveL**: the tool point moves along a straight line at the given
  mm/s, and its orientation turns evenly on the way. An arm with fewer
  than six joints holds the line only: its tool's orientation follows
  the joints.
- Each move speeds up, cruises and slows down. The list shows each move's
  time and the cycle time. **순환 재생** adds a move back to the first
  point and repeats the cycle.
- A move that cannot run as asked is planned as near as it can be, then
  flagged in the list. This happens when a line leaves the reachable
  space, when a line needs a joint to go faster than it can (the move is
  slowed and the factor is shown), or when a line ends in a different
  arm configuration than the one taught.
- The tool's path is drawn in the scene in blue, and the points in white.
- Each point has buttons to go to it (→), re-teach it from the current
  pose (⟳) or delete it (✕). Its J/L chip switches it between MoveJ and
  MoveL.
- The time shown beside the program (**이동**) is its moves'. With
  actions, a cycle also waits for machines, so the cycle time is measured
  by running it.

**Running**: steps run by the simulation's clock, and the CNC machines
run by the same clock.

- A move is planned when it starts, from wherever the robot is then.
- A wait holds until the machine says what it waits for.
- A step the cell refuses stops the run with an alarm that says why, as a
  controller would. For example: starting a cycle with the door open,
  gripping with nothing there, or opening the door while machining.
- **배속** runs the clock 1–20 times real time, so a 90 s cycle takes
  4.5 s at 20×.
- With **순환 재생** the program runs again from its first step. The
  panel shows the step running, how many cycles have run and how long
  the last one took.
- Moving the robot by hand (jog, IK, → to a point, 세션 자세로) stops the
  run.
- The path check (Collisions, below) takes the program's door commands
  into account: a door the program has opened by a move is out of that
  move's way.

Programs, and the cell's roles (below), are saved to the project file
`kynex-project.json` in the directory `kynex view` runs in. Use
`--project FILE` to keep them somewhere else. An older
`kynex-programs.json` is read once if no project file exists yet. Every change
is written straight away. A program records its robot, joints and tool by
identity and by name. When Kreate builds the scene again and numbers the
objects anew, the program finds them again by name.

**URScript** downloads the program as a `.script` file for a Universal
Robots controller (`kynex_kin::urscript`). The program must be for a robot
with six revolute joints. Actions are not exported yet, because they need
the cell's I/O mapped to the controller's inputs and outputs. Each action
is written as a comment where it falls between the moves
(`# KYNEX, not exported until its I/O is mapped: …`), to be completed on
the controller.

- MoveJ becomes `movej` with the taught joint positions.
- MoveL becomes `movel` to the taught tool pose, in the controller's base
  frame (the `base` link, half a turn from `base_link`).
- The tool point is set with `set_tcp`, measured from `tool0`.
- The robot first goes to the first point with `movej`. With 순환 재생
  on, the rest runs in a `while (True):` loop.
- The controller times moves in its own way, so a cycle on the robot is
  close to the planned cycle, but not equal to it.

### The library: placing equipment

The **라이브러리** tab, beside the scene tree, lists equipment to place
(`apps/kynex/src/library.rs`):

- 로봇: UR10e + 이중 그리퍼, UR5e.
- 공작기계: CNC 선반.
- 주변 장치: 투입 / 배출 팔레트 테이블, 로봇 받침대.
- 안전: 안전 펜스.

Drag an entry onto the scene, or click it and then click the scene. It
lands on the floor, or on top of whatever is under the pointer (a robot
on its pedestal). A CNC machine is turned so its door faces the origin.
Select a placed item, here or in the scene, to set its position, turn
and sizes in the inspector, or to move it with a handle (`R` turns it,
`T` moves it). Sizes change the item's shape, and the item keeps its
identities when they do.

Equipment comes with its roles. A CNC machine is a CNC to the cell, a
pallet table's plate is a pallet, and a tending robot's grip points are
grippers. Placed equipment is KYNEX's own: it is added to the scene KYNEX
shows, under identities Kreate never issues (`0xFFFE << 48` on), and kept
in the project file. `kynex view --empty` lays out a cell with no Kreate
session at all.

### The cell: machines, pallets, grippers, parts

For a machine-tending cell, KYNEX keeps what each object is to the process
(`apps/kynex/src/cell.rs`). It stores this in the project file and does
not change the session's scene.

- **Roles**: select an object, then pick its role under **셀 역할** in the
  inspector:
  - CNC 장비: the object whose children include its door and chuck.
  - 투입 팔레트 / 배출 팔레트: pallets.
  - 그리퍼: a grip point on a robot link.
  - 안전 펜스: a safety fence.

  Settings start at industry-typical values (docs/roadmap-vs-visual-components.md
  §4): 90 s machining, 3 s door, 1 s chuck, 5 × 4 places 100 mm apart, and
  0.5 s grip. **셀 자동 구성** sets roles from names. A CNC machine is the
  lowest object with a `…door…` and a `…chuck…` below it. Pallets are
  named `infeed` and `outfeed` (`투입`, `배출`). Grip points are robot links
  named `grip…` with no link below them.
- **Parts**: **소재 채우기** puts a raw part (Ø60 × 80 mm) on every free place
  of a pallet. Parts are KYNEX's own objects in the scene it shows. Their
  identities come from a range Kreate never issues (`0xFFFF << 48` on), so
  they keep their identities whenever the scene is rebuilt.
- **Gripping**: the robot card lists its grip points with **잡기** and
  **놓기**, and the TCP can be any of them. **잡기** takes the part within
  20 mm of the grip point. The part then moves with the gripper and is not
  counted as a collision. **놓기** sets the part into a free pallet place or
  chuck within 30 mm, or leaves it where it is.
- **CNC machines run**: in the inspector of a CNC machine:
  - **문 열기/닫기** slides its door aside over the door time.
  - **척 물림/풀기** clamps the chuck. A clamped part cannot be gripped.
  - **가공 시작** runs the cycle, only with the door shut and a raw part
    clamped. When the cycle time is up, the part in the chuck is finished.

  The door, the chuck, the part in the chuck and the cycle's progress
  are shown as they change.
- The demo cell (`cargo run -p kynex-link --example cnc_cell`) has a UR10e
  with a double gripper on a pedestal, two CNC machines, an infeed and an
  outfeed pallet, and a fence.

### Collisions

KYNEX checks each robot's links against everything else in the scene,
and against the robot's own links (`crates/kynex-collide`).

- **Live**: parts in contact at the pose shown turn red, the contact
  point is marked, and the robot card lists the pairs (**충돌 N곳**). The
  check runs on every scene change, so it covers jogging, programs
  playing and robots moving in Kreate.
- **Along a program**: each move is checked every 20 ms. The first
  contact in a move shows under it, for example
  `충돌: tool0 ↔ pallet (0.17 s)`. The rest of the scene stands where the
  session has it.
- A robot collides with its collision meshes (`RobotPart::Collision`,
  from the URDF) where it has them, and with its visible meshes where it
  does not. Two links joined by one joint are not checked against each
  other, because they always meet at the joint.
- The check is exact on the triangles. A surface has to pass more than
  10 µm into another to count, so parts resting flush do not collide.
  A mesh wholly inside another, with no surfaces crossing, is not seen.
  Along a path, the surfaces cross on the way in, so the path check
  still catches it.
- There is no clearance check (minimum distance) yet.
- Cost: a scene without robots costs a scan of its objects. With robots,
  only the obstacles whose boxes reach the robots' space are tested.
  `kynex bench` measures it (`contacts`, `+arm`). With one arm, a check
  took 1.8 ms at 5,000 parts and 28 ms at 50,000. The live check waits
  at least three times as long as the last check took before running
  again, so a large scene keeps up with its updates. The last change is
  always checked once the scene goes quiet.

The demo session also has a UR5e beside the pallet. It does not move on
its own, so you can program it.

The page sends its requests as JSON to `POST /robot`:
`{"op": "jog", "robot", "joint", "position"}`,
`{"op": "ik", "robot", "tool", "offset", "position", "rotation"}` and
`{"op": "release", "robot"}`, and for programs `teach`, `retouch`, `edit`,
`delete`, `close`, `clear`, `goto`, `play`, `stop` and `urscript`. Programs reach
the page as the `programs` event, and the pose and playback as `pose`.
Identities are strings, positions are in
metres and radians, and `rotation` is a quaternion `[x, y, z, w]` (or
`null` for position only). The server takes only `application/json`
bodies. That way, a page on another site cannot post a pose without
first asking the browser for permission, and this server never grants
it.

To try the viewer without Kreate, start a demo session. `demo_session` has
a robot on a track and a UR5e; `cnc_cell` has the machine-tending cell. It hosts a
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
Results are written to `bench-results/`. Use `--no-open` to serve the
page without opening it, for example to drive it from another browser.

Measured on 2026-10-13 with an RTX 4060 Ti, Chromium on Direct3D 11, a
release build, 1% of parts moving 20 times a second:

| Parts | Overview fps | Close-up fps | First frame | Server update, 1 part moved |
|---|---|---|---|---|
| 500 | 129 | 130 | 0.16 s | 0.2 ms |
| 5,000 | 130 | 130 | 0.22 s | 0.8 ms |
| 20,000 | 117 | 109 | 0.67 s | 4.3 ms |
| 50,000 | 63 | 42 | 1.5 s | 11 ms |

129-130 fps is the display's refresh limit. Kreate's own collab session
does not keep up past a few thousand objects. See K6 in
docs/kreate-change-requests.md.

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
| `crates/kynex-kin` | `Kinematics`: forward kinematics, Jacobian, inverse kinematics with limits and mimic joints; `path`: MoveJ/MoveL timing; `urscript`: UR post-processor |
| `crates/kynex-collide` | Mesh-to-mesh contact: exact triangle tests, a BVH per mesh, and a broad phase over the scene |
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
