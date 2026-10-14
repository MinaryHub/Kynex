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
import { LibraryPanel } from '/library.js';

// ---- state ------------------------------------------------------------------

const records = new Map();    // uid -> object record from KYNEX
let robots = [];
const jointKinds = new Map(); // joint uid -> kind
let selected = null;
let framedOnce = false;
let lastUpdate = 0;
// Benchmark mode (`/?bench`): `kynex bench --browser` opens the page so,
// and the page measures itself and posts the numbers back.
const BENCH = new URLSearchParams(location.search).has('bench');
let benchStarted = false;
let benchUpdates = 0;
const collapsed = new Set();
const view = { joints: true, wire: false, grid: true };

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
controls.dampingFactor = 0.12;
controls.screenSpacePanning = true;
controls.target.set(0, 0, 0.4);

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

// Ground: a shadow catcher, and a grid in metres with tenths faintly.
const ground = new THREE.Mesh(
  new THREE.PlaneGeometry(200, 200),
  new THREE.ShadowMaterial({ color: 0x000000, opacity: 0.28 }),
);
ground.receiveShadow = true;
// It darkens what is behind it and is not a surface of its own: no depth, so
// the composite leaves the background as chosen and finds no edge at the
// horizon.
ground.material.depthWrite = false;
ground.position.z = -0.0005;
scene.add(ground);

const grid = new THREE.Group();
function gridLines(size, step, color, opacity) {
  const g = new THREE.GridHelper(size, Math.round(size / step), color, color);
  g.rotation.x = Math.PI / 2;
  g.material.transparent = true;
  g.material.opacity = opacity;
  g.material.depthWrite = false;
  return g;
}
grid.add(gridLines(10, 0.1, 0x3a404a, 0.35));
grid.add(gridLines(40, 1, 0x4a515c, 0.8));
const axisLine = (to, color) => {
  const geo = new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(), to]);
  return new THREE.Line(geo, new THREE.LineBasicMaterial({ color, transparent: true, opacity: 0.9 }));
};
grid.add(axisLine(new THREE.Vector3(20, 0, 0), 0xd9534f));
grid.add(axisLine(new THREE.Vector3(0, 20, 0), 0x5cb85c));
grid.renderOrder = -1;
scene.add(grid);

const content = new THREE.Group();
scene.add(content);

// ---- materials --------------------------------------------------------------

const DEFAULT_COLOR = new THREE.Color().setRGB(0.62, 0.64, 0.67, THREE.LinearSRGBColorSpace);
const ACCENT = new THREE.Color(0xff8a3d);
// Parts in contact: a robot's link and what it runs into.
const CONTACT = new THREE.Color(0xff3b3b);
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
    batch.move(rec.uid, rec.matrix);
    if (gizmos.has(rec.uid)) setGizmo(rec);
  } else {
    place(rec);
  }
  return !old || old.name !== rec.name || old.parent !== rec.parent || old.visible !== rec.visible;
}

function removeObject(uid) {
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
  touched();
}

const STATUS_TEXT = {
  live: (s) => `Kreate 연결됨 · ${s.address}${s.peers?.length ? ' · ' + s.peers.join(', ') : ''}`,
  waiting: (s) => `세션에 장면이 아직 없습니다 · ${s.address}`,
  offline: (s) => `Kreate 연결 안 됨 · ${s.address} · 다시 시도 중`,
  file: (s) => `파일 · ${s.address}`,
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
  if (s.state === 'live' || s.state === 'file') setStale(false);
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
  source.addEventListener('pose', (e) => robotPanel.setPose(JSON.parse(e.data)));
  source.addEventListener('programs', (e) => robotPanel.setPrograms(JSON.parse(e.data)));
  source.addEventListener('contacts', (e) => setContacts(JSON.parse(e.data)));
  source.addEventListener('library', (e) => libraryPanel.setLibrary(JSON.parse(e.data)));
  source.addEventListener('cell', (e) => {
    const cell = JSON.parse(e.data);
    cellPanel.setCell(cell);
    robotPanel.setCell(cell);
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
});
const cellPanel = new CellPanel({ scene, selected: () => selected, toast, escapeHtml });

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

const libraryPanel = new LibraryPanel({
  records,
  scene,
  camera,
  canvas,
  controls,
  selected: () => selected,
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
  if (selected === rec.uid) return colorOf(rec).lerp(ACCENT, 0.55);
  return null;
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

function select(uid, { frame: andFrame = false } = {}) {
  const was = selected;
  selected = uid != null && records.has(uid) ? uid : null;
  // The selected part is kept whole and in view: see Batch.updateLevels.
  levelsDirty = true;
  if (was != null) highlight(was);
  if (selected != null) highlight(selected);
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
  cellPanel.showFor(uid);
  libraryPanel.showFor(uid);
  document.getElementById('inspector').hidden = false;
}
document.getElementById('insp-close').addEventListener('click', () => select(null));

// Click to pick, unless the pointer travelled: that was an orbit.
const ray = new THREE.Raycaster();
let down = null;
canvas.addEventListener('pointerdown', (e) => (down = { x: e.clientX, y: e.clientY }));
canvas.addEventListener('pointerup', (e) => {
  if (!down || Math.hypot(e.clientX - down.x, e.clientY - down.y) > 4 || e.button !== 0) return;
  if (robotPanel.ownsPointer() || libraryPanel.ownsPointer()) return;
  if (libraryPanel.clicked(e.clientX, e.clientY)) return;
  const r = canvas.getBoundingClientRect();
  ray.setFromCamera(new THREE.Vector2(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1), camera);
  const targets = [...gizmos.values()].filter((g) => g.visible);
  if (batch.mesh) targets.push(batch.mesh);
  const uidOf = (h) => (h.batchId !== undefined ? batch.uidAt(h.batchId) : h.object.userData.uid);
  const hit = ray.intersectObjects(targets, true).find((h) => uidOf(h) != null);
  select(hit ? uidOf(hit) : null);
});

// ---- camera -----------------------------------------------------------------

const VIEWS = {
  iso: new THREE.Vector3(1, -1.35, 0.95),
  top: new THREE.Vector3(0, -0.0001, 1),
  front: new THREE.Vector3(0, -1, 0.0001),
  right: new THREE.Vector3(1, 0, 0.0001),
};

function bounds(uid) {
  const box = new THREE.Box3();
  if (uid != null && records.has(uid)) {
    if (!batch.boxOf(uid, box)) {
      // No geometry: a joint, a link, an empty — a small box where it is.
      const p = new THREE.Vector3().setFromMatrixPosition(new THREE.Matrix4().fromArray(records.get(uid).matrix));
      box.setFromCenterAndSize(p, new THREE.Vector3(0.3, 0.3, 0.3));
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
  const cam = sun.shadow.camera;
  cam.left = -r; cam.right = r; cam.top = r; cam.bottom = -r;
  cam.near = 0.1; cam.far = r * 6;
  sun.target.position.copy(sphere.center);
  sun.position.copy(sphere.center).add(new THREE.Vector3(0.45, -0.6, 0.9).normalize().multiplyScalar(r * 3));
  cam.updateProjectionMatrix();
  shadowsDirty = true;
}

// ---- toolbar and keys -------------------------------------------------------

function setToggle(name, on) {
  view[name] = on;
  document.querySelector(`[data-toggle="${name}"]`)?.classList.toggle('on', on);
  if (name === 'grid') grid.visible = on;
  if (name === 'joints') for (const [uid, g] of gizmos) g.visible = on && records.get(uid)?.visible !== false;
  if (name === 'wire') {
    surface.wireframe = on;
    outline.enabled = !on;
  }
}

document.querySelector('.toolbar').addEventListener('click', (e) => {
  const b = e.target.closest('button');
  if (!b) return;
  if (b.dataset.action === 'frame') frame();
  if (b.dataset.view) frame(selected, b.dataset.view);
  if (b.dataset.toggle) setToggle(b.dataset.toggle, !view[b.dataset.toggle]);
});

window.addEventListener('keydown', (e) => {
  if (e.target.matches('input, textarea') || e.ctrlKey || e.metaKey || e.altKey) return;
  const k = e.key.toLowerCase();
  if (k === 'f') frame();
  else if (k === 'j') setToggle('joints', !view.joints);
  else if (k === 'w') setToggle('wire', !view.wire);
  else if (k === 'g') setToggle('grid', !view.grid);
  else if (k === '0') frame(selected, 'iso');
  else if (k === '7') frame(selected, 'top');
  else if (k === '1') frame(selected, 'front');
  else if (k === '3') frame(selected, 'right');
  else if (libraryPanel.key(k)) return e.preventDefault();
  else if (k === 'escape') {
    libraryPanel.arm(null);
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
renderer.setAnimationLoop(() => {
  if (flight) {
    const t = Math.min((performance.now() - flight.t0) / 420, 1);
    const k = ease(t);
    controls.target.lerpVectors(flight.from.target, flight.to.target, k);
    camera.position.lerpVectors(flight.from.position, flight.to.position, k);
    if (t >= 1) flight = null;
  }
  controls.update();
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
