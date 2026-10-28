// The library: equipment to place in the cell — robots, CNC machines,
// pallet tables, pedestals, fences — dragged from the list onto the scene
// (or picked, then placed with a click), then moved, turned and resized.
// KYNEX makes each piece from its template and adds it to the scene it
// shows (the `library` event); nothing goes to Kreate.

import * as THREE from 'three';
import { TransformControls } from '/vendor/TransformControls.js';
import { tFor, tr, getLang } from './i18n.js';

const t = tFor('panels');

async function post(request) {
  const response = await fetch('/robot', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

// Thumbnails, drawn at twice the size they show at.
const THUMB = [160, 120];

// Each template's thumbnail, as an image: drawn once from its triangles
// (the `templates` event) on a canvas of its own, which is then let go.
function drawThumbnails(templates) {
  const out = new Map();
  const canvas = document.createElement('canvas');
  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, preserveDrawingBuffer: true });
  } catch {
    return out;
  }
  renderer.setSize(THUMB[0], THUMB[1], false);
  renderer.setClearColor(0x000000, 0);
  const camera = new THREE.PerspectiveCamera(30, THUMB[0] / THUMB[1], 0.01, 100);
  camera.up.set(0, 0, 1);
  for (const [key, parts] of Object.entries(templates)) {
    const scene = new THREE.Scene();
    scene.add(new THREE.HemisphereLight(0xffffff, 0x445566, 2.2));
    const sun = new THREE.DirectionalLight(0xffffff, 2.0);
    sun.position.set(-2, -3, 5);
    scene.add(sun);
    const box = new THREE.Box3();
    for (const part of parts) {
      if (!part.p.length) continue;
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.Float32BufferAttribute(part.p, 3));
      geometry.computeVertexNormals();
      const color = new THREE.Color().setRGB(part.c[0], part.c[1], part.c[2], THREE.LinearSRGBColorSpace);
      const mesh = new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({ color, roughness: 0.6, metalness: 0.1, flatShading: true }));
      scene.add(mesh);
      geometry.computeBoundingBox();
      box.union(geometry.boundingBox);
    }
    if (box.isEmpty()) continue;
    // From the front, a little to the left and above: the side work is
    // done from.
    const centre = box.getCenter(new THREE.Vector3());
    const radius = box.getBoundingSphere(new THREE.Sphere()).radius;
    const away = new THREE.Vector3(-0.8, -1.4, 0.9).normalize();
    camera.position.copy(centre).addScaledVector(away, (radius / Math.sin((camera.fov * Math.PI) / 360)) * 1.02);
    camera.lookAt(centre);
    renderer.render(scene, camera);
    out.set(key, canvas.toDataURL('image/png'));
    scene.traverse((o) => {
      o.geometry?.dispose();
      o.material?.dispose();
    });
  }
  renderer.dispose();
  renderer.forceContextLoss();
  return out;
}

const deg = (r) => (r * 180) / Math.PI;
const rad = (d) => (d * Math.PI) / 180;

export class LibraryPanel {
  // ctx: { records, scene, camera, canvas, controls, selected(), select(uid), pick(x, y), toast, escapeHtml }
  constructor(ctx) {
    this.ctx = ctx;
    this.library = { templates: [], items: [] };
    this.thumbs = new Map();
    this.list = document.getElementById('library-list');
    this.box = document.getElementById('insp-item');
    // A template picked to be placed with the next click on the scene.
    this.armed = null;
    // One move on its way at a time; the newest waits.
    this.moving = { busy: false, next: null };

    this.proxy = new THREE.Object3D();
    ctx.scene.add(this.proxy);
    this.handle = new TransformControls(ctx.camera, ctx.canvas);
    this.handle.setSize(0.8);
    // Its arrows and rings along the equipment's own axes, not the world's.
    this.handle.setSpace('local');
    this.helper = this.handle.getHelper();
    this.handleItem = null;
    // The gizmo on the equipment selected, unless turned off in this browser.
    this.gizmo = true;
    try {
      this.gizmo = localStorage.getItem('kynex.gizmo') !== '0';
    } catch {}
    // Items hidden on this page alone, by id (and the name they had, so a
    // project opened anew does not hide its own by chance).
    this.hidden = new Map();
    // The array and mirror numbers as last typed, and whether that part is
    // open.
    this.arrayVals = {};
    this.arrayOpen = false;
    // The handle's grid and angle steps, as last chosen in this browser:
    // turning, by 15° unless chosen otherwise.
    this.snap = { grid: 0, angle: 15 };
    try {
      const kept = JSON.parse(localStorage.getItem('kynex.snap') || '{}');
      if ([0, 10, 50, 100, 500].includes(kept.grid)) this.snap.grid = kept.grid;
      if ([0, 5, 15, 45, 90].includes(kept.angle)) this.snap.angle = kept.angle;
    } catch {}
    this.applySnap();
    this.dragging = false;
    this.draggedAt = 0;
    this.handle.addEventListener('dragging-changed', (e) => {
      ctx.controls.enabled = !e.value;
      this.dragging = e.value;
      // The group as it stood when the drag began: it moves with the item
      // the handle is on, and turns about it.
      const handled = this.handleItem != null && this.itemById(this.handleItem);
      const items = this.groupItems();
      this.dragFrom = e.value && handled && items.length > 1 && items.some((i) => i.id === handled.id)
        ? { at: this.proxy.position.toArray(), yaw: handled.yaw, items: items.map((i) => ({ id: i.id, at: [...i.at], yaw: i.yaw })) }
        : null;
      if (!e.value) {
        this.draggedAt = performance.now();
        // Let go: onto what it connects to, if near, else set down on the
        // floor or what is below it (unless Alt is held) — one item alone;
        // a group, each set down. Turned, only set down: snapped onto what
        // it connects to, a conveyor turned about its middle would be
        // turned back in line with the next, its end still near that one's.
        if (this.handleItem != null && !this.alt && this.groupItems().length <= 1) {
          const z = this.snapAngle(new THREE.Euler().setFromQuaternion(this.proxy.quaternion, 'ZYX').z);
          const snap = this.mode !== 'rotate';
          this.send({ op: 'move', item: this.handleItem, at: this.originAt(), yaw: z, snap, settle: true }, (a) => this.saySnapped(a));
        } else if (this.handleItem != null && !this.alt) {
          const moves = this.groupItems().map((i) => ({ item: i.id, at: [...i.at], yaw: i.yaw }));
          this.move(null, null, null, { op: 'move_many', moves, settle: true });
        }
      }
    });
    // Alt held: let go where it is, connected to nothing.
    this.alt = false;
    for (const type of ['keydown', 'keyup']) window.addEventListener(type, (e) => (this.alt = e.altKey));
    this.handle.addEventListener('objectChange', () => {
      if (!this.dragging || this.handleItem == null) return;
      // Its turn about Z, then its tilt about Y and X, as the item keeps
      // them (`roll`, `pitch`, `yaw`).
      const e = new THREE.Euler().setFromQuaternion(this.proxy.quaternion, 'ZYX');
      // Turning, each angle onto the angle step: 0°, 15°, 30° … whatever
      // it stood at before, not 15° on from that.
      if (this.mode === 'rotate') e.set(this.snapAngle(e.x), this.snapAngle(e.y), this.snapAngle(e.z), 'ZYX');
      // Several together turn about Z only: the floor stays the floor.
      if (this.dragFrom) {
        this.proxy.quaternion.setFromEuler(new THREE.Euler(0, 0, e.z, 'ZYX'));
        return this.moveGroup(this.proxy.position.toArray(), e.z);
      }
      const at = this.originAt();
      this.move(null, null, null, { op: 'move', item: this.handleItem, at, yaw: e.z, pitch: e.y, roll: e.x });
    });

    this.list.addEventListener('dragstart', (e) => {
      const key = e.target.closest('[data-template]')?.dataset.template;
      if (key) e.dataTransfer.setData('text/kynex-template', key);
    });
    this.list.addEventListener('click', (e) => {
      const gone = e.target.closest('[data-delete-custom]')?.dataset.deleteCustom;
      if (gone) {
        e.stopPropagation();
        if (confirm(t('이 장비를 내 장비에서 지울까요?'))) this.send({ op: 'delete_custom', key: gone });
        return;
      }
      const tile = e.target.closest('[data-template]');
      if (tile) return this.arm(this.armed === tile.dataset.template ? null : tile.dataset.template);
      const item = e.target.closest('[data-item]');
      const root = item && this.itemById(Number(item.dataset.item))?.root;
      // Ctrl or Shift: into the group, or out of it.
      if (root != null && (e.ctrlKey || e.metaKey || e.shiftKey)) this.ctx.toggleGroup(root);
      else if (root != null) this.ctx.select(root, { frame: true });
    });
    ctx.canvas.addEventListener('dragover', (e) => {
      if (e.dataTransfer.types.includes('text/kynex-template')) e.preventDefault();
    });
    // A model file: picked, or dropped on the scene.
    const picker = document.getElementById('import-file');
    document.getElementById('import-model').addEventListener('click', () => picker.click());
    picker.addEventListener('change', () => {
      const file = picker.files?.[0];
      picker.value = '';
      if (file) this.importModel(file);
    });
    ctx.canvas.addEventListener('dragover', (e) => {
      if (e.dataTransfer.types.includes('Files')) e.preventDefault();
    });
    ctx.canvas.addEventListener('drop', (e) => {
      const file = e.dataTransfer.files?.[0];
      if (file) {
        e.preventDefault();
        return this.importModel(file);
      }
      const key = e.dataTransfer.getData('text/kynex-template');
      if (!key) return;
      e.preventDefault();
      this.alt = e.altKey;
      this.placeAt(key, e.clientX, e.clientY);
    });
    this.box.addEventListener('change', (e) => this.edited(e));
    this.box.addEventListener('toggle', (e) => {
      if (e.target.classList?.contains('insp-array')) this.arrayOpen = e.target.open;
    }, true);
    this.box.addEventListener('click', (e) => {
      const act = e.target.closest('[data-item-act]')?.dataset.itemAct;
      const item = this.selectedItem();
      if (!item) return;
      if (act === 'lock') {
        const items = this.groupItems();
        this.send({ op: 'lock', items: items.map((i) => i.id), locked: !item.locked }, () =>
          this.ctx.toast(item.locked ? t('잠금을 풀었습니다') : t('장비 {n}개를 잠갔습니다', { n: items.length })),
        );
        if (!item.locked) this.showHandle(null);
      } else if (act === 'hide') {
        const hide = !this.hidden.has(item.id);
        for (const i of this.groupItems()) {
          if (hide) this.hidden.set(i.id, i.name);
          else this.hidden.delete(i.id);
          this.ctx.hideUnder(i.root, hide);
        }
        this.renderList();
        this.showFor(this.ctx.selected());
      } else if (act === 'array-line') {
        this.arrayLine();
      } else if (act === 'array-circle') {
        this.arrayCircle();
      } else if (act === 'mirror') {
        this.mirror();
      } else if (act === 'remove') {
        this.removeSelected();
      } else if (act === 'duplicate') {
        this.duplicate();
      } else if (act === 'handle') {
        this.setGizmo(!this.gizmo);
      } else if (act === 'mode-move') {
        this.setMode('translate');
      } else if (act === 'mode-turn') {
        this.setMode('rotate');
      }
    });
  }

  async send(request, then) {
    try {
      const answer = await post(request);
      if (!answer.ok) this.ctx.toast(tr(answer.error) || t('요청을 처리하지 못했습니다'));
      else then?.(answer);
    } catch (err) {
      this.ctx.toast(t('KYNEX에 요청하지 못했습니다: {error}', { error: err.message }));
    }
  }

  // The selected equipment: the item selected and the group with it.
  groupItems() {
    const items = [];
    const add = (item) => item && !items.some((i) => i.id === item.id) && items.push(item);
    add(this.selectedItem());
    for (const root of this.ctx.group()) add(this.library?.items.find((i) => i.root === root));
    return items;
  }

  // The group changed: the inspector says so.
  groupChanged() {
    this.markGroup();
    // Several turn about Z only.
    if (this.mode === 'rotate') this.handle.showX = this.handle.showY = this.groupItems().length <= 1;
    const s = this.ctx.selected();
    if (s != null) this.showFor(s);
  }

  // The list's placed equipment: those of the group marked.
  markGroup() {
    const ids = new Set(this.ctx.group().size ? this.groupItems().map((i) => i.id) : []);
    for (const li of this.list.querySelectorAll('[data-item]')) li.classList.toggle('grouped', ids.has(Number(li.dataset.item)));
  }

  // The group moved as the handle's item has from where the drag began:
  // carried along, and turned about it.
  moveGroup(at, yaw) {
    const from = this.dragFrom;
    const turn = yaw - from.yaw;
    const [c, s] = [Math.cos(turn), Math.sin(turn)];
    const moves = from.items.map((i) => {
      const [dx, dy, dz] = [i.at[0] - from.at[0], i.at[1] - from.at[1], i.at[2] - from.at[2]];
      return { item: i.id, at: [at[0] + c * dx - s * dy, at[1] + s * dx + c * dy, at[2] + dz], yaw: i.yaw + turn };
    });
    this.move(null, null, null, { op: 'move_many', moves });
  }

  // A CAD or mesh file sent to KYNEX: kept as equipment of the user's own
  // and placed at the origin, then selected.
  async importModel(file) {
    const ext = file.name.split('.').pop().toLowerCase();
    if (!['step', 'stp', 'stl', 'obj', 'glb', 'gltf', '3mf', 'ply', 'fbx', 'zip', 'urdf'].includes(ext)) {
      return this.ctx.toast(t('읽을 수 있는 파일: STEP, STL, OBJ, FBX, glTF·GLB, 3MF, PLY · 로봇은 URDF 폴더를 묶은 ZIP이나 URDF'));
    }
    if (file.size > 256 * 1024 * 1024) return this.ctx.toast(t('파일이 너무 큽니다 (256 MB까지)'));
    const unit = document.getElementById('import-unit').value;
    const keep = document.getElementById('import-keep').checked;
    const query = new URLSearchParams({ name: file.name, ...(unit ? { unit } : {}), ...(keep ? { keep: '1' } : {}) });
    this.ctx.toast(t('{name}을(를) 읽는 중…', { name: file.name }));
    try {
      const response = await fetch(`/import?${query}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/octet-stream' },
        body: file,
      });
      if (!response.ok) throw new Error(`HTTP ${response.status} ${await response.text()}`);
      const a = await response.json();
      if (!a.ok) return this.ctx.toast(t('가져오지 못했습니다: {error}', { error: tr(a.error) }));
      const robot = ext === 'zip' || ext === 'urdf';
      const where = keep && !robot ? t('파일 안의 위치에') : t('원점에');
      const notes = a.notes?.length ? t(' (참고 {n}건)', { n: a.notes.length }) : '';
      this.ctx.toast(t('"{name}"을(를) {kind}으로 가져와 {where} 놓았습니다{notes}', { name: tr(a.name), kind: robot ? t('내 로봇') : t('내 장비'), where, notes }));
      const item = a.item;
      const root = () => this.itemById(item)?.root;
      const pick = (tries = 40) => (root() && this.ctx.records.has(root()) ? this.ctx.select(root(), { frame: true }) : tries > 0 && setTimeout(() => pick(tries - 1), 50));
      pick();
    } catch (err) {
      this.ctx.toast(t('가져오지 못했습니다: {error}', { error: err.message }));
    }
  }

  // The handle's steps: the grid it moves on, the angle it turns by. Kept
  // for this browser.
  applySnap() {
    this.handle.setTranslationSnap(this.snap.grid ? this.snap.grid / 1000 : null);
    // Turned freely under the pointer; the angle sent is snapped, from 0°
    // (`snapAngle`), not by steps from where it stood.
    this.handle.setRotationSnap(null);
    try {
      localStorage.setItem('kynex.snap', JSON.stringify(this.snap));
    } catch {}
  }

  // An angle, radians, onto the angle step if there is one: a multiple of
  // it from 0°.
  snapAngle(a) {
    const step = this.snap.angle ? rad(this.snap.angle) : 0;
    return step ? Math.round(a / step) * step : a;
  }

  // A point placed on: on the grid, if the handle moves on one.
  onGrid(p) {
    const g = this.snap.grid / 1000;
    return g ? [Math.round(p.x / g) * g, Math.round(p.y / g) * g, p.z] : [p.x, p.y, p.z];
  }

  // The group lined up by its boxes: to the selected item's side or
  // middle, one against the next, or spaced evenly. One edit.
  align(kind) {
    const [axisName, how] = kind.split('-');
    const axis = { x: 'x', y: 'y', z: 'z' }[axisName];
    const primary = this.selectedItem();
    const items = this.groupItems();
    if (!primary || items.length < 2) return;
    const boxes = new Map(items.map((i) => [i.id, this.ctx.boxUnder(i.root)]));
    if ([...boxes.values()].some((b) => b.isEmpty())) return this.ctx.toast(t('모양이 아직 없는 장비가 있습니다'));
    const mid = (b) => (b.min[axis] + b.max[axis]) / 2;
    const shift = new Map();
    if (how === 'min' || how === 'mid' || how === 'max') {
      const of = (b) => (how === 'mid' ? mid(b) : b[how][axis]);
      const to = of(boxes.get(primary.id));
      for (const i of items) shift.set(i.id, to - of(boxes.get(i.id)));
    } else {
      const sorted = [...items].sort((a, b) => mid(boxes.get(a.id)) - mid(boxes.get(b.id)));
      const size = (i) => boxes.get(i.id).max[axis] - boxes.get(i.id).min[axis];
      let gap = 0;
      if (how === 'spread') {
        if (items.length < 3) return this.ctx.toast(t('간격 맞추기는 장비 3개 이상일 때 합니다'));
        const first = boxes.get(sorted[0].id);
        const last = boxes.get(sorted[sorted.length - 1].id);
        const used = sorted.reduce((s, i) => s + size(i), 0);
        gap = (last.max[axis] - first.min[axis] - used) / (sorted.length - 1);
      }
      // The first stays; each next starts where the one before ends.
      let end = boxes.get(sorted[0].id).max[axis];
      for (const i of sorted.slice(1)) {
        const b = boxes.get(i.id);
        const d = end + gap - b.min[axis];
        shift.set(i.id, d);
        end = b.max[axis] + d;
      }
    }
    const k = { x: 0, y: 1, z: 2 }[axis];
    const moves = items
      .filter((i) => Math.abs(shift.get(i.id) ?? 0) > 1e-9)
      .map((i) => {
        const at = [...i.at];
        at[k] += shift.get(i.id);
        return { item: i.id, at };
      });
    if (!moves.length) return this.ctx.toast(t('이미 그렇게 놓여 있습니다'));
    this.send({ op: 'move_many', moves }, () => this.ctx.toast(t('장비 {n}개를 옮겼습니다', { n: moves.length })));
  }

  // Delete: the selected equipment, all of it. Whether there was any.
  removeSelected() {
    const items = this.groupItems();
    if (!items.length) return false;
    this.showHandle(null);
    this.send(items.length > 1 ? { op: 'remove', items: items.map((i) => i.id) } : { op: 'remove', item: items[0].id });
    this.ctx.select(null);
    return true;
  }

  // Ctrl+D: the selected equipment copied beside itself, the copies
  // selected in its place.
  duplicate(ids = this.groupItems().map((i) => i.id), offset = [0.5, 0.5, 0]) {
    if (!ids.length) return this.ctx.toast(t('복제할 장비를 고르세요 (라이브러리에서 놓은 장비)'));
    this.send({ op: 'duplicate', items: ids, offset }, (a) => {
      const roots = a.made.map((m) => m.root);
      this.ctx.toast(t('장비 {n}개를 복제했습니다', { n: roots.length }));
      this.selectWhenHere(roots);
    });
  }

  // The copies selected in place of what they copy, once the page has
  // them (the scene and the library come in their own time).
  selectWhenHere(roots, tries = 40) {
    const here = roots.every((root) => this.ctx.records.has(root) && this.library?.items.some((i) => i.root === root));
    if (!here) {
      if (tries > 0) setTimeout(() => this.selectWhenHere(roots, tries - 1), 50);
      return;
    }
    const handled = this.handleItem != null;
    this.ctx.select(roots[0], { keep: roots.length > 1 ? roots : [] });
    if (handled) this.showHandle(this.itemOf(roots[0])?.id ?? null);
  }

  // Ctrl+C: kept to paste. Whether there was anything to keep.
  copy() {
    const ids = this.groupItems().map((i) => i.id);
    if (!ids.length) return false;
    this.clipboard = { ids, pasted: 0 };
    this.ctx.toast(t('장비 {n}개를 복사했습니다 (Ctrl+V로 붙여넣기)', { n: ids.length }));
    return true;
  }

  // Ctrl+V: what was copied, again, each paste a step further along.
  paste() {
    const clip = this.clipboard;
    if (!clip) return this.ctx.toast(t('복사한 장비가 없습니다 (Ctrl+C)'));
    const ids = clip.ids.filter((id) => this.itemById(id));
    if (!ids.length) return this.ctx.toast(t('복사한 장비가 지워졌습니다'));
    clip.pasted += 1;
    this.duplicate(ids, [0.5 * clip.pasted, 0.5 * clip.pasted, 0]);
  }

  async move(item, at, yaw, request = { op: 'move', item, at, yaw }) {
    if (this.moving.busy) {
      this.moving.next = request;
      return;
    }
    this.moving.busy = true;
    let next = request;
    while (next) {
      this.moving.next = null;
      await this.send(next);
      next = this.moving.next;
    }
    this.moving.busy = false;
  }

  // Whether the pointer is the library's: a placing click or the handle.
  ownsPointer() {
    return this.dragging || this.handle.axis !== null || performance.now() - this.draggedAt < 250;
  }

  // A click on the scene, while a template is picked: placed there.
  clicked(x, y) {
    if (!this.armed) return false;
    this.placeAt(this.armed, x, y);
    this.arm(null);
    return true;
  }

  arm(key) {
    this.armed = key;
    for (const el of this.list.querySelectorAll('[data-template]')) el.classList.toggle('armed', el.dataset.template === key);
    this.ctx.canvas.classList.toggle('placing', !!key);
    if (key) this.ctx.toast(t('장면을 눌러 놓을 자리를 고르세요'));
  }

  placeAt(key, x, y) {
    const hit = this.ctx.pick(x, y);
    if (!hit) return;
    // Placed unturned, whatever it is: turned afterwards in the inspector.
    const tpl = this.library.templates.find((x) => x.key === key);
    // In English, the new equipment is named in English too.
    const named = getLang() === 'en' && tpl?.name ? { name: tr(tpl.name) } : {};
    this.send({ op: 'place', template: key, ...named, at: this.onGrid(hit), yaw: 0, snap: !this.alt, settle: !this.alt }, (a) =>
      this.ctx.toast(t('{name}을(를) 놓았습니다', { name: tr(tpl?.name ?? key) }) + (a.snapped ? t(' · {name}에 맞붙였습니다', { name: tr(a.snapped) }) : '')),
    );
  }

  saySnapped(answer) {
    if (answer.snapped) this.ctx.toast(t('{name}에 맞붙였습니다', { name: tr(answer.snapped) }));
  }

  itemById(id) {
    return this.library.items.find((i) => i.id === id);
  }

  // The placed item `uid` belongs to: the object itself or any of its
  // ancestors is an item's root.
  itemOf(uid) {
    for (let at = uid, n = 0; at != null && n < 64; at = this.ctx.records.get(at)?.parent, n++) {
      const item = this.library.items.find((i) => i.root === at);
      if (item) return item;
    }
    return null;
  }

  selectedItem() {
    const s = this.ctx.selected();
    return s == null ? null : this.itemOf(s);
  }

  // The `library` event.
  setLibrary(library) {
    this.library = library;
    // Hidden marks kept only for the same items: by id and name.
    let stale = false;
    for (const [id, name] of this.hidden) {
      const item = library.items.find((i) => i.id === id);
      if (!item || item.name !== name) {
        this.hidden.delete(id);
        stale = true;
      }
    }
    // Shown again, all; then those still hidden hidden again.
    if (stale) {
      this.ctx.unhideAll();
      for (const id of this.hidden.keys()) {
        const item = library.items.find((i) => i.id === id);
        if (item) this.ctx.hideUnder(item.root, true);
      }
    }
    this.renderList();
    if (this.handleItem != null && !this.itemById(this.handleItem)?.root) this.showHandle(null);
    // Locked, or just placed and selected: the gizmo as the selection says.
    this.selectionChanged(this.ctx.selected());
    this.followHandle();
    const s = this.ctx.selected();
    if (s != null) this.showFor(s);
  }

  // The `templates` event: what each template looks like.
  setThumbnails(templates) {
    this.thumbs = drawThumbnails(templates);
    this.renderList();
  }

  renderList() {
    const { escapeHtml } = this.ctx;
    const groups = new Map();
    for (const tpl of this.library.templates) {
      if (!groups.has(tpl.group)) groups.set(tpl.group, []);
      groups.get(tpl.group).push(tpl);
    }
    let html = '';
    for (const [group, list] of groups) {
      html += `<li class="lib-group">${escapeHtml(tr(group))}</li><li class="lib-grid">`;
      for (const tpl of list) {
        const thumb = this.thumbs.get(tpl.key);
        html += `<div class="lib-template${tpl.key === this.armed ? ' armed' : ''}" draggable="true" data-template="${escapeHtml(tpl.key)}" title="${escapeHtml(tr(tpl.name))} · ${escapeHtml(t('장면으로 끌어 놓거나, 누른 뒤 장면을 누르세요'))}">
          ${thumb ? `<img class="lib-thumb" src="${thumb}" alt="" draggable="false">` : '<span class="lib-thumb"></span>'}
          <span class="lib-name">${escapeHtml(tr(tpl.name))}</span>
          ${tpl.custom ? `<button class="lib-del" data-delete-custom="${escapeHtml(tpl.key)}" title="${escapeHtml(t('내 장비에서 지우기'))}">✕</button>` : ''}
        </div>`;
      }
      html += '</li>';
    }
    if (this.library.items.length) {
      html += `<li class="lib-group">${escapeHtml(t('배치된 장비'))}</li>`;
      for (const i of this.library.items) {
        const marks = `${i.locked ? '🔒 ' : ''}${this.hidden.has(i.id) ? '◌ ' : ''}`;
        html += `<li class="lib-item${this.hidden.has(i.id) ? ' hidden-item' : ''}" data-item="${i.id}">${marks}${escapeHtml(tr(i.name))}</li>`;
      }
    }
    this.list.innerHTML = html;
    this.markGroup();
  }

  // The selected object's item, in the inspector.
  showFor(uid) {
    const { escapeHtml } = this.ctx;
    const item = this.itemOf(uid);
    this.box.hidden = !item;
    if (!item) return;
    const tpl = this.library.templates.find((x) => x.key === item.template);
    const params = (tpl?.params || [])
      .map((p) => {
        const v = item.params[p.key] ?? p.default;
        if (p.choices?.length) {
          const options = p.choices.map((name, i) => `<option value="${i}"${i === Math.round(v) ? ' selected' : ''}>${escapeHtml(tr(name))}</option>`).join('');
          return `<label class="insp-field">${escapeHtml(tr(p.label))}<select data-param="${p.key}" data-choice>${options}</select></label>`;
        }
        // A length in millimetres; a time, an angle or a count as it is.
        if (p.unit && p.unit !== 'm') {
          // A time, a count or a speed is the cell role's too: a beginner
          // sets it there, once.
          const min = ['s', '개', 'm/s'].includes(p.unit) ? '' : '';
          return `<label class="insp-field"${min}>${escapeHtml(tr(p.label))}<input type="number" class="num" data-param="${p.key}" data-scale="1" value="${Math.round(v * 100) / 100}" min="${p.min}" max="${p.max}" step="${p.unit === '개' ? 1 : p.unit === '°' ? 15 : 1}"><span>${escapeHtml(tr(p.unit))}</span></label>`;
        }
        return `<label class="insp-field">${escapeHtml(tr(p.label))}<input type="number" class="num" data-param="${p.key}" value="${Math.round(v * 1000)}" min="${Math.round(p.min * 1000)}" max="${Math.round(p.max * 1000)}" step="10"><span>mm</span></label>`;
      })
      .join('');
    const [x, y, z] = item.at.map((v) => Math.round(v * 1000));
    const many = this.groupItems().length;
    const lock = item.locked ? ' disabled' : '';
    const hidden = this.hidden.has(item.id);
    this.box.innerHTML = `
      <div class="insp-item-head"><span class="insp-kind">${t('장비')}</span><input class="insp-name" data-rename value="${escapeHtml(tr(item.name))}" maxlength="60" spellcheck="false" title="${t('이름 바꾸기 (프로그램은 이 이름으로 장비를 찾습니다)')}">
        <button class="icon-mini${item.locked ? ' on' : ''}" data-item-act="lock" title="${item.locked ? t('잠금 풀기') : t('잠그기: 옮기거나 지울 수 없게')}">${item.locked ? '🔒' : '🔓'}</button>
        <button class="icon-mini${hidden ? ' on' : ''}" data-item-act="hide" title="${hidden ? t('다시 보이기') : t('이 화면에서만 숨기기')}">${hidden ? '◌' : '◉'}</button></div>
      <fieldset class="insp-lockable"${lock}>
      <div class="insp-props">
        <label class="insp-field">X<input type="number" class="num" data-at="0" value="${x}" step="10"><span>mm</span></label>
        <label class="insp-field">Y<input type="number" class="num" data-at="1" value="${y}" step="10"><span>mm</span></label>
        <label class="insp-field">Z<input type="number" class="num" data-at="2" value="${z}" step="10"><span>mm</span></label>
        <label class="insp-field">${t('회전')}<input type="number" class="num" data-yaw value="${Math.round(deg(item.yaw))}" step="15"><span>°</span></label>
        <label class="insp-field" title="${t('X축으로 기울이기 (벽에 단 로봇은 90°, 천장에 거꾸로 단 로봇은 180°)')}">${t('X축')}<input type="number" class="num" data-tilt="roll" value="${Math.round(deg(item.roll ?? 0))}" step="15"><span>°</span></label>
        <label class="insp-field" title="${t('Y축으로 기울이기')}">${t('Y축')}<input type="number" class="num" data-tilt="pitch" value="${Math.round(deg(item.pitch ?? 0))}" step="15"><span>°</span></label>
        ${params}
        <label class="insp-field" title="${t('장비 값과 설치비: 통계·시나리오의 투자비, 감가, 개당 원가, 회수 기간에 쓰입니다')}">${t('투자비')}<input type="number" class="num" data-price value="${item.price ?? 0}" min="0" step="100"><span>${t('만원')}</span></label>
      </div>
      ${many > 1 ? `<div class="muted insp-group">${t('함께 선택: 장비 {n}개 · 핸들로 함께 옮기고 돌립니다', { n: many })}</div>
      <label class="insp-field wide insp-align">${t('정렬')}<select data-align>
        <option value="">${t('고르기…')}</option>
        <optgroup label="${escapeHtml(t('{name}에 맞추기', { name: tr(item.name) }))}">
          <option value="x-min">${t('X 왼쪽')}</option><option value="x-mid">${t('X 가운데')}</option><option value="x-max">${t('X 오른쪽')}</option>
          <option value="y-min">${t('Y 앞')}</option><option value="y-mid">${t('Y 가운데')}</option><option value="y-max">${t('Y 뒤')}</option>
          <option value="z-min">${t('바닥 높이')}</option>
        </optgroup>
        <optgroup label="${t('맞붙여 늘어놓기')}">
          <option value="x-chain">${t('X 방향으로')}</option><option value="y-chain">${t('Y 방향으로')}</option>
        </optgroup>
        <optgroup label="${t('간격 같게 (3개 이상)')}">
          <option value="x-spread">${t('X 방향')}</option><option value="y-spread">${t('Y 방향')}</option>
        </optgroup>
      </select></label>` : ''}
      <div class="insp-actions">
        <button class="text-btn${this.gizmo ? ' on' : ''}" data-item-act="handle" title="${t('장비를 고르면 기즈모(이동·회전 화살표)가 나와 마우스로 바로 끌 수 있습니다. R: 회전, T: 이동, Alt: 맞붙이지 않기')}">${this.gizmo ? t('기즈모 끄기') : t('기즈모 켜기')}</button>
        ${this.gizmo ? `<span class="seg gizmo-mode"><button data-item-act="mode-move" class="${this.mode !== 'rotate' ? 'on' : ''}" title="${t('화살표로 X·Y·Z 이동 (T)')}">${t('이동')}</button><button data-item-act="mode-turn" class="${this.mode === 'rotate' ? 'on' : ''}" title="${t('고리로 X·Y·Z축 회전 (R)')}">${t('회전')}</button></span>` : ''}
        ${this.mode === 'rotate'
          ? `<label class="insp-snap" title="${t('핸들로 돌릴 때 각도 단위: 0°부터 그 배수로')}">${t('각도 스냅')}<select data-snap="angle">${[0, 5, 15, 45, 90].map((v) => `<option value="${v}"${v === this.snap.angle ? ' selected' : ''}>${v ? `${v}°` : t('끔')}</option>`).join('')}</select></label>`
          : `<label class="insp-snap" title="${t('핸들로 옮길 때 격자 간격')}">${t('스냅')}<select data-snap="grid">${[0, 10, 50, 100, 500].map((v) => `<option value="${v}"${v === this.snap.grid ? ' selected' : ''}>${v ? `${v} mm` : t('끔')}</option>`).join('')}</select></label>`}
        <button class="text-btn quiet" data-item-act="duplicate" title="${t('옆에 복제 (Ctrl+D). 복사 Ctrl+C, 붙여넣기 Ctrl+V')}">${t('복제')}</button>
        <button class="text-btn quiet danger" data-item-act="remove" title="${t('삭제 (Delete)')}">${many > 1 ? t('{n}개 삭제', { n: many }) : t('삭제')}</button>
      </div>
      </fieldset>
      <details class="insp-array"${this.arrayOpen ? ' open' : ''}>
        <summary>${t('배열·대칭 복사')} <span class="muted">${many > 1 ? t('선택한 {n}개', { n: many }) : t('이 장비')}</span></summary>
        <div class="array-row"><span>${t('직선')}</span>
          <label>${t('개수')}<input type="number" class="num" data-array="n" value="${this.arrayVals.n ?? 3}" min="2" max="50"></label>
          <label>X<input type="number" class="num" data-array="dx" value="${this.arrayVals.dx ?? 2000}" step="100"></label>
          <label>Y<input type="number" class="num" data-array="dy" value="${this.arrayVals.dy ?? 0}" step="100"></label><span class="muted">mm</span>
          <button class="text-btn quiet" data-item-act="array-line">${t('만들기')}</button></div>
        <div class="array-row"><span>${t('원형')}</span>
          <label>${t('개수')}<input type="number" class="num" data-array="m" value="${this.arrayVals.m ?? 4}" min="2" max="50"></label>
          <label>${t('중심 X')}<input type="number" class="num" data-array="cx" value="${this.arrayVals.cx ?? 0}" step="100"></label>
          <label>Y<input type="number" class="num" data-array="cy" value="${this.arrayVals.cy ?? 0}" step="100"></label><span class="muted">mm</span>
          <button class="text-btn quiet" data-item-act="array-circle">${t('만들기')}</button></div>
        <div class="array-row"><span>${t('대칭')}</span>
          <select data-array="axis"><option value="x">${t('X 방향으로 (YZ면)')}</option><option value="y">${t('Y 방향으로 (XZ면)')}</option></select>
          <button class="text-btn quiet" data-item-act="mirror" title="${t('선택한 장비 상자의 끝에 붙여, 위치와 방향을 거울처럼 뒤집은 복사본을 놓습니다. 장비 형상 자체는 뒤집지 않습니다')}">${t('만들기')}</button></div>
      </details>
      <div class="muted insp-hint">${t('Ctrl+클릭: 여러 개 · Alt+클릭: 안의 부품')}</div>`;
  }

  // The array and mirror numbers as typed.
  arrayValue(key) {
    return Number(this.box.querySelector(`[data-array="${key}"]`)?.value);
  }

  // Copies of the selected equipment, each where `place(item, k)` puts it
  // ({at, yaw}), for k = 1 … n-1: one edit.
  placeCopies(n, place, said) {
    const items = this.groupItems();
    if (!items.length) return;
    if (!(Number.isInteger(n) && n >= 2 && n <= 50)) return this.ctx.toast(t('개수는 2–50입니다'));
    const copies = [];
    for (let k = 1; k < n; k++) for (const i of items) copies.push({ item: i.id, ...place(i, k) });
    if (copies.length > 200) return this.ctx.toast(t('한 번에 200개까지 복사합니다'));
    this.send({ op: 'duplicate', copies }, (a) => {
      this.ctx.toast(t('{what}: 장비 {n}개를 놓았습니다', { what: said, n: a.made.length }));
      this.selectWhenHere(a.made.map((m) => m.root));
    });
  }

  arrayLine() {
    const [dx, dy] = [this.arrayValue('dx') / 1000, this.arrayValue('dy') / 1000];
    if (!Number.isFinite(dx) || !Number.isFinite(dy) || (dx === 0 && dy === 0)) return this.ctx.toast(t('간격을 넣으세요'));
    this.placeCopies(this.arrayValue('n'), (i, k) => ({ at: [i.at[0] + k * dx, i.at[1] + k * dy, i.at[2]], yaw: i.yaw }), t('직선 배열'));
  }

  // Round a centre, the whole turn shared out: each copy turned with it.
  arrayCircle() {
    const n = this.arrayValue('m');
    const [cx, cy] = [this.arrayValue('cx') / 1000, this.arrayValue('cy') / 1000];
    if (!Number.isFinite(cx) || !Number.isFinite(cy)) return;
    this.placeCopies(
      n,
      (i, k) => {
        const turn = (2 * Math.PI * k) / n;
        const [x, y] = [i.at[0] - cx, i.at[1] - cy];
        return { at: [cx + x * Math.cos(turn) - y * Math.sin(turn), cy + x * Math.sin(turn) + y * Math.cos(turn), i.at[2]], yaw: i.yaw + turn };
      },
      t('원형 배열'),
    );
  }

  // Mirrored about the plane at the far side of the selection's boxes, so
  // the copies stand beside: places and headings mirrored.
  mirror() {
    const items = this.groupItems();
    if (!items.length) return;
    const axis = this.box.querySelector('[data-array="axis"]')?.value === 'y' ? 1 : 0;
    const key = axis ? 'y' : 'x';
    let edge = -Infinity;
    for (const i of items) {
      const b = this.ctx.boxUnder(i.root);
      if (b.isEmpty()) return this.ctx.toast(t('모양이 아직 없는 장비가 있습니다'));
      edge = Math.max(edge, b.max[key]);
    }
    this.placeCopies(
      2,
      (i) => {
        const at = [...i.at];
        at[axis] = 2 * edge - at[axis];
        return { at, yaw: axis ? -i.yaw : Math.PI - i.yaw };
      },
      t('대칭 복사'),
    );
  }

  edited(e) {
    const item = this.selectedItem();
    if (!item) return;
    if (e.target.dataset.array !== undefined) {
      this.arrayVals[e.target.dataset.array] = e.target.value;
      return;
    }
    if (e.target.dataset.rename !== undefined) {
      const name = e.target.value.trim();
      // Shown in the language chosen: a change is a name other than that.
      if (name && name !== tr(item.name)) {
        this.send({ op: 'rename', item: item.id, name }, () => this.ctx.toast(t('이름을 "{name}"(으)로 바꿨습니다', { name })));
        // Still hidden under its new name.
        if (this.hidden.has(item.id)) this.hidden.set(item.id, name);
      }
      else e.target.value = tr(item.name);
      return;
    }
    if (e.target.dataset.align !== undefined) {
      const kind = e.target.value;
      e.target.value = '';
      return kind && this.align(kind);
    }
    if (e.target.dataset.snap) {
      this.snap[e.target.dataset.snap] = Number(e.target.value);
      return this.applySnap();
    }
    const v = Number(e.target.value);
    if (!Number.isFinite(v)) return;
    if (e.target.dataset.tilt) {
      this.move(null, null, null, { op: 'move', item: item.id, [e.target.dataset.tilt]: rad(v) });
    } else if (e.target.dataset.at !== undefined) {
      const at = [...item.at];
      at[Number(e.target.dataset.at)] = v / 1000;
      this.move(item.id, at, item.yaw);
    } else if (e.target.dataset.yaw !== undefined) {
      this.move(item.id, item.at, rad(v));
    } else if (e.target.dataset.price !== undefined) {
      this.send({ op: 'price', item: item.id, price: Math.max(0, v) });
    } else if (e.target.dataset.param) {
      const scale = e.target.dataset.choice !== undefined || e.target.dataset.scale === '1' ? 1 : 1000;
      this.send({ op: 'resize', item: item.id, params: { [e.target.dataset.param]: v / scale } });
    }
  }

  // The selection changed: the gizmo onto the equipment selected, to drag
  // straight away — not onto locked equipment, nor when it was turned off.
  selectionChanged(uid) {
    const item = uid != null ? this.itemOf(uid) : null;
    const want = item && !item.locked && this.gizmo ? item.id : null;
    if (want !== this.handleItem) this.showHandle(want);
    // Another part of the same equipment: the gizmo onto it.
    else this.followHandle();
  }

  // The gizmo on selecting, or not; kept for this browser.
  setGizmo(on) {
    this.gizmo = on;
    try {
      localStorage.setItem('kynex.gizmo', on ? '1' : '0');
    } catch {}
    this.selectionChanged(this.ctx.selected());
    this.ctx.toast(on ? t('장비를 고르면 기즈모가 나옵니다') : t('기즈모를 껐습니다 (숫자 칸으로 옮길 수 있습니다)'));
  }

  showHandle(id) {
    this.handleItem = id;
    if (id != null) {
      this.handle.attach(this.proxy);
      this.ctx.scene.add(this.helper);
      this.followHandle();
    } else {
      this.handle.detach();
      this.helper.removeFromParent();
    }
    const s = this.ctx.selected();
    if (s != null) this.showFor(s);
  }

  // The handle onto its item, but not while it is being dragged.
  // At the equipment's origin, turned as the equipment is, whatever part of
  // it was picked: the equipment moves as one, as Visual Components moves a
  // component (its parts move in the interact mode). The origin's offset
  // from the gizmo, in its own axes, puts the origin back (`originAt`).
  followHandle() {
    const item = this.handleItem != null && this.itemById(this.handleItem);
    if (!item || this.dragging) return;
    const turn = new THREE.Quaternion().setFromEuler(new THREE.Euler(item.roll ?? 0, item.pitch ?? 0, item.yaw, 'ZYX'));
    const at = new THREE.Vector3().fromArray(item.at);
    const pivot = at.clone();
    this.offset = pivot.clone().sub(at).applyQuaternion(turn.clone().invert());
    this.proxy.position.copy(pivot);
    this.proxy.quaternion.copy(turn);
    this.proxy.updateMatrixWorld();
  }

  // Where the equipment's origin is, the gizmo where it is now.
  originAt() {
    const off = (this.offset ?? new THREE.Vector3()).clone().applyQuaternion(this.proxy.quaternion);
    return this.proxy.position.clone().sub(off).toArray();
  }

  // Keys while the gizmo is on: R turns, T moves.
  key(k) {
    if (this.handleItem == null) return false;
    if (k === 'r') this.setMode('rotate');
    else if (k === 't') this.setMode('translate');
    else return false;
    return true;
  }

  // The gizmo moving (arrows along X, Y, Z) or turning (rings about X, Y,
  // Z); several selected together turn about Z only.
  setMode(mode) {
    this.handle.setMode(mode);
    this.mode = mode;
    this.handle.showX = this.handle.showY = mode !== 'rotate' || this.groupItems().length <= 1;
    const s = this.ctx.selected();
    if (s != null) this.showFor(s);
  }
}
