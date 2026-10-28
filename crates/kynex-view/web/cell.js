// The cell: what each object is to the process — a CNC machine, a pallet,
// a gripper, a fence — set in the inspector, and the places parts stand,
// marked in the scene. KYNEX keeps the roles (the `cell` event) and the
// parts, which arrive in the scene like any other object.

import * as THREE from 'three';
import { tFor, tr } from './i18n.js';

const t = tFor('cell');

// The flow's roles, as a menu offers them.
const FLOW_KINDS = [
  ['source', t('소재 공급')],
  ['station', t('공정 스테이션')],
  ['buffer', t('버퍼')],
  ['sink', t('배출')],
];
const FLOW_NAME = { ...Object.fromEntries(FLOW_KINDS), conveyor: t('컨베이어') };
// What carries parts, or where it stops: never a place in the flow, and
// why a link let go on it is not made there.
const NOT_PLACES = {
  amr_stop: t('AMR 정지 위치는 공정에 넣지 않습니다 — AMR이 서는 곳입니다'),
  operator: t('작업자는 운반 수단입니다 — 화살표를 오른쪽 클릭해 운반: 작업자로 정하세요'),
};
// Where the way splits, how parts are shared out (`rule`); where ways meet
// at a station, how it takes them (`join`).
const RULES = [
  [0, t('번갈아'), t('갈래마다 차례로, 자리 있는 곳으로')],
  [1, t('대기 적은 곳'), t('기다리는 부품이 적은 쪽으로')],
  [2, t('무작위'), t('자리 있는 곳 중 아무 데나')],
  [3, t('비율'), t('정한 비율대로 (자리가 날 때까지 기다림)')],
];
const JOINS = [
  [0, t('도착 순'), t('먼저 온 부품부터, 투입 수만큼씩')],
  [1, t('조립'), t('들어오는 경로마다 1개씩 모아 하나로')],
];
// Templates that are the building, not equipment of the flow.
const STRUCTURE = new Set([
  'fence', 'guard_rail', 'factory_wall', 'factory_column', 'floor_zone', 'ceiling_light',
  'crane_rail', 'oht_rail', 'roll_door', 'pipe_rack', 'amr_stop', 'overhead_crane',
  // What a robot stands on: a click there means the robot.
  'pedestal',
]);

// The roles, the process flow's first; those of a robot cell (a robot
// program's machines and pallets, not the flow's) said so — a robot cell's
// 배출 팔레트 is no 배출 of the flow.
const KINDS = [
  ['', t('없음')],
  ['source', t('공정 흐름: 소재 공급')],
  ['station', t('공정 흐름: 공정 스테이션')],
  ['buffer', t('공정 흐름: 버퍼')],
  ['sink', t('공정 흐름: 배출')],
  ['conveyor', t('컨베이어')],
  ['amr', 'AMR'],
  ['amr_stop', t('AMR 정지 위치')],
  ['operator', t('작업자 (사람)')],
  ['cnc', t('로봇 셀: CNC 장비')],
  ['molder', t('로봇 셀: 사출 성형기')],
  ['infeed', t('로봇 셀: 투입 팔레트')],
  ['outfeed', t('로봇 셀: 배출 팔레트')],
  ['gripper', t('그리퍼')],
  ['positioner', t('포지셔너')],
  ['fence', t('안전 펜스')],
  ['door', t('문 (장비의 움직이는 부분)')],
  ['connector', t('연결점')],
  ['sensor', t('포토센서')],
  ['actuator', t('동작축 (스토퍼·리프터·직선·회전)')],
];

// What each I/O point is, in words: the robot's outputs and its inputs.
const IO_LABELS = {
  'gripper:close': t('그리퍼 닫기'),
  'molder:open': t('가드 열기'),
  'molder:close': t('가드 닫기'),
  'molder:door_open': t('가드 열림'),
  'molder:door_closed': t('가드 닫힘'),
  close: t('문 닫기'),
  closed: t('닫힘 확인'),
  open: t('문 열기'),
  clamp: t('척 물림'),
  unclamp: t('척 풀기'),
  start: t('시작'),
  door_open: t('문 열림'),
  door_closed: t('문 닫힘'),
  clamped: t('척 물림됨'),
  unclamped: t('척 풀림됨'),
  idle: t('대기 중'),
  done: t('완료'),
  part_at_end: t('부품 도착'),
  extend: t('전진'),
  retract: t('후진'),
  extended: t('전진 끝'),
  retracted: t('후진 끝'),
  detected: t('감지'),
};

// An I/O number as the controller names it.
const ioName = (n, output) => {
  const kind = output ? 'DO' : 'DI';
  if (n <= 7) return `${kind} ${n}`;
  if (n <= 15) return t('설정 {kind} {n}', { kind, n: n - 8 });
  return t('툴 {kind} {n}', { kind, n: n - 16 });
};

// Each kind's settings, as the inspector shows them: key, label, unit, and
// how the shown number maps to KYNEX's.
export const FIELDS = {
  cnc: [
    ['cycle', t('가공 시간'), 's', 1],
    ['door', t('문 개폐'), 's', 1],
    ['chuck', t('척 개폐'), 's', 1],
  ],
  infeed: [
    ['rows', t('행'), '', 1],
    ['cols', t('열'), '', 1],
    ['pitch', t('간격'), 'mm', 1000],
  ],
  gripper: [['actuate', t('개폐 시간'), 's', 1]],
  molder: [
    ['cycle', t('성형 시간'), 's', 1],
    ['door', t('가드 개폐'), 's', 1],
  ],
  conveyor: [
    ['rule', t('분기 방식'), '', 1, RULES.map(([v, l]) => [v, l])],
    ['speed', t('속도'), 'mm/s', 1000],
    ['feed', t('공급 간격'), 's', 1],
    ['drain', t('끝에서 치움'), '', 1, [[0, t('아니오')], [1, t('예')]]],
  ],
  amr: [
    ['speed', t('주행 속도'), 'mm/s', 1000],
    ['capacity', t('적재량'), t('개'), 1],
    ['gather', t('모아 싣기 최대 대기'), 's', 1],
    ['battery', t('배터리'), t('분'), 1],
    ['charge', t('충전 시간'), t('분'), 1],
  ],
  amr_stop: [['charger', t('충전소'), '', 1, [[0, t('아님')], [1, t('충전소')]]]],
  positioner: [['speed', t('회전 속도'), '°/s', 180 / Math.PI]],
  // A choice is a list of [value, label].
  door: [
    ['swing', t('동작'), '', 1, [[0, t('미닫이')], [1, t('여닫이')]]],
    ['axis', t('축'), '', 1, [[0, t('부품 X')], [1, t('부품 Y')], [2, t('부품 Z')]]],
    ['travel', t('거리'), 'mm', 1000],
    ['angle', t('각도'), '°', 180 / Math.PI],
  ],
  source: [
    ['rule', t('분기 방식'), '', 1, RULES.map(([v, l]) => [v, l])],
    ['feed', t('공급 간격'), 's', 1],
    ['kanban', t('칸반 한도'), t('개'), 1],
    ['dist', t('시간 분포'), '', 1, [[0, t('고정')], [1, t('균등 ±폭')], [2, t('삼각 ±폭')], [3, t('정규 σ')], [4, t('지수')]]],
    ['spread', t('폭·σ'), 's', 1],
    ['capacity', t('내보낼 자리'), t('개'), 1],
    ['speed', t('이송 속도'), 'mm/s', 1000],
  ],
  station: [
    ['rule', t('분기 방식'), '', 1, RULES.map(([v, l]) => [v, l])],
    ['cycle', t('처리 시간'), 's', 1],
    ['dist', t('시간 분포'), '', 1, [[0, t('고정')], [1, t('균등 ±폭')], [2, t('삼각 ±폭')], [3, t('정규 σ')], [4, t('지수')]]],
    ['spread', t('폭·σ'), 's', 1],
    ['setup', t('품종 교체 시간'), 's', 1],
    ['mtbf', t('고장 간격'), 's', 1],
    ['mttr', t('수리 시간'), 's', 1],
    ['pm_every', t('보전 간격'), 's', 1],
    ['pm_time', t('보전 시간'), 's', 1],
    ['scrap', t('불량률'), '%', 100],
    ['crew', t('작업자'), '', 1, [[0, t('필요 없음')], [1, t('작업자 필요')]]],
    ['holds', t('동시 투입 가능 수'), t('개'), 1],
    ['capacity', t('대기 자리'), t('개'), 1],
    ['join', t('합류 방식'), '', 1, JOINS.map(([v, l]) => [v, l])],
    ['takes', t('투입 수'), t('개'), 1],
    ['makes', t('배출 수'), t('개'), 1],
    ['speed', t('이송 속도'), 'mm/s', 1000],
  ],
  buffer: [
    ['rule', t('분기 방식'), '', 1, RULES.map(([v, l]) => [v, l])],
    ['capacity', t('용량'), t('개'), 1],
    ['policy', t('꺼내는 순서'), '', 1, [[0, t('먼저 들어온 것 (FIFO)')], [1, t('나중 들어온 것 (LIFO)')]]],
    ['speed', t('이송 속도'), 'mm/s', 1000],
  ],
  operator: [
    ['speed', t('걷는 속도'), 'm/s', 1],
    ['capacity', t('한 번에 드는 수'), t('개'), 1],
    ['wage', t('임금'), t('원/시'), 1],
    ['shift_from', t('근무 시작'), t('시'), 1],
    ['shift_to', t('근무 끝'), t('시'), 1],
  ],
  sensor: [
    ['axis', t('빔 방향'), '', 1, [[0, t('부품 X')], [1, t('부품 Y')], [2, t('부품 Z')]]],
    ['travel', t('빔 길이'), 'mm', 1000],
  ],
  actuator: [
    ['swing', t('동작'), '', 1, [[0, t('직선')], [1, t('회전')]]],
    ['axis', t('축'), '', 1, [[0, t('부품 X')], [1, t('부품 Y')], [2, t('부품 Z')]]],
    ['travel', t('행정'), 'mm', 1000],
    ['angle', t('각도'), '°', 180 / Math.PI],
    ['door', t('동작 시간'), 's', 1],
  ],
  connector: [
    ['plug', t('종류'), '', 1, [[0, t('로봇 받침면')], [1, t('컨베이어 입구')], [2, t('컨베이어 출구')], [3, t('받침면에 올라섬')]]],
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
const CARRIER_NAMES = ['', 'AMR', t('작업자'), t('로봇')];
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
          if (file.size > 2 * 1024 * 1024) return this.ctx.toast(t('주문표가 너무 큽니다 (2 MB까지)'));
          this.send({ op: 'flow_orders', object, csv: await file.text() }, () => this.ctx.toast(t('주문표를 넣었습니다')));
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
      // A linked piece in the inspector: selected.
      const pick = e.target.closest('[data-select]')?.dataset.select;
      if (pick != null) return this.ctx.select?.(pick);
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
        return this.ctx.toast(t('I/O 번호는 0–17입니다 (0–7 표준, 8–15 설정 가능, 16–17 툴)'));
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
        if (!broker) return this.ctx.toast(t('MQTT 브로커 주소를 넣으세요 (호스트:1883)'));
        if (!topic) return this.ctx.toast(t('AMR 상태 토픽을 넣으세요 (VDA 5050: uagv/v2/제조사/일련번호/state)'));
        const request = { op: 'mirror', object, kind: 'mqtt', broker, topic, user: value('mirror-mqtt-user') };
        const password = document.getElementById('mirror-mqtt-password').value;
        if (password) request.password = password;
        document.getElementById('mirror-mqtt-password').value = '';
        return this.send(request);
      }
      if (value('mirror-kind') === 'umati') {
        const url = value('mirror-url');
        const node = value('mirror-node');
        if (!url.startsWith('opc.tcp://')) return this.ctx.toast(t('서버 주소를 넣으세요 (opc.tcp://호스트:4840)'));
        if (!node) return this.ctx.toast(t('장비 노드를 넣으세요 (예: ns=3;s=Machine1)'));
        const request = { op: 'mirror', object, kind: 'umati', url, node, security: value('mirror-security'), user: value('mirror-user') };
        const password = document.getElementById('mirror-password').value;
        if (password) request.password = password;
        document.getElementById('mirror-password').value = '';
        return this.send(request);
      }
      const agent = value('mirror-agent');
      if (!agent) return this.ctx.toast(t('에이전트 주소를 넣으세요 (http://호스트:5000)'));
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
  }

  async send(request, then) {
    try {
      const answer = await post(request);
      if (!answer.ok) this.ctx.toast(answer.error ? tr(answer.error) : t('요청을 처리하지 못했습니다'));
      else then?.(answer);
    } catch (err) {
      this.ctx.toast(t('KYNEX에 요청하지 못했습니다: {error}', { error: err.message }));
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
    // Links new since the last: drawn bold a moment, to be seen at once.
    const keys = new Set();
    for (const c of cell.components ?? []) for (const n of c.flow?.next ?? []) keys.add(`${c.object}>${n}`);
    for (const h of cell.hubs ?? []) {
      for (const a of h.ins) keys.add(`${a}>${h.robot}`);
      for (const b of h.outs) keys.add(`${h.robot}>${b}`);
    }
    const added = this.linkKeys ? [...keys].filter((k) => !this.linkKeys.has(k)) : [];
    this.linkKeys = keys;
    if (added.length) {
      this.flash = new Set(added);
      clearTimeout(this.flashOff);
      this.flashOff = setTimeout(() => {
        this.flash = null;
        this.drawMarkers();
      }, 2000);
    }
    this.drawMarkers();
    const selected = this.ctx.selected();
    if (selected != null) this.showFor(selected);
  }

  // The roles offered, as the level has them, and the one `kind` has.
  roles(kind = '') {
    const key = kind;
    if (key === this.rolesKey) return;
    this.rolesKey = key;
    this.kind.innerHTML = KINDS.map(([v, label]) => `<option value="${v}">${label}</option>`)
      .join('');
  }

  // The selected object's role, in the inspector.
  showFor(uid) {
    const { escapeHtml } = this.ctx;
    const c = this.componentOf(uid);
    this.roles(c?.kind ?? '');
    if (document.activeElement !== this.kind) this.kind.value = c?.kind ?? '';
    const fields = c ? FIELDS[c.kind] || [] : [];
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
      actions = `<button class="text-btn" data-cell="fill">${c.kind === 'infeed' ? t('소재 채우기') : t('완성품 채우기')}</button><button class="text-btn quiet" data-cell="empty">${t('비우기')}</button><span class="muted">${t('{n}칸', { n: c.slots.length })}</span>`;
    } else if (c && c.kind === 'cnc') {
      if (!c.seat) {
        actions = `<span class="warn-text">${escapeHtml(t('하위 물체에 chuck이 없습니다'))}</span>`;
      } else {
        const busy = m?.progress != null;
        actions =
          `<button class="text-btn" data-machine="${m?.opening ? 'close' : 'open'}"${busy ? ' disabled' : ''}>${m?.opening ? t('문 닫기') : t('문 열기')}</button>` +
          `<button class="text-btn" data-machine="${m?.clamped ? 'unclamp' : 'clamp'}"${busy ? ' disabled' : ''}>${m?.clamped ? t('척 풀기') : t('척 물림')}</button>` +
          `<button class="text-btn" data-machine="start"${busy ? ' disabled' : ''}>${t('가공 시작')}</button>` +
          `<button class="text-btn quiet" data-cell="empty"${busy || m?.clamped ? ' disabled' : ''}>${t('척 비우기')}</button>`;
      }
    } else if (c && c.kind === 'molder') {
      if (!c.seat) {
        actions = `<span class="warn-text">${escapeHtml(t('하위 물체에 mould가 없습니다'))}</span>`;
      } else {
        const busy = m?.progress != null;
        actions =
          `<button class="text-btn" data-machine="${m?.opening ? 'close' : 'open'}"${busy ? ' disabled' : ''}>${m?.opening ? t('가드 닫기') : t('가드 열기')}</button>` +
          `<button class="text-btn" data-machine="start"${busy ? ' disabled' : ''}>${t('성형 시작')}</button>` +
          `<button class="text-btn quiet" data-cell="empty"${busy ? ' disabled' : ''}>${t('금형 비우기')}</button>`;
      }
    } else if (c && c.kind === 'amr') {
      const stops = this.cell.components.filter((s) => s.kind === 'amr_stop');
      actions = stops.length
        ? stops
            .map((s) => `<button class="text-btn" data-drive="${escapeHtml(s.object)}"${c.amr?.driving ? ' disabled' : ''}>→ ${escapeHtml(tr(s.name))}</button>`)
            .join('') +
          (c.amr?.driving ? `<span class="muted">${t('주행 중')}</span>` : '') +
          (c.amr?.battery != null ? `<span class="muted">${t('배터리 {pct}%', { pct: Math.round(c.amr.battery * 100) })}${c.amr.charging ? ` · ${t('충전하러 감/충전 중')}` : ''}</span>` : '')
        : `<span class="muted">${t('라이브러리에서 AMR 정지 위치를 놓으세요')}</span>`;
    } else if (c && c.kind === 'operator') {
      // The stations this person may work at: none picked, any.
      const stations = this.cell.components.filter((s) => s.kind === 'station');
      const have = c.skills ?? [];
      actions = stations.length
        ? `<span class="muted">${t('맡는 공정')}</span>${stations
            .map((s) => `<button class="text-btn${have.includes(s.object) ? ' on' : ''}" data-skill="${escapeHtml(s.object)}" aria-pressed="${have.includes(s.object)}">${have.includes(s.object) ? '✓ ' : ''}${escapeHtml(tr(s.name))}</button>`)
            .join('')}${have.length ? '' : `<span class="muted">${t('(아무것도 고르지 않으면 모두)')}</span>`}`
        : `<span class="muted">${t('작업자 필요 스테이션이 생기면 맡을 곳을 고를 수 있습니다')}</span>`;
    } else if (c && c.kind === 'amr_stop') {
      // The lanes it has, each with its way: clicked, that stop selected.
      const goes = (x, y) => (this.componentOf(x)?.lanes ?? []).includes(y);
      const linked = this.cell.components.filter((s) => s.kind === 'amr_stop' && s.object !== c.object && (goes(c.object, s.object) || goes(s.object, c.object)));
      const me = this.stopNumber(c.object);
      const way = (s) => {
        const o = this.stopNumber(s.object);
        return goes(c.object, s.object) && goes(s.object, c.object) ? `${me} ↔ ${o}` : goes(c.object, s.object) ? `${me} → ${o}` : `${o} → ${me}`;
      };
      actions =
        `<div class="flow-links"><span class="muted">${t('AMR 길')}</span>${
          linked.length
            ? linked.map((s) => `<button class="text-btn" data-select="${escapeHtml(s.object)}" title="${t('{name} 선택', { name: escapeHtml(tr(s.name)) })}">${way(s)}</button>`).join('')
            : `<span class="muted">${t('없음')}</span>`
        }</div>` + `<span class="muted insp-hint">${t('길 잇기: 연결 모드(Shift+C)에서 번호표를 다른 번호표로 끌기 · 방향·끊기: 길을 오른쪽 클릭')}</span>`;
    } else if (c && c.flow) {
      const f = c.flow;
      // Where its parts come from and go: the links drawn, each with what
      // carries along it — clicked, that piece selected. Links are made
      // with the arrows in the 3D view.
      const via = (from, to) => {
        const k = this.carrierOf(from, to);
        return k.by ? ` <span class="muted">(${escapeHtml(this.carrierText(k.by, k.robot))})</span>` : '';
      };
      const chip = (o, from, to) => `<button class="text-btn" data-select="${escapeHtml(o)}" title="${t('{name} 선택', { name: escapeHtml(this.nameOf(o)) })}">${escapeHtml(this.nameOf(o))}${via(from, to)}</button>`;
      const ins = this.insOf(c.object);
      const outs = this.outsOf(c.object);
      const nexts =
        (c.kind === 'source' ? '' : `<div class="flow-links"><span class="muted">${t('이전 공정')}</span>${ins.length ? ins.map((o) => chip(o, o, c.object)).join('') : `<span class="muted">${t('없음')}</span>`}</div>`) +
        (c.kind === 'sink' ? '' : `<div class="flow-links"><span class="muted">${t('이후 공정')}</span>${outs.length ? outs.map((o) => chip(o, c.object, o)).join('') : `<span class="muted">${t('없음')}</span>`}</div>`);
      const state =
        c.kind === 'sink'
          ? t('배출 {n}개', { n: f.made })
          : c.kind === 'station'
            ? `${f.unreachable ? `⚠ ${t('로봇이 닿지 않아 이 장비로는 보내지 않습니다 ({why})', { why: escapeHtml(tr(f.unreachable)) })} · ` : ''}${f.down ? t('고장 · 수리 중') : f.maint ? t('보전 중') : f.working ? t('작업 중 {s} s', { s: Math.ceil(f.left ?? 0) }) : f.out ? t('막힘 (보낼 곳 없음)') : t('대기')} · ${t('장비 안 {n}개', { n: `${f.inside ?? 0}${f.holds ? `/${f.holds}` : ''}` })} · ${t('대기 {n}', { n: f.queue })} · ${t('완료 {n}', { n: f.made })}${f.oee && f.oee.good + f.oee.bad ? ` · OEE ${Math.round(f.oee.oee * 100)}%` : ''}`
            : c.kind === 'conveyor'
              ? t('끝에서 기다림 {n}', { n: f.out })
              : `${c.kind === 'source' ? `${t('공급 {n}개', { n: f.made })} · ` : ''}${t('보관 {n}', { n: f.out + f.queue + (f.waiting ?? 0) })}${f.waiting ? ` ${t('(운반 대기 {n})', { n: f.waiting })}` : ''}`;
      // Products: as many as any source mixes.
      const last = (m) => (m ?? []).reduce((k, w, i) => (w > 0 ? i + 1 : k), 0);
      const used = Math.max(1, ...this.cell.components.map((s) => last(s.flow?.mix)));
      const names = ['A', 'B', 'C', 'D', 'E', 'F'];
      let products = '';
      if (c.kind === 'source') {
        const mix = f.mix ?? [];
        products = `<div class="flow-products"><span class="muted">${t('제품 비율')}</span>${names
          .map((n, i) => `<label>${n}<input type="number" class="num" data-mix="${i}" value="${mix[i] ?? (i === 0 && !mix.length ? 1 : 0)}" min="0" step="1"></label>`)
          .join('')}</div>`;
        products += `<div class="flow-products"><span class="muted">${t('주문표')}${f.orders ? ` ${t('{n}건 (공급 간격 대신 주문대로)', { n: f.orders })}` : ''}</span><button class="text-btn" data-orders-load title="${t('CSV: 시각(분), 제품(A–F), 수량, 납기(분) — 한 줄에 주문 하나')}">${t('CSV 불러오기')}</button>${f.orders ? `<button class="text-btn quiet" data-orders-clear>${t('지우기')}</button>` : ''}</div>`;
      } else if (c.kind === 'station' && used > 1) {
        const cycles = f.cycles ?? [];
        products = `<div class="flow-products"><span class="muted">${t('품종별 처리 시간 (비우면 기본)')}</span>${names
          .slice(0, used)
          .map((n, i) => `<label>${n}<input type="number" class="num" data-cycle="${i}" value="${cycles[i] > 0 ? cycles[i] : ''}" min="0" step="1" placeholder="${c.props.cycle}"></label>`)
          .join('')}</div>`;
      }
      // The equipment it stands for: picking that shows this role.
      const near = this.ctx.equipmentNear?.(c.object, c.host) ?? [];
      const host =
        c.kind === 'conveyor' || (!near.length && !c.host)
          ? ''
          : `<div class="flow-products"><span class="muted" title="${t('이 공정이 어느 장비의 공정인지: 그 장비를 눌러도 이 설정이 보입니다')}">${t('공정 「{name}」의 장비', { name: escapeHtml(tr(c.name)) })}</span><select data-flow-host><option value="">${t('없음')}</option>${near
              .map((q) => `<option value="${escapeHtml(q.root)}"${q.root === c.host ? ' selected' : ''}>${escapeHtml(tr(q.name))}${q.d != null ? ` (${q.d.toFixed(1)} m)` : ''}</option>`)
              .join('')}</select></div>`;
      // Each link's products, where there are several.
      let routes = '';
      if (used > 1 && c.kind !== 'sink' && f.next.length) {
        routes = f.next
          .map((to) => {
            const target = this.componentOf(to);
            const have = f.routes?.[to] ?? [];
            return `<div class="flow-route"><span class="muted">→ ${escapeHtml(tr(target?.name ?? to))}</span>${names
              .slice(0, used)
              .map((n, i) => `<button class="text-btn${have.includes(i) ? ' on' : ''}" data-route-to="${escapeHtml(to)}" data-product="${i}" title="${t('이 길로 보낼 제품 (아무것도 고르지 않으면 모두)')}">${n}</button>`)
              .join('')}${have.length ? '' : `<span class="muted">${t('모두')}</span>`}</div>`;
          })
          .join('');
      }
      actions =
        `${nexts}<span class="muted">${state}</span>${host}${products}${routes}` +
        `<span class="muted insp-hint">${t('잇기·끊기: 연결 모드(Shift+C)에서 설비를 끌어 다른 설비에 놓기 · 화살촉을 끌어 옮기기(빈 곳: 끊기) · ⑂분기/⑃합류 표시를 눌러 방식 정하기')}</span>`;
    } else if (c && c.kind === 'actuator') {
      const a = c.actuator ?? {};
      const at = a.out >= 1 ? t('전진 끝') : a.out <= 0 ? t('후진 끝') : a.extending ? t('전진 중') : t('후진 중');
      actions =
        `<button class="text-btn" data-machine="extend"${a.extending ? ' disabled' : ''}>${t('전진')}</button>` +
        `<button class="text-btn" data-machine="retract"${a.extending ? '' : ' disabled'}>${t('후진')}</button>` +
        `<span class="muted">${at}</span>`;
    } else if (c && c.kind === 'sensor') {
      actions = c.sensor?.detected ? `<span class="warn-text">${t('감지')}</span>` : `<span class="muted">${t('감지 없음')}</span>`;
    } else if (c && c.kind === 'positioner') {
      const p = c.positioner;
      const angle = p?.angle != null ? Math.round((p.angle * 180) / Math.PI) : 0;
      actions =
        [0, 180]
          .map((a) => `<button class="text-btn" data-turn="${a}"${p?.turning ? ' disabled' : ''}>→ ${a}°</button>`)
          .join('') + `<span class="muted">${p?.turning ? t('회전 중') : t('지금 {angle}°', { angle })}</span>`;
    } else if (c && (c.kind === 'door' || c.kind === 'connector')) {
      const mm = (c.point ?? []).map((v) => Math.round(v * 1000)).join(', ');
      const what = c.kind === 'door' ? t('회전 중심') : t('연결점');
      actions =
        this.pickingFor === uid
          ? `<span class="muted">${t('{what}이 될 점을 3D 화면에서 누르세요 (Esc 취소)', { what })}</span>`
          : `<button class="text-btn" data-pick="1">${t('{what} 찍기', { what })}</button><span class="muted">${mm} mm</span>`;
      if (c.kind === 'door') {
        actions += c.machine_of
          ? `<span class="muted">${t('거리 0: 장비 X 방향으로 자기 폭만큼. 각도·거리가 음수면 반대로')}</span>`
          : `<span class="warn-text">${t('CNC·사출기 역할 물체 아래에 있어야 움직입니다')}</span>`;
      }
    } else if (c && c.kind === 'gripper' && !c.robot) {
      actions = `<span class="warn-text">${t('로봇의 링크가 아닙니다')}</span>`;
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
        return `<label class="insp-field">${output ? '→' : '←'} ${escapeHtml(IO_LABELS[`${c.kind}:${key}`] ?? IO_LABELS[key] ?? key)}<input type="number" class="num" data-io="${escapeHtml(key)}" value="${n ?? ''}" min="0" max="17" placeholder="${t('없음')}"><span>${n != null ? ioName(n, output) : ''}</span></label>`;
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
      document.getElementById('mirror-go').textContent = m ? t('그만 따라가기') : t('따라가기');
      const st = c.machine?.mirror;
      const fleet = c.amr?.follow;
      let text = '';
      if (m && amr && fleet) {
        const said = [
          fleet.connected ? t('연결됨') : t('연결 안 됨'),
          fleet.driving != null ? (fleet.driving ? t('주행 중') : t('정지')) : null,
          fleet.battery != null ? t('배터리 {pct}%', { pct: Math.round(fleet.battery) }) : null,
          fleet.age != null ? t('{n}초 전', { n: fleet.age }) : t('위치 기다리는 중'),
          ...(fleet.errors ?? []).map(tr),
        ].filter(Boolean);
        text = said.join(' · ') + (fleet.error ? ` · ${tr(fleet.error)}` : '');
      } else if (m && st) {
        const last = st.last;
        if (st.error) text = tr(st.error);
        if (last) {
          const said = [last.device, last.execution, last.door ? t('문 {state}', { state: last.door }) : null, last.part_count != null ? t('생산 {n}', { n: last.part_count }) : null, last.program]
            .filter(Boolean)
            .join(' · ');
          text = `${said} (${t('{n}초 전', { n: st.age ?? '?' })})${st.error ? ` · ${tr(st.error)}` : ''}`;
        }
      } else if (m) {
        text = t('연결 중…');
      }
      // A umati server's certificate, trusted on first meeting.
      if (m?.kind === 'umati' && m.trust) text += `${text ? ' · ' : ''}${t('서버 인증서 {id}…', { id: m.trust.slice(0, 16) })}`;
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
        return `<label class="insp-field">${command ? 'PLC →' : '→ PLC'} ${escapeHtml(label)}<input type="text" class="num plc-address" data-plc="${escapeHtml(key)}" value="${escapeHtml(n ?? '')}" placeholder="${t('없음')}" spellcheck="false"></label>`;
      };
      document.getElementById('cell-plc').innerHTML =
        plcPoints.outputs.map((k) => row(k, true)).join('') + plcPoints.inputs.map((k) => row(k, false)).join('');
    }
  }

  // A CNC machine's state, in words and a bar.
  machineHtml(m, kind) {
    const door = m.door >= 1 ? t('열림') : m.door <= 0 ? t('닫힘') : m.opening ? t('여는 중') : t('닫는 중');
    const molder = kind === 'molder';
    const travel = m.chuck ?? (m.clamped ? 1 : 0);
    const chuck = m.clamped ? (travel >= 1 ? t('물림') : t('무는 중')) : travel <= 0 ? t('풀림') : t('푸는 중');
    const part = m.part === 'raw' ? t('소재') : m.part === 'finished' ? (molder ? t('성형품') : t('완성품')) : t('없음');
    let html = molder
      ? `<div class="machine-state"><span>${t('가드')} <b>${door}</b></span><span>${t('금형')} <b>${part}</b></span></div>`
      : `<div class="machine-state"><span>${t('문')} <b>${door}</b></span><span>${t('척')} <b>${chuck}</b></span><span>${t('척 안')} <b>${part}</b></span></div>`;
    if (m.progress != null) {
      const pct = Math.round(m.progress * 100);
      html += `<div class="machine-bar"><span style="width:${(m.progress * 100).toFixed(1)}%"></span><em>${molder ? t('성형 중 {pct}%', { pct }) : t('가공 중 {pct}%', { pct })} · ${t('남은 {s} s', { s: Math.ceil(m.left) })}</em></div>`;
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
    return tr(this.ctx.robots?.().find((r) => r.root === root)?.name || this.ctx.nameOf?.(root) || '');
  }

  // Each link's number in the flow, by "from>to": its step from the
  // sources — 1, 2, 3 — and, where the way splits, which branch: 2-1,
  // 2-2, on along each branch (3-1, 3-2) until they meet again.
  linkNumbers() {
    const flow = this.cell.components.filter((c) => c.flow);
    const next = new Map(flow.map((c) => [c.object, [...(c.flow.next ?? []), ...(c.flow.derived ?? [])].filter((n) => flow.some((t) => t.object === n))]));
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
    const what =
      by === 3 ? (robot ? t('로봇 {name}', { name: this.robotName(robot) }) : t('로봇 (닿는 로봇 자동)')) : by === 1 && robot ? this.amrName(robot) : CARRIER_NAMES[by] || t('직접 (운반 없음)');
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

  carrierText(by, robot) {
    return by === 3 ? (robot ? t('로봇 {name}', { name: this.robotName(robot) }) : t('로봇 (자동)')) : CARRIER_NAMES[by] || t('직접');
  }

  // ---- Robots in the flow's picture ------------------------------------

  // Whether `object` is a robot.
  isRobot(object) {
    return (this.ctx.robots?.() ?? []).some((r) => r.root === object);
  }

  // Whether `object` is an AMR.
  isAmr(object) {
    return this.componentOf(object)?.kind === 'amr';
  }

  // Whether `object` is drawn as a place in the flow that parts pass
  // through, not stop at: a robot, or an AMR.
  isHub(object) {
    return this.isRobot(object) || this.isAmr(object);
  }

  // The robot or AMR among `uids` (and `root`), if one is.
  hubOf(uids, root = null) {
    return this.ctx.robotOf?.(uids) ?? this.amrOf(uids, root);
  }

  // A robot's or an AMR's name, said as such.
  hubName(object) {
    return this.isAmr(object) ? this.amrName(object) : t('로봇 {name}', { name: this.robotName(object) });
  }

  // Over a robot or an AMR, where its arrows meet: [x, y, z], or null.
  hubTop(object) {
    if (!this.isAmr(object)) return this.ctx.robotTop?.(object) ?? null;
    const box = this.ctx.bounds?.(object);
    return box && !box.isEmpty() ? [(box.min.x + box.max.x) / 2, (box.min.y + box.max.y) / 2, box.max.z + 0.1] : null;
  }

  // The robots drawn as places in the flow: [{ robot, ins, outs }].
  hubs() {
    return this.cell.hubs ?? [];
  }

  // Piece `from` sends robot `robot` its parts; or (`to`) the robot puts
  // parts into it. `remove` takes that away.
  hubLink(robot, { from = null, to = null, remove = false }, then = null) {
    const said = from != null ? `${this.nameOf(from)} → ${this.hubName(robot)}` : `${this.hubName(robot)} → ${this.nameOf(to)}`;
    this.send({ op: 'hub_link', robot, from: from ?? undefined, to: to ?? undefined, remove }, () => {
      this.ctx.toast(remove ? t('연결을 끊었습니다: {link}', { link: said }) : t('이었습니다: {link}', { link: said }));
      then?.();
    });
  }

  startLinking() {
    this.link = { from: null };
    this.onLinking?.(true);
    this.drawMarkers();
    this.ctx.toast(t('연결 모드: 설비를 눌러 다음 설비로 끌어 놓으세요 (Shift+C·Esc: 끝)'));
  }

  stopLinking() {
    this.link = null;
    this.showPort(null);
    this.onLinking?.(false);
    this.drawMarkers();
  }

  // Whether a part can go on from component `object` — or a robot, which
  // puts the parts it takes somewhere.
  sends(object) {
    return ['source', 'station', 'buffer', 'conveyor'].includes(this.componentOf(object)?.kind) || this.isHub(object);
  }

  // A click while linking, on object `uids` (it and those above it; `root`
  // the placed equipment's, if it is some): the flow's equipment among them
  // linked from the last. Equipment not yet of the flow is made so: a
  // machine of a robot cell a station keeping its time, one with no role
  // the source if it is the first, else a station. Returns the one clicked,
  // to select.
  linkTo(uids, root = null) {
    // A robot or an AMR: the piece before sends it its parts — a sink
    // doing so becomes a buffer —; and it is where the next link starts.
    const robot = this.hubOf(uids, root);
    if (robot != null) {
      const from = this.link?.from;
      if (from != null && from !== robot && !this.isHub(from)) {
        if (this.componentOf(from)?.kind === 'sink') this.joinAs(from, 'buffer', () => this.hubLink(robot, { from }));
        else this.hubLink(robot, { from });
      } else if (from != null && ((this.isAmr(from) && this.isRobot(robot)) || (this.isRobot(from) && this.isAmr(robot)))) {
        this.hubLink(from, { to: robot });
      } else this.ctx.toast(t('{name}에서 시작 · 부품을 내려놓을 설비를 누르세요', { name: this.hubName(robot) }));
      if (this.link?.once) this.link = null;
      else if (this.link) this.link.from = null;
      this.drawMarkers();
      return null;
    }
    const piece = this.pieceOf(uids, root);
    if (piece?.c?.flow) return this.linkOn(piece.c.object, piece.c.kind, tr(piece.c.name));
    if (!piece || piece.structure) {
      this.ctx.toast(piece?.why ?? t('설비를 누르세요 (공정 흐름에 넣을 수 있는 것)'));
      return null;
    }
    const object = piece.object;
    const machine = piece.c && ['cnc', 'molder'].includes(piece.c.kind) ? piece.c : null;
    if (machine && !confirm(t('{name}의 로봇 셀 역할(가공기)을 공정 스테이션으로 바꿉니다. 로봇 프로그램에서 쓰던 문·척 동작은 쓰지 않게 됩니다.', { name: tr(machine.name) }))) return null;
    const kind = !machine && !this.link.from ? 'source' : 'station';
    const props = machine ? { cycle: machine.props.cycle } : {};
    this.send({ op: 'role', object, kind, props }, () => {
      const made = this.componentOf(object);
      this.linkOn(object, kind, tr(made?.name ?? ''), kind === 'source' ? t('소재 공급으로 정했습니다') : t('공정 스테이션으로 정했습니다 (공정 시간을 넣으세요)'));
    });
    return object;
  }

  // The flow's piece among `uids` (an object and those above it) that
  // sends parts on, if there is one: where a drag to link may start.
  sender(uids) {
    const robot = this.hubOf(uids);
    if (robot != null) return { object: robot, kind: this.isAmr(robot) ? 'amr' : 'robot', name: this.hubName(robot) };
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
    if (once && this.link?.once) this.link = null;
    // A drag is one arrow: no line left trailing from where it ended.
    else if (this.link) this.link.from = null;
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
    const carrier = cs.find((c) => c && (NOT_PLACES[c.kind] || c.kind === 'amr'));
    if (carrier) {
      const why = NOT_PLACES[carrier.kind] ?? t('AMR은 운반 수단입니다');
      return { object: carrier.object, c: carrier, name: tr(carrier.name), structure: true, why };
    }
    const c = cs.find((c) => c?.flow);
    if (c) return { object: c.object, c, name: tr(c.name), structure: false };
    const machine = cs.find((c) => c && ['cnc', 'molder'].includes(c.kind));
    if (machine) return { object: machine.object, c: machine, name: tr(machine.name), structure: false };
    const item = this.ctx.itemOf?.(uids[0]);
    const object = root ?? item?.root ?? uids[uids.length - 1] ?? null;
    if (object == null) return null;
    const structure = item ? STRUCTURE.has(item.template) : false;
    // A zone painted on the floor is the floor: nothing to link to.
    if (item?.template === 'floor_zone') return null;
    // A flow piece standing on the equipment — a station set at a machine
    // — is what the equipment means to the flow.
    const on = structure ? null : this.flowOn(object);
    if (on) return { object: on.object, c: on, name: tr(on.name), structure: false };
    return { object, c: this.componentOf(object) ?? null, name: tr(this.ctx.nameOf?.(object) ?? ''), structure };
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
    // A role on a part of placed equipment — a conveyor's belt, a table's
    // pallet —: the equipment's name, which tells two of them apart.
    const item = this.ctx.itemOf?.(object);
    if (item && item.root !== object && item.name) return tr(item.name);
    return tr(this.componentOf(object)?.name ?? this.ctx.nameOf?.(object) ?? '');
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
    if (have.includes(to)) return this.ctx.toast(t('이미 이어져 있습니다: {link}', { link: said }));
    if (this.componentOf(to)?.kind === 'source') return this.ctx.toast(t('소재 공급으로는 보낼 수 없습니다'));
    this.send({ op: 'flow_next', object: from, next: [...have, to] }, () => {
      this.ctx.toast(t('이었습니다: {link}', { link: said }));
      then?.();
    });
  }

  unlink(from, to, then = null) {
    const have = this.componentOf(from)?.flow?.next ?? [];
    if (this.chosenLink?.from === from && this.chosenLink?.to === to) this.chosenLink = null;
    this.send({ op: 'flow_next', object: from, next: have.filter((n) => n !== to) }, () => {
      this.ctx.toast(t('연결을 끊었습니다: {link}', { link: `${this.nameOf(from)} → ${this.nameOf(to)}` }));
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
    this.ctx.toast(t('{name}의 연결 {n}개를 끊었습니다', { name: this.nameOf(object), n: links.length }));
  }

  // The link the other way round: `to` sends to `from` instead.
  reverseLink({ from, to }) {
    if (!this.sends(to)) return this.ctx.toast(t('{name}에서는 부품이 나가지 않습니다', { name: this.nameOf(to) }));
    if (this.componentOf(from)?.kind === 'source') return this.ctx.toast(t('소재 공급으로는 보낼 수 없습니다'));
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
    if (machine && !confirm(t('{name}의 로봇 셀 역할(가공기)을 공정 스테이션으로 바꿉니다. 로봇 프로그램에서 쓰던 문·척 동작은 쓰지 않게 됩니다.', { name: tr(machine.name) }))) return;
    const props = machine ? { cycle: machine.props.cycle } : {};
    const name = this.nameOf(object);
    this.send({ op: 'role', object, kind, props }, () => {
      this.ctx.toast(kind ? `${name}: ${FLOW_NAME[kind]}${kind === 'station' ? ` ${t('(공정 시간을 넣으세요)')}` : ''}` : `${name}: ${t('공정 흐름에서 뺐습니다')}`);
      then?.();
    });
  }

  // What letting go of a link from `from` on `uids` (and `root`) would do:
  // { ok, text, object } — `object` the equipment it would land on.
  dropPreview(from, uids, root = null) {
    const robot = uids.length ? this.hubOf(uids, root) : null;
    if (robot != null) {
      // An AMR to a robot: the robot takes the parts off the AMR's top
      // plate where it stops beside it.
      if (this.isAmr(from) && this.isRobot(robot)) return { ok: true, text: `${this.hubName(from)} → ${this.hubName(robot)} (${t('로봇이 AMR 위에서 집음')})`, object: robot };
      // A robot to an AMR: the robot puts the parts onto it, called to
      // stop beside it.
      if (this.isRobot(from) && this.isAmr(robot)) return { ok: true, text: `${this.hubName(from)} → ${this.hubName(robot)} (${t('로봇이 AMR 위에 놓음')})`, object: robot };
      if (robot === from || this.isHub(from)) return { ok: false, text: t('로봇끼리, AMR끼리는 잇지 않습니다'), object: robot };
      const sink = this.componentOf(from)?.kind === 'sink' ? ` · ${t('배출은 버퍼가 됩니다')}` : '';
      return { ok: true, text: `${this.nameOf(from)} → ${this.hubName(robot)} (${this.isAmr(robot) ? t('AMR이 실어 감') : t('로봇이 가져감')})${sink}`, object: robot };
    }
    const piece = uids.length ? this.pieceOf(uids, root) : null;
    if (!piece) return { ok: false, text: t('다음 공정이 될 설비 위에 놓으세요'), object: null };
    const { object, c, name } = piece;
    const fromName = this.isHub(from) ? this.hubName(from) : this.nameOf(from);
    if (object === from || uids.includes(from)) return { ok: false, text: t('같은 설비입니다'), object };
    if (piece.structure) return { ok: false, text: piece.why ? `${name}: ${piece.why}` : `${name}: ${t('건물·구조물은 공정에 넣을 수 없습니다')}`, object };
    if (c?.flow) {
      if (c.kind === 'source') return { ok: false, text: `${name}: ${t('소재 공급으로는 보낼 수 없습니다')}`, object };
      if (this.isHub(from)) {
        return { ok: true, text: `${this.hubName(from)} → ${name} (${this.isAmr(from) ? t('AMR이 내려놓음') : t('로봇이 넣음')})`, object };
      }
      if ((this.componentOf(from)?.flow?.next ?? []).includes(object)) return { ok: false, text: t('이미 이어져 있습니다: {link}', { link: `${fromName} → ${name}` }), object };
      return { ok: true, text: `${fromName} → ${name}`, object };
    }
    return { ok: true, text: `${fromName} → ${name} · ${t('놓으면 공정 스테이션이 됩니다')}`, object };
  }

  // The menu for a right click on equipment `uids` (and `root`): linking
  // it on, linking to it, undoing its links, its role; or null for nothing.
  equipmentMenu(uids, root = null) {
    // A robot: what it carries, not a place in the flow.
    const robot = this.hubOf(uids, root);
    if (robot != null) return this.robotMenu(robot);
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
        const targets = this.receivers(object).filter((x) => !have.includes(x.object));
        entries.push({
          label: t('다음 공정으로 연결'),
          sub: targets.map((x) => ({ label: tr(x.name), hint: hint(x), run: () => this.linkDirect(object, x.object) })),
        });
      }
      if (c.kind !== 'source') {
        const from = this.senders(object).filter((s) => !(s.flow.next ?? []).includes(object));
        entries.push({
          label: t('앞 공정에서 연결 받기'),
          sub: from.map((s) => ({ label: tr(s.name), hint: hint(s), run: () => this.linkDirect(s.object, object) })),
        });
      }
      const links = this.linksOf(object);
      if (links.length) {
        entries.push({
          label: t('연결 끊기'),
          sub: [
            ...links.map((l) => ({ label: `${this.nameOf(l.from)} → ${this.nameOf(l.to)}`, danger: true, run: () => this.unlink(l.from, l.to) })),
            ...(links.length > 1 ? [{ sep: true }, { label: t('모두 끊기 ({n})', { n: links.length }), danger: true, run: () => this.unlinkAll(object) }] : []),
          ],
        });
      }
      entries.push({
        label: t('역할 바꾸기'),
        sub: [
          ...FLOW_KINDS.map(([k, label]) => ({ label, checked: c.kind === k, disabled: c.kind === k, run: () => this.joinAs(object, k) })),
          { sep: true },
          { label: t('공정 흐름에서 빼기'), danger: true, run: () => this.joinAs(object, null) },
        ],
      });
    } else if (!piece.structure) {
      entries.push({
        label: t('공정 흐름에 넣기'),
        sub: FLOW_KINDS.map(([k, label]) => ({ label, run: () => this.joinAs(object, k) })),
      });
      const from = this.senders(object);
      entries.push({
        label: t('앞 공정에서 연결 받기'),
        hint: t('공정 스테이션이 됩니다'),
        sub: from.map((s) => ({ label: tr(s.name), hint: hint(s), run: () => this.joinAs(object, 'station', () => this.linkDirect(s.object, object)) })),
      });
      const targets = this.receivers(object);
      const kind = this.cell.components.some((x) => x.kind === 'source') ? 'station' : 'source';
      entries.push({
        label: t('다음 공정으로 연결'),
        hint: t('{kind}이 됩니다', { kind: FLOW_NAME[kind] }),
        sub: targets.map((x) => ({ label: tr(x.name), hint: hint(x), run: () => this.joinAs(object, kind, () => this.linkDirect(object, x.object)) })),
      });
    }
    entries.push({ sep: true }, { label: t('선택'), run: () => this.ctx.select?.(object) }, { label: t('화면 맞춤'), hint: 'F', run: () => this.ctx.frame?.(object) });
    if (this.ctx.remove) entries.push({ label: t('삭제'), hint: 'Delete', danger: true, run: () => this.ctx.remove(object) });
    return entries;
  }

  // The menu for a right click on robot `robot`: the pieces it takes parts
  // from and puts them into, in the flow's picture.
  robotMenu(robot) {
    const name = this.hubName(robot);
    const hub = this.hubs().find((h) => h.robot === robot) ?? { ins: [], outs: [] };
    const pieces = this.cell.components.filter((c) => c.flow);
    const label = (c) => this.nameOf(c.object);
    return [
      { title: name },
      {
        label: t('가져올 곳 (설비 → 여기)'),
        sub: pieces
          .filter((c) => c.kind !== 'sink')
          .map((c) => ({
            label: label(c),
            checked: hub.ins.includes(c.object),
            run: () => this.hubLink(robot, { from: c.object, remove: hub.ins.includes(c.object) }),
          })),
      },
      {
        label: t('내려놓을 곳 (여기 → 설비)'),
        sub: pieces
          .filter((c) => c.kind !== 'source')
          .map((c) => ({
            label: label(c),
            checked: hub.outs.includes(c.object),
            run: () => this.hubLink(robot, { to: c.object, remove: hub.outs.includes(c.object) }),
          })),
      },
      { sep: true },
      { label: t('선택'), run: () => this.ctx.select?.(robot) },
      { label: t('화면 맞춤'), hint: 'F', run: () => this.ctx.frame?.(robot) },
    ];
  }

  // The menu for a right click on a link's arrow.
  linkMenu(link) {
    // An arrow to or from a robot in the picture.
    if (link.hub != null) {
      const a = this.isHub(link.from) ? this.hubName(link.from) : this.nameOf(link.from);
      const b = this.isHub(link.to) ? this.hubName(link.to) : this.nameOf(link.to);
      return [
        { title: `${a} → ${b}` },
        { label: t('연결 끊기'), hint: 'Delete', danger: true, run: () => this.cutHub(link) },
        { sep: true },
        { label: t('{name} 메뉴…', { name: this.hubName(link.hub) }), sub: this.robotMenu(link.hub).slice(1) },
      ];
    }
    const a = this.nameOf(link.from);
    const b = this.nameOf(link.to);
    const between = this.cell.components.filter(
      (x) => x.flow && x.object !== link.from && x.object !== link.to && this.sends(x.object) && x.kind !== 'source',
    );
    const k = this.carrierOf(link.from, link.to);
    const reach = this.reachOf?.get(`${link.from}>${link.to}`);
    const robots = this.ctx.robots?.() ?? [];
    const carry = [
      { label: t('직접 (운반 없음)'), checked: k.by === 0, run: () => this.setCarrier(link, 0) },
      { label: t('AMR (빈 AMR 자동)'), checked: k.by === 1 && !k.robot, run: () => this.setCarrier(link, 1) },
      ...this.cell.components
        .filter((c) => c.kind === 'amr')
        .map((a) => ({ label: t('{name}이 나름', { name: this.amrName(a.object) }), checked: k.by === 1 && k.robot === a.object, run: () => this.setCarrier(link, 1, a.object) })),
      { label: t('작업자'), checked: k.by === 2, run: () => this.setCarrier(link, 2) },
      { sep: true },
      { label: t('로봇 (닿는 로봇 자동)'), checked: k.by === 3 && !k.robot, run: () => this.setCarrier(link, 3) },
      // A robot by name: drawn through it — the arrow becomes two, into
      // the robot and out of it.
      ...robots.map((r) => {
        const got = reach?.get(r.root);
        return {
          label: t('로봇 {name}을 거쳐', { name: tr(r.name) }),
          hint: got ? (got.reach ? t('닿음') : t('⚠ 안 닿음')) : t('도달 확인 중…'),
          run: () => this.unlink(link.from, link.to, () => this.hubLink(r.root, { from: link.from }, () => this.hubLink(r.root, { to: link.to }))),
        };
      }),
    ];
    return [
      { title: `${a} → ${b}` },
      { label: t('운반'), hint: k.by === 3 ? (k.robot ? t('로봇 {name}', { name: this.robotName(k.robot) }) : t('로봇 (자동)')) : k.by === 1 && k.robot ? this.amrName(k.robot) : CARRIER_NAMES[k.by] || t('직접'), sub: carry },
      ...(k.reach ? [{ label: `⚠ ${tr(k.reach)}`, disabled: true }] : []),
      { label: t('연결 끊기'), hint: 'Delete', danger: true, run: () => this.unlink(link.from, link.to) },
      { label: t('방향 뒤집기'), hint: `${b} → ${a}`, run: () => this.reverseLink(link) },
      {
        label: t('사이에 끼우기'),
        sub: between.map((x) => ({ label: tr(x.name), hint: FLOW_NAME[x.kind] ?? x.kind, run: () => this.insertBetween(link, x.object) })),
      },
      { sep: true },
      { label: t('{name} 선택', { name: a }), run: () => this.ctx.select?.(link.from) },
      { label: t('{name} 선택', { name: b }), run: () => this.ctx.select?.(link.to) },
    ];
  }

  // ---- AMR stops and lanes ------------------------------------------------

  // Whether `object` is an AMR stop.
  isStop(object) {
    return this.componentOf(object)?.kind === 'amr_stop';
  }

  // The AMR stop among `uids` (an object and those above it), if one is.
  stopOfUids(uids) {
    return uids.map((u) => this.componentOf(u)).find((c) => c?.kind === 'amr_stop')?.object ?? null;
  }

  // A stop's number, as its name ends: P1, P2 — else its place in the list.
  stopNumber(object) {
    const n = this.nameOf(object).match(/(\d+)\s*$/)?.[1];
    if (n) return `P${n}`;
    const all = this.cell.components.filter((c) => c.kind === 'amr_stop');
    return `P${all.findIndex((c) => c.object === object) + 1}`;
  }

  // The lane between stops `a` and `b`: both ways, `a` to `b` (to), `b` to
  // `a` (from), or none.
  setLane(a, b, way) {
    const [p, q] = [this.stopNumber(a), this.stopNumber(b)];
    const said = { both: t('{p} ↔ {q} 양방향', { p, q }), to: t('{p} → {q} 한 방향', { p, q }), from: t('{p} → {q} 한 방향', { p: q, q: p }), none: t('{p} – {q} 길을 끊었습니다', { p, q }) }[way];
    this.send({ op: 'amr_lane', from: a, to: b, way }, () => this.ctx.toast(t('AMR 길: {said}', { said })));
  }

  // A lane dragged from stop `a` onto stop `b`: one way, `a` to `b`; the
  // other way there already, both ways.
  dragLane(a, b) {
    if (a === b) return;
    const back = (this.componentOf(b)?.lanes ?? []).includes(a);
    this.setLane(a, b, back ? 'both' : 'to');
    if (!back) this.ctx.toast(t('AMR 길: {p} → {q} 한 방향 · 반대로도 끌거나 길을 오른쪽 클릭하면 양방향', { p: this.stopNumber(a), q: this.stopNumber(b) }));
  }

  // The menu for a right click on a lane.
  laneMenu({ a, b }) {
    const [p, q] = [this.stopNumber(a), this.stopNumber(b)];
    const ab = (this.componentOf(a)?.lanes ?? []).includes(b);
    const ba = (this.componentOf(b)?.lanes ?? []).includes(a);
    return [
      { title: t('AMR 길 {p} – {q}', { p, q }) },
      { label: t('양방향'), hint: `${p} ↔ ${q}`, checked: ab && ba, run: () => this.setLane(a, b, 'both') },
      { label: t('{p} → {q} 한 방향', { p, q }), checked: ab && !ba, run: () => this.setLane(a, b, 'to') },
      { label: t('{p} → {q} 한 방향', { p: q, q: p }), checked: ba && !ab, run: () => this.setLane(a, b, 'from') },
      { sep: true },
      { label: t('길 끊기'), hint: 'Delete', danger: true, run: () => this.setLane(a, b, 'none') },
    ];
  }

  // The lane chosen, to undo with Delete; or none.
  chooseLane(lane) {
    this.chosenLane = lane;
    this.drawMarkers();
    if (lane) this.ctx.toast(t('AMR 길 {p} – {q} · Delete: 끊기 · 오른쪽 클릭: 방향', { p: this.stopNumber(lane.a), q: this.stopNumber(lane.b) }));
  }

  // ---- Splits and merges --------------------------------------------------

  // Where `object`'s parts go: the links drawn and those a robot carries.
  outsOf(object) {
    const f = this.componentOf(object)?.flow;
    return [...(f?.next ?? []), ...(f?.derived ?? [])];
  }

  // The pieces whose links go into `object`.
  insOf(object) {
    return this.cell.components.filter((c) => c.flow && c.object !== object && this.outsOf(c.object).includes(object)).map((c) => c.object);
  }

  // Link `from` → `to`'s share of `from`'s parts, 0 to 1, where `from`
  // splits them by share; else null.
  shareOf(from, to) {
    const c = this.componentOf(from);
    if (c?.props?.rule !== 3) return null;
    const outs = this.outsOf(from);
    if (outs.length < 2) return null;
    const w = (t) => c.flow?.shares?.[t] ?? 1;
    const sum = outs.reduce((a, t) => a + w(t), 0);
    return sum > 0 ? w(to) / sum : 0;
  }

  // Some of `object`'s settings changed, keeping its role; `said` once done.
  setProps(object, props, said) {
    const c = this.componentOf(object);
    if (!c) return;
    this.send({ op: 'role', object, kind: c.kind, props }, () => said && this.ctx.toast(said));
  }

  // The shares of `object`'s links asked for, as 7:3, in the order the
  // menu lists them.
  askShares(object) {
    const outs = this.outsOf(object);
    const c = this.componentOf(object);
    const now = outs.map((t) => +(c.flow?.shares?.[t] ?? 1).toFixed(2)).join(':');
    const names = outs.map((t, i) => `${i + 1}. ${this.nameOf(t)}`).join('\n');
    const text = prompt(`${t('{name}에서 갈라지는 비율 (예: 7:3)', { name: this.nameOf(object) })}\n${names}`, now);
    if (text == null) return;
    const ws = text.split(/[:,\s/]+/).filter(Boolean).map(Number);
    if (ws.length !== outs.length || ws.some((v) => !Number.isFinite(v) || v < 0) || !ws.some((v) => v > 0)) {
      return this.ctx.toast(t('갈래 {n}개의 비율을 {form} 꼴로 넣으세요', { n: outs.length, form: outs.map(() => 'n').join(':') }));
    }
    const send = (k) => {
      if (k === outs.length) {
        if (c.props.rule !== 3) this.setProps(object, { rule: 3 });
        return this.ctx.toast(`${this.nameOf(object)}: ${outs.map((t, i) => `${this.nameOf(t)} ${ws[i]}`).join(' : ')}`);
      }
      this.send({ op: 'flow_share', object, to: outs[k], share: ws[k] }, () => send(k + 1));
    };
    send(0);
  }

  // The menu for a split's or a merge's sign: how parts are shared out
  // there, or how the station takes them.
  badgeMenu({ kind, object }) {
    const c = this.componentOf(object);
    if (!c) return [];
    const name = this.nameOf(object);
    if (kind === 'split') {
      const outs = this.outsOf(object);
      const rule = c.props.rule ?? 0;
      const entries = [
        { title: t('⑂ {name}에서 {n}갈래로 갈라짐', { name, n: outs.length }) },
        ...RULES.map(([v, label, hint]) => ({
          label,
          hint,
          checked: rule === v,
          run: () => (v === 3 ? this.askShares(object) : this.setProps(object, { rule: v }, t('{name}: 갈라질 때 {rule}', { name, rule: label }))),
        })),
      ];
      if (rule === 3) {
        entries.push(
          { sep: true },
          ...outs.map((t) => ({ label: `→ ${this.nameOf(t)}`, hint: `${Math.round(100 * (this.shareOf(object, t) ?? 0))}%`, disabled: true })),
          { label: t('비율 바꾸기…'), run: () => this.askShares(object) },
        );
      }
      return entries;
    }
    const ins = this.insOf(object);
    const join = c.props.join ?? 0;
    return [
      { title: t('⑃ {n}갈래가 {name}로 모임', { n: ins.length, name }) },
      ...JOINS.map(([v, label, hint]) => ({
        label,
        hint: v === 1 ? t('경로마다 1개 · {n}개 → 1개', { n: ins.length }) : hint,
        checked: join === v,
        run: () => this.setProps(object, { join: v }, `${name}: ${v === 1 ? t('경로마다 1개씩 {n}개를 하나로 조립', { n: ins.length }) : t('먼저 온 부품부터')}`),
      })),
      { sep: true },
      ...ins.map((f) => ({ label: `← ${this.nameOf(f)}`, disabled: true })),
    ];
  }

  // ---- Ports: where a link is dragged from ---------------------------------

  // The port over equipment `uids` (and `root`) — a handle to drag a link
  // from —: { object, at, fresh } — `fresh` for equipment not yet in the
  // flow, which joins it as the drag ends. Null where no link can start.
  portOf(uids, root = null) {
    if (this.ctx.robotOf?.(uids) != null) return null;
    const amr = this.amrOf(uids, root);
    if (amr != null) {
      const at = this.hubTop(amr);
      return at ? { object: amr, at: [at[0], at[1], at[2] - 0.45], fresh: false } : null;
    }
    const piece = this.pieceOf(uids, root);
    if (!piece || piece.structure) return null;
    if (piece.c?.flow) {
      const at = piece.c.flow.at;
      if (!at) return null;
      // Parts end at a sink: dragged on from, it becomes a buffer that
      // sends them on (to an AMR, say).
      if (piece.c.kind === 'sink') return { object: piece.object, at, fresh: true, kind: 'buffer' };
      return this.sends(piece.object) ? { object: piece.object, at, fresh: false } : null;
    }
    const box = this.ctx.bounds?.(piece.object);
    if (!box || box.isEmpty()) return null;
    return { object: piece.object, at: [(box.min.x + box.max.x) / 2, (box.min.y + box.max.y) / 2, box.max.z - 0.4], fresh: true };
  }

  // The port shown over the equipment under the pointer, or none.
  showPort(port) {
    if ((port?.object ?? null) === (this.port?.object ?? null)) return;
    this.port = port;
    if (!this.portDot) {
      this.portDot = new THREE.Sprite(new THREE.SpriteMaterial({ map: this.dotTexture(), depthTest: false, transparent: true }));
      this.portDot.material.sizeAttenuation = false;
      this.portDot.renderOrder = 11;
      this.portDot.userData.kept = true;
      this.portDot.userData.size = 0.022;
    }
    this.portDot.removeFromParent();
    if (!port) return;
    this.portDot.position.set(port.at[0], port.at[1], port.at[2] + 0.45);
    this.portDot.scale.set(0.022, 0.022, 1);
    this.portDot.userData.handle = port.object;
    this.portDot.userData.fresh = port.fresh;
    // Over a handle link mode draws already: that one.
    if (!this.markers.children.some((o) => o.userData.handle === port.object)) this.markers.add(this.portDot);
  }

  // A drag from equipment `object` not yet in the flow, let go on `to`
  // (uids, and `root`): it joins the flow — the first supply, else a
  // station — and is linked on. Returns the equipment let go on.
  dragFromFresh(object, to, root = null, kind = null) {
    kind ??= this.cell.components.some((t) => t.kind === 'source') ? 'station' : 'source';
    this.joinAs(object, kind, () => {
      // Link mode kept on round it, as a drag from a piece already of the
      // flow keeps it (see `dragLink`).
      const mode = this.link;
      this.link = { from: object, once: true };
      this.linkTo(to, root);
      this.link = mode;
      if (mode) mode.from = null;
    });
    return this.pieceOf(to, root)?.object ?? null;
  }

  // The arrowhead of `link` dragged and let go on `to` (uids, and `root`):
  // the link goes there instead, keeping how parts go along it and its
  // share; let go on nothing (`to` empty), the link is undone.
  rewire(link, to, root = null) {
    if (!to.length) return this.unlink(link.from, link.to);
    const piece = this.pieceOf(to, root);
    if (!piece || piece.object === link.to) return null;
    const preview = this.dropPreview(link.from, to, root);
    if (!preview.ok) return this.ctx.toast(preview.text);
    const k = this.carrierOf(link.from, link.to);
    const share = this.componentOf(link.from)?.flow?.shares?.[link.to];
    const go = () => {
      const have = this.componentOf(link.from)?.flow?.next ?? [];
      const next = have.map((n) => (n === link.to ? piece.object : n));
      if (this.chosenLink?.from === link.from && this.chosenLink?.to === link.to) this.chosenLink = null;
      this.send({ op: 'flow_next', object: link.from, next }, () => {
        this.ctx.toast(t('옮겼습니다: {from} → {to} (전: {was})', { from: this.nameOf(link.from), to: this.nameOf(piece.object), was: this.nameOf(link.to) }));
        if (k.by > 0) this.send({ op: 'flow_carrier', object: link.from, to: piece.object, by: k.by, robot: k.robot ?? null });
        if (share != null) this.send({ op: 'flow_share', object: link.from, to: piece.object, share });
      });
    };
    if (piece.c?.flow) go();
    else this.joinAs(piece.object, 'station', go);
    return piece.object;
  }

  // The piece a link is being drawn from in link mode, and where it
  // stands: { object, at } or null.
  linkingFrom() {
    const from = this.link?.from;
    const at = from == null ? null : this.isHub(from) ? this.hubTop(from) : this.componentOf(from)?.flow?.at;
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
    const more = [note, done ? t('배출까지 이었습니다') : ''].filter(Boolean).join(' · ');
    const tail = more ? ` · ${more}` : '';
    if (from && from !== object) {
      if (kind === 'source') {
        this.ctx.toast(t('소재 공급으로는 보낼 수 없습니다'));
        return null;
      }
      const have = this.componentOf(from)?.flow?.next ?? [];
      const said = `${this.nameOf(from)} → ${name}${tail}`;
      if (this.isHub(from)) {
        this.hubLink(from, { to: object });
      } else if (!have.includes(object)) {
        this.send({ op: 'flow_next', object: from, next: [...have, object] }, () => this.ctx.toast(said));
      } else {
        this.ctx.toast(t('이미 이어져 있습니다: {link}', { link: said }));
      }
    } else if (!from && done) {
      this.ctx.toast(t('보내는 쪽(공급·공정·버퍼·컨베이어)부터 누르세요'));
      return null;
    } else if (note) {
      this.ctx.toast(`${name}: ${note}`);
    } else if (!from) {
      this.ctx.toast(t('{name}에서 시작 · 다음 설비를 누르세요 (로봇이 나르게 하려면 로봇을 먼저)', { name }));
    }
    // One arrow a drag: nothing carried on to the next, and link mode
    // stays on until it is ended.
    if (this.link?.once) this.link = null;
    else if (this.link) this.link.from = null;
    return object;
  }

  // An AMR's name, said as an AMR's once.
  amrName(object) {
    const name = this.nameOf(object);
    return /AMR/i.test(name) ? name : `AMR ${name}`;
  }

  // The AMR among `uids` (and `root`), if one is.
  amrOf(uids, root = null) {
    return [...uids, root].filter((u) => u != null).map((u) => this.componentOf(u)).find((c) => c?.kind === 'amr')?.object ?? null;
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
    this.send({ op: 'point', object, at: [p.x, p.y, p.z] }, () => this.ctx.toast(t('점을 정했습니다')));
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
      const text = c === top && stations.length > 1 ? t('{name} · 병목', { name: tr(c.name) }) : tr(c.name);
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
      this.names.add(this.label(tr(c.name), x, y, z + 0.3 + size, 'name', size));
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
      if (!f || !at.has(c.object) || !['station', 'buffer', 'sink'].includes(c.kind)) continue;
      const [x, y, z] = at.get(c.object);
      let state = '';
      let text;
      if (c.kind === 'station') {
        state = f.down ? 'down' : f.maint ? 'maint' : f.working ? 'run' : f.out ? 'blocked' : f.unreachable ? 'unreach' : 'idle';
        const busy = Math.round((f.oee?.busy ?? 0) * 100);
        text = t('가동 {busy}% · 처리 {made} · 안 {inside}', { busy, made: f.made ?? 0, inside: `${f.inside ?? 0}${f.holds ? `/${f.holds}` : ''}` });
      } else if (c.kind === 'sink') {
        // What has arrived: a sink keeps no parts, it counts them.
        text = t('배출 {n}개', { n: f.made ?? 0 });
      } else {
        // On it now: those it holds, and those sent on but still standing
        // here for a carrier to take.
        text = t('보관 {n}', { n: `${(f.queue ?? 0) + (f.out ?? 0) + (f.waiting ?? 0)}${c.props?.capacity ? `/${c.props.capacity}` : ''}` });
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
        split: 'rgba(196, 104, 20, 0.94)',
        stop: 'rgba(170, 118, 10, 0.95)',
        join: 'rgba(22, 128, 104, 0.94)',
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
      maint: [t('보전'), '#3d7fd9'],
      blocked: ['BLOCKED', '#d99a2b'],
      unreach: [t('로봇 안 닿음'), '#b8443c'],
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
    // An AMR stop's number: the stop, to drag a lane from or onto.
    for (const o of this.markers.children) {
      if (o.userData.stopPick == null) continue;
      const [px, py] = project(o.position);
      const half = Math.min(40, 10 * (o.scale.x / Math.max(o.scale.y, 1e-6)));
      if (Math.abs(px - x) < half && Math.abs(py - y) < 12) return { stop: o.userData.stopPick };
    }
    // A split's or a merge's sign: over its words.
    for (const o of this.markers.children) {
      if (!o.userData.badge) continue;
      const [px, py] = project(o.position);
      const half = Math.min(90, 10 * (o.scale.x / Math.max(o.scale.y, 1e-6)));
      if (Math.abs(px - x) < half && Math.abs(py - y) < 11) return { badge: o.userData.badge };
    }
    // An arrow's head, to drag its link elsewhere: within 14 pixels.
    for (const o of this.markers.children) {
      const l = o.userData.link;
      if (!l || l.hub != null || !o.userData.b || o.userData.last === false) continue;
      const [bx, by] = project(o.userData.b);
      const d = Math.hypot(bx - x, by - y);
      if (d < 14 && (!best || d < best.d)) best = { d, tip: l, a: o.userData.a };
    }
    if (best) return { tip: best.tip, a: best.a };
    // An AMR lane: within 8 pixels of its line.
    let lane = null;
    for (const o of this.markers.children) {
      const l = o.userData.lane;
      if (!l) continue;
      const [ax, ay] = project(o.userData.p);
      const [bx, by] = project(o.userData.q);
      const [dx, dy] = [bx - ax, by - ay];
      const t = Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / Math.max(dx * dx + dy * dy, 1e-9)));
      const d = Math.hypot(ax + t * dx - x, ay + t * dy - y);
      if (d < 8 && (!lane || d < lane.d)) lane = { d, lane: l };
    }
    if (lane) return { lane: lane.lane };
    // A link's number sign: easier to hit than its hairline.
    for (const o of this.markers.children) {
      const l = o.userData.link;
      if (!l || o.userData.a || !o.isSprite) continue;
      const [px, py] = project(o.position);
      const half = Math.min(90, 10 * (o.scale.x / Math.max(o.scale.y, 1e-6)));
      if (Math.abs(px - x) < half && Math.abs(py - y) < 11) return { link: l };
    }
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
      const name = (u) => tr(this.componentOf(u)?.name ?? '');
      this.ctx.toast(t('연결 {from} → {to} · Delete 키로 끊기', { from: name(link.from), to: name(link.to) }));
    }
  }

  // A robot's arrow undone: the piece no longer sends it parts, or it no
  // longer puts parts into the piece.
  cutHub(link) {
    if (this.chosenLink?.from === link.from && this.chosenLink?.to === link.to) this.chosenLink = null;
    if (link.to === link.hub) this.hubLink(link.hub, { from: link.from, remove: true });
    else this.hubLink(link.hub, { to: link.to, remove: true });
  }

  // The arrow chosen, undone. Whether there was one.
  cutChosenLink() {
    if (this.chosenLane) {
      const { a, b } = this.chosenLane;
      this.chosenLane = null;
      this.setLane(a, b, 'none');
      return true;
    }
    const link = this.chosenLink;
    if (!link) return false;
    if (link.hub != null) {
      this.cutHub(link);
      return true;
    }
    this.chosenLink = null;
    const have = this.componentOf(link.from)?.flow?.next ?? [];
    const name = (u) => tr(this.componentOf(u)?.name ?? '');
    this.send({ op: 'flow_next', object: link.from, next: have.filter((n) => n !== link.to) }, () =>
      this.ctx.toast(t('연결을 끊었습니다: {link}', { link: `${name(link.from)} → ${name(link.to)}` })),
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
    // Each arrow first: the links drawn between pieces, and those to and
    // from each robot in the picture. Legs two arrows share are drawn side
    // by side, and each arrow's number stands on a leg of its own.
    const drawn = [];
    const uses = new Map();
    const legKey = (p, q) => [p.x, p.y, q.x, q.y].map((v) => Math.round(v * 10)).join(',');
    const add = (entry) => {
      entry.legs = entry.legs.map(([p, q]) => ({ p, q, key: legKey(p, q) }));
      for (const leg of entry.legs) uses.set(leg.key, (uses.get(leg.key) ?? 0) + 1);
      drawn.push(entry);
    };
    const what = (k) =>
      k.by === 3 ? (k.robot ? t('로봇 {name}', { name: this.robotName(k.robot) }) : t('로봇 (자동)')) : k.by === 1 && k.robot ? this.amrName(k.robot) : CARRIER_NAMES[k.by] ?? '';
    // By AMR: drawn through the AMR that carries it — the one the link
    // names, or the only one there is —, over its top, where it is now.
    const amrs = this.cell.components.filter((c) => c.kind === 'amr');
    const amrTop = (k) => {
      const amr = k.by === 1 ? (k.robot ?? (amrs.length === 1 ? amrs[0].object : null)) : null;
      const box = amr != null ? this.ctx.bounds?.(amr) : null;
      return box && !box.isEmpty() ? new THREE.Vector3((box.min.x + box.max.x) / 2, (box.min.y + box.max.y) / 2, box.max.z + 0.1) : null;
    };
    for (const c of this.cell.components) {
      for (const n of c.flow?.next ?? []) {
        const a = at.get(c.object);
        const b = at.get(n);
        if (!a || !b) continue;
        const from = new THREE.Vector3(a[0], a[1], a[2] + 0.05);
        const to = new THREE.Vector3(b[0], b[1], b[2] + 0.05);
        if (to.distanceTo(from) < 1e-6) continue;
        const k = this.carrierOf(c.object, n);
        const share = this.shareOf(c.object, n);
        const via = amrTop(k);
        // By any robot that reaches: straight, its carrier on its sign —
        // and its share, where the way splits by share. By AMR, into the
        // AMR and out of it.
        add({
          link: { from: c.object, to: n },
          k,
          legs: via ? [[from, via], [via, to]] : [[from, to]],
          num: numbers.get(`${c.object}>${n}`),
          what: [share != null ? `${Math.round(share * 100)}%` : '', what(k)].filter(Boolean).join(' · '),
          bad: k.reach,
        });
      }
    }
    // A robot in the picture: an arrow from each piece it takes from, one
    // to each piece it puts into — however many links it carries.
    for (const h of this.hubs()) {
      const top = this.hubTop(h.robot);
      if (!top) continue;
      const r = new THREE.Vector3(top[0], top[1], top[2]);
      // The links it carries, each with its number and its reach.
      const carried = [];
      for (const c of this.cell.components) {
        for (const k of c.flow?.carriers ?? []) {
          if ((k.by === 3 || k.by === 1) && k.robot === h.robot && (c.flow.derived ?? []).includes(k.to)) carried.push({ from: c.object, to: k.to, num: numbers.get(`${c.object}>${k.to}`), reach: k.reach });
        }
      }
      // What the numbers of several links have in common: 3-1 and 3-2, 3.
      const common = (nums) => {
        const parts = nums.filter(Boolean).map((x) => x.split('-'));
        if (!parts.length) return '';
        let k = 0;
        while (parts.every((p) => p[k] != null && p[k] === parts[0][k])) k++;
        return parts[0].slice(0, Math.max(1, k)).join('-');
      };
      for (const a0 of h.ins) {
        // An AMR handing to this robot, or a robot to this AMR: its arrow
        // is drawn from the giver's side, below.
        if (this.isHub(a0)) continue;
        const a = at.get(a0);
        if (!a) continue;
        const mine = carried.filter((l) => l.from === a0);
        const bad = mine.find((l) => l.reach)?.reach;
        add({ link: { from: a0, to: h.robot, hub: h.robot }, k: { by: this.isAmr(h.robot) ? 1 : 3, robot: h.robot }, legs: [[new THREE.Vector3(a[0], a[1], a[2] + 0.05), r]], num: common(mine.map((l) => l.num)), what: '', bad });
      }
      for (const b0 of h.outs) {
        // To a piece, or (an AMR's) to the robot it hands to.
        const b = at.get(b0) ?? this.hubTop(b0);
        if (!b) continue;
        const mine = carried.filter((l) => l.to === b0);
        const bad = mine.find((l) => l.reach)?.reach;
        add({ link: { from: h.robot, to: b0, hub: h.robot }, k: { by: this.isAmr(h.robot) ? 1 : 3, robot: h.robot }, legs: [[r, new THREE.Vector3(b[0], b[1], b[2] + 0.05)]], num: common(mine.map((l) => l.num)), what: '', bad });
      }
    }
    const taken = new Map();
    for (const { link, k, legs, num, what: by, bad } of drawn) {
      const chosen = this.chosenLink?.from === link.from && this.chosenLink?.to === link.to;
      const fresh = !chosen && this.flash?.has(`${link.from}>${link.to}`);
      const color = chosen ? 0xff8a3d : fresh ? 0x3fcf8e : bad ? 0xff5f5f : CARRIER_COLOURS[k.by] ?? 0x5aa9ff;
      const placed = [];
      for (const [leg, { p: p0, q: q0, key }] of legs.entries()) {
        // Kept to the right of the way it goes: a way back along the same
        // leg is seen beside it; a leg two arrows share, the second
        // further out.
        const nth = taken.get(key) ?? 0;
        taken.set(key, nth + 1);
        const side = new THREE.Vector3().crossVectors(q0.clone().sub(p0), new THREE.Vector3(0, 0, 1));
        if (side.lengthSq() > 1e-9) side.normalize().multiplyScalar(0.07 + 0.12 * nth);
        const p = p0.clone().add(side);
        const q = q0.clone().add(side);
        placed.push({ p, q, key });
        const way = q.clone().sub(p);
        const length = way.length();
        if (length < 1e-6) continue;
        const bold = chosen || fresh;
        const arrow = new THREE.ArrowHelper(way.normalize(), p, length, color, Math.min(bold ? 0.4 : 0.25, length * 0.3), bold ? 0.2 : 0.12);
        // Just made: a thick shaft under it, not a hairline among the signs.
        if (fresh) {
          const shaft = new THREE.Mesh(
            new THREE.CylinderGeometry(size * 0.05, size * 0.05, length, 8),
            new THREE.MeshBasicMaterial({ color, depthTest: false, transparent: true, opacity: 0.85 }),
          );
          shaft.position.copy(p.clone().lerp(q, 0.5));
          shaft.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), way);
          shaft.renderOrder = 10;
          arrows.push(shaft);
        }
        arrow.userData.link = link;
        arrow.userData.a = p.clone();
        arrow.userData.b = q.clone();
        // Only the last leg's head is where the link ends.
        arrow.userData.last = leg === legs.length - 1;
        arrow.traverse((o) => {
          if (o.material) {
            o.material.depthTest = false;
            o.material.transparent = true;
            o.material.opacity = 0.9;
          }
          // Over the pieces' names: an arrow into a machine is not hidden
          // under the machine's own sign.
          o.renderOrder = 10;
        });
        arrows.push(arrow);
      }
      // Its number, and by what it goes when not by itself.
      const text = [num, by, bad ? t('⚠ 안 닿음') : ''].filter(Boolean).join(' · ');
      if (text && placed.length) {
        // Three quarters along its own leg: the numbers of two arrows
        // going either way along one leg stand apart.
        const own = placed.reduce((best, leg) => ((uses.get(leg.key) ?? 0) <= (uses.get(best.key) ?? 0) ? leg : best), placed[0]);
        const mid = own.p.clone().lerp(own.q, 0.75);
        const sign = this.label(text, mid.x, mid.y, mid.z + size * 0.6, bad ? 'linkbad' : 'linknum', size * 0.8);
        sign.userData.link = link;
        arrows.push(sign);
      }
    }
    // Where the way splits, and where ways meet: a sign at the fork, a
    // little along the links, saying how parts go there — clicked, its
    // menu.
    if (this.showArrows) {
      const toward = (p, others) => {
        const v = new THREE.Vector3();
        for (const o of others) v.add(new THREE.Vector3(o[0] - p[0], o[1] - p[1], 0).normalize());
        if (v.lengthSq() < 1e-6) return new THREE.Vector3(p[0], p[1], p[2]);
        const reach = Math.min(...others.map((o) => Math.hypot(o[0] - p[0], o[1] - p[1])));
        v.normalize().multiplyScalar(Math.min(0.35 * reach, 1.5));
        return new THREE.Vector3(p[0] + v.x, p[1] + v.y, p[2]);
      };
      for (const c of this.cell.components) {
        const p = at.get(c.object);
        if (!p) continue;
        const outs = this.outsOf(c.object).map((n) => at.get(n)).filter(Boolean);
        if (outs.length > 1) {
          const rule = RULES.find(([v]) => v === (c.props.rule ?? 0))?.[1] ?? '';
          const q = toward(p, outs);
          const sign = this.label(t('⑂ 분기 · {rule}', { rule }), q.x, q.y, q.z + size * 0.9, 'split', size * 0.8);
          sign.userData.badge = { kind: 'split', object: c.object };
          sign.renderOrder = 11;
          arrows.push(sign);
        }
        if (!['station', 'buffer', 'conveyor', 'sink'].includes(c.kind)) continue;
        const ins = this.insOf(c.object).map((f) => at.get(f)).filter(Boolean);
        if (ins.length > 1) {
          const assembles = c.kind === 'station' && c.props.join === 1;
          const q = toward(p, ins);
          const text = assembles ? t('⑃ 조립 · {n}→1', { n: ins.length }) : `${t('⑃ 합류')}${c.kind === 'station' ? ` · ${t('도착 순')}` : ''}`;
          const sign = this.label(text, q.x, q.y, q.z + size * 0.9, 'join', size * 0.8);
          if (c.kind === 'station') sign.userData.badge = { kind: 'join', object: c.object };
          sign.renderOrder = 11;
          arrows.push(sign);
        }
      }
    }
    if (this.showArrows) for (const a of arrows) this.markers.add(a);
    // In either link mode, a sign over each robot, seen through whatever
    // stands in front of it — a machine hides the robot that tends it —:
    // clicked or dropped on, that robot.
    if (this.link) {
      for (const r of this.ctx.robots?.() ?? []) {
        const top = this.ctx.robotTop?.(r.root);
        if (!top) continue;
        const chosen = this.link.from === r.root;
        const sign = this.label(t('로봇 {name}', { name: tr(r.name) }), top[0], top[1], top[2] + 0.6, chosen ? 'robotchosen' : 'robotpick', 0.3);
        sign.userData.robotPick = r.root;
        sign.renderOrder = 12;
        this.markers.add(sign);
      }
    }
    // A link's handle shows over the equipment under the pointer only (see
    // `showPort`), in link mode too: a dot over every piece hid the arrows
    // just made under it.
    // The port over the equipment under the pointer, kept as it was.
    if (this.port && !this.markers.children.some((o) => o.userData.handle === this.port.object)) this.markers.add(this.portDot);
    this.drawNames(at, this.drawLoads(at));
    this.drawStatus(at);
    this.drawProgress(at);
    // The AMRs' lanes: a line between each two stops linked both ways, an
    // arrow where one way only; and each stop's number over it.
    const stops = new Map(this.cell.components.filter((c) => c.stop_at).map((c) => [c.object, c.stop_at]));
    const goes = (x, y) => (this.componentOf(x)?.lanes ?? []).includes(y);
    const seen = new Set();
    for (const c of this.cell.components) {
      for (const n of c.lanes ?? []) {
        const key = [c.object, n].sort().join('|');
        if (seen.has(key)) continue;
        seen.add(key);
        const a = stops.get(c.object);
        const b = stops.get(n);
        if (!a || !b) continue;
        const both = goes(n, c.object);
        const p = new THREE.Vector3(a[0], a[1], 0.02);
        const q = new THREE.Vector3(b[0], b[1], 0.02);
        const chosen = this.chosenLane && [this.chosenLane.a, this.chosenLane.b].sort().join('|') === key;
        const color = chosen ? 0xff8a3d : 0xf2b53c;
        const way = q.clone().sub(p);
        const length = way.length();
        if (length < 1e-6) continue;
        let shape;
        if (both) {
          const geometry = new THREE.BufferGeometry().setFromPoints([p, q]);
          shape = new THREE.Line(geometry, new THREE.LineBasicMaterial({ color, transparent: true, opacity: 0.9, depthTest: false }));
        } else {
          const head = Math.min(0.5, length * 0.25);
          shape = new THREE.ArrowHelper(way.clone().normalize(), p, length, color, head, head * 0.5);
          shape.traverse((o) => {
            if (o.material) {
              o.material.depthTest = false;
              o.material.transparent = true;
              o.material.opacity = 0.9;
            }
          });
        }
        shape.traverse((o) => (o.renderOrder = 8));
        shape.userData.lane = { a: c.object, b: n, both };
        shape.userData.p = p;
        shape.userData.q = q;
        this.markers.add(shape);
      }
    }
    for (const [object, at] of stops) {
      const sign = this.label(this.stopNumber(object), at[0], at[1], 0.35, 'stop', 0.3);
      sign.userData.stopPick = object;
      sign.renderOrder = 12;
      this.markers.add(sign);
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
