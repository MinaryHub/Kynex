// The cell: what each object is to the process — a CNC machine, a pallet,
// a gripper, a fence — set in the inspector, and the places parts stand,
// marked in the scene. KYNEX keeps the roles (the `cell` event) and the
// parts, which arrive in the scene like any other object.

import * as THREE from 'three';

const KINDS = [
  ['', '없음'],
  ['cnc', 'CNC 장비'],
  ['infeed', '투입 팔레트'],
  ['outfeed', '배출 팔레트'],
  ['gripper', '그리퍼'],
  ['fence', '안전 펜스'],
];

// Each kind's settings, as the inspector shows them: key, label, unit, and
// how the shown number maps to KYNEX's.
const FIELDS = {
  cnc: [
    ['cycle', '가공 시간', 's', 1],
    ['door', '문 개폐', 's', 1],
    ['chuck', '척 개폐', 's', 1],
  ],
  infeed: [
    ['rows', '행', '', 1],
    ['cols', '열', '', 1],
    ['pitch', '간격', 'mm', 1000],
  ],
  gripper: [['actuate', '개폐 시간', 's', 1]],
};
FIELDS.outfeed = FIELDS.infeed;

async function post(request) {
  const response = await fetch('/robot', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

export class CellPanel {
  // ctx: { scene, selected(), toast, escapeHtml }
  constructor(ctx) {
    this.ctx = ctx;
    this.cell = { components: [], parts: [] };
    this.markers = new THREE.Group();
    ctx.scene.add(this.markers);
    this.box = document.getElementById('insp-cell');
    this.kind = document.getElementById('cell-kind');
    this.kind.innerHTML = KINDS.map(([v, label]) => `<option value="${v}">${label}</option>`).join('');
    this.kind.addEventListener('change', () => this.setRole());
    this.box.addEventListener('change', (e) => {
      if (e.target.dataset.prop) this.setRole();
    });
    this.box.addEventListener('click', (e) => {
      const act = e.target.closest('[data-cell]')?.dataset.cell;
      if (act) this.send({ op: act, object: this.ctx.selected() });
      const action = e.target.closest('[data-machine]')?.dataset.machine;
      if (action) this.send({ op: 'machine', object: this.ctx.selected(), action });
    });
    // What the inspector last drew, so that a running machine's progress
    // does not redraw the fields under the pointer.
    this.drawn = { props: '', actions: '' };
    document.getElementById('cell-auto').addEventListener('click', () =>
      this.send({ op: 'auto' }, (a) => this.ctx.toast(a.added ? `셀 역할 ${a.added}개를 이름으로 정했습니다` : '이름으로 정할 역할이 없습니다')),
    );
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

  componentOf(uid) {
    return this.cell.components.find((c) => c.object === uid);
  }

  // The `cell` event.
  setCell(cell) {
    this.cell = cell;
    this.drawMarkers();
    const selected = this.ctx.selected();
    if (selected != null) this.showFor(selected);
  }

  // The selected object's role, in the inspector.
  showFor(uid) {
    const { escapeHtml } = this.ctx;
    const c = this.componentOf(uid);
    if (document.activeElement !== this.kind) this.kind.value = c?.kind ?? '';
    const fields = (c && FIELDS[c.kind]) || [];
    const propsKey = `${uid}|${c?.kind}|${JSON.stringify(c?.props)}`;
    if (propsKey !== this.drawn.props) {
      this.drawn.props = propsKey;
      document.getElementById('cell-props').innerHTML = this.fieldsHtml(c, fields);
    }
    const m = c?.machine;
    const state = document.getElementById('cell-state');
    state.innerHTML = m ? this.machineHtml(m) : '';
    let actions = '';
    if (c && (c.kind === 'infeed' || c.kind === 'outfeed')) {
      actions = `<button class="text-btn" data-cell="fill">${c.kind === 'infeed' ? '소재 채우기' : '완성품 채우기'}</button><button class="text-btn quiet" data-cell="empty">비우기</button><span class="muted">${c.slots.length}칸</span>`;
    } else if (c && c.kind === 'cnc') {
      if (!c.seat) {
        actions = `<span class="warn-text">${escapeHtml('하위 물체에 chuck이 없습니다')}</span>`;
      } else {
        const busy = m?.progress != null;
        actions =
          `<button class="text-btn" data-machine="${m?.opening ? 'close' : 'open'}"${busy ? ' disabled' : ''}>${m?.opening ? '문 닫기' : '문 열기'}</button>` +
          `<button class="text-btn" data-machine="${m?.clamped ? 'unclamp' : 'clamp'}"${busy ? ' disabled' : ''}>${m?.clamped ? '척 풀기' : '척 물림'}</button>` +
          `<button class="text-btn" data-machine="start"${busy ? ' disabled' : ''}>가공 시작</button>` +
          `<button class="text-btn quiet" data-cell="empty"${busy || m?.clamped ? ' disabled' : ''}>척 비우기</button>`;
      }
    } else if (c && c.kind === 'gripper' && !c.robot) {
      actions = '<span class="warn-text">로봇의 링크가 아닙니다</span>';
    }
    if (actions !== this.drawn.actions) {
      this.drawn.actions = actions;
      document.getElementById('cell-actions').innerHTML = actions;
    }
  }

  // A CNC machine's state, in words and a bar.
  machineHtml(m) {
    const door = m.door >= 1 ? '열림' : m.door <= 0 ? '닫힘' : m.opening ? '여는 중' : '닫는 중';
    const part = m.part === 'raw' ? '소재' : m.part === 'finished' ? '완성품' : '없음';
    let html = `<div class="machine-state"><span>문 <b>${door}</b></span><span>척 <b>${m.clamped ? '물림' : '풀림'}</b></span><span>척 안 <b>${part}</b></span></div>`;
    if (m.progress != null) {
      html += `<div class="machine-bar"><span style="width:${(m.progress * 100).toFixed(1)}%"></span><em>가공 중 ${Math.round(m.progress * 100)}% · 남은 ${Math.ceil(m.left)} s</em></div>`;
    }
    return html;
  }

  fieldsHtml(c, fields) {
    return fields
      .map(([key, label, unit, scale]) => {
        const v = c.props[key] * scale;
        const shown = Number.isInteger(v) ? v : Number(v.toFixed(3));
        return `<label class="insp-field">${label}<input type="number" class="num" data-prop="${key}" data-scale="${scale}" value="${shown}" min="0" step="${unit === 'mm' ? 10 : unit === 's' ? 0.5 : 1}">${unit ? `<span>${unit}</span>` : ''}</label>`;
      })
      .join('');
  }

  // The role as the inspector has it now, to KYNEX.
  setRole() {
    const object = this.ctx.selected();
    if (object == null) return;
    const kind = this.kind.value || null;
    const props = {};
    for (const input of this.box.querySelectorAll('[data-prop]')) {
      const v = Number(input.value);
      if (Number.isFinite(v)) props[input.dataset.prop] = v / Number(input.dataset.scale);
    }
    // A new role starts from its own typical values, not another's fields.
    const same = this.componentOf(object)?.kind === kind;
    this.send({ op: 'role', object, kind, props: same ? props : {} });
  }

  // Each pallet's places and each chuck's seat, marked.
  drawMarkers() {
    for (const child of [...this.markers.children]) {
      child.removeFromParent();
      child.geometry.dispose();
      child.material.dispose();
    }
    const slots = this.cell.components.flatMap((c) => c.slots.flat());
    const seats = this.cell.components.filter((c) => c.seat).flatMap((c) => c.seat);
    for (const [points, color, size] of [
      [slots, 0xb8c2d0, 5],
      [seats, 0xff8a3d, 9],
    ]) {
      if (!points.length) continue;
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.Float32BufferAttribute(points, 3));
      const dots = new THREE.Points(
        geometry,
        new THREE.PointsMaterial({ color, size, sizeAttenuation: false, depthTest: false, transparent: true, opacity: 0.8 }),
      );
      dots.renderOrder = 8;
      this.markers.add(dots);
    }
  }
}
