// The site's drawing (the `floor` event): a plan of the factory floor laid
// under the scene at its true size, and the panel that loads it, sizes it,
// places it and takes it away.
import * as THREE from 'three';
import { tFor, tr } from './i18n.js';

const t = tFor('page');

async function post(request) {
  const response = await fetch('/robot', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

const FIELDS = ['width', 'x', 'y', 'yaw', 'opacity'];

export class FloorPanel {
  // ctx: { scene, toast, redraw(), frame(box) }
  constructor(ctx) {
    this.ctx = ctx;
    this.panel = document.getElementById('floor-panel');
    this.state = {};
    this.mesh = null;
    this.image = null;
    this.file = document.getElementById('floor-file');
    document.getElementById('floor-load').addEventListener('click', () => this.file.click());
    this.file.addEventListener('change', () => this.load());
    document.getElementById('floor-clear').addEventListener('click', async () => {
      if (!confirm(t('부지 도면을 지웁니다.'))) return;
      await this.send({ op: 'floor', action: 'clear' });
    });
    for (const k of FIELDS) {
      document.getElementById(`floor-${k}`).addEventListener('change', () => this.place());
    }
  }

  toggle() {
    this.panel.hidden = !this.panel.hidden;
  }

  async send(request) {
    try {
      const a = await post(request);
      if (!a.ok) this.ctx.toast(tr(a.error) || t('도면을 바꾸지 못했습니다'));
      return a.ok;
    } catch (err) {
      this.ctx.toast(t('KYNEX에 요청하지 못했습니다: {error}', { error: err.message }));
      return false;
    }
  }

  // An image file to the drawing: through the import route, it may be
  // megabytes.
  async load() {
    const file = this.file.files?.[0];
    this.file.value = '';
    if (!file) return;
    if (file.size > 8 * 1024 * 1024) return this.ctx.toast(t('도면 이미지는 8 MB까지입니다'));
    try {
      const response = await fetch(`/import?as=floor&name=${encodeURIComponent(file.name)}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/octet-stream' },
        body: await file.arrayBuffer(),
      });
      const a = await response.json();
      if (!a.ok) return this.ctx.toast(tr(a.error) || t('도면을 불러오지 못했습니다'));
      this.framePending = true;
      this.ctx.toast(t('도면을 깔았습니다. 실제 가로 길이(m)를 넣어 크기를 맞추세요'));
    } catch (err) {
      this.ctx.toast(t('도면을 보내지 못했습니다: {error}', { error: err.message }));
    }
  }

  place() {
    const v = (k) => Number(document.getElementById(`floor-${k}`).value);
    const req = { op: 'floor', action: 'set' };
    for (const k of FIELDS) {
      const x = v(k);
      if (Number.isFinite(x)) req[k] = k === 'opacity' ? x / 100 : x;
    }
    this.send(req);
  }

  // The `floor` event.
  setState(state) {
    this.state = state;
    const has = !!state.image;
    document.getElementById('floor-fields').hidden = !has;
    document.getElementById('floor-clear').hidden = !has;
    document.getElementById('floor-load').textContent = has ? t('다른 도면 불러오기') : t('도면 이미지 불러오기');
    document.querySelector('[data-action="floor"]')?.classList.toggle('on', has);
    if (has) {
      const set = (k, x) => {
        const el = document.getElementById(`floor-${k}`);
        if (document.activeElement !== el) el.value = Math.round(x * 100) / 100;
      };
      set('width', state.width);
      set('x', state.x);
      set('y', state.y);
      set('yaw', state.yaw);
      set('opacity', state.opacity * 100);
    }
    this.draw();
  }

  // The plane under the scene: the image at its width, its height as the
  // image is, beneath the grid.
  draw() {
    const s = this.state;
    if (!s.image) {
      if (this.mesh) {
        this.ctx.scene.remove(this.mesh);
        this.mesh.geometry.dispose();
        this.mesh.material.map?.dispose();
        this.mesh.material.dispose();
        this.mesh = null;
        this.image = null;
      }
      this.ctx.redraw();
      return;
    }
    const lay = (texture) => {
      const img = texture.image;
      const h = (s.width * img.height) / Math.max(1, img.width);
      if (!this.mesh) {
        const material = new THREE.MeshBasicMaterial({ map: texture, transparent: true, depthWrite: false });
        this.mesh = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), material);
        this.mesh.renderOrder = -2;
        this.mesh.name = 'floor-drawing';
        this.ctx.scene.add(this.mesh);
      } else if (this.mesh.material.map !== texture) {
        this.mesh.material.map?.dispose();
        this.mesh.material.map = texture;
        this.mesh.material.needsUpdate = true;
      }
      this.mesh.scale.set(s.width, h, 1);
      this.mesh.position.set(s.x, s.y, -0.002);
      this.mesh.rotation.set(0, 0, (s.yaw * Math.PI) / 180);
      this.mesh.material.opacity = s.opacity;
      this.ctx.redraw();
      if (this.framePending) {
        this.framePending = false;
        this.ctx.frame(new THREE.Box3().setFromObject(this.mesh));
      }
    };
    if (this.image === s.image && this.mesh) {
      lay(this.mesh.material.map);
      return;
    }
    this.image = s.image;
    new THREE.TextureLoader().load(s.image, (texture) => {
      texture.colorSpace = THREE.SRGBColorSpace;
      texture.anisotropy = 8;
      lay(texture);
    });
  }
}
