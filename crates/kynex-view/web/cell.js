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
  ['molder', '사출 성형기'],
  ['conveyor', '컨베이어'],
  ['amr', 'AMR'],
  ['amr_stop', 'AMR 정지 위치'],
  ['positioner', '포지셔너'],
  ['door', '문 (장비의 움직이는 부분)'],
  ['connector', '연결점'],
];

// What each I/O point is, in words: the robot's outputs and its inputs.
const IO_LABELS = {
  'gripper:close': '그리퍼 닫기',
  'molder:open': '가드 열기',
  'molder:close': '가드 닫기',
  'molder:door_open': '가드 열림',
  'molder:door_closed': '가드 닫힘',
  close: '문 닫기',
  closed: '닫힘 확인',
  open: '문 열기',
  clamp: '척 물림',
  unclamp: '척 풀기',
  start: '시작',
  door_open: '문 열림',
  door_closed: '문 닫힘',
  clamped: '척 물림됨',
  idle: '대기 중',
  done: '완료',
  part_at_end: '부품 도착',
};

// An I/O number as the controller names it.
const ioName = (n, output) => {
  const kind = output ? 'DO' : 'DI';
  if (n <= 7) return `${kind} ${n}`;
  if (n <= 15) return `설정 ${kind} ${n - 8}`;
  return `툴 ${kind} ${n - 16}`;
};

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
  molder: [
    ['cycle', '성형 시간', 's', 1],
    ['door', '가드 개폐', 's', 1],
  ],
  conveyor: [
    ['speed', '속도', 'mm/s', 1000],
    ['feed', '소재 공급 간격 (0: 없음)', 's', 1],
    ['drain', '끝에서 치움 (1: 예)', '', 1],
  ],
  amr: [['speed', '주행 속도', 'mm/s', 1000]],
  positioner: [['speed', '회전 속도', '°/s', 180 / Math.PI]],
  // A choice is a list of [value, label].
  door: [
    ['swing', '동작', '', 1, [[0, '미닫이'], [1, '여닫이']]],
    ['axis', '축', '', 1, [[0, '부품 X'], [1, '부품 Y'], [2, '부품 Z']]],
    ['travel', '거리', 'mm', 1000],
    ['angle', '각도', '°', 180 / Math.PI],
  ],
  connector: [
    ['plug', '종류', '', 1, [[0, '로봇 받침면'], [1, '컨베이어 입구'], [2, '컨베이어 출구'], [3, '받침면에 올라섬']]],
  ],
};
// Settings that may be below zero: a way back, a turn the other way.
const SIGNED = new Set(['travel', 'angle']);
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
      const stop = e.target.closest('[data-drive]')?.dataset.drive;
      if (stop) this.send({ op: 'drive', object: this.ctx.selected(), stop });
      const turn = e.target.closest('[data-turn]')?.dataset.turn;
      if (turn != null) this.send({ op: 'turn', object: this.ctx.selected(), angle: (Number(turn) * Math.PI) / 180 });
      if (e.target.closest('[data-pick]')) {
        this.pickingFor = this.ctx.selected();
        this.showFor(this.pickingFor);
      }
    });
    document.getElementById('cell-io').addEventListener('change', (e) => {
      const key = e.target.dataset.io;
      if (!key) return;
      const v = e.target.value.trim();
      const port = v === '' ? null : Number(v);
      if (port != null && !(Number.isInteger(port) && port >= 0 && port <= 17)) {
        return this.ctx.toast('I/O 번호는 0–17입니다 (0–7 표준, 8–15 설정 가능, 16–17 툴)');
      }
      this.send({ op: 'io', object: this.ctx.selected(), key, port });
    });
    // What the inspector last drew, so that a running machine's progress
    // does not redraw the fields under the pointer.
    this.drawn = { props: '', actions: '', io: '' };
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
    state.innerHTML = m ? this.machineHtml(m, c.kind) : '';
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
    } else if (c && c.kind === 'molder') {
      if (!c.seat) {
        actions = `<span class="warn-text">${escapeHtml('하위 물체에 mould가 없습니다')}</span>`;
      } else {
        const busy = m?.progress != null;
        actions =
          `<button class="text-btn" data-machine="${m?.opening ? 'close' : 'open'}"${busy ? ' disabled' : ''}>${m?.opening ? '가드 닫기' : '가드 열기'}</button>` +
          `<button class="text-btn" data-machine="start"${busy ? ' disabled' : ''}>성형 시작</button>` +
          `<button class="text-btn quiet" data-cell="empty"${busy ? ' disabled' : ''}>금형 비우기</button>`;
      }
    } else if (c && c.kind === 'amr') {
      const stops = this.cell.components.filter((s) => s.kind === 'amr_stop');
      actions = stops.length
        ? stops
            .map((s) => `<button class="text-btn" data-drive="${escapeHtml(s.object)}"${c.amr?.driving ? ' disabled' : ''}>→ ${escapeHtml(s.name)}</button>`)
            .join('') + (c.amr?.driving ? '<span class="muted">주행 중</span>' : '')
        : '<span class="muted">라이브러리에서 AMR 정지 위치를 놓으세요</span>';
    } else if (c && c.kind === 'positioner') {
      const p = c.positioner;
      const angle = p?.angle != null ? Math.round((p.angle * 180) / Math.PI) : 0;
      actions =
        [0, 180]
          .map((a) => `<button class="text-btn" data-turn="${a}"${p?.turning ? ' disabled' : ''}>→ ${a}°</button>`)
          .join('') + `<span class="muted">${p?.turning ? '회전 중' : `지금 ${angle}°`}</span>`;
    } else if (c && (c.kind === 'door' || c.kind === 'connector')) {
      const mm = (c.point ?? []).map((v) => Math.round(v * 1000)).join(', ');
      const what = c.kind === 'door' ? '회전 중심' : '연결점';
      actions =
        this.pickingFor === uid
          ? `<span class="muted">${what}이 될 점을 3D 화면에서 누르세요 (Esc 취소)</span>`
          : `<button class="text-btn" data-pick="1">${what} 찍기</button><span class="muted">${mm} mm</span>`;
      if (c.kind === 'door') {
        actions += c.machine_of
          ? '<span class="muted">거리 0: 장비 X 방향으로 자기 폭만큼. 각도·거리가 음수면 반대로</span>'
          : '<span class="warn-text">CNC·사출기 역할 물체 아래에 있어야 움직입니다</span>';
      }
    } else if (c && c.kind === 'gripper' && !c.robot) {
      actions = '<span class="warn-text">로봇의 링크가 아닙니다</span>';
    }
    if (actions !== this.drawn.actions) {
      this.drawn.actions = actions;
      document.getElementById('cell-actions').innerHTML = actions;
    }
    // Its wiring, for the program's export.
    const io = c?.io_points;
    const ioBox = document.getElementById('cell-io-box');
    ioBox.hidden = !io || !(io.outputs.length + io.inputs.length);
    const ioKey = `${uid}|${JSON.stringify(c?.io)}`;
    if (!ioBox.hidden && ioKey !== this.drawn.io && !ioBox.contains(document.activeElement)) {
      this.drawn.io = ioKey;
      const row = (key, output) => {
        const n = c.io[key];
        return `<label class="insp-field">${output ? '→' : '←'} ${escapeHtml(IO_LABELS[`${c.kind}:${key}`] ?? IO_LABELS[key] ?? key)}<input type="number" class="num" data-io="${escapeHtml(key)}" value="${n ?? ''}" min="0" max="17" placeholder="없음"><span>${n != null ? ioName(n, output) : ''}</span></label>`;
      };
      document.getElementById('cell-io').innerHTML =
        io.outputs.map((k) => row(k, true)).join('') + io.inputs.map((k) => row(k, false)).join('');
    }
  }

  // A CNC machine's state, in words and a bar.
  machineHtml(m, kind) {
    const door = m.door >= 1 ? '열림' : m.door <= 0 ? '닫힘' : m.opening ? '여는 중' : '닫는 중';
    const molder = kind === 'molder';
    const part = m.part === 'raw' ? '소재' : m.part === 'finished' ? (molder ? '성형품' : '완성품') : '없음';
    let html = molder
      ? `<div class="machine-state"><span>가드 <b>${door}</b></span><span>금형 <b>${part}</b></span></div>`
      : `<div class="machine-state"><span>문 <b>${door}</b></span><span>척 <b>${m.clamped ? '물림' : '풀림'}</b></span><span>척 안 <b>${part}</b></span></div>`;
    if (m.progress != null) {
      html += `<div class="machine-bar"><span style="width:${(m.progress * 100).toFixed(1)}%"></span><em>${molder ? '성형' : '가공'} 중 ${Math.round(m.progress * 100)}% · 남은 ${Math.ceil(m.left)} s</em></div>`;
    }
    return html;
  }

  fieldsHtml(c, fields) {
    return fields
      .map(([key, label, unit, scale, choices]) => {
        const v = c.props[key] * scale;
        if (choices) {
          const options = choices.map(([value, text]) => `<option value="${value}"${value === v ? ' selected' : ''}>${text}</option>`).join('');
          return `<label class="insp-field">${label}<select data-prop="${key}" data-scale="${scale}">${options}</select></label>`;
        }
        const shown = Number.isInteger(v) ? v : Number(v.toFixed(3));
        const min = SIGNED.has(key) ? '' : ' min="0"';
        return `<label class="insp-field">${label}<input type="number" class="num" data-prop="${key}" data-scale="${scale}" value="${shown}"${min} step="${unit === 'mm' ? 10 : unit === 's' ? 0.5 : 1}">${unit ? `<span>${unit}</span>` : ''}</label>`;
      })
      .join('');
  }

  // Picking a point on a shape for the selected door's hinge or connector.
  picking() {
    return this.pickingFor != null;
  }

  stopPicking() {
    this.pickingFor = null;
    const selected = this.ctx.selected();
    if (selected != null) this.showFor(selected);
  }

  pickPoint(p) {
    const object = this.pickingFor;
    this.pickingFor = null;
    this.send({ op: 'point', object, at: [p.x, p.y, p.z] }, () => this.ctx.toast('점을 정했습니다'));
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
    // Doors' hinges and connectors.
    const pins = this.cell.components.filter((c) => c.point).flatMap((c) => c.point);
    for (const [points, color, size] of [
      [slots, 0xb8c2d0, 5],
      [seats, 0xff8a3d, 9],
      [pins, 0x3ddc84, 10],
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
