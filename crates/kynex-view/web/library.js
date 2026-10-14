// The library: equipment to place in the cell — robots, CNC machines,
// pallet tables, pedestals, fences — dragged from the list onto the scene
// (or picked, then placed with a click), then moved, turned and resized.
// KYNEX makes each piece from its template and adds it to the scene it
// shows (the `library` event); nothing goes to Kreate.

import * as THREE from 'three';
import { TransformControls } from '/vendor/TransformControls.js';

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
    this.handle.setSpace('world');
    this.helper = this.handle.getHelper();
    this.handleItem = null;
    this.dragging = false;
    this.draggedAt = 0;
    this.handle.addEventListener('dragging-changed', (e) => {
      ctx.controls.enabled = !e.value;
      this.dragging = e.value;
      if (!e.value) this.draggedAt = performance.now();
    });
    this.handle.addEventListener('objectChange', () => {
      if (!this.dragging || this.handleItem == null) return;
      // Turned about Z only: the floor stays the floor.
      const e = new THREE.Euler().setFromQuaternion(this.proxy.quaternion, 'ZYX');
      this.proxy.quaternion.setFromEuler(new THREE.Euler(0, 0, e.z, 'ZYX'));
      this.move(this.handleItem, this.proxy.position.toArray(), e.z);
    });

    this.list.addEventListener('dragstart', (e) => {
      const key = e.target.closest('[data-template]')?.dataset.template;
      if (key) e.dataTransfer.setData('text/kynex-template', key);
    });
    this.list.addEventListener('click', (e) => {
      const t = e.target.closest('[data-template]');
      if (t) return this.arm(this.armed === t.dataset.template ? null : t.dataset.template);
      const item = e.target.closest('[data-item]');
      if (item) this.ctx.select(this.itemById(Number(item.dataset.item))?.root, { frame: true });
    });
    ctx.canvas.addEventListener('dragover', (e) => {
      if (e.dataTransfer.types.includes('text/kynex-template')) e.preventDefault();
    });
    ctx.canvas.addEventListener('drop', (e) => {
      const key = e.dataTransfer.getData('text/kynex-template');
      if (!key) return;
      e.preventDefault();
      this.placeAt(key, e.clientX, e.clientY);
    });
    this.box.addEventListener('change', (e) => this.edited(e));
    this.box.addEventListener('click', (e) => {
      const act = e.target.closest('[data-item-act]')?.dataset.itemAct;
      const item = this.selectedItem();
      if (!item) return;
      if (act === 'remove') {
        this.showHandle(null);
        this.send({ op: 'remove', item: item.id });
        this.ctx.select(null);
      } else if (act === 'handle') {
        this.showHandle(this.handleItem === item.id ? null : item.id);
      }
    });
  }

  async send(request, then) {
    try {
      const answer = await post(request);
      if (!answer.ok) this.ctx.toast(answer.error || '요청을 처리하지 못했습니다');
      else then?.(answer);
    } catch (err) {
      this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
    }
  }

  async move(item, at, yaw) {
    const request = { op: 'move', item, at, yaw };
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
    if (key) this.ctx.toast('장면을 눌러 놓을 자리를 고르세요');
  }

  placeAt(key, x, y) {
    const hit = this.ctx.pick(x, y);
    if (!hit) return;
    // A machine's front (its −Y) turned to face the origin, where the robot
    // usually stands: (sin ψ, −cos ψ) along (−x, −y).
    const t = this.library.templates.find((t) => t.key === key);
    const yaw = t?.group === '공작기계' ? Math.atan2(-hit.x, hit.y) : 0;
    this.send({ op: 'place', template: key, at: [hit.x, hit.y, hit.z], yaw }, () =>
      this.ctx.toast(`${t?.name ?? key}을(를) 놓았습니다`),
    );
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
    this.renderList();
    if (this.handleItem != null && !this.itemById(this.handleItem)) this.showHandle(null);
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
    for (const t of this.library.templates) {
      if (!groups.has(t.group)) groups.set(t.group, []);
      groups.get(t.group).push(t);
    }
    let html = '';
    for (const [group, list] of groups) {
      html += `<li class="lib-group">${escapeHtml(group)}</li><li class="lib-grid">`;
      for (const t of list) {
        const thumb = this.thumbs.get(t.key);
        html += `<div class="lib-template${t.key === this.armed ? ' armed' : ''}" draggable="true" data-template="${escapeHtml(t.key)}" title="${escapeHtml(t.name)} · 장면으로 끌어 놓거나, 누른 뒤 장면을 누르세요">
          ${thumb ? `<img class="lib-thumb" src="${thumb}" alt="" draggable="false">` : '<span class="lib-thumb"></span>'}
          <span class="lib-name">${escapeHtml(t.name)}</span>
        </div>`;
      }
      html += '</li>';
    }
    if (this.library.items.length) {
      html += '<li class="lib-group">배치된 장비</li>';
      for (const i of this.library.items) {
        html += `<li class="lib-item" data-item="${i.id}">${escapeHtml(i.name)}</li>`;
      }
    }
    this.list.innerHTML = html;
  }

  // The selected object's item, in the inspector.
  showFor(uid) {
    const { escapeHtml } = this.ctx;
    const item = this.itemOf(uid);
    this.box.hidden = !item;
    if (!item) return;
    const t = this.library.templates.find((t) => t.key === item.template);
    const params = (t?.params || [])
      .map((p) => {
        const v = item.params[p.key] ?? p.default;
        return `<label class="insp-field">${escapeHtml(p.label)}<input type="number" class="num" data-param="${p.key}" value="${Math.round(v * 1000)}" min="${Math.round(p.min * 1000)}" max="${Math.round(p.max * 1000)}" step="10"><span>mm</span></label>`;
      })
      .join('');
    const [x, y, z] = item.at.map((v) => Math.round(v * 1000));
    this.box.innerHTML = `
      <div class="insp-item-head"><span class="insp-kind">장비</span><span>${escapeHtml(item.name)}</span></div>
      <div class="insp-props">
        <label class="insp-field">X<input type="number" class="num" data-at="0" value="${x}" step="10"><span>mm</span></label>
        <label class="insp-field">Y<input type="number" class="num" data-at="1" value="${y}" step="10"><span>mm</span></label>
        <label class="insp-field">Z<input type="number" class="num" data-at="2" value="${z}" step="10"><span>mm</span></label>
        <label class="insp-field">회전<input type="number" class="num" data-yaw value="${Math.round(deg(item.yaw))}" step="15"><span>°</span></label>
        ${params}
      </div>
      <div class="insp-actions">
        <button class="text-btn" data-item-act="handle">${this.handleItem === item.id ? '핸들 끄기' : '이동 핸들'}</button>
        <button class="text-btn quiet danger" data-item-act="remove">삭제</button>
      </div>`;
  }

  edited(e) {
    const item = this.selectedItem();
    if (!item) return;
    const v = Number(e.target.value);
    if (!Number.isFinite(v)) return;
    if (e.target.dataset.at !== undefined) {
      const at = [...item.at];
      at[Number(e.target.dataset.at)] = v / 1000;
      this.move(item.id, at, item.yaw);
    } else if (e.target.dataset.yaw !== undefined) {
      this.move(item.id, item.at, rad(v));
    } else if (e.target.dataset.param) {
      this.send({ op: 'resize', item: item.id, params: { [e.target.dataset.param]: v / 1000 } });
    }
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
  followHandle() {
    const item = this.handleItem != null && this.itemById(this.handleItem);
    if (!item || this.dragging) return;
    this.proxy.position.fromArray(item.at);
    this.proxy.quaternion.setFromEuler(new THREE.Euler(0, 0, item.yaw, 'ZYX'));
    this.proxy.updateMatrixWorld();
  }

  // Keys while the handle is on: R turns, T moves.
  key(k) {
    if (this.handleItem == null) return false;
    if (k === 'r') this.handle.setMode('rotate');
    else if (k === 't') this.handle.setMode('translate');
    else return false;
    this.handle.showX = this.handle.showY = k !== 'r';
    return true;
  }
}
