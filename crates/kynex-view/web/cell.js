// The cell: what each object is to the process — a CNC machine, a pallet,
// a gripper, a fence — set in the inspector, and the places parts stand,
// marked in the scene. KYNEX keeps the roles (the `cell` event) and the
// parts, which arrive in the scene like any other object.

import * as THREE from 'three';
import { fieldsFor, getLevel, onLevel, roleShown } from './level.js';

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
    ['speed', '속도', 'mm/s', 1000],
    ['feed', '소재 공급 간격 (0: 없음)', 's', 1],
    ['drain', '끝에서 치움 (1: 예)', '', 1],
  ],
  amr: [
    ['speed', '주행 속도', 'mm/s', 1000],
    ['capacity', '적재량 (한 번에 나르는 수)', '개', 1],
    ['battery', '배터리: 완충으로 주행 (0: 신경 쓰지 않음)', '분', 1],
    ['charge', '완충까지 충전 시간', '분', 1],
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
    ['carrier', '보내는 방법', '', 1, [[0, '직접'], [1, 'AMR'], [2, '작업자'], [3, '로봇']]],
    ['rule', '다음 공정 고르기', '', 1, [[0, '번갈아'], [1, '대기 적은 곳'], [2, '무작위']]],
    ['feed', '공급 간격', 's', 1],
    ['kanban', '칸반: 공정 안 최대 수 (0: 없음)', '개', 1],
    ['dist', '시간 분포', '', 1, [[0, '고정'], [1, '균등 ±폭'], [2, '삼각 ±폭'], [3, '정규 σ'], [4, '지수']]],
    ['spread', '폭·σ', 's', 1],
    ['capacity', '내보낼 자리', '개', 1],
    ['speed', '이송 속도', 'mm/s', 1000],
  ],
  station: [
    ['carrier', '보내는 방법', '', 1, [[0, '직접'], [1, 'AMR'], [2, '작업자'], [3, '로봇']]],
    ['rule', '다음 공정 고르기', '', 1, [[0, '번갈아'], [1, '대기 적은 곳'], [2, '무작위']]],
    ['cycle', '처리 시간', 's', 1],
    ['dist', '시간 분포', '', 1, [[0, '고정'], [1, '균등 ±폭'], [2, '삼각 ±폭'], [3, '정규 σ'], [4, '지수']]],
    ['spread', '폭·σ', 's', 1],
    ['setup', '품종 교체 시간', 's', 1],
    ['mtbf', '고장 간격 평균 (0: 없음)', 's', 1],
    ['mttr', '수리 시간 평균', 's', 1],
    ['scrap', '불량률', '%', 100],
    ['crew', '작업자', '', 1, [[0, '필요 없음'], [1, '작업자 필요']]],
    ['capacity', '대기 자리', '개', 1],
    ['takes', '투입 수 (2 이상: 조립)', '개', 1],
    ['makes', '배출 수 (2 이상: 분해)', '개', 1],
    ['speed', '이송 속도', 'mm/s', 1000],
  ],
  buffer: [
    ['carrier', '보내는 방법', '', 1, [[0, '직접'], [1, 'AMR'], [2, '작업자'], [3, '로봇']]],
    ['rule', '다음 공정 고르기', '', 1, [[0, '번갈아'], [1, '대기 적은 곳'], [2, '무작위']]],
    ['capacity', '용량', '개', 1],
    ['policy', '꺼내는 순서', '', 1, [[0, '먼저 들어온 것 (FIFO)'], [1, '나중 들어온 것 (LIFO)']]],
    ['speed', '이송 속도', 'mm/s', 1000],
  ],
  operator: [
    ['speed', '걷는 속도', 'm/s', 1],
    ['capacity', '한 번에 드는 수', '개', 1],
    ['wage', '임금', '원/시', 1],
    ['shift_from', '근무 시작 (흐름 시작이 0시)', '시', 1],
    ['shift_to', '근무 끝 (시작과 같으면 하루 종일)', '시', 1],
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
            ? `${f.down ? '고장 · 수리 중' : f.working ? `작업 중 ${Math.ceil(f.left ?? 0)} s` : f.out ? '막힘 (보낼 곳 없음)' : '대기'} · 대기 ${f.queue} · 완료 ${f.made}${f.oee && f.oee.good + f.oee.bad ? ` · OEE ${Math.round(f.oee.oee * 100)}%` : ''}`
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
      const link = c.kind === 'sink' ? '' : '<button class="text-btn" data-link-pick title="3D 화면에서 다음 설비들을 차례로 눌러 잇습니다">＋ 화면에서 잇기</button>';
      actions =
        link +
        (getLevel() >= 2
          ? `${nexts}<span class="muted">${state}</span>${products}${routes}`
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

  // Linking in the 3D view: equipment clicked in order, each linked to the
  // one before. `from` the last clicked, or none yet.
  linking() {
    return this.link != null;
  }

  startLinking(from = null) {
    this.link = { from: from && this.sends(from) ? from : null };
    this.onLinking?.(true);
    this.ctx.toast(this.link.from ? '다음 공정이 될 설비를 차례로 누르거나, 끌어다 놓으세요 (Esc: 끝)' : '공급부터 배출까지 설비를 차례로 누르거나, 한 설비에서 다음 설비로 끌어다 놓으세요 (Esc: 끝)');
  }

  stopLinking() {
    this.link = null;
    this.onLinking?.(false);
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
    let c = uids.map((u) => this.componentOf(u)).find((c) => c?.flow);
    if (c) return this.linkOn(c.object, c.kind, c.name);
    const machine = uids.map((u) => this.componentOf(u)).find((c) => c && ['cnc', 'molder'].includes(c.kind));
    const object = machine?.object ?? (root != null && !this.componentOf(root) ? root : null);
    if (object == null) {
      this.ctx.toast('설비를 누르세요 (공정 흐름에 넣을 수 있는 것)');
      return null;
    }
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
    if (once && made == null && this.link?.once) this.link = null;
    return made;
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
      const said = `${this.componentOf(from)?.name ?? ''} → ${name}${tail}`;
      if (!have.includes(object)) {
        this.send({ op: 'flow_next', object: from, next: [...have, object] }, () => this.ctx.toast(said));
      } else {
        this.ctx.toast(`이미 이어져 있습니다: ${said}`);
      }
    } else if (!from && done) {
      this.ctx.toast('보내는 쪽(공급·공정·버퍼·컨베이어)부터 누르세요');
      return null;
    } else if (note) {
      this.ctx.toast(`${name}: ${note}`);
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
      const text = c === top && stations.length > 1 ? `${c.name} · 병목 ${Math.round(u * 100)}%` : `${c.name} · ${Math.round(u * 100)}%`;
      const size = this.signSize(at);
      this.markers.add(this.label(text, x, y, z + 0.3 + size, c === top && stations.length > 1 ? 'bottleneck' : 'load', size));
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
    const key = `${size.toFixed(2)}|` + named.map((c) => `${c.object}:${c.name}:${at.get(c.object).map((v) => v.toFixed(2))}`).join('|');
    if (key === this.namesDrawn) return;
    this.namesDrawn = key;
    for (const child of [...this.names.children]) child.removeFromParent();
    for (const c of named) {
      const [x, y, z] = at.get(c.object);
      this.names.add(this.label(c.name, x, y, z + 0.3 + size, 'name', size));
    }
  }

  // How far each station's work in hand has got: a pie over it, filling
  // as the cycle goes, while it works; none while it does not.
  drawProgress(at) {
    const size = this.signSize(at);
    for (const c of this.cell.components) {
      const done = c.kind === 'station' ? c.flow?.progress : null;
      if (done == null || !at.has(c.object)) continue;
      const [x, y, z] = at.get(c.object);
      const pie = new THREE.Sprite(this.pieMaterial(Math.round(done * 100)));
      pie.userData.kept = true;
      pie.scale.set(size * 1.5, size * 1.5, 1);
      // Over the station's label, which stands `size` high at z + 0.3 + size.
      pie.position.set(x, y, z + 0.3 + size * 2.4);
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
    // Smaller as it is farther, as a sign in the scene is.
    sprite.scale.set((size * kept.w) / 64, size, 1);
    sprite.position.set(x, y, z);
    sprite.renderOrder = 9;
    return sprite;
  }

  // A label's picture, made once for each word.
  paintLabel(text, style) {
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    const font = `600 ${style === 'name' ? 34 : 40}px system-ui, sans-serif`;
    ctx.font = font;
    const w = Math.ceil(ctx.measureText(text).width) + 28;
    canvas.width = w;
    canvas.height = 64;
    ctx.font = font;
    ctx.fillStyle = { bottleneck: 'rgba(200, 40, 40, 0.92)', name: 'rgba(38, 66, 104, 0.88)' }[style] ?? 'rgba(20, 24, 30, 0.82)';
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

  // The dot a link is dragged from: an orange disc, ringed white. Made
  // once (drawMarkers leaves it be).
  dotTexture() {
    if (this.dot) return this.dot;
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 64;
    const ctx = canvas.getContext('2d');
    ctx.beginPath();
    ctx.arc(32, 32, 26, 0, Math.PI * 2);
    ctx.fillStyle = '#ff8a3d';
    ctx.fill();
    ctx.lineWidth = 7;
    ctx.strokeStyle = '#ffffff';
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
    for (const o of this.markers.children) {
      if (!o.userData.handle) continue;
      const [px, py] = project(o.position);
      const d = Math.hypot(px - x, py - y);
      if (d < 12 && (!best || d < best.d)) best = { d, handle: o.userData.handle };
    }
    if (best) return { handle: best.handle };
    for (const o of this.markers.children) {
      const l = o.userData.link;
      if (!l) continue;
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
    // The process flow: an arrow from each part of it to each it sends to.
    const at = new Map(this.cell.components.filter((c) => c.flow?.at).map((c) => [c.object, c.flow.at]));
    const arrows = [];
    for (const c of this.cell.components) {
      for (const n of c.flow?.next ?? []) {
        const a = at.get(c.object);
        const b = at.get(n);
        if (!a || !b) continue;
        const from = new THREE.Vector3(a[0], a[1], a[2] + 0.05);
        const to = new THREE.Vector3(b[0], b[1], b[2] + 0.05);
        const way = to.clone().sub(from);
        const length = way.length();
        if (length < 1e-6) continue;
        const chosen = this.chosenLink?.from === c.object && this.chosenLink?.to === n;
        const arrow = new THREE.ArrowHelper(
          way.normalize(),
          from,
          length,
          chosen ? 0xff8a3d : 0x5aa9ff,
          Math.min(chosen ? 0.4 : 0.25, length * 0.3),
          chosen ? 0.2 : 0.12,
        );
        arrow.userData.link = { from: c.object, to: n };
        arrow.userData.a = from.clone();
        arrow.userData.b = to.clone();
        arrow.traverse((o) => {
          if (o.material) {
            o.material.depthTest = false;
            o.material.transparent = true;
            o.material.opacity = 0.85;
          }
          o.renderOrder = 8;
        });
        arrows.push(arrow);
      }
    }
    for (const a of arrows) this.markers.add(a);
    // A dot over each piece that sends parts on: dragged onto other
    // equipment, it links the two.
    for (const c of this.cell.components) {
      const a = at.get(c.object);
      if (!a || !this.sends(c.object)) continue;
      const dot = new THREE.Sprite(new THREE.SpriteMaterial({ map: this.dotTexture(), depthTest: false, transparent: true }));
      dot.material.sizeAttenuation = false;
      dot.scale.set(0.022, 0.022, 1);
      dot.position.set(a[0], a[1], a[2] + 0.45);
      dot.renderOrder = 11;
      dot.userData.handle = c.object;
      this.markers.add(dot);
    }
    this.drawNames(at, this.drawLoads(at));
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
