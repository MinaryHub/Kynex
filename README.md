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
kynex                                    # on its own, in the browser: the last project
cargo run -p kynex -- view --open        # the same, from the source
cargo run -p kynex -- view --project 셀.json
cargo run -p kynex -- view 127.0.0.1:7878 --key <session key> --open   # with a Kreate session
cargo run -p kynex -- view --kreate      # Kreate's session on this machine
```

KYNEX runs on its own: no Kreate is needed for anything but showing a
Kreate scene. Started with nothing (`kynex`, as an installed program is
started), it serves the viewer and opens it in the browser, with the
project last opened; the first time, a new one, `Documents\KYNEX\새
프로젝트.json`. Its own settings and the list of recent projects are kept in
`%APPDATA%\kynex`; nothing is written beside the program. A project open in
one KYNEX is not opened by another (a `<project>.lock` held beside it while
open, on Windows). A Kreate session is joined only when its address, or
`--kreate`, is given.

**Importing models.** **모델 가져오기** in the library (or a file dropped on
the scene) reads a CAD or mesh file with Kreate's readers, without Kreate:
STEP, STL, OBJ, glTF/GLB, 3MF and PLY. The model is kept as equipment of the
user's own, standing on the floor at its middle, placed at the origin and
selected; one edit to undo. STEP, glTF and 3MF name their units; for STL,
OBJ and PLY the unit is chosen (자동: mm for STL, m for the others). A model
over 2 million triangles, or over a kilometre, is refused. It is read on a
thread of its own, so the viewer goes on meanwhile. Files go to `POST
/import` as bytes, from this computer only, at most 256 MB.

This opens `http://127.0.0.1:8090` in a browser. The viewer is a web page
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
- **Edges**: **모서리** turns clicks in the scene into picking an edge of
  a part, from its mesh: where two faces meet at more than 30°, joined
  end to end until three or more meet. The edge is drawn; **프로그램에
  추가** puts a path along it into the program after the chosen step: a
  MoveJ to a point clear of its start (50 mm by default), MoveL along it
  at the speed given (50 mm/s by default, a deburring pace) through as
  few points as keep within 0.5 mm of it, and a MoveL out again. The tool
  points into the edge along the bisector of its two faces, its X along
  the way it goes; each point is solved from the one before, and how
  many it cannot reach is said. Pick the TCP the work is done with (the
  tool's tip) first: the path is for it. Its moves after the first are
  the tool's work: within 0.2 m of where its tip goes on them, coming
  close to the part is not warned of, as at a gripper's grip.
- **Measure, report, video** (the toolbar): the ruler (M) measures between
  two points clicked on shapes — the distance and ΔX/ΔY/ΔZ in mm; the
  report opens a page to print or keep as a PDF: the cell from above and
  at an angle, every piece of equipment where it stands and its size,
  the robots and their programs, every role and its settings; ● records
  the 3D view as a WebM video until pressed again.
- **Snapping**: a robot let go within 0.25 m of a pedestal's or an AMR's
  top stands on it; a conveyor let go near another's end runs on from it,
  in line, flowing the same way, its belt as high. Placing and letting go
  of the move handle snap; hold Alt to let go where it is.
- **My equipment**: **내 장비로 저장** in the inspector keeps the selected
  object and everything below it — shapes, colours, roles and their
  settings — in the library under 내 장비, saved with the project, to be
  placed again like any template (✕ takes it out, once none is placed).
  An object with a robot in it cannot be saved so.
- **Moving parts and connection points** (roles, so they are saved with
  the project and with 내 장비): a part below a CNC machine or moulding
  machine given the role **문** moves as that machine's door does —
  **미닫이** slides along its own X, Y or Z by **거리** (0: aside by its
  own width along the machine's X, as a part named `door` does), and
  **여닫이** swings by **각도** about that axis through a hinge picked on
  the shape with **회전 중심 찍기**. A machine may have several. A part
  given the role **연결점** at a picked point makes it a connection point
  of the equipment saved from it: a robot's stand, a conveyor's start or
  end, or the foot it stands on another's mount by. Both show as green
  dots.
- **Reach**: **범위** draws where the flange reaches within the joints'
  limits, see-through, swept round the first joint, with its section
  outlined; it follows the robot when moved. As a datasheet's working
  range, the arm running into itself or the floor is not counted.
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
2. Add the pose with **+ MoveJ** or **+ MoveL**. **+ MoveC** (a circular
   move) is pressed twice: at the pose its arc passes through, then at
   its end. The tool goes along the arc of the circle through where it
   starts, the via point and the end. A picked circular point shows its
   via point, to go to (→) or re-teach (⟳).
3. Between moves, add an action from **+ 동작**:
   - 그리퍼 잡기 / 놓기 (grip, release).
   - 장비 명령: open or close a CNC machine's door, clamp or unclamp its
     chuck, or start its cycle.
   - 신호 대기: wait until a machine says its door is open or closed, its
     chuck is clamped, it is idle, or its cycle is done.
   - 시간 대기: wait a set number of seconds.
   - 팔레트 다음 칸: move a pallet's count on to its next place (see
     below).

   An action's machine, grip point, command or signal is changed in its
   row. Clicking a step's row picks it, and the next step added goes
   after it.
4. **▶ 실행** runs the program (`apps/kynex/src/run.rs`). Each robot runs
   its own: started on another robot, a program goes on beside those
   running, on the same clock, and the timeline has a lane for each.

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
  slowed and the factor is shown), when a line ends in a different
  arm configuration than the one taught, or when a line or an arc passes
  near a singularity: where, and which joints would race there (the
  Jacobian's least singular value under 10 mm/rad, a turn of the tool
  counted as 0.1 m — about 10° from a wrist singularity, 5° from an
  elbow's). The robot panel says so for the pose the robot is in, too.
- The tool's path is drawn in the scene in blue, and the points in white.
- Each point has buttons to go to it (→), re-teach it from the current
  pose (⟳) or delete it (✕). They show while the row is pointed at. Its
  J/L chip switches it between MoveJ and MoveL.
- **⌖** sets a point in the frame of the equipment nearest it: a CNC
  machine or a pallet. The line under the point shows its frame, to be
  changed there (월드 for none).
  - In a machine's frame, the point moves with the machine: move or turn
    the machine in the library and the program still reaches its chuck.
  - On a pallet, a point also goes from place to place. Teach the moves
    over one place of the pallet: above it, down to it, back up. A run
    takes each such point to the place the pallet's count has reached,
    at the same height and turn. **팔레트 다음 칸** moves the count on,
    from the last place back to the first. One program then goes through
    the whole pallet: the infeed place by place, the outfeed filling up.
  - The joints are solved from the taught ones, so the arm keeps the
    posture it was taught in.
  - The place a point was taught over is the one nearest the tool point
    or a grip point, so a double gripper's offset grip points count too.
  - A place the arm cannot reach stops the run with an alarm.
- **Blend**: a picked point shows its blend radius (**블렌드**, mm). A
  point with one is passed without stopping: the next move starts while
  the tool is still that close to it, and the tool rounds the corner
  within about the radius (`◠50` beside its name). Only into another
  move: before an action the robot stops, as it must to grip or wait.
  The moves' time and a run both count the time saved. URScript writes
  it as the move's `r=`, and a circular move as `movec`.
- **Programs and routines**: a robot has its main program (메인) and the
  routines it calls, picked beside **프로그램**: **+** makes a routine,
  **✎** renames the one shown, **✕** removes it (not the main one).
  **서브루틴 호출** in **+ 동작** runs another of the robot's programs a
  number of times and comes back. Renaming a routine renames its calls.
  - A run, and URScript, take the program with its calls replaced by what
    they call, so blends, pallet places and frames work across them; the
    run shows the routine and its own step (`쉬기 · 2단계`).
  - A routine calling itself, or calls deeper than eight, stop the run
    with an alarm saying so.
  - ▶ 실행 runs the program shown; a scenario comparison runs each
    robot's main program.
- **Order**: a picked step, or a span picked with Shift-click, moves up
  or down past its neighbour with ↑ and ↓ under the add buttons. Tending
  two machines in turn is laid out this way: load one, load the other,
  then unload each as it is done.
- **The same work at another machine**: click a step, then Shift-click
  another to pick the steps between. **CNC 골라 복제…** copies them after
  the span for another machine. Commands and waits for the first machine
  become the other's. The points in the first machine's frame are solved
  again to stand as far from the other. Other points, such as those on a
  pallet, are copied as they are. Set the points' frames before copying.
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
- A run starts with the infeed stocked: every free place gets a raw part.
  While a program runs round, an operator is assumed to keep the cell
  supplied. A pallet the program goes through with 팔레트 다음 칸 is
  changed whole: the infeed once it is empty, the outfeed once it is
  full. A pallet the program always uses at one place is kept supplied
  place by place, as in a scenario comparison.
- Moving the robot by hand (jog, IK, → to a point, 세션 자세로) stops the
  run.
- The path check (Collisions, below) takes the program's door commands
  into account: a door the program has opened by a move is out of that
  move's way.

**타임라인** (toolbar) shows what the robot and each CNC machine did
while the program ran, as a Gantt chart (`apps/kynex/src/timeline.rs`):

- The robot's lane: moving, gripping, waiting for a machine's signal,
  waiting a set time, and the alarm that stopped the run.
- Each machine's lane: cutting, its door moving, a finished part waiting
  to be taken out, and idle.
- Beside each lane is its share of the run: a machine's utilization (the
  time it cut), the robot's working and waiting time.
- At the top: the time run, the parts finished and the parts an hour.
  Run with 순환 재생 for the rate of a steady cycle.
- A new run starts a new timeline. The panel keeps the last one.

**시나리오 비교** (the panel's second tab) answers questions such as "how
many parts an hour with one CNC machine, and with two?"
(`apps/kynex/src/scenario.rs`):

1. Lay out the cell and make its program. Save it with **현재 구성 저장**.
   A scenario holds the equipment, the cell's roles and the programs, in
   the project file.
2. Change something, such as a second machine, a machining cycle or a
   gripper, and save again under another name. The same name saves over.
3. **▶ 모두 실행** runs every scenario for 1, 8 or 24 hours on the
   simulation's clock, away from the view. The table shows each one's
   parts finished, parts an hour (the best is marked), CNC utilization,
   and the robot's work and waiting time. A program that stops on an
   alarm shows why.
4. **불러오기** puts a scenario's equipment, roles and programs back, to
   change it further.

While comparing:

- Every program runs round and round.
- An operator is assumed to keep the cell supplied: a raw part on every
  free place of the infeed, and finished parts taken off the outfeed as
  they are set down. These count as the parts delivered.
- The first cycle, before a part is done, is in the figures, so a longer
  run gives a rate closer to steady.

The same comparison runs from the command line, without the viewer:

```
cargo run -p kynex -- compare --project kynex-project.json --hours 8
cargo run -p kynex -- compare --file cell.kreate --json   # the session's scene from a file
```

Programs, and the cell's roles (below), are saved to the project file:
the one last opened, or one named with `--project FILE` (a
`kynex-project.json` left by an older KYNEX in the folder it is started in
is still opened, when no project has been opened yet). An older
`kynex-programs.json` is read once if no project file exists yet. Every change
is written straight away. A program records its robot, joints and tool by
identity and by name. When Kreate builds the scene again and numbers the
objects anew, the program finds them again by name.

**Undo.** Ctrl+Z undoes the last edit to the project and Ctrl+Y (or
Ctrl+Shift+Z) redoes it; so do the arrows beside the project button,
which say what they would undo or redo. Edits are equipment placed,
moved, sized, copied or removed, cell roles and their settings, program
steps and routines, and PLC wiring; a handle dragged or a number typed
is one edit, not one per frame or key. Up to 100 are kept, until another
project is opened. What runs stops on an undo; the robots stay as posed.
Like loading a scenario, only the computer KYNEX runs on can undo.

**Several at once.** Ctrl- or Shift-click on placed equipment, in the
scene or in the library's list, adds it to the selection or takes it out;
the inspector says how many are selected. The handle then moves all of
them, and turns them about the one it is on; Delete removes them all;
Ctrl+D copies them half a metre aside and selects the copies; Ctrl+C and
Ctrl+V copy and paste them, each paste a step further along. A copy
takes its original's size, turn and cell settings (cycle time, door,
grid), not its PLC wiring or the real machine it follows. Each of these
is one edit to undo. A new item takes the first number its kind's names
do not have, so no two names are alike.

**Placing exactly.** The inspector's **스냅** sets the handle's steps: a
grid of 10, 50, 100 or 500 mm to move on (a click that places equipment
lands on it too) and 5, 15, 45 or 90° to turn by; the choice is kept in
the browser. **X축** and **Y축** tilt equipment, before its turn about
Z: a robot on a wall at 90°, one hung under a gantry at 180°, which
still reaches as it should. Tilted equipment does not snap onto
connection points. With several selected, **정렬** lines them up by
their boxes: to a side or the middle of the one selected, its floor
height, one against the next along X or Y, or evenly spaced between the
outermost two.

**More of one.** The inspector's name is equipment's name, to change:
programs find it by name, its cell role takes it, and no other item may
have it. 🔓 locks the equipment selected (🔒 in the list): it is not
moved, sized or removed until unlocked, and stays locked in the project.
◉ hides it on this page alone (◌ in the list), until shown again.
**배열·대칭 복사** copies the equipment selected along a line (count,
spacing in X and Y), round a centre (count; each copy turned with it), or
mirrored beside itself across the far side of its box in X or Y: places
and headings mirrored, its shape not. Each is one edit. The measuring
tool (M) takes a third point for the angle at the second.

**Program steps.** A step clicked, and another Shift-clicked, picks the
steps between. The bar under the add buttons then copies them (⧉,
Ctrl+C while the program was last clicked in) and pastes what was copied
after the step picked (⎘, Ctrl+V), into this robot's program or another
robot's of as many joints: by its joints' names, else in their order.
A robot of other joints refuses them. With several picked, **J**, **L**
and **R** set their MoveJ speed (%), MoveL and MoveC speed (mm/s) or
blend radius (mm) at once. Steps are dragged in the list to another
place, the span picked with them if the step dragged is in it. Each is
one edit to undo.

**Projects.** The folder button at the left of the toolbar shows the
project open; it opens the project panel. **새로 만들기** makes an empty
project under the name typed and opens it. **다른 이름으로 저장** saves
the project as it is under that name, and saves to it from then on. The
list below holds the projects in the projects folder, the last changed
first; one clicked is opened. That folder is `Documents/KYNEX` in the
user's home (`KYNEX_PROJECTS` sets another). A project is a file there,
`<name>.json`; names are only names, never paths. Before another
project opens, the one open is saved; its programs stop, its PLC link
and real machines followed are let go, and the viewers are shown the
new one whole. The scene's geometry still comes from Kreate, or from
`--file`; the project holds what KYNEX adds to it. As with the PLC, only
the computer KYNEX runs on can make, open or save projects.

**URScript** downloads the program as a `.script` file for a Universal
Robots controller, **RAPID** as a `.mod` module for an ABB one, **DRL**
as a `.drl` program for a Doosan Robotics one, and **TP** as an `.ls` file for a FANUC one. Only the button for the
robot's own controller shows, when its description tells whose it is. The program is first made into one any vendor's
controller can be given (`kynex_kin::post`: its moves, and between them
outputs set or pulsed, inputs waited for, pauses, a pallet's count moved
on), then written by the vendor's writer (`kynex_kin::urscript` for
Universal Robots, `kynex_kin::rapid` for ABB). The exports of three
programs, and of stage 1's cell as RAPID, are kept as they are
(`apps/kynex/src/run/golden`): a writer that writes them otherwise fails
the tests. The program must be for a robot
with six revolute joints. Actions go as script once the cell is wired to
the controller: under **I/O 연결** in the inspector of a gripper, a
machine or a conveyor, give each of its outputs and inputs a number (0–7
the standard digital I/O, 8–15 the configurable, 16–17 the tool's; for
ABB, signals `DO_KYNEX_n` and `DI_KYNEX_n`, listed at the top of the
module, to be set up on the controller). In RAPID, joint moves go as
`MoveAbsJ` with the taught joints, lines and arcs as `MoveL`/`MoveC` to
targets in the base frame (`wobj0`), blends as `zonedata`, pallet points
by `Offs`. Names and comments are kept to ASCII; other letters become
`?` or `_`.

- A grip sets the gripper's output (release clears it), then waits for
  its "closed" input, or its open/close time if it has none.
- A command to a machine pulses its output for 0.2 s.
- A wait holds until its input is on (`while (not get_…_digital_in(n)):
  sync()`).
- A pause is `sleep`. Points on a pallet go to the place the script
  counts to: the pallet's places are a list at the top, **팔레트 다음 칸**
  counts on, and the point's pose is moved by `pose_add`.
- An action not wired, and an AMR's drive, is written as a comment where
  it falls (`# KYNEX, not exported until its I/O is mapped: …`), to be
  completed on the controller.

- On URSim (free, in Docker) the program runs as exported, and its moves
  take what KYNEX plans to within a few percent (`tools/ursim`):

  ```
  docker run -d --rm --name kynex-ursim -e ROBOT_MODEL=UR10 \
      -p 30001-30004:30001-30004 -p 29999:29999 universalrobots/ursim_e-series
  cargo test -p kynex ursim_program -- --ignored
  python tools/ursim/check.py
  ```

  The e-Series image takes `UR10`, not `UR10E`; given a name it does not
  know, it runs a UR5e. `cargo test -p kynex ursim_wired_program --
  --ignored` and `check.py --io` run the program wired (its gripper on DO
  0, the machine's commands on DO 2–6) twice round: the tool ends every
  move where KYNEX says, the second time at the pallets' next places, and
  the gripper's output goes on and off. Before its first program the robot is powered on
  and its brakes released over the dashboard server.

  Each move's number goes on register 0 as it starts and on register 1
  once it is done, so a move is timed alone: a gripper's time or a pulse
  to a machine after it is not counted as moving.
- **Stage 1's cell** (two CNC machines, a double gripper; the test
  `stage_ones_cell_runs_half_an_hour_with_both_machines_busy`): the main
  program loads both machines, then calls the swap at each — the finished
  part out with grip_b, the raw part in with grip_a — over and over.
  Its paths touch nothing and come within 20 mm of nothing. Half an hour
  of it runs without an alarm: each machine cuts about 80 % of the time,
  60 parts an hour. On URSim (`cargo test -p kynex ursim_stage_one --
  --ignored`, then `check.py --name stage1` and `--io --name stage1`) its
  81 moves take 3.3 % longer than KYNEX plans, and every one ends where
  KYNEX says.
- **On Doosan's emulator** (free, in Docker; the same cell on an M1013,
  on a 1.2 m pedestal for its shorter reach to a tool held down):

  ```
  docker run -d --rm --privileged --name kynex_emulator -e ROBOT_MODEL=M1013       -p 12345:12345 doosanrobot/dsr_emulator:3.0.1
  docker build -t kynex-run-drl tools/doosan
  cargo test -p kynex doosan_stage_one -- --ignored
  python tools/doosan/check.py
  ```

  `tools/doosan/run_drl` (built with Doosan's DRFL) registers the TCP and
  starts the program; what it does is read from the emulator's log. Its
  81 moves take 7.3 % longer than KYNEX plans — the controller takes about
  0.1 s more over each move, short ones the most — and each ends within
  1 mm of where KYNEX says.
- DRL: joint moves are `movej` to the taught joints, each joint at its
  share of its own top speed; lines and arcs `movel`/`movec` to poses in
  `DR_BASE` (mm, ZYZ degrees); a joint move to a pallet place `movejx` in
  the taught posture (`get_solution_space`). DRL cannot make a TCP: the
  program names `KYNEX` with `set_tcp`, and gives its values at the top,
  to register on the teach pendant. I/O 0–15 are the controller's 1–16,
  16–17 the tool's 1–2.
- **TP** (FANUC, `kynex_kin::fanuc`) downloads an `.ls` file, the TP
  program's text form, for a controller with the ASCII program loader or
  to be made into a TP file with FANUC's tools. Positions are in joint
  form, exactly as taught, with J3 written from level as FANUC counts it
  (the description's joint 3 less joint 2, the J2/J3 interaction); `J` at
  a share of top speed, `L`/`C` in mm/sec; a blend is `CNT` with its
  radius in mm as the value (CNT is no distance on a FANUC, so the corner
  is only near it). The tool is user tool 1, set at the start from
  `PR[49]`; positions are in the world frame (user frame 0, the `base`
  link at J2's height). I/O 0–15 are `DO`/`DI[1–16]`, 16–17 the wrist's
  `RO`/`RI[1–2]`. Pallets: places in `PR[51]`–`PR[100]`, counts in
  `R[81]` on, `R[90]` and `PR[50]` scratch; a move on a pallet goes in
  Cartesian form with its `CONFIG` worked out from the taught joints, by
  `Offset,PR[50]`. Markers in `R[98]`/`R[99]`. A robot with a track is
  refused; a positioner's turn is a remark. No free simulator runs TP, so
  it is checked against a kept file (`crates/kynex-kin/src/fanuc/golden.ls`)
  and has not run on a controller.
- MoveJ becomes `movej` with the taught joint positions.
- MoveL becomes `movel` to the taught tool pose, in the controller's base
  frame (the `base` link, half a turn from `base_link`).
- The tool point is set with `set_tcp`, measured from `tool0`.
- The robot first goes to the first point with `movej`. With 순환 재생
  on, the rest runs in a `while (True):` loop.
- The controller times moves in its own way, so a cycle on the robot is
  close to the planned cycle, but not equal to it.

### The library: placing equipment

The **라이브러리** tab, beside the scene tree, lists equipment to place as
thumbnails: each drawn from the piece at its usual size
(`apps/kynex/src/library.rs`):

- 로봇: UR10e + 이중 그리퍼, UR5e, UR3e, UR16e (UR3e·UR16e·UR5e는 Universal
  Robots의 `Universal_Robots_ROS2_Description` 기구학·관절 한계 값),
  ABB IRB 1600-10/1.45 + 이중 그리퍼, ABB IRB 1600 + 주행 트랙,
  Doosan M1013 + 이중 그리퍼(두산 `dsr_description2`의 관절·범위·속도.
  에뮬레이터에서 영점 플랜지 `[0, 34.5, 1452.5]`와 대기 자세가 같음을 확인),
  FANUC LR Mate 200iD/7L + 이중 그리퍼(ROS-Industrial
  `fanuc_lrmate200id_support`의 관절·범위·속도, 911 mm 도달. 영점에서
  면판이 월드 좌표계(J2 높이)로부터 `[550, 0, 475]` mm임을 시험으로 확인).
  IRB 1600은 ROS-Industrial `abb_irb1600_support`의 관절 원점과 축,
  ABB 제품 사양서의 관절 범위와 최고 속도를 따릅니다. 관절값이 컨트롤러와
  같은 방향을 가리키고 `tool0`이 컨트롤러의 플랜지라서, 교정 자세의
  플랜지가 ABB와 같은 `[[815, 0, 1186.5], [0.707107, 0, 0.707107, 0]]`로
  나옵니다(시험으로 확인).
- 툴: every robot carries the tool picked under **툴** in its inspector,
  on its flange (`tool0`): 없음, 2지 그리퍼 (one grip point), 이중 그리퍼
  (two), 진공 그리퍼 (a plate of four suction cups, one grip point),
  디버링 스핀들 (its burr's tip, the `tcp` link, 250 mm out on the
  flange's axis), 용접 토치 (the gooseneck bent 22°, the wire's tip the
  `tcp` link). Grip points come as grippers; a process tool's `tcp` is the
  robot's TCP. The tending robots carry the dual gripper unless told,
  the bare URs nothing. Changing a tool drops the roles of grip points it
  no longer has.
- 공작기계: CNC 선반, CNC 머시닝센터 (수직형).
- 성형기: 사출 성형기 (a 150 t class machine: clamp and mould behind a
  guard, injection unit with its hopper; length, width, height).
- 주변 장치: 투입 / 배출 팔레트 테이블, 로봇 받침대.
- 물류: 벨트 컨베이어 (length, belt width, height), AMR (a mobile base
  with a top plate to carry a load; length, width, height), AMR 정지 위치
  (a frame its size on the floor, an arrow the way it faces).
- 안전: 안전 펜스.

The conveyor, the injection moulding machine, the AMR and its stops come
with their roles and run (see below).

Drag an entry onto the scene, or click it and then click the scene. It
lands on the floor, or on top of whatever is under the pointer (a robot
on its pedestal). A CNC machine is turned so its door faces the origin.
Select a placed item, here or in the scene, and its gizmo is on it at
once: drag an arrow to move it along that axis, a square to move it in
that plane; in **회전** (or `R`; `T` or **이동** back) drag the red, green
or blue ring to turn it about X, Y or Z — its X and Y tilt and its turn,
in the angle snap's steps. Several selected together turn about Z only;
Alt held as it is let go keeps it from snapping onto a connection point.
The inspector sets its position, turn and sizes by number too, and
**기즈모 끄기** turns the gizmo off (kept for the browser). Locked
equipment has none. Sizes change the item's shape, and the item keeps its
identities when they do.

Equipment comes with its roles. A CNC machine is a CNC to the cell, a
pallet table's plate is a pallet, and a tending robot's grip points are
grippers. Placed equipment is KYNEX's own: it is added to the scene KYNEX
shows, under identities Kreate never issues (`0xFFFE << 48` on), and kept
in the project file. With no Kreate session, as KYNEX runs by default,
the cell is laid out from the library alone.

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
  - 사출 성형기: the object whose children include its guard (`…door…`)
    and its mould (`…mould…`).
  - 컨베이어: a belt, a box whose top carries parts along its own X.
  - AMR: a mobile robot, with its top plate (`…top…`) below it.
  - AMR 정지 위치: where an AMR stops, facing the stop's X.
  - 포지셔너: a turntable, turned about its own vertical axis, with its
    top plate (`…top…`).

  Settings start at industry-typical values (docs/roadmap-vs-visual-components.md
  §4): 90 s machining, 3 s door, 1 s chuck, 5 × 4 places 100 mm apart, and
  0.5 s grip. **셀 자동 구성** sets roles from names. A CNC machine is the
  lowest object with a `…door…` and a `…chuck…` below it, an injection
  moulding machine one with a `…door…` and a `…mould…`, and a conveyor a
  mesh named `belt` or `…conveyor…`. Pallets are
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
- **Injection moulding machines run** much as CNC machines do, but make
  their parts themselves. **성형 시작** runs a cycle (30 s; 1.5 s guard)
  with the guard shut and the mould empty. At its end a moulded part
  stands on the mould, for the robot to take out: open the guard, wait for
  it, grip, close, start again. The program's rows offer a moulding
  machine's own commands and signals (가드 열기/닫기, 성형 시작, 성형 완료).
- **Conveyors carry**: a part on a belt goes along it at the belt's speed
  (0.2 m/s), up to its end or the part ahead. A part let go over a belt
  stands on it. Its settings:
  - 소재 공급 간격: every so many seconds a raw part is put on at its
    start, if there is room (0: none).
  - 끝에서 치움: a part reaching the end is taken away and counted, as at
    an outfeed (1: yes).

  A program waits for a part with the conveyor's signal 부품 도착, its
  sensor at the end. A scenario comparison counts the parts taken away at
  a conveyor's end as delivered.
- **AMRs drive** to stops: they turn to face the stop, drive there
  straight (1 m/s), and turn as the stop faces (1 rad/s). In the
  inspector of an AMR, a button for each stop sends it there. In a
  program, **AMR 주행** sends it, and a wait for **AMR 도착** holds the
  robot until it is there. A part let go over an AMR's top plate stands on
  it and rides along. The timeline shows each AMR's driving.
- **Positioners turn** (90°/s unless set otherwise): in the inspector, →
  0° and → 180° turn one; in a program, **포지셔너 회전** turns it to an
  angle from where it was placed, and the robot waits until it is there.
  What stands on its plate turns with it. For ABB it is the external axis
  after the robot's own, turned with `MoveExtJ` (a mechanical unit to set
  up on the controller); a UR or a Doosan has no such axis, and the
  program says where it is to be turned, as a comment.
- **A robot on a track** (ABB IRB 1600 + 주행 트랙: 3 m of travel, 1.6
  m/s): the track is its first joint, jogged and solved with the arm. On
  a line or an arc the track goes evenly from end to end, as the
  controller moves it, and the arm keeps the tool on the line. Its RAPID
  gives the track as external axis a, in millimetres, in every target's
  `extax`, and its targets in the frame the track stands in. URScript and
  DRL refuse a robot with an axis outside the arm.
- **Parts go with their equipment**: move a pallet table, a machine or a
  conveyor in the library and the parts on it go with it; remove it and
  they go too.
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
- **Clearance**: a move that touches nothing but brings the robot within
  20 mm of the rest of the cell shows how near, under it, for example
  `근접: forearm_link ↔ door 12 mm (0.40 s)`: a margin for the cell as
  built against the cell as drawn. The distance is exact on the
  triangles.
  - Around where a gripper works, at a point a grip or a release follows
    and leaving one after, nearness is the work: within 0.2 m of the grip
    point it is not warned of. A tool going into a machine's door is
    still checked away from the chuck.
  - A robot's base, which rides on no joint, is not checked: it stands on
    its pedestal.
  - The check runs across the cores: some 30 ms for the M1 program.
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

## PLC (virtual commissioning)

The cell's machines can be run by a PLC's program instead of the robot's,
to try the PLC's logic before the machines exist (`apps/kynex/src/plc.rs`).
KYNEX is a client of the PLC, as Visual Components is, over **Modbus TCP**
or **OPC UA**:

- **PLC** in the toolbar: Modbus TCP or OPC UA, the PLC's address and
  port (502 or 4840 by default), Modbus's unit or the OPC UA endpoint's
  path (`/freeopcua/server/`), and **연결**. Where the PLC is is kept in
  the project file, but opening a project never connects: someone presses
  연결 each time. The dot on the button is green while KYNEX exchanges
  with the PLC, every 50 ms; addresses the PLC would not read or write
  are listed beside it, the link going on with the rest. The link and the
  addresses are changed only from the computer KYNEX runs on: a viewer
  shared with `--lan` refuses them from elsewhere, and with them program
  steps on PLC signals and taking a scenario back. While linked to a PLC,
  another computer only watches: everything it asks to change is refused,
  as anything that changes the cell can change what reaches the PLC.
- **PLC 주소** in a machine's inspector, for each command and each state:
  over Modbus a coil's number (from 0, as Modbus addresses them), over
  OPC UA a Boolean variable's node id (`ns=2;s=Lathe.Open`, `ns=3;i=1001`;
  a number read as a Boolean, on when it is not 0). A command — 문 열기,
  척 물림, 시작 … — is given as the PLC turns it on; a state — 문 열림, 척
  물림됨, 척 풀림됨, 완료 … — is written as it changes. Machines may share
  a command's address (each takes it); a state's address shared by two is
  on only when both say so, and is warned of. A command the machine
  cannot take (a door while machining) is shown beside the link's state.
- OPC UA: KYNEX's own client (`apps/kynex/src/plc/opcua.rs`). **보안**:
  none (the `None` policy), or Basic256Sha256 signed, or signed and
  encrypted (`plc/secure.rs`, on the RustCrypto crates: RSA-OAEP and RSA
  PKCS #1 v1.5 for opening the channel, AES-256-CBC and HMAC-SHA256 for its
  messages, P_SHA256 between). KYNEX makes its own application
  certificate the first time it needs one (RSA-2048, application URI
  `urn:kynex:client`) and keeps it with its key in `%APPDATA%/kynex/pki`;
  the panel shows its thumbprint, for the PLC's administrator to trust.
  The server's certificate is trusted on first use: its thumbprint is
  kept in the project, a server with another refused, and **다시 믿기**
  forgets it. **사용자** and **비밀번호** log in as a user (anonymous when
  empty); the password, encrypted for the server as its policy says, is
  held in memory only, never saved, and kept only for the same server and
  user. A server that would take it unencrypted is refused unless the
  channel itself is encrypted. On a kept channel, the server's session
  signature is checked, and every answer must be this channel's, newer
  than the last and for the request sent: none is taken twice.
- **신호** in the PLC panel: signals besides the machines', each a name,
  a direction (PLC → KYNEX, or KYNEX → PLC), a bit or a number, an
  address (over Modbus a bit is a coil, a number a holding register,
  signed 16-bit; over OPC UA a number is written in the variable's own
  type, read once to learn it), and what it is tied to. PLC → KYNEX: a
  robot's program started, or stopped, as it comes on; or nothing, for
  programs to wait on. KYNEX → PLC: on while a robot's program runs, on
  once it has run to its end, a machine's count of cycles; or nothing,
  for programs to set. The table is applied whole (**적용**), kept with
  the PLC's settings in the project file, and shows each signal's value.
- Robot programs: **+ 동작 → PLC 신호 쓰기** sets a signal of the cell's
  to a value; **PLC 신호 대기** holds the robot until a signal of the
  PLC's reads a value. A signal no longer in the table stops the run with
  an alarm. Exported to a robot controller, they are comments: the PLC's
  signals are the cell's, to be wired to the controller's own I/O there.
- Robot programs run side by side, each on its own: started from the page
  or by the PLC, a program goes on beside those running.
- The chuck closes and opens over the machine's **척 개폐** time: **척
  물림됨** comes on once it has closed, **척 풀림됨** once it is open
  again, and a cycle starts only clamped.

Checked against servers that are not KYNEX's own, free ones, with a small
PLC program in `tools/plc/check.py` running a lathe's door and chuck over
and over: pymodbus for Modbus, asyncua for OPC UA. asyncua 1.1.8 does not
work on Python 3.14 (not even with itself), so it runs on Python 3.8 in a
virtual environment of its own:

```
pip install pymodbus
cargo run -p kynex -- view --empty --port 8097 --project target/plc/kynex-project.json
python tools/plc/check.py --cycles 3

py -3.8 -m venv target/plc/venv38
target/plc/venv38/Scripts/python -m pip install --upgrade pip
target/plc/venv38/Scripts/python -m pip install --only-binary=:all: asyncua
cargo run -p kynex -- view --empty --port 8098 --project target/plc/kynex-project-ua.json
target/plc/venv38/Scripts/python tools/plc/check.py --protocol opcua --kynex http://127.0.0.1:8098 --cycles 3
target/plc/venv38/Scripts/python tools/plc/check.py --protocol opcua --kynex http://127.0.0.1:8098 --security encrypt --user kynex --password secret
```

Over a Basic256Sha256 channel, signed and encrypted, logged in as a user,
the same program ran two cycles within 0.125 s of the lathe's times;
signed only, within 0.11 s. With the server's certificate replaced KYNEX
refused it, saying both thumbprints; with a wrong password the server's
`BadUserAccessDenied` showed in the panel.

Every step took the lathe's own time to within 0.13 s over either (3 s
doors, 1 s chuck), the rest being the 50 ms exchange and the program's
scan; also with a second lathe wired to the same nodes.

A PLC starting a robot: with a UR5e's program (MoveJ, **PLC 신호 쓰기**
준비 = 1, MoveL) and the table tying coil 10 to its start, coil 110 to it
running and coil 111 to 준비, `tools/plc/robot_check.py` on pymodbus
turned coil 10 on: the program ran 0.05 s later, set 준비 at 0.42 s, and
was done at 1.05 s.

## Machine tools (MTConnect, umati)

A machine of the cell can follow a real machine tool on the shop floor
(`apps/kynex/src/mtconnect.rs`): **실제 장비 따라가기** in a CNC machine's or
moulding machine's inspector takes the machine's MTConnect agent
(`http://host:5000`, plain HTTP as shop-floor agents serve it) and its
device's name (empty: the agent's first). KYNEX asks the agent for the
device's current state about once a second and shows it beside the box;
the machine then is the real one: its door open or shut as the real
door is (`DoorState`), cutting while it is `ACTIVE` (`Execution`) — its
own cycle not run — and its cycles the parts made since it was first
read (`PartCount`, counted again after a reset). It takes no commands
while it follows. Following is kept in the project file; it is set only
from the computer KYNEX runs on.

Checked two ways, as no reference agent ran here (Docker was off):

- The MTConnect Institute's public demo agent, live
  (`http://demo.mtconnect.org:5000`): its Mazak, cutting, followed in the
  viewer ("Mazak · ACTIVE · 문 CLOSED · 생산 0 · 101010"); its whole answer
  kept as a test's fixture (`mtconnect/demo_current.xml`), read for both
  its devices.
- `tools/mtconnect/check.py`: a small agent answering in the same form,
  its machine opening its door for 2 s, shutting it, cutting for 4 s and
  making a part, over and over; KYNEX's lathe followed each change, its
  cycles counting the parts.

The same box follows a machine over **umati**, OPC UA for Machine Tools
(OPC 40501-1, on Machinery, OPC 40001-1; `apps/kynex/src/umati.rs`): its
way set to umati, it takes the machine's OPC UA server
(`opc.tcp://host:4840/path`), the node id of the machine's own object
(`ns=3;s=Machine1`) and the server's security and user, as the PLC
connection takes them (same client, same certificate, the server's
certificate trusted on first meeting and refused if it changes, the
password never saved). KYNEX finds the machine's parts by the
specifications' browse names from that node and reads them about once a
second:

- `Monitoring/MachineTool/MachineryItemState/CurrentState`: `Executing`
  is cutting; `NotAvailable` and `OutOfService` are the machine away.
- `Production/ActiveProgram/State/CurrentState`: `Running` is cutting
  too, and each time it comes to `Ended` a part is counted.
- `Production/ActiveProgram/Name`: the program.

The specifications name no door, so a machine followed over umati keeps
its door as it is.

Checked with `tools/umati/check.py`, on asyncua (1.1.5 on Python 3.8, as
for the PLC): an OPC UA server loading the OPC Foundation's own nodesets
(DI, IA, Machinery, ISA95 Job Control, MachineTool, from
OPCFoundation/UA-Nodeset) and making one machine of `MachineToolType`,
idle, then running its program for 4 s, then the program ended, over and
over. KYNEX's lathe, set to follow it through the viewer's request, cut
while it ran and counted every ended program as a part ("ACTIVE · 생산 3
· O1234").

```
kynex view --empty --port 8101 --project target/umati/kynex-project.json
target/plc/venv38/Scripts/python tools/umati/check.py
```

## Installing KYNEX

```
powershell -ExecutionPolicy Bypass -File tools\package.ps1
```

builds `target\dist\KYNEX-<version>-setup.exe` and
`target\dist\KYNEX-<version>.msi` from a release build. Both install two
programs:

- **`kynex-studio.exe`**, what the Start menu's (and the desktop's) KYNEX
  starts: KYNEX on its own in a window of its own, the system's WebView2
  (part of Windows 10 and 11), no console and no browser. It serves the
  viewer on 8090, or the next free port up to 8099, with the project last
  opened. Closing the window ends KYNEX; the project is saved as it goes.
  WebView2's own data is kept in `%APPDATA%\kynex\webview`. If KYNEX cannot
  start (its project open in another KYNEX, say), the window says why.
- **`kynex.exe`**, the command line, as before (`kynex view`, `compare`,
  `link`, `inspect`, `bench`).

The setup.exe (Inno Setup 6, Korean or English) installs for the user alone
without administrator rights, or for all users if chosen; it adds a Start
menu entry, a desktop one if asked, and starts KYNEX at the end. The MSI
(WiX 5) installs for all users, as IT deploys it, and replaces an older
version; it shows no wizard pages, as the standard ones need a licence to
accept and KYNEX has none yet. Removing either leaves the user's projects
(`Documents\KYNEX`) and settings (`%APPDATA%\kynex`). Both programs carry
KYNEX's icon and version (`apps/kynex/assets/kynex.rc`, built in by
`build.rs` with the Windows SDK's `rc.exe`).

The tools are free: Inno Setup (`winget install JRSoftware.InnoSetup`) and
WiX 5 (`dotnet tool install --global wix --version 5.0.2`; WiX 6 and later
come with a maintenance-fee licence). The installers are not code-signed:
Windows SmartScreen warns about an unsigned setup.exe until a code-signing
certificate signs it.

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
