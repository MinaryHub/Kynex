// The cell: what each object is to the process — a CNC machine, a pallet,
// a gripper, a fence — set in the inspector, and the places parts stand,
// marked in the scene. KYNEX keeps the roles (the `cell` event) and the
// parts, which arrive in the scene like any other object.

import * as THREE from 'three';
import { fieldsFor, getLevel, onLevel, roleShown } from './level.js';

// The flow's roles, as a menu offers them.
const FLOW_KINDS = [
  ['source', '소재 공급'],
  ['station', '공정 스테이션'],
  ['buffer', '버퍼'],
  ['sink', '배출'],
];
const FLOW_NAME = { ...Object.fromEntries(FLOW_KINDS), conveyor: '컨베이어' };
// Templates that are the building, not equipment of the flow.
const STRUCTURE = new Set([
  'fence', 'guard_rail', 'factory_wall', 'factory_column', 'floor_zone', 'ceiling_light',
  'crane_rail', 'oht_rail', 'roll_door', 'pipe_rack', 'amr_stop', 'overhead_crane',
  // What a robot stands on: a click there means the robot.
  'pedestal',
]);

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
  ['sensor', '포토센서'],
  ['source', '공정 흐름: 소재 공급'],
  ['station', '공정 흐름: 공정 스테이션'],
  ['buffer', '공정 흐름: 버퍼'],
  ['sink', '공정 흐름: 배출'],
  ['operator', '작업자 (사람)'],
  ['actuator', '동작축 (스토퍼·리프터·직선·회전)'],
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
  unclamped: '척 풀림됨',
  idle: '대기 중',
  done: '완료',
  part_at_end: '부품 도착',
  extend: '전진',
  retract: '후진',
  extended: '전진 끝',
  retracted: '후진 끝',
  detected: '감지',
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
export const FIELDS = {
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
    ['rule', '다음 공정', '', 1, [[0, '번갈아'], [1, '대기 적은 곳'], [2, '무작위']]],
    ['speed', '속도', 'mm/s', 1000],
    ['feed', '공급 간격', 's', 1],
    ['drain', '끝에서 치움', '', 1, [[0, '아니오'], [1, '예']]],
  ],
  amr: [
    ['speed', '주행 속도', 'mm/s', 1000],
    ['capacity', '적재량', '개', 1],
    ['battery', '배터리', '분', 1],
    ['charge', '충전 시간', '분', 1],
  ],
  amr_stop: [['charger', '충전소', '', 1, [[0, '아님'], [1, '충전소']]]],
  positioner: [['speed', '회전 속도', '°/s', 180 / Math.PI]],
  // A choice is a list of [value, label].
  door: [
    ['swing', '동작', '', 1, [[0, '미닫이'], [1, '여닫이']]],
    ['axis', '축', '', 1, [[0, '부품 X'], [1, '부품 Y'], [2, '부품 Z']]],
    ['travel', '거리', 'mm', 1000],
    ['angle', '각도', '°', 180 / Math.PI],
  ],
  source: [
    ['rule', '다음 공정', '', 1, [[0, '번갈아'], [1, '대기 적은 곳'], [2, '무작위']]],
    ['feed', '공급 간격', 's', 1],
    ['kanban', '칸반 한도', '개', 1],
    ['dist', '시간 분포', '', 1, [[0, '고정'], [1, '균등 ±폭'], [2, '삼각 ±폭'], [3, '정규 σ'], [4, '지수']]],
    ['spread', '폭·σ', 's', 1],
    ['capacity', '내보낼 자리', '개', 1],
    ['speed', '이송 속도', 'mm/s', 1000],
  ],
  station: [
    ['rule', '다음 공정', '', 1, [[0, '번갈아'], [1, '대기 적은 곳'], [2, '무작위']]],
    ['cycle', '처리 시간', 's', 1],
    ['dist', '시간 분포', '', 1, [[0, '고정'], [1, '균등 ±폭'], [2, '삼각 ±폭'], [3, '정규 σ'], [4, '지수']]],
    ['spread', '폭·σ', 's', 1],
    ['setup', '품종 교체 시간', 's', 1],
    ['mtbf', '고장 간격', 's', 1],
    ['mttr', '수리 시간', 's', 1],
    ['pm_every', '보전 간격', 's', 1],
    ['pm_time', '보전 시간', 's', 1],
    ['scrap', '불량률', '%', 100],
    ['crew', '작업자', '', 1, [[0, '필요 없음'], [1, '작업자 필요']]],
    ['holds', '동시 투입 가능 수', '개', 1],
    ['capacity', '대기 자리', '개', 1],
    ['takes', '투입 수', '개', 1],
    ['makes', '배출 수', '개', 1],
    ['speed', '이송 속도', 'mm/s', 1000],
  ],
  buffer: [
    ['rule', '다음 공정', '', 1, [[0, '번갈아'], [1, '대기 적은 곳'], [2, '무작위']]],
    ['capacity', '용량', '개', 1],
    ['policy', '꺼내는 순서', '', 1, [[0, '먼저 들어온 것 (FIFO)'], [1, '나중 들어온 것 (LIFO)']]],
    ['speed', '이송 속도', 'mm/s', 1000],
  ],
  operator: [
    ['speed', '걷는 속도', 'm/s', 1],
    ['capacity', '한 번에 드는 수', '개', 1],
    ['wage', '임금', '원/시', 1],
    ['shift_from', '근무 시작', '시', 1],
    ['shift_to', '근무 끝', '시', 1],
  ],
  sensor: [
    ['axis', '빔 방향', '', 1, [[0, '부품 X'], [1, '부품 Y'], [2, '부품 Z']]],
    ['travel', '빔 길이', 'mm', 1000],
  ],
  actuator: [
    ['swing', '동작', '', 1, [[0, '직선'], [1, '회전']]],
    ['axis', '축', '', 1, [[0, '부품 X'], [1, '부품 Y'], [2, '부품 Z']]],
    ['travel', '행정', 'mm', 1000],
    ['angle', '각도', '°', 180 / Math.PI],
    ['door', '동작 시간', 's', 1],
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

// What carries along a link, by its number, and its arrow's colour.
const CARRIER_NAMES = ['', 'AMR', '작업자', '로봇'];
const CARRIER_COLOURS = [0x5aa9ff, 0x3fcf8e, 0xf2b53c, 0xb57bff];

export class CellPanel {
  // ctx: { scene, selected(), toast, escapeHtml }
  constructor(ctx) {
    this.ctx = ctx;
    this.cell = { components: [], parts: [] };
    this.markers = new THREE.Group();
    ctx.scene.add(this.markers);
    // Each flow piece's name over it: drawn again only when a name or a
    // place changes, not with every report of the cell.
    this.names = new THREE.Group();
    // The signs over the flow — names, loads, progress — shown or not, as
    // last chosen in this browser.
    try {
      this.showLabels = localStorage.getItem('kynex.labels') !== 'off';
      this.showArrows = localStorage.getItem('kynex.arrows') !== 'off';
    } catch {
      this.showLabels = true;
      this.showArrows = true;
    }
    this.namesDrawn = '';
    ctx.scene.add(this.names);
    this.box = document.getElementById('insp-cell');
    this.kind = document.getElementById('cell-kind');
    this.roles();
    this.kind.addEventListener('change', () => this.setRole());
    this.box.addEventListener('change', (e) => {
      if (e.target.dataset.prop) this.setRole();
      // A source's mix, or a station's time for each product.
      if (e.target.dataset.mix !== undefined || e.target.dataset.cycle !== undefined) {
        const key = e.target.dataset.mix !== undefined ? 'mix' : 'cycle';
        const values = [...this.box.querySelectorAll(`[data-${key}]`)].map((i) => Number(i.value) || 0);
        this.send({ op: 'flow_products', object: this.ctx.selected(), [key === 'mix' ? 'mix' : 'cycles']: values });
      }
      // The equipment this piece of the flow stands for.
      if (e.target.dataset.flowHost !== undefined) this.send({ op: 'flow_host', object: this.ctx.selected(), host: e.target.value });
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
      // A stop's lanes: one more, or one fewer, both ways.
      const lane = e.target.closest('[data-lane]');
      if (lane) {
        const object = this.ctx.selected();
        const have = this.componentOf(object)?.lanes ?? [];
        const t = lane.dataset.lane;
        this.send({ op: 'amr_lanes', object, next: have.includes(t) ? have.filter((x) => x !== t) : [...have, t] });
      }
      // A person's stations: one more, or one fewer.
      const skill = e.target.closest('[data-skill]');
      if (skill) {
        const object = this.ctx.selected();
        const have = this.componentOf(object)?.skills ?? [];
        const t = skill.dataset.skill;
        return this.send({ op: 'operator_skills', object, stations: have.includes(t) ? have.filter((x) => x !== t) : [...have, t] });
      }
      // A source's orders: from a CSV file, or none.
      if (e.target.closest('[data-orders-load]')) {
        const object = this.ctx.selected();
        const input = document.createElement('input');
        input.type = 'file';
        input.accept = '.csv,.txt';
        input.hidden = true;
        document.body.appendChild(input);
        input.addEventListener('change', async () => {
          input.remove();
          const file = input.files?.[0];
          if (!file) return;
          if (file.size > 2 * 1024 * 1024) return this.ctx.toast('주문표가 너무 큽니다 (2 MB까지)');
          this.send({ op: 'flow_orders', object, csv: await file.text() }, () => this.ctx.toast('주문표를 넣었습니다'));
        });
        input.click();
        return;
      }
      if (e.target.closest('[data-orders-clear]')) {
        return this.send({ op: 'flow_orders', object: this.ctx.selected(), csv: '' });
      }
      // Which products a link takes: one more, or one fewer.
      const chip = e.target.closest('[data-route-to]');
      if (chip) {
        const object = this.ctx.selected();
        const routes = this.componentOf(object)?.flow?.routes ?? {};
        const to = chip.dataset.routeTo;
        const p = Number(chip.dataset.product);
        const have = routes[to] ?? [];
        const products = have.includes(p) ? have.filter((x) => x !== p) : [...have, p];
        return this.send({ op: 'flow_products', object, to, products });
      }
      // A process flow's next: one more, or one fewer.
      const next = e.target.closest('[data-flow-next]');
      if (next) {
        const object = this.ctx.selected();
        const have = this.componentOf(object)?.flow?.next ?? [];
        const t = next.dataset.flowNext;
        const list = have.includes(t) ? have.filter((x) => x !== t) : [...have, t];
        this.send({ op: 'flow_next', object, next: list });
      }
      if (e.target.closest('[data-link-pick]')) return this.startLinking(this.ctx.selected());
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
    // Following a real machine, or no more.
    document.getElementById('mirror-go').addEventListener('click', () => {
      const object = this.ctx.selected();
      const c = this.componentOf(object);
      const following = !!c?.mirror;
      if (following) return this.send({ op: 'mirror', object, agent: '' });
      const value = (id) => document.getElementById(id).value.trim();
      if (value('mirror-kind') === 'mqtt') {
        const broker = value('mirror-broker');
        const topic = value('mirror-topic');
        if (!broker) return this.ctx.toast('MQTT 브로커 주소를 넣으세요 (호스트:1883)');
        if (!topic) return this.ctx.toast('AMR 상태 토픽을 넣으세요 (VDA 5050: uagv/v2/제조사/일련번호/state)');
        const request = { op: 'mirror', object, kind: 'mqtt', broker, topic, user: value('mirror-mqtt-user') };
        const password = document.getElementById('mirror-mqtt-password').value;
        if (password) request.password = password;
        document.getElementById('mirror-mqtt-password').value = '';
        return this.send(request);
      }
      if (value('mirror-kind') === 'umati') {
        const url = value('mirror-url');
        const node = value('mirror-node');
        if (!url.startsWith('opc.tcp://')) return this.ctx.toast('서버 주소를 넣으세요 (opc.tcp://호스트:4840)');
        if (!node) return this.ctx.toast('장비 노드를 넣으세요 (예: ns=3;s=Machine1)');
        const request = { op: 'mirror', object, kind: 'umati', url, node, security: value('mirror-security'), user: value('mirror-user') };
        const password = document.getElementById('mirror-password').value;
        if (password) request.password = password;
        document.getElementById('mirror-password').value = '';
        return this.send(request);
      }
      const agent = value('mirror-agent');
      if (!agent) return this.ctx.toast('에이전트 주소를 넣으세요 (http://호스트:5000)');
      this.send({ op: 'mirror', object, agent, device: value('mirror-device') });
    });
    document.getElementById('mirror-kind').addEventListener('change', () => this.mirrorFields());
    document.getElementById('cell-plc').addEventListener('change', (e) => {
      const key = e.target.dataset.plc;
      if (!key) return;
      // A coil's number or a node's id; the link says which it cannot use.
      this.send({ op: 'plc', object: this.ctx.selected(), key, address: e.target.value.trim() });
    });
    // What the inspector last drew, so that a running machine's progress
    // does not redraw the fields under the pointer.
    this.drawn = { props: '', actions: '', io: '', plc: '' };
    // Another level: the inspector drawn anew, with what it shows.
    onLevel(() => {
      this.drawn = { props: '', actions: '', io: '', plc: '' };
      this.rolesKey = null;
      const uid = this.ctx.selected();
      if (uid != null) this.showFor(uid);
    });
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

  // The mirror box's fields for the way chosen.
  mirrorFields() {
    const kind = document.getElementById('mirror-kind').value;
    for (const el of document.querySelectorAll('#cell-mirror-box [data-mirror]')) el.hidden = el.dataset.mirror !== kind;
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

  // The roles offered, as the level has them, and the one `kind` has.
  roles(kind = '') {
    const key = `${getLevel()}|${kind}`;
    if (key === this.rolesKey) return;
    this.rolesKey = key;
    this.kind.innerHTML = KINDS.filter(([v]) => roleShown(v) || v === kind)
      .map(([v, label]) => `<option value="${v}">${label}</option>`)
      .join('');
  }

  // The selected object's role, in the inspector.
  showFor(uid) {
    const { escapeHtml } = this.ctx;
    const c = this.componentOf(uid);
    this.roles(c?.kind ?? '');
    if (document.activeElement !== this.kind) this.kind.value = c?.kind ?? '';
    const fields = c ? fieldsFor(c.kind, FIELDS[c.kind] || []) : [];
    const propsKey = `${uid}|${c?.kind}|${getLevel()}|${JSON.stringify(c?.props)}`;
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
            .join('') +
          (c.amr?.driving ? '<span class="muted">주행 중</span>' : '') +
          (c.amr?.battery != null ? `<span class="muted">배터리 ${Math.round(c.amr.battery * 100)}%${c.amr.charging ? ' · 충전하러 감/충전 중' : ''}</span>` : '')
        : '<span class="muted">라이브러리에서 AMR 정지 위치를 놓으세요</span>';
    } else if (c && c.kind === 'operator') {
      // The stations this person may work at: none picked, any.
      const stations = this.cell.components.filter((s) => s.kind === 'station');
      const have = c.skills ?? [];
      actions = stations.length
        ? `<span class="muted">맡는 공정</span>${stations
            .map((s) => `<button class="text-btn${have.includes(s.object) ? ' on' : ''}" data-skill="${escapeHtml(s.object)}" aria-pressed="${have.includes(s.object)}">${have.includes(s.object) ? '✓ ' : ''}${escapeHtml(s.name)}</button>`)
            .join('')}${have.length ? '' : '<span class="muted">(아무것도 고르지 않으면 모두)</span>'}`
        : '<span class="muted">작업자 필요 스테이션이 생기면 맡을 곳을 고를 수 있습니다</span>';
    } else if (c && c.kind === 'amr_stop') {
      const others = this.cell.components.filter((s) => s.kind === 'amr_stop' && s.object !== c.object);
      const have = c.lanes ?? [];
      actions = others.length
        ? `<span class="muted">AMR 경로</span>${others
            .map(
              (s) =>
                `<button class="text-btn${have.includes(s.object) ? ' on' : ''}" data-lane="${escapeHtml(s.object)}" aria-pressed="${have.includes(s.object)}" title="이 정지 위치와 잇는 길 (양방향)">${have.includes(s.object) ? '✓ ' : ''}${escapeHtml(s.name)}</button>`,
            )
            .join('')}`
        : '<span class="muted">다른 AMR 정지 위치를 놓으면 그 사이를 길로 이을 수 있습니다</span>';
    } else if (c && c.flow) {
      const f = c.flow;
      // Where its parts go: every other part of the flow, those it sends
      // to on.
      const targets = this.cell.components.filter(
        (t) => t.object !== c.object && ['station', 'buffer', 'sink', 'conveyor'].includes(t.kind),
      );
      const kindName = Object.fromEntries(KINDS);
      const nexts =
        c.kind === 'sink'
          ? ''
          : targets.length
            ? `<span class="muted">다음 공정</span>${targets
                .map(
                  (t) =>
                    `<button class="text-btn${f.next.includes(t.object) ? ' on' : ''}" data-flow-next="${escapeHtml(t.object)}" aria-pressed="${f.next.includes(t.object)}" title="${escapeHtml(kindName[t.kind] ?? t.kind)}">${f.next.includes(t.object) ? '✓ ' : ''}${escapeHtml(t.name)}</button>`,
                )
                .join('')}`
            : '<span class="muted">다음 공정으로 보낼 공정·버퍼·배출 역할이 없습니다</span>';
      const state =
        c.kind === 'sink'
          ? `배출 ${f.made}개`
          : c.kind === 'station'
            ? `${f.unreachable ? `⚠ 로봇이 닿지 않아 이 장비로는 보내지 않습니다 (${escapeHtml(f.unreachable)}) · ` : ''}${f.down ? '고장 · 수리 중' : f.maint ? '보전 중' : f.working ? `작업 중 ${Math.ceil(f.left ?? 0)} s` : f.out ? '막힘 (보낼 곳 없음)' : '대기'} · 장비 안 ${f.inside ?? 0}${f.holds ? `/${f.holds}` : ''}개 · 대기 ${f.queue} · 완료 ${f.made}${f.oee && f.oee.good + f.oee.bad ? ` · OEE ${Math.round(f.oee.oee * 100)}%` : ''}`
            : c.kind === 'conveyor'
              ? `끝에서 기다림 ${f.out}`
              : `${c.kind === 'source' ? `공급 ${f.made}개 · ` : ''}보관 ${f.out + f.queue}`;
      // Products: as many as any source mixes.
      const last = (m) => (m ?? []).reduce((k, w, i) => (w > 0 ? i + 1 : k), 0);
      const used = Math.max(1, ...this.cell.components.map((s) => last(s.flow?.mix)));
      const names = ['A', 'B', 'C', 'D', 'E', 'F'];
      let products = '';
      if (c.kind === 'source') {
        const mix = f.mix ?? [];
        products = `<div class="flow-products"><span class="muted">제품 비율</span>${names
          .map((n, i) => `<label>${n}<input type="number" class="num" data-mix="${i}" value="${mix[i] ?? (i === 0 && !mix.length ? 1 : 0)}" min="0" step="1"></label>`)
          .join('')}</div>`;
        products += `<div class="flow-products"><span class="muted">주문표${f.orders ? ` ${f.orders}건 (공급 간격 대신 주문대로)` : ''}</span><button class="text-btn" data-orders-load title="CSV: 시각(분), 제품(A–F), 수량, 납기(분) — 한 줄에 주문 하나">CSV 불러오기</button>${f.orders ? '<button class="text-btn quiet" data-orders-clear>지우기</button>' : ''}</div>`;
      } else if (c.kind === 'station' && used > 1) {
        const cycles = f.cycles ?? [];
        products = `<div class="flow-products"><span class="muted">품종별 처리 시간 (비우면 기본)</span>${names
          .slice(0, used)
          .map((n, i) => `<label>${n}<input type="number" class="num" data-cycle="${i}" value="${cycles[i] > 0 ? cycles[i] : ''}" min="0" step="1" placeholder="${c.props.cycle}"></label>`)
          .join('')}</div>`;
      }
      // The equipment it stands for: picking that shows this role.
      const near = this.ctx.equipmentNear?.(c.object, c.host) ?? [];
      const host =
        c.kind === 'conveyor' || (!near.length && !c.host)
          ? ''
          : `<div class="flow-products"><span class="muted" title="이 공정이 어느 장비의 공정인지: 그 장비를 눌러도 이 설정이 보입니다">공정 「${escapeHtml(c.name)}」의 장비</span><select data-flow-host><option value="">없음</option>${near
              .map((q) => `<option value="${escapeHtml(q.root)}"${q.root === c.host ? ' selected' : ''}>${escapeHtml(q.name)}${q.d != null ? ` (${q.d.toFixed(1)} m)` : ''}</option>`)
              .join('')}</select></div>`;
      // Each link's products, where there are several.
      let routes = '';
      if (used > 1 && c.kind !== 'sink' && f.next.length) {
        routes = f.next
          .map((to) => {
            const t = this.componentOf(to);
            const have = f.routes?.[to] ?? [];
            return `<div class="flow-route"><span class="muted">→ ${escapeHtml(t?.name ?? to)}</span>${names
              .slice(0, used)
              .map((n, i) => `<button class="text-btn${have.includes(i) ? ' on' : ''}" data-route-to="${escapeHtml(to)}" data-product="${i}" title="이 길로 보낼 제품 (아무것도 고르지 않으면 모두)">${n}</button>`)
              .join('')}${have.length ? '' : '<span class="muted">모두</span>'}</div>`;
          })
          .join('');
      }
      // A beginner's: where it sends and how it is doing; the rest on
      // the way up.
      const link =
        (c.kind === 'sink' ? '' : '<button class="text-btn" data-link-pick title="3D 화면에서 다음 설비들을 차례로 눌러 잇습니다">＋ 화면에서 잇기</button>') +
        '<span class="muted insp-hint">오른쪽 클릭: 연결 메뉴 · 연결 모드에서 ➔ 손잡이를 끌어 잇기</span>';
      actions =
        link +
        (getLevel() >= 2
          ? `${nexts}<span class="muted">${state}</span>${host}${products}${routes}`
          : `${nexts}<span class="muted">${state}</span><span class="muted level-hint">시간 분포·고장·불량·제품·반송은 <b>중급</b>에서 정합니다</span>`);
    } else if (c && c.kind === 'actuator') {
      const a = c.actuator ?? {};
      const at = a.out >= 1 ? '전진 끝' : a.out <= 0 ? '후진 끝' : a.extending ? '전진 중' : '후진 중';
      actions =
        `<button class="text-btn" data-machine="extend"${a.extending ? ' disabled' : ''}>전진</button>` +
        `<button class="text-btn" data-machine="retract"${a.extending ? '' : ' disabled'}>후진</button>` +
        `<span class="muted">${at}</span>`;
    } else if (c && c.kind === 'sensor') {
      actions = c.sensor?.detected ? '<span class="warn-text">감지</span>' : '<span class="muted">감지 없음</span>';
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
    // The real machine it follows, and how that is going.
    const mirrorBox = document.getElementById('cell-mirror-box');
    mirrorBox.hidden = !(c && (c.kind === 'cnc' || c.kind === 'molder' || c.kind === 'amr'));
    if (!mirrorBox.hidden) {
      const m = c.mirror;
      // An AMR follows its fleet over MQTT; a machine, its agent or server.
      const amr = c.kind === 'amr';
      document.getElementById('mirror-ways').textContent = amr ? 'MQTT · VDA 5050' : 'MTConnect · umati';
      const kindSelect = document.getElementById('mirror-kind');
      for (const o of kindSelect.options) o.hidden = (o.value === 'mqtt') !== amr;
      if ((kindSelect.value === 'mqtt') !== amr) kindSelect.value = amr ? 'mqtt' : 'mtconnect';
      if (m && !mirrorBox.contains(document.activeElement)) {
        const set = (id, v) => (document.getElementById(id).value = v ?? '');
        set('mirror-kind', m.kind === 'umati' || m.kind === 'mqtt' ? m.kind : 'mtconnect');
        if (m.kind === 'mqtt') {
          set('mirror-broker', m.broker);
          set('mirror-topic', m.topic);
          set('mirror-mqtt-user', m.user);
        } else if (m.kind === 'umati') {
          set('mirror-url', m.url);
          set('mirror-node', m.node);
          set('mirror-security', m.security || 'none');
          set('mirror-user', m.user);
        } else {
          set('mirror-agent', m.agent);
          set('mirror-device', m.device);
        }
      }
      for (const el of mirrorBox.querySelectorAll('input, select')) el.disabled = !!m;
      this.mirrorFields();
      document.getElementById('mirror-go').textContent = m ? '그만 따라가기' : '따라가기';
      const st = c.machine?.mirror;
      const fleet = c.amr?.follow;
      let text = '';
      if (m && amr && fleet) {
        const said = [
          fleet.connected ? '연결됨' : '연결 안 됨',
          fleet.driving != null ? (fleet.driving ? '주행 중' : '정지') : null,
          fleet.battery != null ? `배터리 ${Math.round(fleet.battery)}%` : null,
          fleet.age != null ? `${fleet.age}초 전` : '위치 기다리는 중',
          ...(fleet.errors ?? []),
        ].filter(Boolean);
        text = said.join(' · ') + (fleet.error ? ` · ${fleet.error}` : '');
      } else if (m && st) {
        const last = st.last;
        if (st.error) text = st.error;
        if (last) {
          const said = [last.device, last.execution, last.door ? `문 ${last.door}` : null, last.part_count != null ? `생산 ${last.part_count}` : null, last.program]
            .filter(Boolean)
            .join(' · ');
          text = `${said} (${st.age ?? '?'}초 전)${st.error ? ` · ${st.error}` : ''}`;
        }
      } else if (m) {
        text = '연결 중…';
      }
      // A umati server's certificate, trusted on first meeting.
      if (m?.kind === 'umati' && m.trust) text += `${text ? ' · ' : ''}서버 인증서 ${m.trust.slice(0, 16)}…`;
      document.getElementById('mirror-state').textContent = text;
    }
    // Its PLC coils: commands the PLC gives it, states it gives the PLC.
    // A gripper is the robot's: its program closes it.
    const plcBox = document.getElementById('cell-plc-box');
    const plcPoints = c && c.kind !== 'gripper' ? c.io_points : null;
    plcBox.hidden = !plcPoints || !(plcPoints.outputs.length + plcPoints.inputs.length);
    const plcKey = `${uid}|${JSON.stringify(c?.plc)}`;
    if (!plcBox.hidden && plcKey !== this.drawn.plc && !plcBox.contains(document.activeElement)) {
      this.drawn.plc = plcKey;
      const row = (key, command) => {
        const n = c.plc?.[key];
        const label = IO_LABELS[`${c.kind}:${key}`] ?? IO_LABELS[key] ?? key;
        return `<label class="insp-field">${command ? 'PLC →' : '→ PLC'} ${escapeHtml(label)}<input type="text" class="num plc-address" data-plc="${escapeHtml(key)}" value="${escapeHtml(n ?? '')}" placeholder="없음" spellcheck="false"></label>`;
      };
      document.getElementById('cell-plc').innerHTML =
        plcPoints.outputs.map((k) => row(k, true)).join('') + plcPoints.inputs.map((k) => row(k, false)).join('');
    }
  }

  // A CNC machine's state, in words and a bar.
  machineHtml(m, kind) {
    const door = m.door >= 1 ? '열림' : m.door <= 0 ? '닫힘' : m.opening ? '여는 중' : '닫는 중';
    const molder = kind === 'molder';
    const travel = m.chuck ?? (m.clamped ? 1 : 0);
    const chuck = m.clamped ? (travel >= 1 ? '물림' : '무는 중') : travel <= 0 ? '풀림' : '푸는 중';
    const part = m.part === 'raw' ? '소재' : m.part === 'finished' ? (molder ? '성형품' : '완성품') : '없음';
    let html = molder
      ? `<div class="machine-state"><span>가드 <b>${door}</b></span><span>금형 <b>${part}</b></span></div>`
      : `<div class="machine-state"><span>문 <b>${door}</b></span><span>척 <b>${chuck}</b></span><span>척 안 <b>${part}</b></span></div>`;
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

  // How parts go along the link `from` → `to`: { by, robot, via, reach }.
  carrierOf(from, to) {
    return this.componentOf(from)?.flow?.carriers?.find((k) => k.to === to) ?? { by: 0 };
  }

  robotName(root) {
    return this.ctx.robots?.().find((r) => r.root === root)?.name ?? this.ctx.nameOf?.(root) ?? '로봇';
  }

  // Each link's number in the flow, by "from>to": its step from the
  // sources — 1, 2, 3 — and, where the way splits, which branch: 2-1,
  // 2-2, on along each branch (3-1, 3-2) until they meet again.
  linkNumbers() {
    const flow = this.cell.components.filter((c) => c.flow);
    const next = new Map(flow.map((c) => [c.object, (c.flow.next ?? []).filter((n) => flow.some((t) => t.object === n))]));
    const into = new Map(flow.map((c) => [c.object, []]));
    for (const [from, ns] of next) for (const n of ns) into.get(n)?.push(from);
    // Where each piece stands, x then y: branches told apart the same way
    // each time.
    const order = (u) => this.componentOf(u)?.flow?.at ?? [0, 0, 0];
    const byPlace = (a, b) => order(a)[0] - order(b)[0] || order(a)[1] - order(b)[1];
    // Steps from the sources, the longest way (loops not followed round).
    const depth = new Map();
    // From the sources (or, with none, what nothing feeds); then what the
    // sources do not reach, numbered on its own.
    let starts = flow.filter((c) => c.kind === 'source').map((c) => c.object).sort(byPlace);
    if (!starts.length) starts = flow.filter((c) => !into.get(c.object).length).map((c) => c.object).sort(byPlace);
    const walk = (u, d, on) => {
      if (on.has(u) || (depth.get(u) ?? -1) >= d) return;
      depth.set(u, d);
      on.add(u);
      for (const n of next.get(u) ?? []) walk(n, d + 1, on);
      on.delete(u);
    };
    for (const s0 of starts) walk(s0, 0, new Set());
    const left = flow.filter((c) => !depth.has(c.object) && !into.get(c.object).length).map((c) => c.object).sort(byPlace);
    for (const u of left) walk(u, 0, new Set());
    // Branch tags, piece by piece in step order.
    const tag = new Map();
    starts.forEach((u, i) => tag.set(u, starts.length > 1 ? [i + 1] : []));
    for (const u of left) tag.set(u, []);
    const edgeTag = new Map();
    const pieces = [...depth.keys()].sort((a, b) => depth.get(a) - depth.get(b));
    for (const u of pieces) {
      if (!tag.has(u)) {
        // Where branches meet: what they have in common.
        const ins = into.get(u).map((f) => edgeTag.get(`${f}>${u}`)).filter(Boolean);
        let common = ins[0] ?? [];
        for (const t of ins.slice(1)) {
          let k = 0;
          while (k < common.length && k < t.length && common[k] === t[k]) k++;
          common = common.slice(0, k);
        }
        if (ins.length > 1 && ins.some((t) => t.length !== common.length)) tag.set(u, common);
        else tag.set(u, ins.length ? ins[0] : []);
      }
      const outs = [...(next.get(u) ?? [])].sort(byPlace);
      outs.forEach((n, i) => edgeTag.set(`${u}>${n}`, outs.length > 1 ? [...tag.get(u), i + 1] : tag.get(u)));
    }
    const out = new Map();
    for (const [key, t] of edgeTag) {
      const from = key.split('>')[0];
      out.set(key, [String((depth.get(from) ?? 0) + 1), ...t].join('-'));
    }
    return out;
  }

  // How parts go along `link`, set: { by, robot }.
  setCarrier(link, by, robot = null) {
    const said = `${this.nameOf(link.from)} → ${this.nameOf(link.to)}`;
    const what = by === 3 ? `로봇 ${robot ? this.robotName(robot) : '(닿는 로봇 자동)'}` : CARRIER_NAMES[by] || '직접 (운반 없음)';
    this.send({ op: 'flow_carrier', object: link.from, to: link.to, by, robot }, () => this.ctx.toast(`${said}: ${what}`));
  }

  // Each link's robots' reach asked ahead, one link at a time, for its
  // menu to say at once: asked again once anything has moved.
  prefetchReach(at) {
    const moved = JSON.stringify([...at.entries()].map(([o, p]) => [o, p.map((v) => v.toFixed(2))])) + JSON.stringify(this.ctx.robots?.() ?? []);
    if (moved !== this.reachMoved) {
      this.reachMoved = moved;
      this.reachOf = new Map();
    }
    if (this.reachAsking) return;
    for (const c of this.cell.components) {
      for (const n of c.flow?.next ?? []) {
        if (this.reachOf.has(`${c.object}>${n}`)) continue;
        this.reachAsking = true;
        this.askReach({ from: c.object, to: n }).finally(() => {
          this.reachAsking = false;
          this.prefetchReach(at);
        });
        return;
      }
    }
  }

  // Which robots reach both ends of `link`, asked of KYNEX and kept for
  // the menu: { robot: { reach, why } }.
  async askReach(link) {
    const key = `${link.from}>${link.to}`;
    try {
      const answer = await post({ op: 'link_reach', from: link.from, to: link.to });
      if (answer.ok) {
        this.reachOf ??= new Map();
        this.reachOf.set(key, new Map(answer.robots.map((r) => [r.robot, r])));
      }
    } catch {}
    // Not to be asked again and again when KYNEX cannot say.
    this.reachOf ??= new Map();
    if (!this.reachOf.has(key)) this.reachOf.set(key, new Map());
    return this.reachOf.get(key);
  }

  // Linking in the 3D view: equipment clicked in order, each linked to the
  // one before. `from` the last clicked, or none yet.
  linking() {
    return this.link != null;
  }

  // ---- Logistics mode: what carries along the links ----------------------

  // Whether the logistics mode is on: a carrier chosen — { by, robot } —
  // given to each arrow clicked, and to each link made by clicking two
  // pieces of the flow in turn (`from` the first).
  carrying() {
    return this.carry != null;
  }

  startCarrying() {
    if (this.link) this.stopLinking();
    this.carry = { by: 3, robot: null, from: null };
    this.onCarrying?.(true);
    this.drawMarkers();
    this.ctx.toast('물류 연결: 위에서 운반 수단을 고르거나 로봇을 누른 뒤, 화살표를 누르거나 보내는 설비 → 받는 설비를 누르세요 (Esc: 끝)');
  }

  stopCarrying() {
    this.carry = null;
    this.onCarrying?.(false);
    this.drawMarkers();
  }

  // The carrier given from now: by (0 직접, 1 AMR, 2 작업자, 3 로봇) and,
  // by robot, which (none: any that reaches).
  carryWith(by, robot = null) {
    if (!this.carry) return;
    this.carry.by = by;
    this.carry.robot = by === 3 ? robot : null;
    this.onCarrying?.(true);
    this.drawMarkers();
  }

  carrierText(by, robot) {
    return by === 3 ? `로봇 ${robot ? this.robotName(robot) : '(자동)'}` : CARRIER_NAMES[by] || '직접';
  }

  // An arrow clicked in the logistics mode: carried as chosen.
  carryArrow(link) {
    if (!this.carry) return;
    this.setCarrier(link, this.carry.by, this.carry.robot);
  }

  // A click in the logistics mode on `uids` (an object and those above
  // it): a robot chosen to carry; else the sender, then the receiver of a
  // link made carried as chosen. Returns what to select, if anything.
  carryClick(uids, root = null) {
    if (!this.carry) return null;
    const robot = this.ctx.robotOf?.(uids);
    if (robot != null) {
      this.carryWith(3, robot);
      this.ctx.toast(`운반 수단: 로봇 ${this.robotName(robot)} · 화살표나 보내는 설비 → 받는 설비를 누르세요`);
      return null;
    }
    const piece = this.pieceOf(uids, root);
    if (!piece?.c?.flow) {
      this.ctx.toast('공정 흐름의 설비(공급·공정·버퍼·컨베이어·배출)를 누르세요');
      return null;
    }
    const object = piece.c.object;
    const from = this.carry.from;
    if (from == null || from === object) {
      if (!this.sends(object)) {
        this.ctx.toast(`${piece.name}에서는 부품이 나가지 않습니다: 보내는 설비부터 누르세요`);
        return null;
      }
      this.carry.from = object;
      this.ctx.toast(`${piece.name} → 받을 설비를 누르세요 (${this.carrierText(this.carry.by, this.carry.robot)})`);
      return object;
    }
    if (piece.c.kind === 'source') {
      this.ctx.toast('소재 공급으로는 보낼 수 없습니다');
      return null;
    }
    this.carry.from = this.sends(object) ? object : null;
    this.setCarrier({ from, to: object }, this.carry.by, this.carry.robot);
    return object;
  }

  startLinking(from = null) {
    if (this.carry) this.stopCarrying();
    this.link = { from: from && this.sends(from) ? from : null };
    this.onLinking?.(true);
    this.drawMarkers();
    this.ctx.toast(this.link.from ? '다음 공정이 될 설비를 차례로 누르거나, 끌어다 놓으세요 (Esc: 끝)' : '공급부터 배출까지 설비를 차례로 누르거나, 한 설비에서 다음 설비로 끌어다 놓으세요 (Esc: 끝)');
  }

  stopLinking() {
    this.link = null;
    this.onLinking?.(false);
    this.drawMarkers();
  }

  // Whether a part can go on from component `object`.
  sends(object) {
    return ['source', 'station', 'buffer', 'conveyor'].includes(this.componentOf(object)?.kind);
  }

  // A click while linking, on object `uids` (it and those above it; `root`
  // the placed equipment's, if it is some): the flow's equipment among them
  // linked from the last. Equipment not yet of the flow is made so: a
  // machine of a robot cell a station keeping its time, one with no role
  // the source if it is the first, else a station. Returns the one clicked,
  // to select.
  linkTo(uids, root = null) {
    // A robot: the next link is carried by it.
    const robot = this.ctx.robotOf?.(uids);
    if (robot != null) {
      if (!this.link?.from) {
        this.ctx.toast('보내는 설비를 먼저 누르고, 로봇, 받을 설비 순서로 누르세요');
        return null;
      }
      this.link.via = robot;
      this.drawMarkers();
      this.ctx.toast(`${this.nameOf(this.link.from)} → 로봇 ${this.robotName(robot)} → 받을 설비를 누르세요`);
      return null;
    }
    const piece = this.pieceOf(uids, root);
    if (piece?.c?.flow) return this.linkOn(piece.c.object, piece.c.kind, piece.c.name);
    if (!piece || piece.structure) {
      this.ctx.toast('설비를 누르세요 (공정 흐름에 넣을 수 있는 것)');
      return null;
    }
    const object = piece.object;
    const machine = piece.c && ['cnc', 'molder'].includes(piece.c.kind) ? piece.c : null;
    if (machine && getLevel() >= 3 && !confirm(`${machine.name}의 로봇 셀 역할(가공기)을 공정 스테이션으로 바꿉니다. 로봇 프로그램에서 쓰던 문·척 동작은 쓰지 않게 됩니다.`)) return null;
    const kind = !machine && !this.link.from ? 'source' : 'station';
    const props = machine ? { cycle: machine.props.cycle } : {};
    this.send({ op: 'role', object, kind, props }, () => {
      const made = this.componentOf(object);
      this.linkOn(object, kind, made?.name ?? '', kind === 'source' ? '소재 공급으로 정했습니다' : '공정 스테이션으로 정했습니다 (공정 시간을 넣으세요)');
    });
    return object;
  }

  // The flow's piece among `uids` (an object and those above it) that
  // sends parts on, if there is one: where a drag to link may start.
  sender(uids) {
    return uids.map((u) => this.componentOf(u)).find((c) => c?.flow && this.sends(c.object)) ?? null;
  }

  // A drag in the 3D view from equipment `from` (uids) to `to`: linked, as
  // clicking the one then the other would.
  dragLink(from, to, root = null) {
    const c = this.sender(from);
    if (!c) return null;
    // Out of link mode, for this link alone.
    const once = !this.link;
    if (once) this.link = { from: c.object, once: true };
    else this.link.from = c.object;
    const made = this.linkTo(to, root);
    // Dropped on a robot: the next click, the equipment it carries to.
    if (this.link?.via != null) return made;
    if (once && made == null && this.link?.once) this.link = null;
    return made;
  }

  // ---- Links made from a menu, a drag, a drop ------------------------------

  // The equipment `uids` (an object and those above it; `root` the placed
  // item's, if it is one) is in the flow: { object, c, name, structure }
  // — the flow's piece among them, else a machine (CNC, molder), else the
  // placed item, else the topmost object. `structure` for the building
  // (walls, fences, lights), which never joins the flow. Null for nothing.
  pieceOf(uids, root = null) {
    const cs = uids.map((u) => this.componentOf(u));
    const c = cs.find((c) => c?.flow);
    if (c) return { object: c.object, c, name: c.name, structure: false };
    const machine = cs.find((c) => c && ['cnc', 'molder'].includes(c.kind));
    if (machine) return { object: machine.object, c: machine, name: machine.name, structure: false };
    const item = this.ctx.itemOf?.(uids[0]);
    const object = root ?? item?.root ?? uids[uids.length - 1] ?? null;
    if (object == null) return null;
    const structure = item ? STRUCTURE.has(item.template) : false;
    // A zone painted on the floor is the floor: nothing to link to.
    if (item?.template === 'floor_zone') return null;
    // A flow piece standing on the equipment — a station set at a machine
    // — is what the equipment means to the flow.
    const on = structure ? null : this.flowOn(object);
    if (on) return { object: on.object, c: on, name: on.name, structure: false };
    return { object, c: this.componentOf(object) ?? null, name: this.ctx.nameOf?.(object) ?? '', structure };
  }

  // The flow's piece standing within equipment `object`'s footprint (a
  // little beyond it), nearest its middle; or null.
  flowOn(object) {
    const box = this.ctx.bounds?.(object);
    if (!box || box.isEmpty()) return null;
    const m = 0.3;
    const cx = (box.min.x + box.max.x) / 2;
    const cy = (box.min.y + box.max.y) / 2;
    let best = null;
    for (const c of this.cell.components) {
      const at = c.flow?.at;
      if (!at || c.object === object) continue;
      if (at[0] < box.min.x - m || at[0] > box.max.x + m || at[1] < box.min.y - m || at[1] > box.max.y + m) continue;
      const d = Math.hypot(at[0] - cx, at[1] - cy);
      if (!best || d < best.d) best = { d, c };
    }
    return best?.c ?? null;
  }

  nameOf(object) {
    return this.componentOf(object)?.name ?? this.ctx.nameOf?.(object) ?? '';
  }

  // Metres between two pieces of the flow, or null when either has no place.
  far(a, b) {
    const p = this.componentOf(a)?.flow?.at;
    const q = this.componentOf(b)?.flow?.at;
    if (!p || !q) return null;
    return Math.hypot(p[0] - q[0], p[1] - q[1]);
  }

  // Pieces parts can go on to from `object` (not itself, not a source),
  // nearest first.
  receivers(object) {
    return this.cell.components
      .filter((t) => t.object !== object && t.flow && ['station', 'buffer', 'sink', 'conveyor'].includes(t.kind))
      .sort((a, b) => (this.far(object, a.object) ?? 1e9) - (this.far(object, b.object) ?? 1e9));
  }

  // Pieces that send parts on, other than `object`, nearest first.
  senders(object) {
    return this.cell.components
      .filter((t) => t.object !== object && t.flow && this.sends(t.object))
      .sort((a, b) => (this.far(object, a.object) ?? 1e9) - (this.far(object, b.object) ?? 1e9));
  }

  // Every link from or to `object`: [{ from, to }].
  linksOf(object) {
    const links = [];
    for (const c of this.cell.components) {
      for (const n of c.flow?.next ?? []) {
        if (c.object === object || n === object) links.push({ from: c.object, to: n });
      }
    }
    return links;
  }

  // `from` sends on to `to` as well; `then` once it does.
  linkDirect(from, to, then = null) {
    const have = this.componentOf(from)?.flow?.next ?? [];
    const said = `${this.nameOf(from)} → ${this.nameOf(to)}`;
    if (have.includes(to)) return this.ctx.toast(`이미 이어져 있습니다: ${said}`);
    if (this.componentOf(to)?.kind === 'source') return this.ctx.toast('소재 공급으로는 보낼 수 없습니다');
    this.send({ op: 'flow_next', object: from, next: [...have, to] }, () => {
      this.ctx.toast(`이었습니다: ${said}`);
      then?.();
    });
  }

  unlink(from, to, then = null) {
    const have = this.componentOf(from)?.flow?.next ?? [];
    if (this.chosenLink?.from === from && this.chosenLink?.to === to) this.chosenLink = null;
    this.send({ op: 'flow_next', object: from, next: have.filter((n) => n !== to) }, () => {
      this.ctx.toast(`연결을 끊었습니다: ${this.nameOf(from)} → ${this.nameOf(to)}`);
      then?.();
    });
  }

  // Every link from and to `object` undone.
  unlinkAll(object) {
    const links = this.linksOf(object);
    const froms = [...new Set(links.map((l) => l.from))];
    for (const from of froms) {
      const have = this.componentOf(from)?.flow?.next ?? [];
      const next = from === object ? [] : have.filter((n) => n !== object);
      this.send({ op: 'flow_next', object: from, next });
    }
    this.chosenLink = null;
    this.ctx.toast(`${this.nameOf(object)}의 연결 ${links.length}개를 끊었습니다`);
  }

  // The link the other way round: `to` sends to `from` instead.
  reverseLink({ from, to }) {
    if (!this.sends(to)) return this.ctx.toast(`${this.nameOf(to)}에서는 부품이 나가지 않습니다`);
    if (this.componentOf(from)?.kind === 'source') return this.ctx.toast('소재 공급으로는 보낼 수 없습니다');
    this.unlink(from, to, () => this.linkDirect(to, from));
  }

  // `x` put into the link: `from` → `x` → `to`.
  insertBetween({ from, to }, x) {
    this.unlink(from, to, () => this.linkDirect(from, x, () => this.linkDirect(x, to)));
  }

  // Equipment `object` given the flow role `kind` (a machine keeping its
  // cycle time), then `then`. `kind` null takes it out of the flow.
  joinAs(object, kind, then = null) {
    const c = this.componentOf(object);
    const machine = c && ['cnc', 'molder'].includes(c.kind) ? c : null;
    if (machine && getLevel() >= 3 && !confirm(`${machine.name}의 로봇 셀 역할(가공기)을 공정 스테이션으로 바꿉니다. 로봇 프로그램에서 쓰던 문·척 동작은 쓰지 않게 됩니다.`)) return;
    const props = machine ? { cycle: machine.props.cycle } : {};
    const name = this.nameOf(object);
    this.send({ op: 'role', object, kind, props }, () => {
      this.ctx.toast(kind ? `${name}: ${FLOW_NAME[kind]}${kind === 'station' ? ' (공정 시간을 넣으세요)' : ''}` : `${name}: 공정 흐름에서 뺐습니다`);
      then?.();
    });
  }

  // What letting go of a link from `from` on `uids` (and `root`) would do:
  // { ok, text, object } — `object` the equipment it would land on.
  dropPreview(from, uids, root = null) {
    const robot = this.ctx.robotOf?.(uids);
    if (robot != null && this.carry) return { ok: true, text: `운반 수단을 로봇 ${this.robotName(robot)}(으)로`, object: robot };
    if (robot != null) return { ok: true, text: `${this.nameOf(from)} → 로봇 ${this.robotName(robot)} · 놓은 뒤 받을 설비를 누르세요`, object: robot };
    const piece = uids.length ? this.pieceOf(uids, root) : null;
    if (!piece) return { ok: false, text: '다음 공정이 될 설비 위에 놓으세요', object: null };
    const { object, c, name } = piece;
    const fromName = this.nameOf(from);
    if (object === from || uids.includes(from)) return { ok: false, text: '같은 설비입니다', object };
    if (piece.structure) return { ok: false, text: `${name}: 건물·구조물은 공정에 넣을 수 없습니다`, object };
    if (c?.flow) {
      if (c.kind === 'source') return { ok: false, text: `${name}: 소재 공급으로는 보낼 수 없습니다`, object };
      if (this.carry) return { ok: true, text: `${fromName} → ${name} · ${this.carrierText(this.carry.by, this.carry.robot)}`, object };
      if ((this.componentOf(from)?.flow?.next ?? []).includes(object)) return { ok: false, text: `이미 이어져 있습니다: ${fromName} → ${name}`, object };
      return { ok: true, text: `${fromName} → ${name}`, object };
    }
    return { ok: true, text: `${fromName} → ${name} · 놓으면 공정 스테이션이 됩니다`, object };
  }

  // The menu for a right click on equipment `uids` (and `root`): linking
  // it on, linking to it, undoing its links, its role; or null for nothing.
  equipmentMenu(uids, root = null) {
    const piece = this.pieceOf(uids, root);
    if (!piece) return null;
    const { object, c, name } = piece;
    const hint = (t) => {
      const d = this.far(object, t.object);
      return `${FLOW_NAME[t.kind] ?? t.kind}${d != null ? ` · ${d.toFixed(1)} m` : ''}`;
    };
    const entries = [{ title: c?.flow ? `${name} · ${FLOW_NAME[c.kind] ?? c.kind}` : name }];
    if (c?.flow) {
      if (this.sends(object)) {
        const have = c.flow.next ?? [];
        const targets = this.receivers(object).filter((t) => !have.includes(t.object));
        entries.push({
          label: '다음 공정으로 연결',
          sub: [
            ...targets.map((t) => ({ label: t.name, hint: hint(t), run: () => this.linkDirect(object, t.object) })),
            ...(targets.length ? [{ sep: true }] : []),
            { label: '화면에서 끌어 잇기…', hint: '다음 설비를 누르면 이어짐', run: () => this.startLinking(object) },
          ],
        });
      }
      if (c.kind !== 'source') {
        const from = this.senders(object).filter((s) => !(s.flow.next ?? []).includes(object));
        entries.push({
          label: '앞 공정에서 연결 받기',
          sub: from.map((s) => ({ label: s.name, hint: hint(s), run: () => this.linkDirect(s.object, object) })),
        });
      }
      const links = this.linksOf(object);
      if (links.length) {
        entries.push({
          label: '연결 끊기',
          sub: [
            ...links.map((l) => ({ label: `${this.nameOf(l.from)} → ${this.nameOf(l.to)}`, danger: true, run: () => this.unlink(l.from, l.to) })),
            ...(links.length > 1 ? [{ sep: true }, { label: `모두 끊기 (${links.length})`, danger: true, run: () => this.unlinkAll(object) }] : []),
          ],
        });
      }
      entries.push({
        label: '역할 바꾸기',
        sub: [
          ...FLOW_KINDS.map(([k, label]) => ({ label, checked: c.kind === k, disabled: c.kind === k, run: () => this.joinAs(object, k) })),
          { sep: true },
          { label: '공정 흐름에서 빼기', danger: true, run: () => this.joinAs(object, null) },
        ],
      });
    } else if (!piece.structure) {
      entries.push({
        label: '공정 흐름에 넣기',
        sub: FLOW_KINDS.map(([k, label]) => ({ label, run: () => this.joinAs(object, k) })),
      });
      const from = this.senders(object);
      entries.push({
        label: '앞 공정에서 연결 받기',
        hint: '공정 스테이션이 됩니다',
        sub: from.map((s) => ({ label: s.name, hint: hint(s), run: () => this.joinAs(object, 'station', () => this.linkDirect(s.object, object)) })),
      });
      const targets = this.receivers(object);
      const kind = this.cell.components.some((t) => t.kind === 'source') ? 'station' : 'source';
      entries.push({
        label: '다음 공정으로 연결',
        hint: `${FLOW_NAME[kind]}이 됩니다`,
        sub: targets.map((t) => ({ label: t.name, hint: hint(t), run: () => this.joinAs(object, kind, () => this.linkDirect(object, t.object)) })),
      });
    }
    entries.push({ sep: true }, { label: '선택', run: () => this.ctx.select?.(object) }, { label: '화면 맞춤', hint: 'F', run: () => this.ctx.frame?.(object) });
    if (this.ctx.remove) entries.push({ label: '삭제', hint: 'Delete', danger: true, run: () => this.ctx.remove(object) });
    return entries;
  }

  // The menu for a right click on a link's arrow.
  linkMenu(link) {
    const a = this.nameOf(link.from);
    const b = this.nameOf(link.to);
    const between = this.cell.components.filter(
      (x) => x.flow && x.object !== link.from && x.object !== link.to && this.sends(x.object) && x.kind !== 'source',
    );
    const k = this.carrierOf(link.from, link.to);
    const reach = this.reachOf?.get(`${link.from}>${link.to}`);
    const robots = this.ctx.robots?.() ?? [];
    const carry = [
      { label: '직접 (운반 없음)', checked: k.by === 0, run: () => this.setCarrier(link, 0) },
      { label: 'AMR', checked: k.by === 1, run: () => this.setCarrier(link, 1) },
      { label: '작업자', checked: k.by === 2, run: () => this.setCarrier(link, 2) },
      { sep: true },
      { label: '로봇 (닿는 로봇 자동)', checked: k.by === 3 && !k.robot, run: () => this.setCarrier(link, 3) },
      ...robots.map((r) => {
        const got = reach?.get(r.root);
        return {
          label: `로봇 ${r.name}`,
          checked: k.by === 3 && k.robot === r.root,
          hint: got ? (got.reach ? '닿음' : '⚠ 안 닿음') : '도달 확인 중…',
          run: () => this.setCarrier(link, 3, r.root),
        };
      }),
    ];
    return [
      { title: `${a} → ${b}` },
      { label: '운반', hint: k.by === 3 ? `로봇 ${k.robot ? this.robotName(k.robot) : '(자동)'}` : CARRIER_NAMES[k.by] || '직접', sub: carry },
      ...(k.reach ? [{ label: `⚠ ${k.reach}`, disabled: true }] : []),
      { label: '연결 끊기', hint: 'Delete', danger: true, run: () => this.unlink(link.from, link.to) },
      { label: '방향 뒤집기', hint: `${b} → ${a}`, run: () => this.reverseLink(link) },
      {
        label: '사이에 끼우기',
        sub: between.map((x) => ({ label: x.name, hint: FLOW_NAME[x.kind] ?? x.kind, run: () => this.insertBetween(link, x.object) })),
      },
      { sep: true },
      { label: `${a} 선택`, run: () => this.ctx.select?.(link.from) },
      { label: `${b} 선택`, run: () => this.ctx.select?.(link.to) },
    ];
  }

  // The piece a link is being drawn from in link mode, and where it
  // stands: { object, at } or null.
  linkingFrom() {
    const from = this.link?.from ?? this.carry?.from;
    const at = from != null ? this.componentOf(from)?.flow?.at : null;
    return from != null && at ? { object: from, at } : null;
  }

  // The link handle the pointer is over, grown a little.
  hoverHandle(object) {
    if (this.hovered === object) return;
    this.hovered = object;
    for (const o of this.markers.children) {
      if (!o.userData.handle) continue;
      const s = o.userData.size * (o.userData.handle === object ? 1.3 : 1);
      o.scale.set(s, s, 1);
    }
  }

  // Linking on to `object` of role `kind`: from the last, and on from it
  // unless the parts end there. `note` said with it.
  linkOn(object, kind, name, note = '') {
    const from = this.link?.from;
    if (!this.link) return null;
    // The parts end there: linking ends with it.
    const done = !['source', 'station', 'buffer', 'conveyor'].includes(kind);
    const more = [note, done ? '배출까지 이었습니다' : ''].filter(Boolean).join(' · ');
    const tail = more ? ` · ${more}` : '';
    if (from && from !== object) {
      if (kind === 'source') {
        this.ctx.toast('소재 공급으로는 보낼 수 없습니다');
        return null;
      }
      const have = this.componentOf(from)?.flow?.next ?? [];
      const via = this.link.via;
      this.link.via = null;
      const said = `${this.componentOf(from)?.name ?? ''} →${via != null ? ` 로봇 ${this.robotName(via)} →` : ''} ${name}${tail}`;
      if (via != null) {
        this.send({ op: 'flow_carrier', object: from, to: object, by: 3, robot: via }, () => this.ctx.toast(said));
      } else if (!have.includes(object)) {
        this.send({ op: 'flow_next', object: from, next: [...have, object] }, () => this.ctx.toast(said));
      } else {
        this.ctx.toast(`이미 이어져 있습니다: ${said}`);
      }
    } else if (!from && done) {
      this.ctx.toast('보내는 쪽(공급·공정·버퍼·컨베이어)부터 누르세요');
      return null;
    } else if (note) {
      this.ctx.toast(`${name}: ${note}`);
    } else if (!from) {
      this.ctx.toast(`${name}에서 시작 · 다음 설비를 누르세요 (로봇이 나르게 하려면 로봇을 먼저)`);
    }
    if (this.link?.once) this.link = null;
    else if (done) this.stopLinking();
    else this.link.from = object;
    return object;
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
  // How hard each station works, once the flow has run: a disc over it
  // from green (idle) through yellow to red (always at work or down), and
  // the busiest marked as the bottleneck.
  drawLoads(at) {
    const stations = this.cell.components.filter((c) => c.kind === 'station' && c.flow?.oee && at.has(c.object));
    const load = (c) => (c.flow.oee.busy ?? 0) + (c.flow.oee.down ?? 0);
    if (!stations.some((c) => load(c) > 0)) return new Set();
    const top = stations.reduce((b, c) => (load(c) > load(b) ? c : b), stations[0]);
    for (const c of stations) {
      const u = Math.min(1, load(c));
      // Green to yellow to red.
      const color = new THREE.Color().setHSL((1 - u) * 0.33, 0.85, 0.5);
      const [x, y, z] = at.get(c.object);
      const disc = new THREE.Mesh(
        new THREE.CircleGeometry(0.28, 40),
        new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0.75, depthTest: false }),
      );
      disc.position.set(x, y, z + 0.03);
      disc.renderOrder = 7;
      this.markers.add(disc);
      // Its name with its load, in one: two labels would cover each other.
      const text = c === top && stations.length > 1 ? `${c.name} · 병목` : c.name;
      const size = this.signSize(at);
      if (this.showLabels) {
        this.markers.add(this.label(text, x, y, z + 0.3 + size, c === top && stations.length > 1 ? 'bottleneck' : 'load', size));
      }
    }
    return new Set(stations.map((c) => c.object));
  }

  // Each process's name over it — source, station, buffer, sink — as a
  // sign its own size in the scene: small from afar, so that a big floor's
  // do not pile up, and read as one comes near. A station whose load is
  // shown has its name with it (`loaded`), not again.
  drawNames(at, loaded) {
    const named = this.cell.components.filter(
      (c) => ['source', 'station', 'buffer', 'sink'].includes(c.kind) && at.has(c.object) && !loaded.has(c.object),
    );
    const size = this.signSize(at);
    const key = `${this.showLabels}|${size.toFixed(2)}|` + named.map((c) => `${c.object}:${c.name}:${at.get(c.object).map((v) => v.toFixed(2))}`).join('|');
    if (key === this.namesDrawn) return;
    this.namesDrawn = key;
    for (const child of [...this.names.children]) child.removeFromParent();
    if (!this.showLabels) return;
    for (const c of named) {
      const [x, y, z] = at.get(c.object);
      this.names.add(this.label(c.name, x, y, z + 0.3 + size, 'name', size));
    }
  }

  // Over each station once the flow has run, a sign of how it is doing:
  // its state — RUN, IDLE, DOWN, 보전, BLOCKED — in its colour, the share
  // of the time it has worked, the cycles it has done and the parts in it
  // now against the most it may hold. A buffer: the parts in it.
  drawStatus(at) {
    if (!this.showLabels || !this.cell.flow?.started) return;
    const size = this.signSize(at);
    for (const c of this.cell.components) {
      const f = c.flow;
      if (!f || !at.has(c.object) || !['station', 'buffer'].includes(c.kind)) continue;
      const [x, y, z] = at.get(c.object);
      let state = '';
      let text;
      if (c.kind === 'station') {
        state = f.down ? 'down' : f.maint ? 'maint' : f.working ? 'run' : f.out ? 'blocked' : f.unreachable ? 'unreach' : 'idle';
        const busy = Math.round((f.oee?.busy ?? 0) * 100);
        text = `가동 ${busy}% · 처리 ${f.made ?? 0} · 안 ${f.inside ?? 0}${f.holds ? `/${f.holds}` : ''}`;
      } else {
        text = `보관 ${(f.queue ?? 0) + (f.out ?? 0)}${c.props?.capacity ? `/${c.props.capacity}` : ''}`;
      }
      const sign = this.label(text, x, y, z + 0.3 + size * 2.15, `status:${state}`, size);
      this.markers.add(sign);
    }
  }

  // How far each station's work in hand has got: a pie over it, filling
  // as the cycle goes, while it works; none while it does not.
  drawProgress(at) {
    if (!this.showLabels) return;
    const size = this.signSize(at);
    for (const c of this.cell.components) {
      const done = c.kind === 'station' ? c.flow?.progress : null;
      if (done == null || !at.has(c.object)) continue;
      const [x, y, z] = at.get(c.object);
      const pie = new THREE.Sprite(this.pieMaterial(Math.round(done * 100)));
      pie.userData.kept = true;
      pie.scale.set(size * 1.5, size * 1.5, 1);
      pie.userData.sign = { size: size * 1.5, aspect: 1 };
      // Over the station's label, which stands `size` high at z + 0.3 + size.
      pie.position.set(x, y, z + 0.3 + size * (this.cell.flow?.started ? 3.6 : 2.4));
      pie.renderOrder = 9;
      this.markers.add(pie);
    }
  }

  // The pie for `pct` per cent done, drawn once and kept: a cycle's
  // progress is told ten times a second.
  pieMaterial(pct) {
    this.pies ??= new Map();
    let m = this.pies.get(pct);
    if (m) return m;
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 128;
    const ctx = canvas.getContext('2d');
    ctx.beginPath();
    ctx.arc(64, 64, 60, 0, Math.PI * 2);
    ctx.fillStyle = 'rgba(20, 24, 30, 0.85)';
    ctx.fill();
    // From twelve o'clock, round the way a clock goes.
    ctx.beginPath();
    ctx.moveTo(64, 64);
    ctx.arc(64, 64, 54, -Math.PI / 2, -Math.PI / 2 + (Math.PI * 2 * pct) / 100);
    ctx.closePath();
    ctx.fillStyle = '#3ddc84';
    ctx.fill();
    ctx.beginPath();
    ctx.arc(64, 64, 30, 0, Math.PI * 2);
    ctx.fillStyle = 'rgba(20, 24, 30, 0.95)';
    ctx.fill();
    ctx.fillStyle = '#fff';
    ctx.font = '600 26px system-ui, sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(`${pct}%`, 64, 66);
    const texture = new THREE.CanvasTexture(canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    m = new THREE.SpriteMaterial({ map: texture, depthTest: false, transparent: true });
    this.pies.set(pct, m);
    return m;
  }

  // How tall a sign over a flow piece is, in the scene: 1/80 of the flow's
  // spread, 0.35–1 m — a cell's under half a metre, a plant's a metre —
  // so that from afar a big floor's do not pile up over it, and they are
  // read as one comes near.
  signSize(at) {
    const all = [...at.values()];
    const spread = all.length
      ? Math.hypot(
          Math.max(...all.map((p) => p[0])) - Math.min(...all.map((p) => p[0])),
          Math.max(...all.map((p) => p[1])) - Math.min(...all.map((p) => p[1])),
        )
      : 0;
    return Math.min(1.0, Math.max(0.35, spread / 80));
  }

  // The signs shown, or hidden: names, loads and progress go; the load
  // discs and the flow's arrows stay.
  // The flow's arrows shown, or hidden: hidden, none can be chosen either.
  setArrows(on) {
    this.showArrows = on;
    try {
      localStorage.setItem('kynex.arrows', on ? 'on' : 'off');
    } catch {}
    if (!on) this.chosenLink = null;
    this.drawMarkers();
  }

  setLabels(on) {
    this.showLabels = on;
    try {
      localStorage.setItem('kynex.labels', on ? 'on' : 'off');
    } catch {}
    this.namesDrawn = null;
    this.drawMarkers();
  }

  // Each sign as tall as it is in the scene, but never taller on the screen
  // than `most` pixels: close up, it no longer grows to cover what it
  // names. `perPixel` gives metres a pixel at a point's distance.
  fitSigns(camera, perPixel, most = 20) {
    for (const group of [this.markers, this.names]) {
      for (const o of group.children) {
        const sign = o.userData.sign;
        if (!sign) continue;
        const h = Math.min(sign.size, most * perPixel(o.position.distanceTo(camera.position)));
        o.scale.set(h * sign.aspect, h, 1);
      }
    }
  }

  // A word over the scene, always facing the camera, `size` metres high in
  // it: a `load` dark, the `bottleneck` red, a `name` blue-grey. The same
  // word drawn again reuses its picture: a running flow's loads are told
  // ten times a second, and a picture made each time makes the view stutter.
  label(text, x, y, z, style, size) {
    const key = `${text}|${style}`;
    this.labels ??= new Map();
    let kept = this.labels.get(key);
    if (!kept) {
      if (this.labels.size > 400) {
        for (const k of this.labels.values()) {
          k.material.map.dispose();
          k.material.dispose();
        }
        this.labels.clear();
      }
      kept = this.paintLabel(text, style);
      this.labels.set(key, kept);
    }
    const sprite = new THREE.Sprite(kept.material);
    // Kept for the next time: not to be let go with the markers.
    sprite.userData.kept = true;
    // Smaller as it is farther, as a sign in the scene is; never taller on
    // the screen than a line of text (see `fitSigns`).
    sprite.scale.set((size * kept.w) / 64, size, 1);
    sprite.userData.sign = { size, aspect: kept.w / 64 };
    sprite.position.set(x, y, z);
    sprite.renderOrder = 9;
    return sprite;
  }

  // A label's picture, made once for each word.
  paintLabel(text, style) {
    if (style.startsWith('status:')) return this.paintStatus(text, style.slice(7));
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    const font = `600 ${style === 'name' ? 34 : 40}px system-ui, sans-serif`;
    ctx.font = font;
    const w = Math.ceil(ctx.measureText(text).width) + 28;
    canvas.width = w;
    canvas.height = 64;
    ctx.font = font;
    ctx.fillStyle =
      {
        bottleneck: 'rgba(200, 40, 40, 0.92)',
        name: 'rgba(38, 66, 104, 0.88)',
        linknum: 'rgba(30, 90, 160, 0.9)',
        linkbad: 'rgba(190, 50, 50, 0.92)',
        robotpick: 'rgba(110, 70, 190, 0.92)',
        robotchosen: 'rgba(170, 90, 255, 0.98)',
      }[style] ??
      'rgba(20, 24, 30, 0.82)';
    ctx.beginPath();
    ctx.roundRect(0, 4, w, 56, 12);
    ctx.fill();
    ctx.fillStyle = '#fff';
    ctx.textBaseline = 'middle';
    ctx.fillText(text, 14, 33);
    const texture = new THREE.CanvasTexture(canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    const material = new THREE.SpriteMaterial({ map: texture, depthTest: false, transparent: true });
    return { material, w };
  }

  // A status sign: a chip of the state's colour with its word, then the
  // figures.
  paintStatus(text, state) {
    const STATES = {
      run: ['RUN', '#2fbf71'],
      idle: ['IDLE', '#6b7480'],
      down: ['DOWN', '#d9534f'],
      maint: ['보전', '#3d7fd9'],
      blocked: ['BLOCKED', '#d99a2b'],
      unreach: ['로봇 안 닿음', '#b8443c'],
    };
    const chip = STATES[state];
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    const font = '600 34px system-ui, sans-serif';
    const bold = '800 30px system-ui, sans-serif';
    ctx.font = bold;
    const cw = chip ? Math.ceil(ctx.measureText(chip[0]).width) + 24 : 0;
    ctx.font = font;
    const w = Math.ceil(ctx.measureText(text).width) + 28 + (chip ? cw + 8 : 0);
    canvas.width = w;
    canvas.height = 64;
    ctx.fillStyle = 'rgba(20, 24, 30, 0.86)';
    ctx.beginPath();
    ctx.roundRect(0, 4, w, 56, 12);
    ctx.fill();
    ctx.textBaseline = 'middle';
    if (chip) {
      ctx.fillStyle = chip[1];
      ctx.beginPath();
      ctx.roundRect(8, 10, cw, 44, 9);
      ctx.fill();
      ctx.fillStyle = '#fff';
      ctx.font = bold;
      ctx.fillText(chip[0], 20, 33);
    }
    ctx.font = font;
    ctx.fillStyle = '#fff';
    ctx.fillText(text, (chip ? cw + 16 : 0) + 14, 33);
    const texture = new THREE.CanvasTexture(canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    const material = new THREE.SpriteMaterial({ map: texture, depthTest: false, transparent: true });
    return { material, w };
  }

  // The handle a link is dragged from: an arrow in an orange disc, ringed
  // white. Made once (drawMarkers leaves it be).
  dotTexture() {
    if (this.dot) return this.dot;
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 64;
    const ctx = canvas.getContext('2d');
    ctx.beginPath();
    ctx.arc(32, 32, 27, 0, Math.PI * 2);
    ctx.fillStyle = '#ff8a3d';
    ctx.fill();
    ctx.lineWidth = 5;
    ctx.strokeStyle = '#ffffff';
    ctx.stroke();
    ctx.lineWidth = 6;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.beginPath();
    ctx.moveTo(18, 32);
    ctx.lineTo(44, 32);
    ctx.moveTo(34, 22);
    ctx.lineTo(44, 32);
    ctx.lineTo(34, 42);
    ctx.stroke();
    this.dot = new THREE.CanvasTexture(canvas);
    this.dot.colorSpace = THREE.SRGBColorSpace;
    return this.dot;
  }

  // The dot or the arrow at (x, y) on the screen, `project` giving a
  // point's place there: { handle } — a piece's object to drag a link
  // from — within 12 pixels, else { link } — an arrow, { from, to } —
  // within 10 of its line; or null. Judged on the screen, the same however
  // near or far.
  pickMarker(x, y, project) {
    let best = null;
    // A piece's handle first: small, and where a link starts.
    for (const o of this.markers.children) {
      if (!o.userData.handle) continue;
      const [px, py] = project(o.position);
      const d = Math.hypot(px - x, py - y);
      if (d < 16 && (!best || d < best.d)) best = { d, handle: o.userData.handle };
    }
    if (best) return { handle: best.handle };
    // Then a robot's sign: within its width across, 12 pixels up and down
    // of its middle — the nearest, where they overlap.
    let sign = null;
    for (const o of this.markers.children) {
      if (o.userData.robotPick == null) continue;
      const [px, py] = project(o.position);
      const half = Math.min(60, 10 * (o.scale.x / Math.max(o.scale.y, 1e-6)));
      const d = Math.hypot(px - x, py - y);
      if (Math.abs(px - x) < half && Math.abs(py - y) < 12 && (!sign || d < sign.d)) sign = { d, robot: o.userData.robotPick };
    }
    if (sign) return { robot: sign.robot };
    for (const o of this.markers.children) {
      const l = o.userData.link;
      if (!l || !o.userData.a) continue;
      const [ax, ay] = project(o.userData.a);
      const [bx, by] = project(o.userData.b);
      const [dx, dy] = [bx - ax, by - ay];
      const t = Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / Math.max(dx * dx + dy * dy, 1e-9)));
      const d = Math.hypot(ax + t * dx - x, ay + t * dy - y);
      if (d < 10 && (!best || d < best.d)) best = { d, link: l };
    }
    return best ? { link: best.link } : null;
  }

  // The arrow chosen in the 3D view, to undo with Delete; or none.
  chooseLink(link) {
    const same = link && this.chosenLink?.from === link.from && this.chosenLink?.to === link.to;
    if (same || (!link && !this.chosenLink)) return;
    this.chosenLink = link;
    this.drawMarkers();
    if (link) {
      const name = (u) => this.componentOf(u)?.name ?? '';
      this.ctx.toast(`연결 ${name(link.from)} → ${name(link.to)} · Delete 키로 끊기`);
    }
  }

  // The arrow chosen, undone. Whether there was one.
  cutChosenLink() {
    const link = this.chosenLink;
    if (!link) return false;
    this.chosenLink = null;
    const have = this.componentOf(link.from)?.flow?.next ?? [];
    const name = (u) => this.componentOf(u)?.name ?? '';
    this.send({ op: 'flow_next', object: link.from, next: have.filter((n) => n !== link.to) }, () =>
      this.ctx.toast(`연결을 끊었습니다: ${name(link.from)} → ${name(link.to)}`),
    );
    this.drawMarkers();
    return true;
  }

  drawMarkers() {
    for (const child of [...this.markers.children]) {
      child.removeFromParent();
      if (child.userData.kept) continue;
      // An arrow is a group: its shaft and its head each their own.
      child.traverse((o) => {
        o.geometry?.dispose();
        // The link dot's picture is made once, kept.
        if (o.material?.map && o.material.map !== this.dot) o.material.map.dispose();
        o.material?.dispose();
      });
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
    // The process flow: an arrow from each part of it to each it sends to,
    // the colour of what carries along it — by robot through that robot —,
    // red where its robot cannot reach, and its number in the flow.
    const at = new Map(this.cell.components.filter((c) => c.flow?.at).map((c) => [c.object, c.flow.at]));
    const arrows = [];
    const numbers = this.linkNumbers();
    this.prefetchReach(at);
    const size = this.signSize(at);
    for (const c of this.cell.components) {
      for (const n of c.flow?.next ?? []) {
        const a = at.get(c.object);
        const b = at.get(n);
        if (!a || !b) continue;
        const from = new THREE.Vector3(a[0], a[1], a[2] + 0.05);
        const to = new THREE.Vector3(b[0], b[1], b[2] + 0.05);
        if (to.distanceTo(from) < 1e-6) continue;
        const k = this.carrierOf(c.object, n);
        const chosen = this.chosenLink?.from === c.object && this.chosenLink?.to === n;
        // In the logistics mode, those not carried as chosen are faint.
        const same = !this.carry || (k.by === this.carry.by && (this.carry.by !== 3 || (k.robot ?? null) === this.carry.robot));
        const color = chosen ? 0xff8a3d : k.reach ? 0xff5f5f : CARRIER_COLOURS[k.by] ?? 0x5aa9ff;
        // By robot: over the robot's base, the way the part goes.
        const via = k.by === 3 && k.via ? new THREE.Vector3(k.via[0], k.via[1], Math.max(a[2], b[2]) + 0.4) : null;
        const legs = via ? [[from, via], [via, to]] : [[from, to]];
        legs.forEach(([p0, q0], i) => {
          const last = i === legs.length - 1;
          // Kept a little to the right of the way it goes: a way back
          // along the same leg — to a robot and back from it — is seen
          // beside it, not on top of it.
          const side = new THREE.Vector3().crossVectors(q0.clone().sub(p0), new THREE.Vector3(0, 0, 1));
          if (side.lengthSq() > 1e-9) side.normalize().multiplyScalar(0.07);
          const p = p0.clone().add(side);
          const q = q0.clone().add(side);
          const way = q.clone().sub(p);
          const length = way.length();
          if (length < 1e-6) return;
          // The leg to a robot with a smaller head: the way it goes.
          const head = Math.min(last ? (chosen ? 0.4 : 0.25) : 0.15, length * 0.3);
          const arrow = new THREE.ArrowHelper(way.normalize(), p, length, color, head, last ? (chosen ? 0.2 : 0.12) : 0.08);
          arrow.userData.link = { from: c.object, to: n };
          arrow.userData.a = p.clone();
          arrow.userData.b = q.clone();
          arrow.traverse((o) => {
            if (o.material) {
              o.material.depthTest = false;
              o.material.transparent = true;
              o.material.opacity = same ? 0.9 : 0.3;
            }
            o.renderOrder = 8;
          });
          arrows.push(arrow);
        });
        // Its number, and by what it goes when not by itself.
        const num = numbers.get(`${c.object}>${n}`);
        const what = k.by === 3 ? `로봇 ${k.robot ? this.robotName(k.robot) : '(자동)'}` : CARRIER_NAMES[k.by] ?? '';
        // The chosen robot's reach along it, in the logistics mode.
        const r = this.carry?.by === 3 && this.carry.robot ? this.reachOf?.get(`${c.object}>${n}`)?.get(this.carry.robot) : null;
        const can = r ? (r.reach ? '' : `${this.robotName(this.carry.robot)} 안 닿음`) : '';
        const text = [num, what, k.reach ? '⚠ 안 닿음' : '', can].filter(Boolean).join(' · ');
        if (text) {
          // Not over the robot, which stays to be clicked: along the way on.
          const mid = via ? via.clone().add(to).multiplyScalar(0.5) : from.clone().add(to).multiplyScalar(0.5);
          const sign = this.label(text, mid.x, mid.y, mid.z + size * 0.6, k.reach ? 'linkbad' : 'linknum', size * 0.8);
          sign.userData.link = { from: c.object, to: n };
          arrows.push(sign);
        }
      }
    }
    if (this.showArrows) for (const a of arrows) this.markers.add(a);
    // In either link mode, a sign over each robot, seen through whatever
    // stands in front of it — a machine hides the robot that tends it —:
    // clicked or dropped on, that robot.
    if ((this.link && !this.link.once) || this.carry || this.link?.via != null) {
      for (const r of this.ctx.robots?.() ?? []) {
        const top = this.ctx.robotTop?.(r.root);
        if (!top) continue;
        const chosen = this.link?.via === r.root || (this.carry?.by === 3 && this.carry.robot === r.root);
        const sign = this.label(`로봇 ${r.name}`, top[0], top[1], top[2] + 0.6, chosen ? 'robotchosen' : 'robotpick', 0.3);
        sign.userData.robotPick = r.root;
        sign.renderOrder = 12;
        this.markers.add(sign);
      }
    }
    // A link on its way through a robot: from the sender to the robot,
    // dashed, until the receiver is clicked.
    if (this.link?.via != null && this.link.from != null) {
      const a = at.get(this.link.from);
      const top = this.ctx.robotTop?.(this.link.via);
      if (a && top) {
        const geometry = new THREE.BufferGeometry().setFromPoints([
          new THREE.Vector3(a[0], a[1], a[2] + 0.05),
          new THREE.Vector3(top[0], top[1], top[2]),
        ]);
        const line = new THREE.Line(geometry, new THREE.LineDashedMaterial({ color: 0xb57bff, dashSize: 0.08, gapSize: 0.05, depthTest: false, transparent: true }));
        line.computeLineDistances();
        line.renderOrder = 9;
        this.markers.add(line);
      }
    }
    // In link mode, a handle over each piece that sends parts on: dragged
    // onto other equipment, it links the two. Not shown otherwise — the
    // right-click menu links as well, without them over everything.
    const linking = this.link != null && !this.link.once;
    for (const c of linking ? this.cell.components : []) {
      const a = at.get(c.object);
      if (!a || !this.sends(c.object)) continue;
      const dot = new THREE.Sprite(new THREE.SpriteMaterial({ map: this.dotTexture(), depthTest: false, transparent: true }));
      dot.material.sizeAttenuation = false;
      const size = this.hovered === c.object ? 0.0286 : 0.022;
      dot.scale.set(size, size, 1);
      dot.position.set(a[0], a[1], a[2] + 0.45);
      dot.renderOrder = 11;
      dot.userData.handle = c.object;
      dot.userData.size = 0.022;
      this.markers.add(dot);
    }
    this.drawNames(at, this.drawLoads(at));
    this.drawStatus(at);
    this.drawProgress(at);
    // The AMRs' lanes: a line between each two stops linked.
    const stops = new Map(this.cell.components.filter((c) => c.stop_at).map((c) => [c.object, c.stop_at]));
    const lanes = [];
    for (const c of this.cell.components) {
      for (const n of c.lanes ?? []) {
        if (c.object > n) continue;
        const a = stops.get(c.object);
        const b = stops.get(n);
        if (a && b) lanes.push(a[0], a[1], 0.01, b[0], b[1], 0.01);
      }
    }
    if (lanes.length) {
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.Float32BufferAttribute(lanes, 3));
      const lines = new THREE.LineSegments(geometry, new THREE.LineBasicMaterial({ color: 0xf2b53c, transparent: true, opacity: 0.9, depthTest: false }));
      lines.renderOrder = 8;
      this.markers.add(lines);
    }
    // Each sensor's beam: red while a part breaks it.
    for (const c of this.cell.components) {
      const b = c.sensor;
      if (!b?.from || !b?.to) continue;
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.Float32BufferAttribute([...b.from, ...b.to], 3));
      const line = new THREE.Line(
        geometry,
        new THREE.LineBasicMaterial({ color: b.detected ? 0xff4d4d : 0x3ddc84, transparent: true, opacity: 0.9, depthTest: false }),
      );
      line.renderOrder = 8;
      this.markers.add(line);
    }
  }
}
