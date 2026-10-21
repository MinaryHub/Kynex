// KYNEX viewer: the scene KYNEX holds, drawn with three.js and kept live by
// the /events stream. Every object arrives with its world matrix (metres,
// Z up), so the scene here is flat: nothing is parented, nothing is
// recomputed, a pose is just new matrices.

import * as THREE from 'three';
import { OrbitControls } from '/vendor/OrbitControls.js';
import { Batch } from '/batch.js';
import { Outline } from '/outline.js';
import { RobotPanel } from '/robot.js';
import { CellPanel } from '/cell.js';
import { PlcPanel } from '/plc.js';
import { ProjectPanel } from '/project.js';
import { FloorPanel } from '/floor.js';
import { LibraryPanel } from '/library.js';
import { TimelinePanel } from '/timeline.js';
import { openReport } from '/report.js';
import { showMenu } from '/menu.js';
import { mountHelp } from '/help.js';

// ---- state ------------------------------------------------------------------

const records = new Map();    // uid -> object record from KYNEX
let robots = [];
const jointKinds = new Map(); // joint uid -> kind
let selected = null;
// Placed equipment selected along with it (Ctrl or Shift and a click), by
// root: moved, copied and removed together.
let group = new Set();
let framedOnce = false;
let lastUpdate = 0;
// Benchmark mode (`/?bench`): `kynex bench --browser` opens the page so,
// and the page measures itself and posts the numbers back.
const BENCH = new URLSearchParams(location.search).has('bench');
let benchStarted = false;
let benchUpdates = 0;
const collapsed = new Set();
const view = { joints: false, wire: false, grid: true, labels: true, timeline: false, measure: false, interact: false };

// ---- renderer, camera, lights ----------------------------------------------

const canvas = document.getElementById('canvas');
const host = document.getElementById('viewport');
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.NeutralToneMapping;
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;
renderer.setClearColor(0x1a1d22);
renderer.shadowMap.autoUpdate = false;
renderer.info.autoReset = false;

THREE.Object3D.DEFAULT_UP.set(0, 0, 1);
const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(40, 1, 0.01, 2000);
camera.up.set(0, 0, 1);
camera.position.set(2.4, -3.2, 2.2);

const controls = new OrbitControls(camera, canvas);
controls.enableDamping = true;
// Brisk: the view stops soon after the pointer does, rather than gliding on.
controls.dampingFactor = 0.25;
controls.zoomSpeed = 1.6;
controls.panSpeed = 1.3;
controls.screenSpacePanning = true;
controls.target.set(0, 0, 0.4);
// The wheel: toward what is under the pointer — the equipment there, or
// the floor —, the view and what it turns about drawn in to it together,
// so that what the view turns and pans about is what is looked at, and
// they go as far as it is near. Never nearer to it than this, metres (set
// by the scene's size in fitShadows); never through it.
controls.minDistance = 0.3;
const floorPlane = new THREE.Plane(new THREE.Vector3(0, 0, 1), 0);
canvas.addEventListener(
  'wheel',
  (e) => {
    // The orbit's own zoom does not see it.
    e.preventDefault();
    e.stopImmediatePropagation();
    const r = canvas.getBoundingClientRect();
    const toward = new THREE.Raycaster();
    toward.setFromCamera(new THREE.Vector2(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1), camera);
    // The nearer of what is there and the floor; else as far as the view
    // is from what it turns about.
    const hit = batch.mesh && toward.intersectObject(batch.mesh, false)[0];
    const floor = toward.ray.intersectPlane(floorPlane, new THREE.Vector3());
    const candidates = [hit?.point, floor].filter((p) => p && p.distanceTo(camera.position) > 1e-6);
    candidates.sort((a, b) => a.distanceTo(camera.position) - b.distanceTo(camera.position));
    const at = candidates[0] ?? toward.ray.at(camera.position.distanceTo(controls.target), new THREE.Vector3());
    // A notch in, an eighth nearer; out, as much further.
    const notches = THREE.MathUtils.clamp(e.deltaY / 100, -3, 3);
    let scale = Math.pow(1.14, notches);
    const far = camera.position.distanceTo(at);
    if (far * scale < controls.minDistance) scale = controls.minDistance / far;
    if (far * scale > camera.far * 0.6) scale = (camera.far * 0.6) / far;
    if (Math.abs(scale - 1) < 1e-4) return;
    camera.position.sub(at).multiplyScalar(scale).add(at);
    controls.target.sub(at).multiplyScalar(scale).add(at);
  },
  { capture: true, passive: false },
);

const hemi = new THREE.HemisphereLight(0xe8eef7, 0x30343b, 1.6);
hemi.position.set(0, 0, 1);
scene.add(hemi);
const sun = new THREE.DirectionalLight(0xffffff, 2.2);
sun.position.set(3, -4, 6);
sun.castShadow = true;
sun.shadow.mapSize.set(2048, 2048);
sun.shadow.bias = -0.0004;
sun.shadow.normalBias = 0.02;
scene.add(sun, sun.target);

// Ground: a shadow catcher, and a grid in metres.
const ground = new THREE.Mesh(
  new THREE.PlaneGeometry(4000, 4000),
  new THREE.ShadowMaterial({ color: 0x000000, opacity: 0.28 }),
);
ground.receiveShadow = true;
// It darkens what is behind it and is not a surface of its own: no depth, so
// the composite leaves the background as chosen and finds no edge at the
// horizon.
ground.material.depthWrite = false;
ground.position.z = -0.0005;
scene.add(ground);

// The grid: the whole floor to the horizon, one grid, its lines as far
// apart as the view needs where they are — tenths of a metre close up,
// metres, tens, hundreds further off —, the finer fading out as they grow
// too close to see; every tenth line a little stronger. The X axis red and the
// Y axis green from the origin.
const grid = new THREE.Mesh(
  new THREE.PlaneGeometry(4000, 4000),
  new THREE.ShaderMaterial({
    transparent: true,
    depthWrite: false,
    uniforms: {
      uMajor: { value: new THREE.Color(0x5a626e) },
      uCamera: { value: new THREE.Vector3() },
      uFar: { value: camera.far },
    },
    vertexShader: `
      varying vec3 vWorld;
      void main() {
        vec4 w = modelMatrix * vec4(position, 1.0);
        vWorld = w.xyz;
        gl_Position = projectionMatrix * viewMatrix * w;
      }`,
    fragmentShader: `
      uniform vec3 uMajor;
      uniform vec3 uCamera;
      uniform float uFar;
      varying vec3 vWorld;
      // How much this point is on a line of a grid \`step\` apart: 1 on it,
      // 0 a pixel or more off.
      float lines(vec2 p, float step) {
        vec2 c = p / step;
        vec2 d = fwidth(c);
        vec2 g = abs(fract(c - 0.5) - 0.5) / max(d, vec2(1e-6));
        return 1.0 - min(min(g.x, g.y), 1.0);
      }
      void main() {
        vec2 p = vWorld.xy;
        vec2 fw = fwidth(p);
        float px = max(fw.x, fw.y);
        // The finest step at least fifteen pixels apart here, and how far
        // it is on to the next: it fades out as it nears it.
        float lg = log(max(px * 15.0, 1e-4)) / log(10.0);
        // Tenths of a metre at the finest: closer still, they stay as they
        // are; else they fade as they close up, as every step does.
        float level = floor(lg);
        float s0 = pow(10.0, max(level, -1.0));
        float t = level >= -1.0 ? fract(lg) : 0.0;
        // One grid: its lines, every tenth a little stronger; the finer
        // ones fading out as they close up, the tenths becoming the lines.
        float a0 = lines(p, s0) * 0.32 * (1.0 - t) * (1.0 - t);
        float a1 = lines(p, s0 * 10.0) * (0.32 + 0.18 * (1.0 - t));
        float a = max(a0, a1);
        vec3 colour = uMajor;
        // The axes, from the origin out.
        float ax = p.x >= 0.0 ? 1.0 - min(abs(p.y) / (fw.y * 1.5), 1.0) : 0.0;
        float ay = p.y >= 0.0 ? 1.0 - min(abs(p.x) / (fw.x * 1.5), 1.0) : 0.0;
        if (ax > 0.0 && ax >= ay) { colour = mix(colour, vec3(0.85, 0.33, 0.31), ax); a = max(a, ax * 0.9); }
        if (ay > 0.0 && ay > ax) { colour = mix(colour, vec3(0.36, 0.72, 0.36), ay); a = max(a, ay * 0.9); }
        // Fading out towards the horizon, so it has no edge.
        a *= 1.0 - smoothstep(uFar * 0.35, uFar * 0.9, distance(vWorld, uCamera));
        if (a < 0.01) discard;
        gl_FragColor = vec4(colour, a);
      }`,
  }),
);
grid.renderOrder = -1;
grid.frustumCulled = false;
scene.add(grid);

const content = new THREE.Group();
scene.add(content);

// ---- materials --------------------------------------------------------------

const DEFAULT_COLOR = new THREE.Color().setRGB(0.62, 0.64, 0.67, THREE.LinearSRGBColorSpace);
const ACCENT = new THREE.Color(0xff8a3d);
// Parts in contact: a robot's link and what it runs into.
const CONTACT = new THREE.Color(0xff3b3b);
// Equipment a link being drawn would land on.
const LANDING = new THREE.Color(0x3fcf8e);
let contacted = new Set();
// The parts' one material: each part's own colour is its instance colour.
const surface = new THREE.MeshStandardMaterial({
  color: 0xffffff,
  roughness: 0.58,
  metalness: 0.08,
  side: THREE.DoubleSide,
  flatShading: true,
});

const batch = new Batch(surface);
const outline = new Outline();

function colorOf(rec) {
  return rec.color
    ? new THREE.Color().setRGB(rec.color[0], rec.color[1], rec.color[2], THREE.LinearSRGBColorSpace)
    : DEFAULT_COLOR.clone();
}

// ---- geometry ---------------------------------------------------------------

function bytes(b64) {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out.buffer;
}

// A mesh's levels of detail, finest first: one geometry each, all over the
// same vertices.
function levelsOf(positions, indexLists) {
  const position = new THREE.BufferAttribute(positions, 3);
  return indexLists.map((indices) => {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', position);
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));
    return geometry;
  });
}

// A mesh as an update carries it: base64 inside the JSON.
function buildGeometry(mesh) {
  const lists = [mesh.indices, ...(mesh.lods || [])].map((b) => new Uint32Array(bytes(b)));
  return levelsOf(new Float32Array(bytes(mesh.positions)), lists);
}

// ---- gizmos -----------------------------------------------------------------

// Joints and links are few — a robot's dozen, not a factory's thousands — so
// each keeps an object of its own, drawn over the parts.
const gizmos = new Map(); // object uid -> THREE.Group

function makeJointGizmo(kind) {
  const g = new THREE.Group();
  const mat = new THREE.MeshBasicMaterial({ color: ACCENT, depthTest: false, transparent: true, opacity: 0.95 });
  const shaft = new THREE.Mesh(new THREE.CylinderGeometry(0.0035, 0.0035, 0.16, 8), mat);
  shaft.rotation.x = Math.PI / 2;
  shaft.position.z = 0.08;
  const tip = new THREE.Mesh(new THREE.ConeGeometry(0.012, 0.032, 14), mat);
  tip.rotation.x = Math.PI / 2;
  tip.position.z = 0.17;
  g.add(shaft, tip);
  if (kind !== 'prismatic' && kind !== 'fixed') {
    g.add(new THREE.Mesh(new THREE.TorusGeometry(0.045, 0.004, 8, 48, Math.PI * 1.6), mat));
  }
  return g;
}

function makeLinkGizmo() {
  const a = new THREE.AxesHelper(0.06);
  a.material.depthTest = false;
  a.material.transparent = true;
  a.material.opacity = 0.7;
  return a;
}

const _axis = new THREE.Vector3();

function setGizmo(rec) {
  const want = rec.role === 'joint' ? 'joint:' + jointKinds.get(rec.uid) : rec.role === 'link' ? 'link' : null;
  let node = gizmos.get(rec.uid);
  if (node && node.userData.kind !== want) {
    removeGizmo(rec.uid);
    node = null;
  }
  if (!want) return;
  if (!node) {
    node = new THREE.Group();
    node.matrixAutoUpdate = false;
    node.userData.kind = want;
    node.userData.uid = rec.uid;
    const inner = rec.role === 'joint' ? makeJointGizmo(jointKinds.get(rec.uid)) : makeLinkGizmo();
    inner.traverse((o) => {
      o.renderOrder = 10;
      o.userData.uid = rec.uid;
    });
    node.add(inner);
    gizmos.set(rec.uid, node);
    // Faint or hidden like the others, if a gizmo is out.
    markersLook = 'stale';
    content.add(node);
  }
  node.matrix.fromArray(rec.matrix);
  // The gizmo keeps its size whatever the object's scale.
  const m = node.matrix;
  const sx = _axis.setFromMatrixColumn(m, 0).length() || 1;
  const sy = _axis.setFromMatrixColumn(m, 1).length() || 1;
  const sz = _axis.setFromMatrixColumn(m, 2).length() || 1;
  const grow = selected === rec.uid ? 1.5 : 1;
  node.children[0].scale.set(grow / sx, grow / sy, grow / sz);
  node.children[0].updateMatrix();
  node.matrixWorldNeedsUpdate = true;
  node.visible = view.joints && rec.visible;
}

function removeGizmo(uid) {
  const node = gizmos.get(uid);
  if (!node) return;
  node.removeFromParent();
  node.traverse((o) => {
    o.geometry?.dispose();
    o.material?.dispose();
  });
  gizmos.delete(uid);
}

// ---- objects ----------------------------------------------------------------

function place(rec) {
  // Collision shapes are hidden in Kreate too.
  const visible = rec.visible && rec.role !== 'collision';
  if (rec.kind === 'mesh' && rec.mesh != null && batch.hasGeometry(rec.mesh)) {
    batch.set(rec.uid, rec.mesh, rec.matrix, colorOf(rec), visible);
    const tint = tintOf(rec);
    if (tint) batch.tint(rec.uid, tint);
  } else {
    batch.remove(rec.uid);
  }
  setGizmo(rec);
}

function applyObject(rec) {
  const old = records.get(rec.uid);
  records.set(rec.uid, rec);
  const same = old && old.mesh === rec.mesh && old.kind === rec.kind && old.role === rec.role &&
    old.visible === rec.visible && String(old.color) === String(rec.color);
  if (same && (batch.has(rec.uid) || rec.kind !== 'mesh')) {
    // Moved, nothing else: the common case, sixteen numbers.
    moveShown(rec);
  } else {
    tracks.delete(rec.uid);
    place(rec);
  }
  return !old || old.name !== rec.name || old.parent !== rec.parent || old.visible !== rec.visible;
}

// ---- smooth motion while the simulation runs -------------------------------
// KYNEX tells poses some twenty times a second while a program or the flow
// runs, not evenly: drawn as they come, a robot steps and stutters. Each
// update says when KYNEX made it (`t`, ms); meanwhile every object moved
// keeps its last few poses on that clock, and is drawn as it was a little
// while ago — between the two poses either side of then —, so that it
// moves every frame at the speed it was made to. Not while nothing runs:
// a pose asked for — a drag, a jog — shows as soon as it is told.
const tracks = new Map();
// The page's clock less KYNEX's, as small as it has been: when an update
// made at `t` is due here.
let clockOffset = null;
let toldAt = 0;
let lastMade = null;
let madeGap = 50;
const _gm = new THREE.Matrix4();
const _gp = new THREE.Vector3();
const _gq = new THREE.Quaternion();
const _gs = new THREE.Vector3();
const running = () => !!timelinePanel.flowState?.on || robotPanel.going().length > 0;

// An update has come, made at `t` by KYNEX's clock (or now, if it does not
// say): when it is due on the page's clock.
function told(t) {
  const now = performance.now();
  if (t == null) {
    toldAt = now;
    return;
  }
  // The least delay seen, let creep up slowly so a clock drifting is
  // followed.
  const offset = now - t;
  clockOffset = clockOffset == null ? offset : Math.min(clockOffset + 0.5, offset);
  if (lastMade != null) madeGap = madeGap * 0.85 + Math.min(Math.max(t - lastMade, 5), 400) * 0.15;
  lastMade = t;
  toldAt = t + clockOffset;
}

// How long behind the drawing is kept: time for the next pose to come.
const playDelay = () => Math.min(Math.max(madeGap * 1.8, 40), 600);

function poseOf(matrix) {
  const pose = { p: new THREE.Vector3(), q: new THREE.Quaternion(), s: new THREE.Vector3() };
  _gm.fromArray(matrix).decompose(pose.p, pose.q, pose.s);
  return pose;
}

function moveShown(rec) {
  if (!running() || jointPreview) {
    tracks.delete(rec.uid);
    batch.move(rec.uid, rec.matrix);
    if (gizmos.has(rec.uid)) setGizmo(rec);
    return;
  }
  let track = tracks.get(rec.uid);
  if (!track) {
    // Starting from where it is drawn, a moment before.
    const it = batch.inst.get(rec.uid);
    let drawn;
    if (it) drawn = batch.mesh.getMatrixAt(it.id, new THREE.Matrix4()).toArray();
    else if (gizmos.has(rec.uid)) drawn = gizmos.get(rec.uid).matrix.toArray();
    else drawn = Array.from(rec.matrix);
    track = { rec, poses: [{ at: toldAt - madeGap, ...poseOf(drawn) }] };
    tracks.set(rec.uid, track);
  }
  track.rec = rec;
  track.poses.push({ at: toldAt, ...poseOf(rec.matrix) });
  if (track.poses.length > 6) track.poses.shift();
}

// Each frame: every object followed where it was `playDelay` ago.
function glide(now) {
  if (!tracks.size) return;
  const at = now - playDelay();
  const going = running();
  for (const [uid, track] of tracks) {
    const poses = track.poses;
    let k = poses.length - 1;
    while (k > 0 && poses[k - 1].at > at) k--;
    const b = poses[k];
    const a = poses[Math.max(k - 1, 0)];
    const t = b.at > a.at ? Math.min(Math.max((at - a.at) / (b.at - a.at), 0), 1) : 1;
    _gp.lerpVectors(a.p, b.p, t);
    _gq.slerpQuaternions(a.q, b.q, t);
    _gs.lerpVectors(a.s, b.s, t);
    const m = _gm.compose(_gp, _gq, _gs).toArray();
    batch.move(uid, m);
    if (gizmos.has(uid)) setGizmo({ ...track.rec, matrix: m });
    // Caught up with the last pose: no longer followed, once nothing
    // runs or it has stood still a while.
    const last = poses[poses.length - 1];
    if (at >= last.at && (!going || at - last.at > 1000)) {
      batch.move(uid, track.rec.matrix);
      if (gizmos.has(uid)) setGizmo(track.rec);
      tracks.delete(uid);
    }
  }
  shadowsDirty = true;
}

function removeObject(uid) {
  tracks.delete(uid);
  batch.remove(uid);
  removeGizmo(uid);
  records.delete(uid);
  if (selected === uid) select(null);
}

function setRobots(list) {
  robots = list;
  jointKinds.clear();
  for (const r of list) for (const j of r.joints) jointKinds.set(j.uid, j.kind);
  // A joint whose kind changed draws a different gizmo.
  for (const uid of jointKinds.keys()) {
    const rec = records.get(uid);
    if (rec) setGizmo(rec);
  }
  robotPanel.setRobots(list);
}

// ---- the stream -------------------------------------------------------------

// The build of KYNEX that served this page, as its stream first said.
let servedBy = null;

// The version of the scene this page holds: the scene file's, then each
// update's after it.
let heldVersion = -1;
// The latest scene the server has said exists.
let wantedVersion = -1;
// Updates that came while a scene was on its way. They may be newer than the
// scene file will be — it is made when asked for, not when announced — or
// older; either way they wait for it, and the newer ones are applied after.
let fetching = false;
const waiting = [];

function takeUpdate(data) {
  if (fetching) waiting.push(data);
  else onUpdate(data);
}

function afterScene() {
  fetching = heldVersion < wantedVersion;
  if (fetching) return;
  for (const data of waiting.splice(0)) onUpdate(data);
}

// The scene file: see `ViewState::scene_bin` on the server for its layout.
function readScene(buffer) {
  const view = new DataView(buffer);
  const magic = String.fromCharCode(...new Uint8Array(buffer, 0, 4));
  if (magic !== 'KYNX' || view.getUint32(4, true) !== 2) throw new Error('not a scene this viewer reads');
  const version = Number(view.getBigUint64(8, true));
  const length = view.getUint32(16, true);
  const header = JSON.parse(new TextDecoder().decode(new Uint8Array(buffer, 20, length)));
  let at = Math.ceil((20 + length) / 4) * 4;
  // Copied out, so that holding the matrices does not hold the whole file.
  const matrices = new Float32Array(buffer.slice(at, at + header.objects.length * 64));
  at += matrices.byteLength;
  header.objects.forEach((o, i) => (o.matrix = matrices.subarray(i * 16, i * 16 + 16)));
  const meshes = new Map();
  for (const m of header.meshes) {
    const positions = new Float32Array(buffer, at, m.vertices * 3);
    at += positions.byteLength;
    const lists = [];
    for (const count of [m.indices, ...m.lods]) {
      lists.push(new Uint32Array(buffer, at, count));
      at += count * 4;
    }
    if (!m.indices) continue;
    meshes.set(m.uid, levelsOf(positions, lists));
  }
  return { version, objects: header.objects, robots: header.robots, meshes };
}

async function fetchScene(version, noticed) {
  const response = await fetch(`/scene.bin?v=${version}`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const buffer = await response.arrayBuffer();
  // A later scene was announced while this one came: that one wins.
  if (version < wantedVersion) return;
  const fetched = performance.now();
  const data = readScene(buffer);
  heldVersion = data.version;
  const parsed = performance.now();
  onScene(data);
  // The scene file may be newer than its notice: it says what it holds.
  wantedVersion = Math.max(wantedVersion, data.version);
  afterScene();
  if (BENCH && !benchStarted) {
    benchStarted = true;
    runBench({ arrived: noticed, fetched, parsed, built: performance.now(), bytes: buffer.byteLength });
  }
}

function onScene(data) {
  setStale(false);
  for (const uid of [...gizmos.keys()]) removeGizmo(uid);
  if (selected != null) select(null);
  records.clear();
  jointKinds.clear();
  for (const r of data.robots) for (const j of r.joints) jointKinds.set(j.uid, j.kind);
  // See-through copies drew the scene before this one: let go. (A scene
  // arrives after the page has run through, so `xray` is there by then.)
  xrayOff();
  xray.items = null;
  batch.reset(data.meshes, data.objects.length);
  // The batch has its own copy of every vertex now.
  for (const levels of data.meshes.values()) for (const g of levels) g.dispose();
  content.add(batch.mesh);
  for (const rec of data.objects) applyObject(rec);
  setRobots(data.robots);
  renderTree();
  touched();
  document.getElementById('empty').hidden = true;
  if (!framedOnce) {
    framedOnce = true;
    requestAnimationFrame(() => frame(null, 'iso', false));
  }
  fitShadows();
  toast(`장면을 받았습니다 · 물체 ${data.objects.length}개 · 로봇 ${data.robots.length}대`);
}

function onUpdate(data) {
  // Already in the scene that was fetched, or older than it.
  if (!batch.mesh || data.v <= heldVersion) return;
  heldVersion = data.v;
  told(data.t);
  for (const [id, mesh] of Object.entries(data.meshes)) {
    const levels = buildGeometry(mesh);
    if (batch.hasGeometry(id)) batch.replaceGeometry(id, levels);
    else batch.addGeometry(id, levels);
    for (const g of levels) g.dispose();
  }
  let treeChanged = data.removed.length > 0;
  if (data.robots) {
    for (const r of data.robots) for (const j of r.joints) jointKinds.set(j.uid, j.kind);
  }
  for (const rec of data.objects) {
    // A part whose mesh just arrived is placed anew, not merely moved.
    if (data.meshes[rec.mesh] !== undefined) batch.remove(rec.uid);
    treeChanged = applyObject(rec) || treeChanged;
  }
  for (const uid of data.removed) removeObject(uid);
  if (data.robots) setRobots(data.robots);
  else robotPanel.refresh();
  if (treeChanged) renderTree();
  if (selected != null) showInspector(selected);
  // A joint still being dragged: shown where the slider is, over the pose
  // just told; let go and caught up with, as told.
  if (jointPreview?.ending && jointCaughtUp()) endJointPreview();
  else if (jointPreview) showJointPreview();
  touched();
}

// A joint dragged on the robot panel moves on the screen at once: what is
// beyond it turned about its axis (or slid along it) from the pose KYNEX
// last told to where the slider is, without waiting for KYNEX to answer.
// Each pose it tells is drawn so too, until the slider is let go; then
// what it told last stands. `uid` null ends it.
let jointPreview = null;
const _jp = new THREE.Matrix4();
const _jm = new THREE.Matrix4();
const _jo = new THREE.Vector3();
const _ja = new THREE.Vector3();
function previewJoint(uid, position) {
  const was = jointPreview;
  if (uid == null) {
    // Let go: kept until KYNEX tells the pose it was let go at (or a
    // moment has passed), so that it does not jump back meanwhile.
    if (!was) return;
    was.ending = true;
    clearTimeout(was.timer);
    was.timer = setTimeout(() => jointPreview === was && endJointPreview(), 1500);
    if (jointCaughtUp()) endJointPreview();
    return;
  }
  // Another joint: the last one's parts as told first.
  if (was && was.uid !== uid) endJointPreview();
  else if (was?.timer) clearTimeout(was.timer);
  jointPreview = { uid, position, below: was?.uid === uid ? was.below : partsBeyond(uid) };
  showJointPreview();
}

// Whether the pose KYNEX told has the joint where the slider was let go.
function jointCaughtUp() {
  const p = jointPreview;
  const j = p && robots.flatMap((r) => r.joints).find((x) => x.uid === p.uid);
  return !j || Math.abs(j.position - p.position) < 1e-4;
}

function endJointPreview() {
  const was = jointPreview;
  jointPreview = null;
  if (!was) return;
  clearTimeout(was.timer);
  // As KYNEX last told it.
  for (const u of was.below) {
    const rec = records.get(u);
    if (!rec) continue;
    batch.move(u, rec.matrix);
    if (gizmos.has(u)) setGizmo(rec);
  }
  shadowsDirty = true;
}

// Every object below `uid`: what a joint moves.
function partsBeyond(uid) {
  const children = new Map();
  for (const rec of records.values()) {
    if (rec.parent == null) continue;
    if (!children.has(rec.parent)) children.set(rec.parent, []);
    children.get(rec.parent).push(rec.uid);
  }
  const out = [];
  const stack = [...(children.get(uid) ?? [])];
  while (stack.length) {
    const u = stack.pop();
    out.push(u);
    stack.push(...(children.get(u) ?? []));
  }
  return out;
}

function showJointPreview() {
  const p = jointPreview;
  const j = robots.flatMap((r) => r.joints).find((x) => x.uid === p.uid);
  if (!j) return;
  // From the joint as told: its place and axis in the world, its position.
  const d = p.position - j.position;
  _jo.fromArray(j.origin);
  _ja.fromArray(j.axis).normalize();
  if (j.slides) {
    _jp.makeTranslation(_ja.x * d, _ja.y * d, _ja.z * d);
  } else {
    _jp.makeTranslation(_jo.x, _jo.y, _jo.z)
      .multiply(_jm.makeRotationAxis(_ja, d))
      .multiply(_jm.makeTranslation(-_jo.x, -_jo.y, -_jo.z));
  }
  for (const u of p.below) {
    const rec = records.get(u);
    if (!rec) continue;
    const m = _jm.multiplyMatrices(_jp, new THREE.Matrix4().fromArray(rec.matrix)).toArray();
    batch.move(u, m);
    if (gizmos.has(u)) setGizmo({ ...rec, matrix: m });
  }
  shadowsDirty = true;
}

const STATUS_TEXT = {
  live: (s) => `Kreate 연결됨 · ${s.address}${s.peers?.length ? ' · ' + s.peers.join(', ') : ''}`,
  waiting: (s) => `세션에 장면이 아직 없습니다 · ${s.address}`,
  offline: (s) => `Kreate 연결 안 됨 · ${s.address} · 다시 시도 중`,
  file: (s) => `파일 · ${s.address}`,
  alone: () => 'KYNEX 단독 실행',
};

// A scene shown while nothing keeps it in step says so, and how old it is.
let staleSince = 0;
function setStale(stale, why) {
  if (stale && !records.size) stale = false;
  if (stale && !staleSince) staleSince = lastUpdate || performance.now();
  if (!stale) staleSince = 0;
  document.getElementById('stale').hidden = !stale;
  host.classList.toggle('is-stale', stale);
  if (stale) document.getElementById('stale-text').textContent = why;
}

function onStatus(s) {
  if (s.state === 'live' || s.state === 'file' || s.state === 'alone') setStale(false);
  else if (s.state === 'offline') setStale(true, 'Kreate와 연결이 끊겼습니다 · 마지막으로 받은 장면입니다');
  else if (s.state === 'waiting') setStale(true, 'Kreate 세션에 장면이 없습니다 · 마지막으로 받은 장면입니다');
  const el = document.getElementById('status');
  el.dataset.state = s.state;
  const text = STATUS_TEXT[s.state]?.(s) ?? s.state;
  document.getElementById('status-text').textContent = text;
  el.title = s.reason ? `${text}\n${s.reason}` : text;
  if (!records.size) {
    const title = document.getElementById('empty-title');
    const body = document.getElementById('empty-body');
    if (s.state === 'offline') {
      title.textContent = 'Kreate에 연결할 수 없습니다';
      body.textContent = `${s.address}에 세션이 열려 있는지, 세션 키가 맞는지 확인하세요. 계속 다시 시도합니다.`;
    } else if (s.state === 'waiting') {
      title.textContent = 'Kreate 세션을 기다리는 중';
      body.innerHTML = 'Kreate에서 <b>파일 ▸ 공동 편집 세션 ▸ 이 장면으로 세션 열기</b>를 누르면 장면이 여기에 나타납니다.';
    }
  }
}

function connect() {
  const source = new EventSource('/events');
  source.addEventListener('scene', (e) => {
    const { v } = JSON.parse(e.data);
    wantedVersion = Math.max(wantedVersion, v);
    fetching = true;
    fetchScene(v, performance.now()).catch((err) => {
      console.error('scene', v, err);
      toast(`장면을 읽지 못했습니다: ${err.message}`);
      // Nothing to apply the waiting updates to: the next notice starts over.
      fetching = false;
      waiting.length = 0;
    });
  });
  source.addEventListener('update', () => benchUpdates++);
  source.addEventListener('update', (e) => takeUpdate(JSON.parse(e.data)));
  source.addEventListener('status', (e) => onStatus(JSON.parse(e.data)));
  source.addEventListener('pose', (e) => {
    const pose = JSON.parse(e.data);
    robotPanel.setPose(pose);
    timelinePanel.setClock(pose.clock);
  });
  source.addEventListener('programs', (e) => robotPanel.setPrograms(JSON.parse(e.data)));
  source.addEventListener('contacts', (e) => setContacts(JSON.parse(e.data)));
  source.addEventListener('library', (e) => libraryPanel.setLibrary(JSON.parse(e.data)));
  source.addEventListener('templates', (e) => libraryPanel.setThumbnails(JSON.parse(e.data)));
  source.addEventListener('timeline', (e) => timelinePanel.setTimeline(JSON.parse(e.data)));
  source.addEventListener('scenarios', (e) => timelinePanel.setScenarios(JSON.parse(e.data)));
  source.addEventListener('plc', (e) => plcPanel.setState(JSON.parse(e.data)));
  source.addEventListener('project', (e) => projectPanel.setState(JSON.parse(e.data)));
  source.addEventListener('floor', (e) => floorPanel.setState(JSON.parse(e.data)));
  source.addEventListener('history', (e) => setHistory(JSON.parse(e.data)));
  source.addEventListener('cell', (e) => {
    const cell = JSON.parse(e.data);
    cellPanel.setCell(cell);
    timelinePanel.setFlow(cell);
    robotPanel.setCell(cell);
    plcPanel.setCell(cell);
    freshStart();
  });
  // A page served by one KYNEX that reconnects to a newer one would read
  // its stream wrongly: it reloads instead.
  source.addEventListener('hello', (e) => {
    const { build } = JSON.parse(e.data);
    if (servedBy && servedBy !== build) location.reload();
    servedBy = build;
  });
  source.onerror = () => {
    const el = document.getElementById('status');
    el.dataset.state = 'lost';
    document.getElementById('status-text').textContent = 'KYNEX 서버와 연결이 끊겼습니다 · 다시 시도 중';
    setStale(true, 'KYNEX 서버와 연결이 끊겼습니다 · 마지막으로 받은 장면입니다');
  };
}

// ---- panels -----------------------------------------------------------------

const GLYPH = {
  mesh: '<svg class="glyph mesh" viewBox="0 0 16 16"><path d="M2.5 5L8 2l5.5 3v6L8 14l-5.5-3zM2.5 5L8 8l5.5-3M8 8v6"/></svg>',
  joint: '<svg class="glyph joint" viewBox="0 0 16 16"><circle cx="8" cy="8" r="3"/><path d="M8 1.5v3M8 11.5v3"/></svg>',
  link: '<svg class="glyph link" viewBox="0 0 16 16"><path d="M3 13L13 3M3 13h4M3 13V9"/></svg>',
  empty: '<svg class="glyph" viewBox="0 0 16 16"><path d="M8 3v10M3 8h10"/></svg>',
  camera: '<svg class="glyph" viewBox="0 0 16 16"><path d="M2 5h8v6H2zM10 7l4-2v6l-4-2"/></svg>',
  light: '<svg class="glyph" viewBox="0 0 16 16"><circle cx="8" cy="7" r="3.5"/><path d="M6.5 13h3"/></svg>',
  armature: '<svg class="glyph" viewBox="0 0 16 16"><path d="M4 12L12 4M3 13l2-1-1-1zM12 4l1-1"/></svg>',
};
const CHEVRON = '<svg viewBox="0 0 10 10"><path d="M2 3.5l3 3 3-3"/></svg>';

// Only the rows in view are in the page. A factory's tree is tens of
// thousands of rows, and laying every one of them out cost seconds before
// the first frame could be drawn.
const ROW = 26; // .node's height in app.css
let treeRows = []; // { rec, depth, kids }, in the order shown
let treeRowIndex = new Map(); // uid -> its row
let foldedLargeScene = false;
const treeList = document.getElementById('tree');
const treeSpacer = document.getElementById('tree-spacer');
const treeWindow = document.getElementById('tree-window');

function renderTree() {
  const query = document.getElementById('search').value.trim().toLowerCase();
  const children = new Map();
  for (const rec of records.values()) {
    const p = rec.parent != null && records.has(rec.parent) ? rec.parent : null;
    if (!children.has(p)) children.set(p, []);
    children.get(p).push(rec);
  }
  for (const arr of children.values()) arr.sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));

  // A large scene opens with its groups folded: the first look at a factory
  // is its stations, not its bolts.
  if (!foldedLargeScene && records.size > 1000) {
    for (const p of children.keys()) if (p != null) collapsed.add(p);
    foldedLargeScene = true;
  }

  // What matches the search, or holds something that does: once, from the
  // leaves up, rather than asked again at every level.
  let keep = null;
  if (query) {
    keep = new Set();
    const mark = (rec) => {
      let any = rec.name.toLowerCase().includes(query);
      for (const kid of children.get(rec.uid) || []) any = mark(kid) || any;
      if (any) keep.add(rec.uid);
      return any;
    };
    for (const rec of children.get(null) || []) mark(rec);
  }

  const rows = [];
  const walk = (parent, depth) => {
    for (const rec of children.get(parent) || []) {
      if (keep && !keep.has(rec.uid)) continue;
      const kids = (children.get(rec.uid) || []).length;
      rows.push({ rec, depth, kids });
      // A search shows what it found wherever it is folded away.
      if (kids && (query || !collapsed.has(rec.uid))) walk(rec.uid, depth + 1);
    }
  };
  walk(null, 0);
  treeRows = rows;
  treeRowIndex = new Map(rows.map((r, i) => [r.rec.uid, i]));
  treeSpacer.style.height = `${rows.length * ROW}px`;
  drawTreeWindow();
  document.getElementById('object-count').textContent = records.size;
}

function drawTreeWindow() {
  const query = document.getElementById('search').value.trim();
  const first = Math.max(0, Math.floor(treeList.scrollTop / ROW) - 10);
  const last = Math.min(treeRows.length, Math.ceil((treeList.scrollTop + treeList.clientHeight) / ROW) + 10);
  const html = [];
  for (let i = first; i < last; i++) {
    const { rec, depth, kids } = treeRows[i];
    const folded = !query && collapsed.has(rec.uid);
    const glyph = GLYPH[rec.role === 'joint' ? 'joint' : rec.role === 'link' ? 'link' : rec.kind] || GLYPH.empty;
    const cls = ['node'];
    if (rec.uid === selected) cls.push('selected');
    if (!rec.visible || rec.role === 'collision') cls.push('hidden-obj');
    if (folded) cls.push('collapsed');
    html.push(
      `<li class="${cls.join(' ')}" role="treeitem" data-uid="${escapeHtml(rec.uid)}" style="padding-left:${4 + depth * 10}px">` +
        `<span class="twist" data-twist="${kids ? escapeHtml(rec.uid) : ''}">${kids ? CHEVRON : ''}</span>` +
        glyph +
        `<span class="label">${escapeHtml(rec.name)}</span>` +
        (rec.role ? `<span class="tag">${rec.role}</span>` : kids && folded ? `<span class="tag">${kids}</span>` : '') +
        `</li>`,
    );
  }
  treeWindow.style.transform = `translateY(${first * ROW}px)`;
  treeWindow.innerHTML = html.join('');
}

let treeScrollQueued = false;
treeList.addEventListener('scroll', () => {
  if (treeScrollQueued) return;
  treeScrollQueued = true;
  requestAnimationFrame(() => {
    treeScrollQueued = false;
    drawTreeWindow();
  });
});

// Unfolds what holds `uid` and scrolls its row into view.
function revealInTree(uid) {
  let unfolded = false;
  for (let p = records.get(uid)?.parent; p != null && records.has(p); p = records.get(p).parent) {
    unfolded = collapsed.delete(p) || unfolded;
  }
  if (unfolded) renderTree();
  const i = treeRowIndex.get(uid);
  if (i !== undefined) {
    const top = i * ROW;
    if (top < treeList.scrollTop) treeList.scrollTop = top;
    else if (top + ROW > treeList.scrollTop + treeList.clientHeight) treeList.scrollTop = top + ROW - treeList.clientHeight;
  }
  drawTreeWindow();
}

document.getElementById('tree').addEventListener('click', (e) => {
  const twist = e.target.closest('[data-twist]');
  if (twist && twist.dataset.twist) {
    const uid = twist.dataset.twist;
    collapsed.has(uid) ? collapsed.delete(uid) : collapsed.add(uid);
    renderTree();
    return;
  }
  const item = e.target.closest('.node');
  if (item) select(item.dataset.uid, { frame: e.detail === 2 });
});
document.getElementById('tree').addEventListener('dblclick', (e) => {
  const item = e.target.closest('.node');
  if (item) frame(item.dataset.uid);
});
document.getElementById('search').addEventListener('input', renderTree);

const robotPanel = new RobotPanel({
  host: document.getElementById('robots'),
  records,
  scene,
  camera,
  canvas,
  controls,
  selected: () => selected,
  toast,
  escapeHtml,
  previewJoint: (uid, position) => previewJoint(uid, position),
});
// The object whose cell role the inspector shows and sets for `uid`: it,
// if it has a role; else, for equipment chosen whole, the part of it that
// has the equipment's role — a conveyor's belt, a machine's body —, not a
// part such as a robot's gripper. Kept while the selection and the cell
// stay as they are.
let roleOf = { uid: null, key: '', object: null };
function roleObject(uid) {
  if (uid == null) return null;
  // Not yet made while the panel itself is being made.
  let comps = [];
  try {
    comps = cellPanel.cell.components ?? [];
  } catch {}
  const key = `${records.size}|${comps.length}`;
  if (roleOf.uid === uid && roleOf.key === key) return roleOf.object;
  let object = uid;
  // Equipment a piece of the flow stands for: that piece.
  const hosted = comps.find((c) => c.host != null && c.host === uid);
  if (hosted && !comps.some((c) => c.object === uid)) object = hosted.object;
  else if (!comps.some((c) => c.object === uid)) {
    const WHOLE = new Set(['source', 'station', 'buffer', 'sink', 'conveyor', 'cnc', 'molder', 'positioner', 'amr']);
    const inner = comps.find((c) => WHOLE.has(c.kind) && c.object !== uid && underOf(c.object, uid));
    if (inner) object = inner.object;
    else object = markerIn(uid, comps, WHOLE) ?? uid;
  }
  roleOf = { uid, key, object };
  return object;
}
// Equipment with no role of its own — a cluster tool, a paint booth — whose
// process is a flow piece set in it or at its load port, as the demos have
// it: that piece's role, the one nearest its middle. Not for what a hall is
// built of, which stands over or around many.
const NOT_EQUIPMENT = new Set([
  'factory_wall', 'factory_column', 'floor_zone', 'overhead_crane', 'ceiling_light', 'roll_door',
  'guard_rail', 'pipe_rack', 'oht_rail', 'crane_rail', 'fence', 'pallet_rack', 'pedestal', 'forklift',
  'pallet_load', 'drum_pallet', 'car_body', 'operator', 'amr', 'amr_stop', 'conveyor', 'bottle_conveyor',
]);
// Placed item `item` is a machine or a tool, not a robot, a flow piece or
// what the hall is built of.
function isEquipment(item) {
  if (!item || NOT_EQUIPMENT.has(item.template) || item.template.startsWith('flow_')) return false;
  let group = '';
  try {
    group = libraryPanel.library?.templates?.find((t) => t.key === item.template)?.group ?? '';
  } catch {}
  return group !== '로봇';
}
function markerIn(uid, comps, kinds) {
  let item = null;
  try {
    item = libraryPanel.itemOf(uid);
  } catch {}
  if (!item || item.root !== uid || !isEquipment(item)) return null;
  const box = bounds(uid);
  box.min.x -= 0.3;
  box.min.y -= 0.3;
  box.max.x += 0.3;
  box.max.y += 0.3;
  const mid = box.getCenter(new THREE.Vector3());
  let best = null;
  let near = Infinity;
  for (const c of comps) {
    if (!kinds.has(c.kind) || !records.has(c.object) || underOf(uid, c.object)) continue;
    const at = bounds(c.object).getCenter(new THREE.Vector3());
    if (at.x < box.min.x || at.x > box.max.x || at.y < box.min.y || at.y > box.max.y) continue;
    const d = Math.hypot(at.x - mid.x, at.y - mid.y);
    if (d < near) [best, near] = [c.object, d];
  }
  return best;
}
// The placed equipment a piece of the flow could stand for: within 15 m of
// it, the nearest first, and the one it stands for now wherever it is.
function equipmentNear(uid, host) {
  let items = [];
  try {
    items = libraryPanel.library?.items ?? [];
  } catch {}
  const at = bounds(uid).getCenter(new THREE.Vector3());
  const out = [];
  for (const i of items) {
    if (i.root == null || !records.has(i.root) || !isEquipment(i) || underOf(uid, i.root)) continue;
    const box = bounds(i.root);
    const p = box.clampPoint(at, new THREE.Vector3());
    const d = Math.hypot(p.x - at.x, p.y - at.y);
    if (d <= 15 || i.root === host) out.push({ root: i.root, name: i.name ?? records.get(i.root)?.name ?? '', d });
  }
  return out.sort((a, b) => a.d - b.d).slice(0, 12);
}
const cellPanel = new CellPanel({
  scene,
  selected: () => roleObject(selected),
  toast,
  escapeHtml,
  select: (uid) => select(uid),
  frame: (uid) => frame(uid),
  nameOf: (uid) => records.get(uid)?.name ?? '',
  itemOf: (uid) => libraryPanel.itemOf(uid),
  // The robots, and the one among `uids` (an object and those above it).
  robots: () => robots.map((r) => ({ root: r.root, name: r.name })),
  robotOf: (uids) => uids.find((u) => robots.some((r) => r.root === u)) ?? null,
  bounds: (uid) => bounds(uid),
  equipmentNear: (uid, host) => equipmentNear(uid, host),
  remove: (uid) => {
    select(uid);
    if (!libraryPanel.removeSelected()) toast('라이브러리에서 놓은 장비만 지울 수 있습니다');
  },
});
// The signs over the flow as last chosen in this browser.
view.labels = cellPanel.showLabels;
document.querySelector('[data-toggle="labels"]')?.classList.toggle('on', view.labels);
// And the flow's arrows.
view.arrows = cellPanel.showArrows;
document.querySelector('[data-toggle="arrows"]')?.classList.toggle('on', view.arrows);

// Where a point on the screen lands in the scene: on the part under it, or
// on the floor.
const _floor = new THREE.Plane(new THREE.Vector3(0, 0, 1), 0);
function pick(x, y) {
  const r = canvas.getBoundingClientRect();
  const at = new THREE.Raycaster();
  at.setFromCamera(new THREE.Vector2(((x - r.left) / r.width) * 2 - 1, -((y - r.top) / r.height) * 2 + 1), camera);
  const hit = batch.mesh && at.intersectObject(batch.mesh, false)[0];
  if (hit) return hit.point;
  return at.ray.intersectPlane(_floor, new THREE.Vector3());
}

const plcPanel = new PlcPanel({ toast, escapeHtml, robots: () => robotPanel.robots });
// A ? beside each setting, for what it is.
mountHelp();
// Linking mode shown on its button.
const linkButton = () => document.querySelector('[data-action="link"]');
cellPanel.onLinking = (on) => {
  linkButton()?.classList.toggle('on', on);
  canvas.style.cursor = on ? 'crosshair' : '';
  if (!on) previewLink(null);
};
const projectPanel = new ProjectPanel({ toast, escapeHtml });
// The site's drawing: looked at from above when first laid.
const floorPanel = new FloorPanel({
  scene,
  toast,
  redraw: () => {},
  frame: (box) => {
    const sphere = box.getBoundingSphere(new THREE.Sphere());
    const fov = (camera.fov * Math.PI) / 180;
    const dist = (Math.max(sphere.radius, 0.5) / Math.sin(Math.min(fov, fov * camera.aspect) / 2)) * 1.05;
    const to = { target: sphere.center.clone(), position: sphere.center.clone().add(VIEWS.top.clone().normalize().multiplyScalar(dist)) };
    flight = { from: { target: controls.target.clone(), position: camera.position.clone() }, to, t0: performance.now() };
  },
});
const timelinePanel = new TimelinePanel({
  escapeHtml,
  toast,
  toggle: (on) => setToggle('timeline', on),
  select: (uid) => select(uid),
  // The flow chart's right-click menus: the same as the 3D view's.
  showMenu,
  equipmentMenu: (uid) => cellPanel.equipmentMenu(chainOf(uid), libraryPanel.itemOf(uid)?.root ?? null),
  linkMenu: (link) => cellPanel.linkMenu(link),
});

const libraryPanel = new LibraryPanel({
  records,
  scene,
  camera,
  canvas,
  controls,
  selected: () => selected,
  group: () => group,
  toggleGroup: (uid) => toggleGroup(uid),
  boxUnder: (root) => boxUnder(root),
  hideUnder: (root, hidden) => hideUnder(root, hidden),
  bounds: (uid) => bounds(uid),
  unhideAll: () => batch.hide([...batch.hidden], false),
  select,
  pick,
  toast,
  escapeHtml,
});

// The left panel's two pages: the scene's tree, and the library.
document.querySelector('.panel-head.tabs').addEventListener('click', (e) => {
  const tab = e.target.closest('[data-tab]');
  if (!tab) return;
  for (const t of document.querySelectorAll('[data-tab]')) t.classList.toggle('on', t === tab);
  for (const p of document.querySelectorAll('[data-page]')) p.hidden = p.dataset.page !== tab.dataset.tab;
});

document.getElementById('robots').addEventListener('click', (e) => {
  const row = e.target.closest('.joint');
  if (row) select(row.dataset.uid);
});
document.getElementById('robots').addEventListener('dblclick', (e) => {
  const row = e.target.closest('.joint');
  if (row) frame(row.dataset.uid);
});

// ---- selection --------------------------------------------------------------

// A part's colour over its own: red in contact, orange selected.
function tintOf(rec) {
  if (contacted.has(rec.uid)) return colorOf(rec).lerp(CONTACT, 0.75);
  if (dropTarget != null && underOf(rec.uid, dropTarget)) return colorOf(rec).lerp(LANDING, 0.6);
  if (selected === rec.uid) return colorOf(rec).lerp(ACCENT, 0.55);
  if (group.size && underGroup(rec.uid)) return colorOf(rec).lerp(ACCENT, 0.4);
  return null;
}

// Whether `uid` is in, or below, equipment of the group.
function underGroup(uid) {
  for (let at = uid, n = 0; at != null && n < 64; at = records.get(at)?.parent, n++) {
    if (group.has(at)) return true;
  }
  return false;
}

// The group anew: what was in it and what is now shown as it is.
function setGroup(roots) {
  const was = group;
  group = new Set(roots);
  if (!was.size && !group.size) return;
  for (const uid of records.keys()) {
    let at = uid;
    for (let n = 0; at != null && n < 64 && !was.has(at) && !group.has(at); n++) at = records.get(at)?.parent;
    if (at != null && (was.has(at) || group.has(at))) highlight(uid);
  }
  libraryPanel.groupChanged();
}

// A Ctrl- or Shift-click on equipment: into the group, or out of it. The
// one selected stays the one the inspector and the handle are for.
function toggleGroup(uid) {
  const item = libraryPanel.itemOf(uid);
  if (!item) return toast('여러 개 선택은 라이브러리에서 놓은 장비만 됩니다');
  const primary = libraryPanel.selectedItem();
  if (!primary) return select(uid);
  if (primary.root === item.root) return;
  const next = new Set(group);
  next.add(primary.root);
  if (next.has(item.root)) next.delete(item.root);
  else next.add(item.root);
  setGroup(next.size > 1 ? next : []);
}

function highlight(uid) {
  const rec = records.get(uid);
  if (!rec) return;
  batch.tint(uid, tintOf(rec));
  if (gizmos.has(uid)) setGizmo(rec);
}

// The `contacts` event: every robot part in contact now, and with what.
function setContacts(list) {
  const was = contacted;
  contacted = new Set(list.flatMap((c) => [c.part, c.other]));
  for (const uid of new Set([...was, ...contacted])) highlight(uid);
  robotPanel.setContacts(list);
}

function select(uid, { frame: andFrame = false, keep = [] } = {}) {
  const was = selected;
  selected = uid != null && records.has(uid) ? uid : null;
  // Anything chosen anew: the group goes, unless it is kept.
  setGroup(keep);
  // The selected part is kept whole and in view: see Batch.updateLevels.
  levelsDirty = true;
  if (was != null) highlight(was);
  if (selected != null) highlight(selected);
  // The gizmo onto the equipment selected, or away; none while the parts
  // themselves are dragged (the interact mode).
  libraryPanel.selectionChanged(view.interact ? null : selected);
  document.querySelectorAll('.node.selected, .joint.selected').forEach((n) => n.classList.remove('selected'));
  if (selected != null) {
    revealInTree(selected);
    document.querySelector(`.joint[data-uid="${selected}"]`)?.classList.add('selected');
    showInspector(selected);
    if (andFrame) frame(selected);
  } else {
    document.getElementById('inspector').hidden = true;
  }
}

const KIND_LABEL = { mesh: '메시', empty: '빈 객체', camera: '카메라', light: '조명', armature: '아마추어' };
const ROLE_LABEL = { joint: '관절', link: '링크', collision: '충돌', rig: '리그' };

// The selected object, and what is below it, kept in the library as
// equipment of the user's own (see library.rs, `save_custom`).
document.getElementById('save-custom').addEventListener('click', async () => {
  if (selected == null) return;
  const name = prompt('내 장비 이름', records.get(selected)?.name ?? '');
  if (!name) return;
  try {
    const response = await fetch('/robot', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ op: 'save_custom', object: selected, name }),
    });
    const answer = await response.json();
    toast(answer.ok ? `"${name}"을(를) 라이브러리의 내 장비에 저장했습니다` : answer.error || '저장하지 못했습니다');
  } catch (err) {
    toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
  }
});

function showInspector(uid) {
  const rec = records.get(uid);
  if (!rec) return;
  const m = new THREE.Matrix4().fromArray(rec.matrix);
  const p = new THREE.Vector3(), q = new THREE.Quaternion(), s = new THREE.Vector3();
  m.decompose(p, q, s);
  const e = new THREE.Euler().setFromQuaternion(q, 'XYZ');
  const deg = (r) => ((r * 180) / Math.PI).toFixed(1);
  document.getElementById('insp-kind').textContent = ROLE_LABEL[rec.role] || KIND_LABEL[rec.kind] || rec.kind;
  document.getElementById('insp-name').textContent = rec.name;
  document.getElementById('insp-pos').textContent = `${(p.x * 1000).toFixed(1)}, ${(p.y * 1000).toFixed(1)}, ${(p.z * 1000).toFixed(1)}`;
  document.getElementById('insp-rot').textContent = `${deg(e.x)}, ${deg(e.y)}, ${deg(e.z)}`;
  document.getElementById('insp-parent').textContent = rec.parent != null ? records.get(rec.parent)?.name ?? '—' : '—';
  cellPanel.showFor(roleObject(uid));
  libraryPanel.showFor(uid);
  // Placed equipment: its own section heads the inspector.
  document.getElementById('inspector').classList.toggle('is-item', !!libraryPanel.itemOf(uid));
  document.getElementById('inspector').hidden = false;
}
document.getElementById('insp-close').addEventListener('click', () => select(null));

// Click to pick, unless the pointer travelled: that was an orbit.
const ray = new THREE.Raycaster();
// Thin marks — a link's axes, a joint's arrows — are met within a few
// pixels of the pointer, not within a metre as three.js has it: in a small
// cell a metre is the equipment beside them.
function lineSlack() {
  const r = canvas.getBoundingClientRect();
  const metresPerPixel = (2 * camera.position.distanceTo(controls.target) * Math.tan((camera.fov * Math.PI) / 360)) / Math.max(1, r.height);
  ray.params.Line.threshold = metresPerPixel * 5;
  ray.params.Points.threshold = metresPerPixel * 5;
}
let down = null;
// ---- interact: the parts that move, dragged along their own way ------------
//
// As Visual Components' Interact: with it on (toolbar, I), a machine's door
// or a robot's joint under the pointer is dragged along its degree of
// freedom, within its limits — a door from shut to open, a joint about its
// axis. The layout does not change; 초기 상태로 puts them all back.

let interacting = null; // the drag going on
const interacted = { doors: new Set(), robots: new Set() }; // what was moved

// What part under the pointer moves, and how: a door of a machine, or a
// robot's moving joint. Null for the rest.
function grabbable(uid) {
  for (let at = uid, n = 0; at != null && n < 64; at = records.get(at)?.parent, n++) {
    for (const c of cellPanel.cell.components) {
      const way = c.machine?.doors?.find((d) => d.uid === at);
      if (way) return { kind: 'door', machine: c.object, way, open: c.machine.door ?? 0 };
    }
  }
  const joint = robotPanel.movingJointAt(uid);
  if (joint) {
    const j = robotPanel.jointOf(joint);
    const rec = records.get(joint.uid);
    if (!j || !rec) return null;
    const m = new THREE.Matrix4().fromArray(rec.matrix);
    const origin = new THREE.Vector3().setFromMatrixPosition(m);
    const axis = new THREE.Vector3().setFromMatrixColumn(m, 2).normalize();
    return { kind: 'joint', joint, slides: joint.slides, origin, axis, value: j.position };
  }
  return null;
}

// The part and point under screen point (x, y), if any.
function partAt(x, y) {
  const r = canvas.getBoundingClientRect();
  ray.setFromCamera(new THREE.Vector2(((x - r.left) / r.width) * 2 - 1, -((y - r.top) / r.height) * 2 + 1), camera);
  if (!batch.mesh) return null;
  const hit = shapeHit(ray.intersectObjects([batch.mesh, ...xray.group.children], false));
  return hit ? { uid: shapeUid(hit), point: hit.point.clone() } : null;
}

// A hit on a shape: on the batch, or on a see-through copy standing in for
// equipment hidden while it shows a product through it.
const isShape = (h) => h.batchId !== undefined || h.object.userData.xray === true;
const shapeUid = (h) => (h.batchId !== undefined ? batch.uidAt(h.batchId) : h.object.userData.uid);

// Of `hits`, nearest first, the one meant: the nearest shape, but a
// product seen through see-through equipment rather than the equipment.
function shapeHit(hits) {
  const shapes = hits.filter((h) => isShape(h) && shapeUid(h) != null);
  const first = shapes[0];
  if (!first?.object.userData.xray) return first ?? null;
  for (const h of shapes) {
    if (!h.object.userData.xray) return isProduct(shapeUid(h)) ? h : first;
  }
  return first;
}

// Whether `uid` is one of the flow's products (see `updateXray`).
function isProduct(uid) {
  try {
    const n = BigInt(uid);
    return n >= 0xffffn << 48n && n < (0xffffn << 48n) + (1n << 40n);
  } catch {
    return false;
  }
}

// How far round `axis` through `pivot` the pointer has gone from `from`
// (a point), radians; null when the ray misses the plane.
function turnAbout(pivot, axis, from) {
  const plane = new THREE.Plane().setFromNormalAndCoplanarPoint(axis, pivot);
  const p = ray.ray.intersectPlane(plane, new THREE.Vector3());
  if (!p) return null;
  const a = plane.projectPoint(from, new THREE.Vector3()).sub(pivot);
  const b = p.sub(pivot);
  return Math.atan2(axis.dot(new THREE.Vector3().crossVectors(a, b)), a.dot(b));
}

// How far along unit `dir` through `from` the pointer's ray comes nearest.
function alongLine(from, dir) {
  const w0 = ray.ray.origin.clone().sub(from);
  const b = ray.ray.direction.dot(dir);
  const d = ray.ray.direction.dot(w0);
  const e = dir.dot(w0);
  const denom = 1 - b * b;
  return Math.abs(denom) < 1e-9 ? 0 : (e - b * d) / denom;
}

// One door request on its way at a time; the newest waits.
const doorLane = { busy: false, next: null };
async function sendDoor(request) {
  if (doorLane.busy) return void (doorLane.next = request);
  doorLane.busy = true;
  for (let next = request; next; ) {
    doorLane.next = null;
    try {
      const a = await (await fetch('/robot', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(next) })).json();
      if (!a.ok) toast(a.error);
    } catch (e) {
      toast(`KYNEX에 요청하지 못했습니다: ${e.message}`);
    }
    next = doorLane.next;
  }
  doorLane.busy = false;
}

function interactMove(e) {
  const r = canvas.getBoundingClientRect();
  ray.setFromCamera(new THREE.Vector2(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1), camera);
  const g = interacting;
  if (g.kind === 'door') {
    let open = g.open;
    if (g.way.slide) {
      const by = new THREE.Vector3().fromArray(g.way.slide);
      const len = by.length();
      open += alongLine(g.point, by.clone().normalize()) / len;
    } else {
      const t = turnAbout(new THREE.Vector3().fromArray(g.way.pivot), new THREE.Vector3().fromArray(g.way.axis), g.point);
      if (t == null) return;
      open += t / g.way.angle;
    }
    sendDoor({ op: 'door_to', object: g.machine, open: Math.min(Math.max(open, 0), 1) });
  } else {
    const r2 = robotPanel.robots.find((x) => x.root === g.joint.root);
    if (!r2) return;
    const d = g.slides ? alongLine(g.point, g.axis) : turnAbout(g.origin, g.axis, g.point);
    if (d == null) return;
    robotPanel.send(r2, { op: 'jog', robot: r2.root, joint: g.joint.uid, position: g.value + d });
  }
}

canvas.addEventListener(
  'pointerdown',
  (e) => {
    if (!view.interact || e.button !== 0) return;
    const hit = partAt(e.clientX, e.clientY);
    const g = hit && grabbable(hit.uid);
    if (!g) return;
    // The part, not the view: the orbit does not see this press.
    e.stopImmediatePropagation();
    e.preventDefault();
    interacting = { ...g, point: hit.point };
    if (g.kind === 'door') interacted.doors.add(g.machine);
    else interacted.robots.add(g.joint.root);
    canvas.setPointerCapture(e.pointerId);
    canvas.style.cursor = 'grabbing';
    document.getElementById('interact-reset').hidden = false;
  },
  { capture: true },
);
canvas.addEventListener('pointermove', (e) => {
  if (interacting) return interactMove(e);
  if (!view.interact || e.buttons) return;
  // A hand over what can be dragged.
  const hit = partAt(e.clientX, e.clientY);
  canvas.style.cursor = hit && grabbable(hit.uid) ? 'grab' : '';
});
const endInteract = () => {
  if (!interacting) return;
  interacting = null;
  canvas.style.cursor = '';
  interactEnded = performance.now();
};
let interactEnded = 0;
canvas.addEventListener('pointerup', endInteract);
canvas.addEventListener('pointercancel', endInteract);

// Everything dragged back as it was: doors shut, robots as the scene has them.
document.getElementById('interact-reset').addEventListener('click', async () => {
  const post = (b) => fetch('/robot', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(b) });
  for (const machine of interacted.doors) await post({ op: 'door_to', object: machine, open: 0 });
  for (const root of interacted.robots) await post({ op: 'release', robot: root });
  interacted.doors.clear();
  interacted.robots.clear();
  document.getElementById('interact-reset').hidden = true;
  toast('움직인 문과 관절을 처음 상태로 되돌렸습니다');
});

// Linking by dragging: in link mode, pressed on equipment that sends parts
// on and dragged onto another, a line follows the pointer and the two are
// linked where it is let go. A press anywhere else turns the view as ever.
let linkDrag = null;
const linkLine = new THREE.Line(
  new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(), new THREE.Vector3()]),
  new THREE.LineDashedMaterial({ color: 0x5aa9ff, dashSize: 0.3, gapSize: 0.2, depthTest: false, transparent: true }),
);
linkLine.renderOrder = 10;
linkLine.visible = false;
scene.add(linkLine);
const chainOf = (uid) => {
  const uids = [];
  for (let at = uid, n = 0; at != null && n < 64; at = records.get(at)?.parent, n++) uids.push(at);
  return uids;
};
// What of the flow's markers is under the pointer: a dot, an arrow.
const markerAt = (x, y) => {
  const r = canvas.getBoundingClientRect();
  const project = (v) => {
    const p = v.clone().project(camera);
    // Behind the camera: nowhere on the screen.
    if (p.z > 1) return [Infinity, Infinity];
    return [r.left + ((p.x + 1) / 2) * r.width, r.top + ((1 - p.y) / 2) * r.height];
  };
  return cellPanel.pickMarker(x, y, project);
};
canvas.addEventListener(
  'pointerdown',
  (e) => {
    if (e.button !== 0) return;
    // A dot over a piece that sends parts on: always a link's start.
    const marker = markerAt(e.clientX, e.clientY);
    if (marker?.handle) {
      const c = cellPanel.componentOf(marker.handle);
      const at = c?.flow?.at;
      e.stopImmediatePropagation();
      e.preventDefault();
      controls.enabled = false;
      linkDrag = {
        uids: [marker.handle],
        x: e.clientX,
        y: e.clientY,
        from: new THREE.Vector3(at[0], at[1], at[2] + 0.45),
        moved: false,
      };
      return;
    }
    if (!cellPanel.linking()) return;
    const hit = partAt(e.clientX, e.clientY);
    const uids = hit ? chainOf(hit.uid) : [];
    if (!cellPanel.sender(uids)) return;
    // The view stays still while the line is drawn.
    controls.enabled = false;
    linkDrag = { uids, x: e.clientX, y: e.clientY, from: hit.point, moved: false };
  },
  { capture: true },
);
// A hand over a handle: it can be dragged, and it grows a little. In link
// mode, a line from the last piece follows the pointer, and what is under
// the pointer is shown as the link's end would be.
canvas.addEventListener('pointermove', (e) => {
  if (e.buttons || linkDrag || view.interact) return;
  const over = markerAt(e.clientX, e.clientY);
  cellPanel.hoverHandle(over?.handle ?? null);
  canvas.style.cursor = over?.handle ? 'grab' : over?.link ? 'pointer' : cellPanel.linking() ? 'crosshair' : '';
  const from = cellPanel.linking() ? cellPanel.linkingFrom() : null;
  if (from) previewLink(from.object, new THREE.Vector3(from.at[0], from.at[1], from.at[2] + 0.45), e.clientX, e.clientY);
  else if (linkLine.visible) previewLink(null);
});
canvas.addEventListener('pointermove', (e) => {
  if (!linkDrag) return;
  if (!linkDrag.moved && Math.hypot(e.clientX - linkDrag.x, e.clientY - linkDrag.y) < 6) return;
  linkDrag.moved = true;
  const from = cellPanel.sender(linkDrag.uids)?.object;
  if (from != null) previewLink(from, linkDrag.from, e.clientX, e.clientY);
});
window.addEventListener('pointerup', (e) => {
  if (!linkDrag) return;
  const drag = linkDrag;
  linkDrag = null;
  controls.enabled = true;
  previewLink(null);
  if (!drag.moved) return;
  const hit = partAt(e.clientX, e.clientY);
  if (!hit) return toast('다음 공정이 될 설비 위에 놓으세요');
  const uids = chainOf(hit.uid);
  if (uids.some((u) => drag.uids.includes(u))) return;
  const root = libraryPanel.itemOf(uids[0])?.root ?? null;
  const from = cellPanel.sender(drag.uids)?.object;
  const preview = from != null ? cellPanel.dropPreview(from, uids, root) : null;
  if (preview && !preview.ok) return toast(preview.text);
  const to = cellPanel.dragLink(drag.uids, uids, root);
  if (to != null) select(to);
});

// The link being drawn — dragged from a handle, or from the last piece in
// link mode — shown from `from` (its object, and `point` where the line
// starts) to the pointer at (x, y): the line green where letting go would
// link, red where it would not, a note by the pointer saying what would
// happen, and the equipment under it tinted. `from` null takes it all
// away.
let dropTarget = null;
const dragTip = document.getElementById('drag-tip');
function previewLink(from, point = null, x = 0, y = 0) {
  if (from == null) {
    linkLine.visible = false;
    dragTip.hidden = true;
    setDropTarget(null);
    return;
  }
  const hit = partAt(x, y);
  const uids = hit ? chainOf(hit.uid) : [];
  const to = hit?.point ?? ray.ray.intersectPlane(_floor, new THREE.Vector3());
  const preview = cellPanel.dropPreview(from, uids, uids.length ? (libraryPanel.itemOf(uids[0])?.root ?? null) : null);
  if (to) {
    linkLine.geometry.setFromPoints([point, to]);
    linkLine.computeLineDistances();
    linkLine.material.color.set(!uids.length ? 0x5aa9ff : preview.ok ? 0x3fcf8e : 0xff5f5f);
    linkLine.visible = true;
  }
  dragTip.textContent = preview.text;
  dragTip.className = `drag-tip ${preview.ok ? 'ok' : uids.length ? 'bad' : ''}`;
  dragTip.style.left = `${x + 14}px`;
  dragTip.style.top = `${y + 18}px`;
  dragTip.hidden = false;
  setDropTarget(preview.ok ? preview.object : null);
}

// The equipment a link would land on, tinted (see tintOf).
function setDropTarget(uid) {
  if (uid === dropTarget) return;
  const was = dropTarget;
  dropTarget = uid;
  for (const u of records.keys()) {
    if ((was != null && underOf(u, was)) || (uid != null && underOf(u, uid))) highlight(u);
  }
}

// Whether `uid` is `root` or below it.
function underOf(uid, root) {
  for (let at = uid, n = 0; at != null && n < 64; at = records.get(at)?.parent, n++) if (at === root) return true;
  return false;
}

// A right click — pressed and let go without moving — opens a menu for
// what is under the pointer: a link's arrow, equipment, or the floor.
// Dragged, the right button pans the view as ever.
canvas.addEventListener('contextmenu', (e) => e.preventDefault());
let rightDown = null;
canvas.addEventListener(
  'pointerdown',
  (e) => {
    if (e.button === 2) rightDown = { x: e.clientX, y: e.clientY };
  },
  true,
);
window.addEventListener('pointerup', (e) => {
  if (e.button !== 2 || !rightDown) return;
  const d = rightDown;
  rightDown = null;
  if (Math.hypot(e.clientX - d.x, e.clientY - d.y) > 4) return;
  sceneMenu(e.clientX, e.clientY);
});
function sceneMenu(x, y) {
  const marker = markerAt(x, y);
  if (marker?.link) return showMenu(x, y, cellPanel.linkMenu(marker.link));
  const hit = partAt(x, y);
  if (hit) {
    const uids = chainOf(hit.uid);
    const entries = cellPanel.equipmentMenu(uids, libraryPanel.itemOf(uids[0])?.root ?? null);
    if (entries) return showMenu(x, y, entries);
  }
  showMenu(x, y, [
    {
      label: cellPanel.linking() ? '연결 모드 끝' : '연결 모드 시작',
      hint: '설비를 차례로 눌러 잇기',
      run: () => (cellPanel.linking() ? cellPanel.stopLinking() : cellPanel.startLinking(selected)),
    },
    { label: view.labels ? '이름표 숨기기' : '이름표 보이기', hint: 'L', run: () => setToggle('labels', !view.labels) },
    { label: '화면 맞춤', hint: 'F', run: () => frame() },
  ]);
}
// The tree's rows too.
document.getElementById('tree').addEventListener('contextmenu', (e) => {
  const li = e.target.closest('.node[data-uid]');
  if (!li) return;
  e.preventDefault();
  const uids = chainOf(li.dataset.uid);
  const entries = cellPanel.equipmentMenu(uids, libraryPanel.itemOf(uids[0])?.root ?? null);
  if (entries) showMenu(e.clientX, e.clientY, entries);
});

canvas.addEventListener('pointerdown', (e) => (down = { x: e.clientX, y: e.clientY }));
canvas.addEventListener('pointerup', (e) => {
  if (!down || Math.hypot(e.clientX - down.x, e.clientY - down.y) > 4 || e.button !== 0) return;
  if (robotPanel.ownsPointer() || libraryPanel.ownsPointer()) return;
  if (libraryPanel.clicked(e.clientX, e.clientY)) return;
  const r = canvas.getBoundingClientRect();
  ray.setFromCamera(new THREE.Vector2(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1), camera);
  const targets = [...gizmos.values()].filter((g) => g.visible);
  if (batch.mesh) targets.push(batch.mesh, ...xray.group.children);
  const uidOf = shapeUid;
  lineSlack();
  const hits = ray.intersectObjects(targets, true);
  // A joint's or link's marker nearer than any shape is chosen as it is;
  // else the shape meant (see `shapeHit`).
  const marker = hits.find((h) => !isShape(h) && uidOf(h) != null);
  const shape = shapeHit(hits);
  const hit = marker && (!shape || marker.distance < shape.distance) ? marker : shape;
  // An arrow of the flow: chosen, to undo with Delete.
  if (!view.measure && !cellPanel.picking()) {
    const marker = markerAt(e.clientX, e.clientY);
    if (marker?.link) return cellPanel.chooseLink(marker.link);
    cellPanel.chooseLink(null);
  }
  // Measuring: the click is a point on a shape.
  if (view.measure) {
    const shape = hits.find(isShape);
    if (shape) measureAt(shape.point);
    return;
  }
  // Picking an edge to follow: the click is on a part's shape, not a
  // choice of it, and not on a joint's marker.
  if (robotPanel.picking()) {
    const shape = hits.find((h) => isShape(h) && uidOf(h) != null);
    if (shape) robotPanel.pickEdge(uidOf(shape), shape.point);
    return;
  }
  // Linking: the equipment clicked, linked from the one before.
  if (cellPanel.linking()) {
    if (!hit) return;
    const uids = [];
    for (let at = uidOf(hit), n = 0; at != null && n < 64; at = records.get(at)?.parent, n++) uids.push(at);
    const to = cellPanel.linkTo(uids, libraryPanel.itemOf(uids[0])?.root ?? null);
    if (to != null) select(to);
    return;
  }
  // Picking a door's hinge or a connector: a point on any shape.
  if (cellPanel.picking()) {
    const shape = hits.find(isShape);
    if (shape) cellPanel.pickPoint(shape.point);
    return;
  }
  if ((e.ctrlKey || e.metaKey || e.shiftKey) && hit) return toggleGroup(uidOf(hit));
  // A click on a part chooses the whole of its equipment: the placed item
  // it is part of, else its topmost object. With Alt, the part itself; a
  // joint's marker is that joint.
  const chosen = (h) => {
    const uid = uidOf(h);
    if (e.altKey || !isShape(h)) return uid;
    const item = libraryPanel.itemOf(uid);
    if (item) return item.root;
    let top = uid;
    for (let at = records.get(uid)?.parent, n = 0; at != null && n < 64; at = records.get(at)?.parent, n++) top = at;
    return top;
  };
  select(hit ? chosen(hit) : null);
});

// ---- camera -----------------------------------------------------------------

const VIEWS = {
  iso: new THREE.Vector3(1, -1.35, 0.95),
  top: new THREE.Vector3(0, -0.0001, 1),
  front: new THREE.Vector3(0, -1, 0.0001),
  right: new THREE.Vector3(1, 0, 0.0001),
};

// `root` and everything below it hidden on this page, or shown again.
function hideUnder(root, hidden) {
  const under = [];
  for (const uid of records.keys()) {
    let at = uid;
    for (let n = 0; at != null && at !== root && n < 64; n++) at = records.get(at)?.parent;
    if (at === root) under.push(uid);
  }
  batch.hide(under, hidden);
}

// The box about `root` and everything below it that has a shape.
function boxUnder(root) {
  const box = new THREE.Box3();
  const part = new THREE.Box3();
  for (const uid of records.keys()) {
    let at = uid;
    for (let n = 0; at != null && at !== root && n < 64; n++) at = records.get(at)?.parent;
    if (at === root && batch.boxOf(uid, part)) box.union(part);
  }
  return box;
}

function bounds(uid) {
  const box = new THREE.Box3();
  if (uid != null && records.has(uid)) {
    // No geometry of its own: what is below it, as placed equipment's root
    // has; nothing there either (a joint, a link, an empty), a small box
    // where it is.
    if (!batch.boxOf(uid, box)) {
      box.copy(boxUnder(uid));
      if (box.isEmpty()) {
        const p = new THREE.Vector3().setFromMatrixPosition(new THREE.Matrix4().fromArray(records.get(uid).matrix));
        box.setFromCenterAndSize(p, new THREE.Vector3(0.3, 0.3, 0.3));
      }
    }
  } else {
    batch.boxAll(box);
    for (const g of gizmos.values()) box.expandByPoint(new THREE.Vector3().setFromMatrixPosition(g.matrix));
  }
  if (box.isEmpty()) box.setFromCenterAndSize(new THREE.Vector3(0, 0, 0.5), new THREE.Vector3(2, 2, 1));
  return box;
}

let flight = null;
function frame(uid = selected, direction = null, animate = true) {
  const box = bounds(uid);
  const sphere = box.getBoundingSphere(new THREE.Sphere());
  const radius = Math.max(sphere.radius, 0.05);
  const dir = direction ? VIEWS[direction].clone().normalize() : camera.position.clone().sub(controls.target).normalize();
  const fov = (camera.fov * Math.PI) / 180;
  const dist = (radius / Math.sin(Math.min(fov, fov * camera.aspect) / 2)) * 1.1;
  const to = { target: sphere.center.clone(), position: sphere.center.clone().add(dir.multiplyScalar(dist)) };
  if (!animate) {
    controls.target.copy(to.target);
    camera.position.copy(to.position);
  } else {
    flight = { from: { target: controls.target.clone(), position: camera.position.clone() }, to, t0: performance.now() };
  }
  fitShadows();
}

function fitShadows() {
  const box = bounds(null);
  const sphere = box.getBoundingSphere(new THREE.Sphere());
  const r = Math.max(sphere.radius * 1.4, 1);
  // The nearest the view comes to what it looks at: a two-hundredth of the
  // scene, between 0.3 m (a cell's parts) and 1 m (a plant).
  controls.minDistance = THREE.MathUtils.clamp(sphere.radius * 0.005, 0.3, 1);
  const cam = sun.shadow.camera;
  cam.left = -r; cam.right = r; cam.top = r; cam.bottom = -r;
  cam.near = 0.1; cam.far = r * 6;
  sun.target.position.copy(sphere.center);
  sun.position.copy(sphere.center).add(new THREE.Vector3(0.45, -0.6, 0.9).normalize().multiplyScalar(r * 3));
  cam.updateProjectionMatrix();
  shadowsDirty = true;
}

// ---- toolbar and keys -------------------------------------------------------

// ---- measuring ---------------------------------------------------------------

// Two points clicked on shapes, the line between them, and how far apart:
// a third click starts again.
const measured = { points: [], group: new THREE.Group() };
measured.group.renderOrder = 12;
scene.add(measured.group);

function clearMeasure() {
  for (const child of [...measured.group.children]) {
    child.removeFromParent();
    child.geometry.dispose();
    child.material.dispose();
  }
  measured.points = [];
  document.getElementById('measure').hidden = true;
}

// Measuring: two points give their distance; a third, the angle at the
// second between the other two. A fourth starts again.
function measureAt(point) {
  if (measured.points.length === 3) clearMeasure();
  if (measured.points.length === 2) return measureAngle(point);
  measured.points.push(point.clone());
  const dot = new THREE.Mesh(
    new THREE.SphereGeometry(0.008, 12, 8),
    new THREE.MeshBasicMaterial({ color: 0xffd23c, depthTest: false }),
  );
  dot.position.copy(point);
  dot.renderOrder = 12;
  measured.group.add(dot);
  const out = document.getElementById('measure');
  out.hidden = false;
  if (measured.points.length === 1) {
    out.textContent = '두 번째 점을 누르세요';
    return;
  }
  const [a, b] = measured.points;
  const line = new THREE.Line(
    new THREE.BufferGeometry().setFromPoints([a, b]),
    new THREE.LineBasicMaterial({ color: 0xffd23c, depthTest: false }),
  );
  line.renderOrder = 12;
  measured.group.add(line);
  const d = b.clone().sub(a);
  const f = (v) => (v * 1000).toFixed(1);
  out.textContent = `거리 ${f(d.length())} mm · ΔX ${f(d.x)} · ΔY ${f(d.y)} · ΔZ ${f(d.z)} · 세 번째 점: 각도`;
}

function measureAngle(point) {
  const [a, b] = measured.points;
  measured.points.push(point.clone());
  const color = new THREE.LineBasicMaterial({ color: 0x7fd4ff, depthTest: false });
  const dot = new THREE.Mesh(new THREE.SphereGeometry(0.008, 12, 8), new THREE.MeshBasicMaterial({ color: 0x7fd4ff, depthTest: false }));
  dot.position.copy(point);
  dot.renderOrder = 12;
  const line = new THREE.Line(new THREE.BufferGeometry().setFromPoints([b, point]), color);
  line.renderOrder = 12;
  measured.group.add(dot, line);
  const [u, v] = [a.clone().sub(b), point.clone().sub(b)];
  const out = document.getElementById('measure');
  if (u.length() < 1e-9 || v.length() < 1e-9) {
    out.textContent = '두 번째 점과 같은 점입니다';
    return;
  }
  const angle = THREE.MathUtils.radToDeg(u.angleTo(v));
  const f = (x) => (x * 1000).toFixed(1);
  out.textContent = `각도 ${angle.toFixed(2)}° · 첫 선 ${f(u.length())} mm · 둘째 선 ${f(v.length())} mm`;
}

function setToggle(name, on) {
  view[name] = on;
  document.querySelector(`[data-toggle="${name}"]`)?.classList.toggle('on', on);
  if (name === 'grid') grid.visible = on;
  if (name === 'labels') cellPanel.setLabels(on);
  if (name === 'arrows') cellPanel.setArrows(on);
  if (name === 'joints') for (const [uid, g] of gizmos) g.visible = on && records.get(uid)?.visible !== false;
  if (name === 'timeline') {
    timelinePanel.show(on);
    document.getElementById('hint').hidden = on;
  }
  if (name === 'wire') {
    surface.wireframe = on;
    outline.enabled = !on;
  }
  if (name === 'interact') {
    // The parts, not the equipment: its gizmo away meanwhile.
    libraryPanel.selectionChanged(on ? null : selected);
    if (!on) canvas.style.cursor = '';
    toast(on ? '상호작용: 문이나 로봇 관절을 끌어 움직입니다 (I로 끄기)' : '상호작용을 껐습니다');
  }
  if (name === 'measure') {
    clearMeasure();
    if (on) {
      document.getElementById('measure').hidden = false;
      document.getElementById('measure').textContent = '첫 번째 점을 누르세요';
    }
  }
}

// The layout report (see report.js): the cell drawn without the joints'
// markers and what is being measured.
function report() {
  const markers = [...gizmos.values()].filter((g) => g.visible);
  openReport({
    renderer,
    scene,
    escapeHtml,
    toast,
    project: document.getElementById('status-text').textContent.replace(/^파일 · /, ''),
    bounds: () => {
      const all = new THREE.Box3();
      const one = new THREE.Box3();
      for (const uid of records.keys()) if (batch.boxOf(uid, one)) all.union(one);
      return all;
    },
    hide: () => {
      for (const g of markers) g.visible = false;
      measured.group.visible = false;
    },
    show: () => {
      for (const g of markers) g.visible = true;
      measured.group.visible = true;
    },
    library: libraryPanel.library,
    robots: robotPanel.robots,
    programs: robotPanel.programs,
    cell: cellPanel.cell,
  });
}

// The scene as a video, for a proposal: what the canvas draws, from the
// press to the next, kept as a WebM file.
let recording = null;
function record(button) {
  if (recording) {
    recording.recorder.stop();
    return;
  }
  const type = ['video/webm;codecs=vp8', 'video/webm;codecs=vp9', 'video/webm'].find((t) => MediaRecorder.isTypeSupported(t));
  if (!type) return toast('이 브라우저는 화면 녹화를 지원하지 않습니다');
  const recorder = new MediaRecorder(canvas.captureStream(30), { mimeType: type, videoBitsPerSecond: 8_000_000 });
  const chunks = [];
  const started = performance.now();
  const time = button.querySelector('.rec-time');
  const tick = setInterval(() => {
    const s = Math.floor((performance.now() - started) / 1000);
    time.textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  }, 250);
  recorder.ondataavailable = (e) => e.data.size && chunks.push(e.data);
  recorder.onstop = () => {
    clearInterval(tick);
    button.classList.remove('on', 'recording');
    time.hidden = true;
    recording = null;
    const stamp = new Date().toISOString().slice(0, 19).replace(/[-:T]/g, '');
    const link = document.createElement('a');
    link.href = URL.createObjectURL(new Blob(chunks, { type: 'video/webm' }));
    link.download = `kynex-${stamp}.webm`;
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 1000);
    toast(`${link.download}을(를) 내려받았습니다`);
  };
  recorder.start(1000);
  recording = { recorder };
  button.classList.add('on', 'recording');
  time.textContent = '0:00';
  time.hidden = false;
  toast('녹화를 시작했습니다. 녹화 버튼을 다시 누르면 끝납니다');
}

document.querySelector('.toolbar').addEventListener('click', (e) => {
  const b = e.target.closest('button');
  if (!b) return;
  if (b.dataset.action === 'record') record(b);
  if (b.dataset.action === 'report') report();
  // One panel over the toolbar at a time.
  if (b.dataset.action === 'plc') {
    projectPanel.panel.hidden = true;
    floorPanel.panel.hidden = true;
    plcPanel.toggle();
  }
  if (b.dataset.action === 'project') {
    plcPanel.panel.hidden = true;
    floorPanel.panel.hidden = true;
    projectPanel.toggle();
  }
  if (b.dataset.action === 'link') {
    if (cellPanel.linking()) cellPanel.stopLinking();
    else cellPanel.startLinking(selected);
  }
  if (b.dataset.action === 'floor') {
    plcPanel.panel.hidden = true;
    projectPanel.panel.hidden = true;
    floorPanel.toggle();
  }
  if (b.dataset.action === 'play') playPause();
  if (b.dataset.action === 'reset-run') resetRun();
  if (b.dataset.action === 'undo') history('undo');
  if (b.dataset.action === 'redo') history('redo');
  if (b.dataset.action === 'frame') frame();
  if (b.dataset.view) frame(selected, b.dataset.view);
  if (b.dataset.toggle) setToggle(b.dataset.toggle, !view[b.dataset.toggle]);
});

// Undoing and redoing the project's edits (the `history` event says what
// they would do).
async function history(op) {
  try {
    const response = await fetch('/robot', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ op }),
    });
    const answer = await response.json();
    toast(answer.ok ? `${op === 'undo' ? '되돌림' : '다시 함'}: ${answer.edit}` : answer.error);
  } catch (e) {
    toast(`${op === 'undo' ? '되돌리기' : '다시 하기'}: ${e.message}`);
  }
}

function setHistory(h) {
  const undo = document.getElementById('undo-btn');
  const redo = document.getElementById('redo-btn');
  undo.disabled = !h.undo;
  redo.disabled = !h.redo;
  undo.title = h.undo ? `되돌리기: ${h.undo} (Ctrl+Z)` : '되돌리기 (Ctrl+Z)';
  redo.title = h.redo ? `다시 하기: ${h.redo} (Ctrl+Y)` : '다시 하기 (Ctrl+Y)';
}

// Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z — but in a field, the field's own.
// The panels over the toolbar — projects, the site's drawing, the PLC —
// close when something outside them is clicked (but their own button,
// which toggles them), or on Esc, before Esc does anything else.
const toolbarPanels = () => [projectPanel.panel, floorPanel.panel, plcPanel.panel];
document.addEventListener(
  'pointerdown',
  (e) => {
    if (e.target.closest('.modal')) return;
    for (const panel of toolbarPanels()) {
      if (panel.hidden || panel.contains(e.target)) continue;
      const own = { 'project-panel': 'project', 'floor-panel': 'floor', 'plc-panel': 'plc' }[panel.id];
      if (e.target.closest(`.toolbar [data-action="${own}"]`)) continue;
      panel.hidden = true;
    }
  },
  true,
);
window.addEventListener(
  'keydown',
  (e) => {
    if (e.key !== 'Escape' || document.querySelector('.modal:not([hidden])')) return;
    const open = toolbarPanels().filter((p) => !p.hidden);
    if (!open.length) return;
    for (const p of open) p.hidden = true;
    e.preventDefault();
    e.stopImmediatePropagation();
  },
  true,
);

window.addEventListener('keydown', (e) => {
  if (!(e.ctrlKey || e.metaKey) || e.altKey || e.target.matches('input, textarea, select')) return;
  const k = e.key.toLowerCase();
  if (k === 'z' && !e.shiftKey) history('undo');
  else if (k === 'y' || (k === 'z' && e.shiftKey)) history('redo');
  // Program steps, while the program was last clicked in; else the
  // selected equipment: copied beside itself, kept to paste, pasted.
  else if ((k === 'c' || k === 'v') && robotPanel.stepKey(k)) {
    // Taken by the program.
  } else if (k === 'd') libraryPanel.duplicate();
  else if (k === 'c') {
    if (!libraryPanel.copy()) return;
  } else if (k === 'v') libraryPanel.paste();
  else return;
  e.preventDefault();
});

// Space: everything going stopped, or, nothing going, the process flow and
// every robot with a program set going.
function playPause() {
  const flow = timelinePanel.flowState;
  const going = !!flow?.on || robotPanel.going().length > 0;
  if (going) {
    if (flow?.on) timelinePanel.flowOp('stop');
    robotPanel.playAll(false);
    toast('정지했습니다 (스페이스: 다시 실행)');
    return;
  }
  const flows = (cellPanel.cell.components ?? []).some((c) => c.flow);
  if (flows) timelinePanel.flowOp('start');
  const robots = robotPanel.playAll(true);
  if (!flows && !robots) return toast('실행할 공정 흐름도, 프로그램이 있는 로봇도 없습니다');
  toast(`실행합니다${flows ? ' · 공정 흐름' : ''}${robots ? ` · 로봇 ${robots}대` : ''} (스페이스: 정지, +/−: 배속)`);
}

// Back to the start: the run stopped, every product taken away, the robots
// posed as they stood when it started.
async function resetRun() {
  try {
    const answer = await (
      await fetch('/robot', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ op: 'reset_run' }) })
    ).json();
    if (!answer.ok) return toast(answer.error || '처음으로 되돌리지 못했습니다');
    // What was gliding drawn where it was last told, and as told from now.
    for (const [uid, track] of tracks) {
      batch.move(uid, track.rec.matrix);
      if (gizmos.has(uid)) setGizmo(track.rec);
    }
    tracks.clear();
    toast('처음으로 되돌렸습니다: 실행 정지, 로봇은 실행 전 자세, 제품은 모두 지움');
  } catch (err) {
    toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
  }
}

// The page opened or reloaded: the simulation stopped, at 1×, as a fresh
// start is — not running on at whatever speed it was left at.
let fresh = false;
function freshStart() {
  if (fresh) return;
  fresh = true;
  const ask = (body) =>
    fetch('/robot', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }).catch(() => {});
  // A busy KYNEX — running fast — may not answer at once: asked again
  // until it stands still at 1×, for half a minute at most.
  let tries = 0;
  const settle = async () => {
    const going = !!timelinePanel.flowState?.on || robotPanel.going().length > 0;
    const speed = robotPanel.clock?.speed ?? 1;
    if ((!going && speed === 1) || tries++ > 30) return;
    if (robotPanel.going().length) await ask({ op: 'stop' });
    if (timelinePanel.flowState?.on) await ask({ op: 'flow', action: 'stop' });
    if (speed !== 1) await ask({ op: 'speed', speed: 1 });
    setTimeout(settle, 1000);
  };
  ask({ op: 'stop' }).then(() => setTimeout(settle, 500));
}

// + and −: the simulation's clock a step faster or slower, through the
// speeds the speed list offers.
const SPEEDS = [0.25, 0.5, 1, 2, 5, 10, 20, 50, 100, 200, 300];
async function stepSpeed(up) {
  const now = robotPanel.clock?.speed ?? 1;
  // The next of the list past where it is now, either way.
  const speed = up ? SPEEDS.find((s) => s > now + 1e-9) : [...SPEEDS].reverse().find((s) => s < now - 1e-9);
  if (speed === undefined) return toast(`배속 ${now}배 (${up ? '가장 빠름' : '가장 느림'})`);
  try {
    const answer = await fetch('/robot', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ op: 'speed', speed }),
    }).then((r) => r.json());
    if (!answer.ok) return toast(answer.error || '배속을 바꾸지 못했습니다');
    toast(`배속 ${speed}배`);
  } catch (err) {
    toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
  }
}

window.addEventListener('keydown', (e) => {
  if (e.target.matches('input, textarea, select') || e.ctrlKey || e.metaKey || e.altKey) return;
  const k = e.key.toLowerCase();
  // Space on a button pressed last: the key is the view's, not the button's.
  if (k === ' ' && e.target.closest('button')) e.target.blur();
  if (k === ' ') playPause();
  else if (k === '+' || k === '=') stepSpeed(true);
  else if (k === '-' || k === '_') stepSpeed(false);
  else if (k === 'f') frame();
  else if (k === 'j') setToggle('joints', !view.joints);
  else if (k === 'w') setToggle('wire', !view.wire);
  else if (k === 'g') setToggle('grid', !view.grid);
  else if (k === 'l') setToggle('labels', !view.labels);
  else if (k === 'a') setToggle('arrows', !view.arrows);
  else if (k === 'home') resetRun();
  else if (k === 'm') setToggle('measure', !view.measure);
  else if (k === 'i') setToggle('interact', !view.interact);
  else if (k === '0') frame(selected, 'iso');
  else if (k === '7') frame(selected, 'top');
  else if (k === '1') frame(selected, 'front');
  else if (k === '3') frame(selected, 'right');
  else if (libraryPanel.key(k)) return e.preventDefault();
  else if ((k === 'delete' || k === 'backspace') && cellPanel.cutChosenLink()) return e.preventDefault();
  else if (k === 'delete' && libraryPanel.removeSelected()) return e.preventDefault();
  else if (k === 'escape') {
    libraryPanel.arm(null);
    if (view.measure) setToggle('measure', false);
    if (cellPanel.picking()) return cellPanel.stopPicking();
    if (cellPanel.linking()) return cellPanel.stopLinking();
    select(null);
  }
  else return;
  e.preventDefault();
});

// Narrow screens: the panels are drawers.
const treePanel = document.getElementById('tree-panel');
const robotDrawer = document.getElementById('robot-panel');
document.getElementById('toggle-tree').addEventListener('click', () => {
  robotDrawer.classList.remove('open');
  treePanel.classList.toggle('open');
});
document.getElementById('toggle-robots').addEventListener('click', () => {
  treePanel.classList.remove('open');
  robotDrawer.classList.toggle('open');
});
canvas.addEventListener('pointerdown', () => {
  treePanel.classList.remove('open');
  robotDrawer.classList.remove('open');
});

// ---- small things -----------------------------------------------------------

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

let toastTimer = 0;
function toast(text) {
  const el = document.getElementById('toast');
  el.textContent = text;
  el.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove('show'), 2200);
}

const pulse = document.getElementById('pulse');
// The shadow map only changes when the scene or the light does: a camera
// moving round it needs no new one. Redrawing it every frame was a third of
// every frame's triangles.
let shadowsDirty = true;

// Each part's level of detail follows its size on screen: worked out again
// when the camera or the scene moved, at most ten times a second.
let levelsDirty = true;
let levelsAt = 0;
controls.addEventListener('change', () => (levelsDirty = true));

function touched() {
  shadowsDirty = true;
  levelsDirty = true;
  lastUpdate = performance.now();
  pulse.classList.add('hot');
  requestAnimationFrame(() => requestAnimationFrame(() => pulse.classList.remove('hot')));
}
setInterval(() => {
  const el = document.getElementById('updated');
  if (!lastUpdate) return;
  const s = (performance.now() - lastUpdate) / 1000;
  el.textContent = s < 1 ? '방금 갱신' : s < 60 ? `${Math.floor(s)}초 전` : `${Math.floor(s / 60)}분 전`;
}, 500);

// ---- loop -------------------------------------------------------------------

function resize() {
  const w = host.clientWidth, h = host.clientHeight;
  if (!w || !h) return;
  renderer.setSize(w, h, false);
  const size = renderer.getDrawingBufferSize(new THREE.Vector2());
  outline.setSize(size.x, size.y);
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
}
new ResizeObserver(resize).observe(host);
resize();

const ease = (t) => 1 - Math.pow(1 - t, 3);
const _size = new THREE.Vector2();
// Where each frame's time goes on this side, for the benchmark: what the
// browser spends preparing and submitting, not what the GPU spends drawing.
const cpu = { levels: 0, scene: 0, frames: 0, shadowFrames: 0 };
// While a gizmo is out — the robot's TCP handle, or equipment's — the
// joint and link markers step back so its arrows and rings read: faint
// while it is shown, gone while it is dragged.
let markersLook = 'normal';
function quietMarkers() {
  const dragging = robotPanel.dragging || libraryPanel.dragging || interacting != null;
  const shown = robotPanel.handleRoot != null || libraryPanel.handleItem != null;
  const look = dragging ? 'hidden' : shown ? 'faint' : 'normal';
  if (look === markersLook) return;
  markersLook = look;
  for (const node of gizmos.values()) {
    node.children[0]?.traverse((o) => {
      if (!o.material) return;
      o.material.userData.opacity ??= o.material.opacity;
      o.material.opacity = look === 'faint' ? 0.15 : o.material.userData.opacity;
    });
    // Only the joints that move stay, faint: fixed joints' arrows and the
    // links' axes, crowded about the tool, go while a gizmo is out.
    const moving = node.userData.kind?.startsWith('joint:') && node.userData.kind !== 'joint:fixed';
    node.children[0].visible = look === 'normal' || (look === 'faint' && moving);
  }
}

// ---- see-through equipment ---------------------------------------------------
// A product of the flow hidden by equipment — a car body in a paint booth or
// an oven — shows through it: equipment between the camera and a product is
// drawn see-through, while it is. Looked at by each piece of equipment's
// box, a few times a second, not by every part's triangles: the batch has
// thousands, the floor a few hundred pieces of equipment.

const PARTS = 0xffffn << 48n;
const PIECES = 1n << 40n;
// A car body's root stands on the floor: its middle is this far up.
const BODY_NAMES = new Set(['차체 (BIW)', '도장 차체', '완성차']);
const xray = { group: new THREE.Group(), on: new Map(), items: null, walls: new Map(), key: '', at: 0, built: 0, seen: new Map() };
// The building and what hangs in it are never drawn see-through for a
// product they hide: columns and lamps in the way as the view moves would
// flicker in and out. Walls are, by another rule (see `WALLS`).
const XRAY_NEVER = new Set(['factory_wall', 'factory_column', 'ceiling_light', 'pipe_rack', 'floor_zone',
  'overhead_crane', 'oht_rail', 'roll_door', 'guard_rail', 'fence', 'assembly_line']);
// Walls, drawn see-through whenever the view is from outside them: the
// near side of the building cut away, steady as the view moves.
const WALLS = new Set(['factory_wall', 'roll_door']);
// How long equipment stays see-through after it last hid a product: not
// flickering back as a product or the view passes the edge of its box.
const XRAY_HOLD = 600;
scene.add(xray.group);
const xrayMaterials = new Map();

function xrayMaterial(color) {
  const key = color ? color.getHexString() : 'none';
  let m = xrayMaterials.get(key);
  if (!m) {
    m = new THREE.MeshStandardMaterial({
      color: color ?? DEFAULT_COLOR,
      roughness: 0.58,
      metalness: 0.08,
      side: THREE.DoubleSide,
      flatShading: true,
      transparent: true,
      opacity: 0.18,
      depthWrite: false,
    });
    xrayMaterials.set(key, m);
  }
  return m;
}

// Each piece of placed equipment's objects and box, by its root: worked out
// again when the scene or the equipment changes, or every few seconds.
function xrayItems(now) {
  const placed = libraryPanel.library?.items ?? [];
  const walls = new Set(placed.filter((i) => WALLS.has(i.template)).map((i) => i.root));
  // A robot is what does the work: never drawn see-through, nor what it
  // stands in (its cell, its track) — a copy left standing as it moves.
  const holdsRobot = new Set();
  for (const r of robots) {
    for (let at = r.root, n = 0; at != null && n < 64; at = records.get(at)?.parent, n++) holdsRobot.add(at);
  }
  const roots = new Set(
    placed.filter((i) => (!XRAY_NEVER.has(i.template) || walls.has(i.root)) && !holdsRobot.has(i.root)).map((i) => i.root),
  );
  const key = `${records.size}|${robots.length}|${[...roots].join(',')}`;
  if (xray.items && key === xray.key && now - xray.built < 5000) return xray.items;
  xrayOff();
  const items = new Map();
  const top = new Map();
  const rootOf = (uid) => {
    if (top.has(uid)) return top.get(uid);
    const rec = records.get(uid);
    const r = roots.has(uid) ? uid : rec?.parent != null ? rootOf(rec.parent) : null;
    top.set(uid, r);
    return r;
  };
  const part = new THREE.Box3();
  for (const uid of records.keys()) {
    const root = rootOf(uid);
    if (root == null) continue;
    let it = items.get(root);
    if (!it) items.set(root, (it = { uids: [], box: new THREE.Box3() }));
    it.uids.push(uid);
    if (batch.boxOf(uid, part)) it.box.union(part);
  }
  // Each wall's middle and the way out of the building across it: along
  // its thin side, away from the middle of all the walls.
  xray.walls = new Map();
  const middle = new THREE.Box3();
  for (const root of walls) if (items.has(root)) middle.union(items.get(root).box);
  if (!middle.isEmpty()) {
    const c = middle.getCenter(new THREE.Vector3());
    for (const root of walls) {
      const box = items.get(root)?.box;
      if (!box || box.isEmpty()) continue;
      const at = box.getCenter(new THREE.Vector3());
      const size = box.getSize(new THREE.Vector3());
      const axis = size.x < size.y ? 'x' : 'y';
      xray.walls.set(root, { at, axis, out: Math.sign(at[axis] - c[axis]) || 1 });
    }
  }
  xray.items = items;
  xray.key = key;
  xray.built = now;
  return items;
}

function xrayOn(root, it) {
  const hid = it.uids.filter((u) => !batch.hidden.has(u));
  const meshes = [];
  for (const uid of it.uids) {
    const copy = batch.xrayCopy(uid);
    if (!copy) continue;
    const mesh = new THREE.Mesh(copy.geometry, xrayMaterial(copy.color));
    mesh.matrixAutoUpdate = false;
    mesh.matrix.copy(copy.matrix);
    mesh.userData.uid = uid;
    mesh.userData.xray = true;
    mesh.renderOrder = 5;
    xray.group.add(mesh);
    meshes.push(mesh);
  }
  batch.hide(hid, true);
  xray.on.set(root, { meshes, hid });
}

function xrayOff(root) {
  for (const r of root === undefined ? [...xray.on.keys()] : [root]) {
    const on = xray.on.get(r);
    if (!on) continue;
    for (const m of on.meshes) m.removeFromParent();
    batch.hide(on.hid, false);
    xray.on.delete(r);
    shadowsDirty = true;
  }
}

function updateXray(now) {
  // See-through copies follow what they copy as it moves: a door opening,
  // a press coming down.
  for (const on of xray.on.values()) {
    for (const m of on.meshes) {
      if (batch.matrixOf(m.userData.uid, _xm)) m.matrix.copy(_xm);
    }
  }
  if (now - xray.at < 200 || !batch.mesh) return;
  xray.at = now;
  // The flow's products: its parts, not their pieces.
  const products = [];
  const p = new THREE.Vector3();
  for (const [uid, rec] of records) {
    let n;
    try {
      n = BigInt(uid);
    } catch {
      continue;
    }
    if (n < PARTS || n >= PARTS + PIECES) continue;
    p.setFromMatrixPosition(_xm.fromArray(rec.matrix));
    if (BODY_NAMES.has(rec.name)) p.z += 0.7;
    products.push(p.clone());
  }
  const want = new Set();
  const items = xrayItems(now);
  const eye = camera.position;
  for (const [root, w] of xray.walls) {
    if ((eye[w.axis] - w.at[w.axis]) * w.out > 0) want.add(root);
  }
  if (products.length) {
    const ray = new THREE.Ray();
    const hit = new THREE.Vector3();
    for (const q of products) {
      const far = eye.distanceTo(q);
      ray.set(eye, q.clone().sub(eye).normalize());
      for (const [root, it] of items) {
        if (xray.walls.has(root) || it.box.isEmpty() || it.box.containsPoint(eye)) continue;
        if (ray.intersectBox(it.box, hit) && eye.distanceTo(hit) < far - 0.05) want.add(root);
      }
    }
  }
  for (const root of want) xray.seen.set(root, now);
  for (const root of [...xray.on.keys()]) {
    if (!want.has(root) && now - (xray.seen.get(root) ?? 0) > XRAY_HOLD) xrayOff(root);
  }
  for (const root of want) {
    if (!xray.on.has(root)) {
      xrayOn(root, xray.items.get(root));
      shadowsDirty = true;
    }
  }
}
const _xm = new THREE.Matrix4();

// The run button: ▶ while nothing runs, ■ while something does, and the
// clock's speed beside it when it is not 1×. Looked at each frame, drawn
// only when it changes.
let playShown = '';
function showPlay() {
  const on = running();
  const speed = robotPanel.clock?.speed ?? 1;
  const key = `${on}|${speed}`;
  if (key === playShown) return;
  playShown = key;
  const b = document.querySelector('[data-action="play"]');
  if (!b) return;
  b.classList.toggle('on', on);
  b.title = on ? '정지 (스페이스)' : '실행: 공정 흐름과 프로그램이 있는 로봇을 함께 돌립니다 (스페이스)';
  b.querySelector('.play-speed').textContent = speed !== 1 ? `${speed}×` : '';
  // And the badge over the view.
  const badge = document.getElementById('run-badge');
  if (!badge) return;
  badge.classList.toggle('on', on);
  badge.querySelector('.run-word').textContent = on ? '실행 중' : '정지';
  const sp = badge.querySelector('.run-speed');
  sp.textContent = `${speed}×`;
  sp.classList.toggle('fast', speed !== 1);
}
document.getElementById('run-badge')?.addEventListener('click', (e) => {
  const act = e.target.closest('[data-run]')?.dataset.run;
  if (act === 'toggle') playPause();
  else if (act === 'faster') stepSpeed(true);
  else if (act === 'slower') stepSpeed(false);
});

renderer.setAnimationLoop(() => {
  showPlay();
  glide(performance.now());
  quietMarkers();
  grid.material.uniforms.uCamera.value.copy(camera.position);
  // The flow's signs never taller on the screen than a line of text.
  const perPixel = (d) => (2 * d * Math.tan((camera.fov * Math.PI) / 360)) / Math.max(1, canvas.clientHeight);
  cellPanel.fitSigns(camera, perPixel);
  updateXray(performance.now());
  if (flight) {
    const t = Math.min((performance.now() - flight.t0) / 420, 1);
    const k = ease(t);
    controls.target.lerpVectors(flight.from.target, flight.to.target, k);
    camera.position.lerpVectors(flight.from.position, flight.to.position, k);
    if (t >= 1) flight = null;
  }
  controls.update();
  // The near plane as far out as the view allows: a near plane 1 cm away
  // seen from 100 m leaves the depth buffer telling apart nothing closer
  // than several centimetres, and the floor's marks, the outlines drawn from
  // depth, shimmer as the view moves. A four-hundredth of the distance to
  // what is looked at keeps a millimetre there.
  const near = THREE.MathUtils.clamp(camera.position.distanceTo(controls.target) / 400, 0.01, 1.0);
  if (Math.abs(near - camera.near) > camera.near * 0.1) {
    camera.near = near;
    camera.updateProjectionMatrix();
  }
  let t = performance.now();
  const lap = (name) => {
    const now = performance.now();
    cpu[name] += now - t;
    t = now;
  };
  if (batch.mesh && levelsDirty && t - levelsAt > 100) {
    batch.updateLevels(camera, renderer.getDrawingBufferSize(_size).y, selected);
    levelsDirty = false;
    levelsAt = t;
  }
  lap('levels');
  // Counted over the whole frame — shadows, scene and composite — not just
  // its last pass.
  renderer.info.reset();
  renderer.shadowMap.needsUpdate = shadowsDirty;
  if (shadowsDirty) cpu.shadowFrames++;
  shadowsDirty = false;
  outline.render(renderer, scene, camera);
  lap('scene');
  cpu.frames++;
});

fitShadows();
// ---- benchmark --------------------------------------------------------------

function gpuName() {
  const gl = renderer.getContext();
  const ext = gl.getExtension('WEBGL_debug_renderer_info');
  return ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
}

// In benchmark mode the pieces are reachable from the console, for
// measuring what each costs.
if (BENCH) window.__kynex = { renderer, batch, outline, sun, controls, camera };

async function runBench(t) {
  // A tab in the background draws no frames: say so rather than measure
  // nothing until the benchmark gives up.
  if (document.hidden) {
    await fetch('/bench', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ error: 'hidden' }) });
    toast('벤치마크: 이 탭이 화면에 보여야 측정할 수 있습니다');
    benchStarted = false;
    return;
  }
  const nextFrame = () => new Promise((r) => requestAnimationFrame(r));
  // The first frame drawn with the scene in it: shaders compiled, buffers up.
  await nextFrame();
  await nextFrame();
  const firstFrame = performance.now();
  frame(null, 'iso', false);
  const target = controls.target.clone();
  const radius = camera.position.distanceTo(target);
  const height = camera.position.z - target.z;
  const updatesBefore = benchUpdates;
  // Orbit the camera all the way round: every part of the scene comes into
  // view, as a person looking around it would bring it. Once from where the
  // whole scene is seen, where most parts are small and drawn coarse, and
  // once from six times closer, where the near ones are drawn whole.
  const orbit = async (scale, duration) => {
    for (const k of Object.keys(cpu)) cpu[k] = 0;
    const times = [];
    let last = performance.now();
    const start = last;
    let triangles = 0;
    while (performance.now() - start < duration) {
      await nextFrame();
      const now = performance.now();
      times.push(now - last);
      last = now;
      triangles = Math.max(triangles, renderer.info.render.triangles);
      const a = ((now - start) / duration) * Math.PI * 2;
      camera.position.set(target.x + Math.cos(a) * radius * scale, target.y + Math.sin(a) * radius * scale, target.z + height * scale);
      camera.lookAt(target);
      levelsDirty = true;
    }
    const elapsed = performance.now() - start;
    times.sort((x, y) => x - y);
    const pct = (p) => times[Math.min(times.length - 1, Math.floor(times.length * p))];
    const per = (k) => cpu[k] / Math.max(cpu.frames, 1);
    const cpuMs = { levels: per('levels'), scene: per('scene'), shadow_share: cpu.shadowFrames / Math.max(cpu.frames, 1) };
    return { fps: (times.length / elapsed) * 1000, median: pct(0.5), p95: pct(0.95), worst: times[times.length - 1], frames: times.length, triangles, cpuMs };
  };
  const near = await orbit(1 / 6, 5000);
  const far = await orbit(1, 6000);
  const triangles = batch.triangles();
  const report = {
    gpu: gpuName(),
    user_agent: navigator.userAgent,
    pixels: [renderer.domElement.width, renderer.domElement.height],
    scene_bytes: t.bytes,
    fetch_ms: t.fetched - t.arrived,
    parse_ms: t.parsed - t.fetched,
    build_ms: t.built - t.parsed,
    first_frame_ms: firstFrame - t.arrived,
    // From the page being asked for to the scene first drawn.
    load_total_ms: firstFrame,
    frames: far.frames,
    fps: far.fps,
    frame_ms_median: far.median,
    frame_ms_p95: far.p95,
    frame_ms_worst: far.worst,
    // From six times closer: parts near the camera at full detail.
    close_fps: near.fps,
    close_frame_ms_median: near.median,
    close_frame_ms_p95: near.p95,
    close_triangles_drawn: near.triangles,
    cpu_ms_per_frame: far.cpuMs,
    close_cpu_ms_per_frame: near.cpuMs,
    draw_calls: renderer.info.render.calls,
    triangles_drawn: far.triangles,
    triangles_in_scene: triangles,
    objects: records.size,
    geometries: renderer.info.memory.geometries,
    updates_during_run: benchUpdates - updatesBefore,
    js_heap_mb: performance.memory ? performance.memory.usedJSHeapSize / 1048576 : null,
  };
  // The next tier arrives as a new scene the moment the server has this
  // report — possibly before the post below has finished — and is measured
  // the same way.
  benchStarted = false;
  await fetch('/bench', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(report) });
  toast(`벤치마크 완료 · ${report.fps.toFixed(1)} fps`);
}

connect();

