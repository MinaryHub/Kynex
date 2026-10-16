// The robot panel: each robot's joints, and posing it here — a joint dragged
// along its bar, the tool jogged step by step like a teach pendant, or
// pulled by a handle in the scene — and its program: poses taught, the
// moves between them timed, played back and drawn as the tool's path.
// KYNEX works it all out (POST /robot) and the scene comes back on the
// stream like any other change; nothing goes to Kreate.

import * as THREE from 'three';
import { TransformControls } from '/vendor/TransformControls.js';

const KIND_CHIP = { revolute: 'R', continuous: 'C', prismatic: 'P', fixed: 'F', floating: '6', planar: '2' };

// Jog steps: metres and radians together, one choice for both.
const STEPS = [
  { label: '1 mm · 1°', m: 0.001, rad: Math.PI / 180 },
  { label: '10 mm · 5°', m: 0.01, rad: (5 * Math.PI) / 180 },
  { label: '50 mm · 15°', m: 0.05, rad: (15 * Math.PI) / 180 },
];
// What a step can tell a CNC machine, and what it can wait for.
const COMMANDS = [
  ['open', '문 열기'],
  ['close', '문 닫기'],
  ['clamp', '척 물림'],
  ['unclamp', '척 풀기'],
  ['start', '가공 시작'],
];
const SIGNALS = [
  ['door_open', '문 열림'],
  ['door_closed', '문 닫힘'],
  ['clamped', '척 물림'],
  ['unclamped', '척 풀림'],
  ['idle', '가공 대기'],
  ['done', '가공 완료'],
];

// What each kind of machine takes and says, as the program's rows offer it.
const COMMANDS_OF = {
  cnc: COMMANDS,
  molder: [
    ['open', '가드 열기'],
    ['close', '가드 닫기'],
    ['start', '성형 시작'],
  ],
};
const SIGNALS_OF = {
  cnc: SIGNALS,
  molder: [
    ['door_open', '가드 열림'],
    ['door_closed', '가드 닫힘'],
    ['idle', '성형 대기'],
    ['done', '성형 완료'],
  ],
  conveyor: [['part_at_end', '부품 도착']],
  amr: [['arrived', 'AMR 도착']],
};
const SPEEDS = [1, 2, 5, 10, 20];

const AXES = { x: new THREE.Vector3(1, 0, 0), y: new THREE.Vector3(0, 1, 0), z: new THREE.Vector3(0, 0, 1) };

// A held jog button steps again after this long, then this often.
const REPEAT_DELAY = 380;
const REPEAT_EVERY = 90;

// One decimal, and no "-0.0" for a value that rounds to nothing.
const tenths = (v) => (Math.abs(v) < 0.05 ? 0 : v).toFixed(1);
const mm = (m) => tenths(m * 1000);
const deg = (r) => tenths((r * 180) / Math.PI);

function jointValue(j) {
  return j.slides ? `${mm(j.position)} mm` : `${deg(j.position)}°`;
}
function limitText(j, v) {
  return j.slides ? `${(v * 1000).toFixed(0)}` : `${((v * 180) / Math.PI).toFixed(0)}°`;
}

// The span a joint's bar shows: its limits with a little room either side,
// so a joint past one shows where it went.
function spanOf(j) {
  const limited = j.limited && j.upper > j.lower;
  let lo = j.lower, hi = j.upper;
  if (!limited) {
    lo = j.slides ? -1 : -Math.PI;
    hi = j.slides ? 1 : Math.PI;
  }
  const pad = (hi - lo) * 0.08;
  return { limited, lo, hi, a: lo - pad, b: hi + pad };
}

async function post(request) {
  const response = await fetch('/robot', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

export class RobotPanel {
  // ctx: { host, records, scene, camera, canvas, controls, selected(), toast, escapeHtml }
  constructor(ctx) {
    this.ctx = ctx;
    this.host = ctx.host;
    this.robots = [];
    this.held = new Set();
    // Per robot, what its controls are set to: kept across redraws.
    this.ui = new Map();
    // One request on its way per robot; a newer one waits and replaces any
    // older one waiting, so a fast drag never queues up stale poses.
    this.lanes = new Map();
    // Program requests go one after another, none dropped.
    this.queue = Promise.resolve();
    // The cell, as the `cell` event has it: the grippers on each robot.
    this.cell = { components: [], parts: [] };
    // By robot root, as the `programs` event has them.
    this.programs = {};
    // The programs running, a robot's each, and the clock, as the `pose`
    // event has them.
    this.running = [];
    this.clock = { speed: 1, time: 0 };
    // The tool paths and taught points, drawn over the scene.
    this.paths = new THREE.Group();
    ctx.scene.add(this.paths);
    // Where parts are in contact now: by robot root, as the event has them.
    this.contacts = [];
    this.contactDots = null;

    // Picking an edge to follow, for one robot at a time: its root, what
    // was picked, and the edge drawn over the scene.
    this.edge = null;

    // Where a robot reaches, shown for one at a time: its root, and the
    // shape, placed from the root's frame as the robot moves.
    this.reach = null;

    this.proxy = new THREE.Object3D();
    ctx.scene.add(this.proxy);
    this.handle = new TransformControls(ctx.camera, ctx.canvas);
    this.handle.setSize(0.75);
    this.handle.setSpace('world');
    this.handleHelper = this.handle.getHelper();
    this.handleRoot = null;
    this.dragging = false;
    this.draggedAt = 0;
    this.handle.addEventListener('dragging-changed', (e) => {
      ctx.controls.enabled = !e.value;
      this.dragging = e.value;
      if (!e.value) {
        this.draggedAt = performance.now();
        // Back onto where the tool really went.
        this.refresh();
      }
    });
    this.handle.addEventListener('objectChange', () => {
      if (this.dragging) this.pulled();
    });

    // A joint's gizmo: on the joint selected, or the one that moves the
    // part selected — its ring about the joint's axis (an arrow along it
    // for a sliding joint), dragged to turn the joint.
    this.jointProxy = new THREE.Object3D();
    ctx.scene.add(this.jointProxy);
    this.jointHandle = new TransformControls(ctx.camera, ctx.canvas);
    this.jointHandle.setSize(0.7);
    this.jointHandle.setSpace('local');
    this.jointHandle.showX = this.jointHandle.showY = false;
    this.jointHelper = this.jointHandle.getHelper();
    this.joint = null; // { root, uid, slides, from: { position, quaternion, value } }
    this.jointDragging = false;
    this.jointHandle.addEventListener('dragging-changed', (e) => {
      ctx.controls.enabled = !e.value;
      this.jointDragging = e.value;
      if (e.value && this.joint) {
        const j = this.jointOf(this.joint);
        this.joint.from = {
          position: this.jointProxy.position.clone(),
          quaternion: this.jointProxy.quaternion.clone(),
          value: j?.position ?? 0,
        };
      }
      if (!e.value) this.draggedAt = performance.now();
    });
    this.jointHandle.addEventListener('objectChange', () => {
      if (this.jointDragging) this.jointPulled();
    });

    this.host.addEventListener('pointerdown', (e) => this.pointerDown(e));
    this.host.addEventListener('click', (e) => this.clicked(e));
    this.host.addEventListener('change', (e) => this.optionChanged(e));
    // Anywhere else clicked: the keys are not the program's.
    document.addEventListener('pointerdown', (e) => {
      if (!this.host.contains(e.target)) this.keysFor = null;
    });
    this.wireDrag();
  }

  // Whether the pointer is the handle's: a click on it picks nothing.
  ownsPointer() {
    return (
      this.dragging ||
      this.jointDragging ||
      this.handle.axis !== null ||
      this.jointHandle.axis !== null ||
      performance.now() - this.draggedAt < 250
    );
  }

  // The selection changed: the joint gizmo onto the joint selected, or onto
  // the moving joint nearest above the part selected. Whether it took the
  // selection (then the equipment's gizmo stays away).
  selectionChanged(uid) {
    const found = uid != null ? this.movingJointAt(uid) : null;
    this.joint = found;
    if (found) {
      this.jointHandle.setMode(found.slides ? 'translate' : 'rotate');
      this.jointHandle.showX = this.jointHandle.showY = false;
      this.jointHandle.showZ = true;
      this.jointHandle.attach(this.jointProxy);
      this.ctx.scene.add(this.jointHelper);
      this.followJoint();
    } else {
      this.jointHandle.detach();
      this.jointHelper.removeFromParent();
    }
    return !!found;
  }

  // The robot's moving joint `uid` is, or is the nearest above it: walking
  // up from the part, not past the robot's root.
  movingJointAt(uid) {
    const records = this.ctx.records;
    for (let at = uid, n = 0; at != null && n < 64; at = records.get(at)?.parent, n++) {
      for (const r of this.robots) {
        const j = r.joints.find((x) => x.uid === at);
        if (j) return j.moves ? { root: r.root, uid: j.uid, slides: j.slides } : null;
        if (r.root === at) return null;
      }
    }
    return null;
  }

  jointOf(joint) {
    return this.robots.find((r) => r.root === joint.root)?.joints.find((j) => j.uid === joint.uid);
  }

  // The joint gizmo where the joint is now, but not while it is dragged.
  followJoint() {
    if (!this.joint || this.jointDragging) return;
    const rec = this.ctx.records.get(this.joint.uid);
    if (!rec) return;
    const m = new THREE.Matrix4().fromArray(rec.matrix);
    const scale = new THREE.Vector3();
    m.decompose(this.jointProxy.position, this.jointProxy.quaternion, scale);
    this.jointProxy.updateMatrixWorld();
  }

  // The joint turned (or slid) as far as its gizmo has been, from where
  // the drag began: about its own Z, which is its axis.
  jointPulled() {
    const { joint } = this;
    const j = joint && this.jointOf(joint);
    const r = joint && this.robots.find((x) => x.root === joint.root);
    if (!j || !r || !joint.from) return;
    let v;
    if (joint.slides) {
      const axis = new THREE.Vector3(0, 0, 1).applyQuaternion(joint.from.quaternion);
      v = joint.from.value + this.jointProxy.position.clone().sub(joint.from.position).dot(axis);
    } else {
      const d = joint.from.quaternion.clone().invert().multiply(this.jointProxy.quaternion);
      v = joint.from.value + 2 * Math.atan2(d.z, d.w);
    }
    const { limited, lo, hi } = spanOf(j);
    if (limited) v = Math.min(Math.max(v, lo), hi);
    this.send(r, { op: 'jog', robot: r.root, joint: j.uid, position: v });
  }

  uiOf(r) {
    let ui = this.ui.get(r.root);
    if (!ui) {
      const dof = r.joints.filter((j) => j.moves).length;
      // An arm that cannot hold an orientation everywhere is asked only for
      // the point.
      ui = { step: 1, frame: 'world', lock: dof >= 6, offset: 0, mode: 'translate', status: '', bad: false };
      this.ui.set(r.root, ui);
    }
    return ui;
  }

  // Robot `root`'s program running, if it has one.
  runOf(root) {
    return this.running.find((r) => r.robot === root) ?? null;
  }

  // The `pose` event: the robots held here, the programs running, and the
  // simulation's clock.
  setPose(pose) {
    this.held = new Set(pose.held);
    this.running = Array.isArray(pose.running) ? pose.running : [];
    if (pose.clock) this.clock = pose.clock;
    for (const section of this.host.querySelectorAll('.robot')) {
      const root = section.dataset.root;
      const held = this.held.has(root);
      section.classList.toggle('held', held);
      section.querySelector('.robot-local').hidden = !held;
      this.showRun(section, this.runOf(root));
    }
  }

  // A robot's run: its button, its state, and the step it is at.
  showRun(section, run) {
    const play = section.querySelector('[data-act="play"]');
    if (!play) return;
    const going = run && run.state !== 'done' && run.state !== 'alarm';
    play.textContent = going ? '■ 정지' : '▶ 실행';
    play.setAttribute('aria-pressed', String(!!going));
    const speed = section.querySelector('[data-opt="clock"]');
    if (speed && document.activeElement !== speed) speed.value = String(this.clock.speed);
    const state = section.querySelector('.run-state');
    let text = '';
    if (run) {
      const what = {
        running: '실행 중',
        moving: `이동 ${run.detail}`,
        waiting: `대기: ${run.detail}`,
        pausing: `잠시 ${run.detail}`,
        done: '끝',
        alarm: `알람: ${run.detail}`,
      }[run.state];
      const cycles = run.cycles ? ` · 사이클 ${run.cycles}회, 직전 ${run.last_cycle.toFixed(1)} s` : '';
      // In a routine it calls, by its name and its own step.
      const program = this.programs[section.dataset.root];
      const here = program?.routines?.[program.active];
      const local = run.local ?? run.step;
      const where = run.routine && run.routine !== here ? `${run.routine} · ${local + 1}단계` : `${local + 1}단계`;
      text = run.state === 'done' ? `${what}${cycles}` : `${where} · ${what}${cycles}`;
    }
    state.textContent = text;
    state.classList.toggle('alarm', run?.state === 'alarm');
    const program = this.programs[section.dataset.root];
    const shown = !run?.routine || run.routine === program?.routines?.[program.active];
    for (const row of section.querySelectorAll('.point[data-index]')) {
      row.classList.toggle('running', !!going && shown && Number(row.dataset.index) === (run.local ?? run.step));
    }
  }

  // The `contacts` event: the parts in contact now, listed under their
  // robot and marked where they meet.
  setContacts(list) {
    this.contacts = list;
    this.renderContacts();
    if (this.contactDots) {
      this.contactDots.removeFromParent();
      this.contactDots.geometry.dispose();
      this.contactDots.material.dispose();
      this.contactDots = null;
    }
    if (list.length) {
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.Float32BufferAttribute(list.flatMap((c) => c.at), 3));
      this.contactDots = new THREE.Points(
        geometry,
        new THREE.PointsMaterial({ color: 0xff3b3b, size: 11, sizeAttenuation: false, depthTest: false }),
      );
      this.contactDots.renderOrder = 11;
      this.ctx.scene.add(this.contactDots);
    }
  }

  renderContacts() {
    const { escapeHtml } = this.ctx;
    for (const section of this.host.querySelectorAll('.robot')) {
      const mine = this.contacts.filter((c) => c.robot === section.dataset.root);
      const box = section.querySelector('.robot-contacts');
      box.hidden = !mine.length;
      section.classList.toggle('in-contact', mine.length > 0);
      if (!mine.length) continue;
      // Each pair once, however many places they meet.
      const pairs = [...new Set(mine.map((c) => `${c.part_name} ↔ ${c.other_name}`))];
      box.innerHTML =
        `<span class="contact-title">충돌 ${pairs.length}곳</span>` +
        pairs.map((p) => `<span class="contact-pair">${escapeHtml(p)}</span>`).join('');
    }
  }

  // The `cell` event: which grip points each robot has.
  setCell(cell) {
    this.cell = cell;
    this.render();
  }

  grippersOf(r) {
    return this.cell.components.filter((c) => c.kind === 'gripper' && c.robot === r.root);
  }

  // The link the tool point is on: the robot's tool, or one of its grip
  // points if chosen.
  toolOf(r) {
    const ui = this.uiOf(r);
    return ui.tool && this.grippersOf(r).some((g) => g.object === ui.tool) ? ui.tool : r.tool;
  }

  // The `programs` event: every robot's program, timed.
  setPrograms(programs) {
    this.programs = programs;
    this.renderPrograms();
    this.drawPaths();
  }

  setRobots(list) {
    this.robots = list;
    for (const root of [...this.ui.keys()]) if (!list.some((r) => r.root === root)) this.ui.delete(root);
    if (this.handleRoot && !list.some((r) => r.root === this.handleRoot && r.tool)) this.showHandle(null);
    this.render();
  }

  render() {
    const { escapeHtml } = this.ctx;
    const host = this.host;
    document.getElementById('robot-count').textContent = this.robots.length;
    if (!this.robots.length) {
      host.dataset.shape = '';
      host.innerHTML = '<p class="muted pad">장면에 로봇이 없습니다. 라이브러리에서 로봇을 놓으면 관절과 프로그램이 여기에 나타납니다.</p>';
      return;
    }
    // Same robots and joints as drawn: update the numbers in place, so a pose
    // streaming in does not rebuild the panel under the pointer.
    const shape = this.robots
      .map((r) => [r.root, r.tool, this.toolOf(r), ...this.grippersOf(r).map((g) => g.object), ...r.joints.map((j) => j.uid + j.kind)].join(','))
      .join('|');
    if (host.dataset.shape !== shape) {
      host.dataset.shape = shape;
      host.innerHTML = this.robots.map((r) => this.markup(r, escapeHtml)).join('');
      this.setPose({ held: [...this.held], running: this.running, clock: this.clock });
      this.renderPrograms();
      this.renderContacts();
    }
    this.refresh();
  }

  markup(r, escapeHtml) {
    const ui = this.uiOf(r);
    const tool = r.tool ? this.ctx.records.get(r.tool) : null;
    const grippers = this.grippersOf(r);
    const current = this.toolOf(r);
    const nameOf = (uid) => escapeHtml(this.ctx.records.get(uid)?.name ?? uid);
    const toolPick = grippers.length
      ? `<select class="tcp-pick" data-opt="tool" title="TCP로 쓸 링크">${[r.tool, ...grippers.map((g) => g.object)]
          .map((uid) => `<option value="${escapeHtml(uid)}"${uid === current ? ' selected' : ''}>${nameOf(uid)}</option>`)
          .join('')}</select>`
      : `<span class="tcp-tool" title="툴 링크">${escapeHtml(tool?.name ?? '')}</span>`;
    const gripRows = grippers
      .map(
        (g) => `
          <div class="grip-row" data-gripper="${escapeHtml(g.object)}">
            <span class="grip-name">${nameOf(g.object)}</span>
            <span class="grip-state"></span>
            <button class="chip-btn" data-act="grip">잡기</button>
            <button class="chip-btn" data-act="release">놓기</button>
          </div>`,
      )
      .join('');
    const jog = (axis, label) => `
      <div class="jog-axis">
        <button class="jog-btn" data-jog="${axis}" data-dir="-1" aria-label="${label} −">−</button>
        <span>${label}</span>
        <button class="jog-btn" data-jog="${axis}" data-dir="1" aria-label="${label} +">+</button>
      </div>`;
    return `
      <section class="robot" data-root="${escapeHtml(r.root)}">
        <div class="robot-head">
          <span class="robot-name">${escapeHtml(r.name)}</span>
          <span class="robot-meta">${r.joints.filter((j) => j.moves).length} DOF${r.mass ? ` · ${r.mass.toFixed(1)} kg` : ''}</span>
        </div>
        <div class="robot-contacts" hidden></div>
        <div class="robot-local" hidden>
          <span class="local-badge">로컬 자세</span>
          <span class="local-note">Kreate에는 반영되지 않습니다</span>
          <button class="text-btn" data-act="session">세션 자세로</button>
        </div>
        <ul class="joints">
          ${r.joints
            .map(
              (j) => `
            <li class="joint${j.moves ? ' movable' : ''}" data-uid="${escapeHtml(j.uid)}">
              <span class="kind ${j.kind === 'fixed' ? 'fixed' : ''}" title="${j.kind}">${KIND_CHIP[j.kind] || '?'}</span>
              <span class="jname">${escapeHtml(j.name)}</span>
              <span class="jval"></span>
              ${j.moves ? '<span class="track" title="끌어서 관절 이동"><span class="fill"></span><span class="mark"></span></span><span class="limits"><span class="lo"></span><span class="hi"></span></span>' : ''}
            </li>`,
            )
            .join('')}
        </ul>
        ${
          r.tool
            ? `
        <div class="tcp">
          <div class="tcp-head">
            <span class="tcp-title">TCP</span>
            ${toolPick}
            <button class="chip-btn" data-act="handle" aria-pressed="false" title="장면에서 끌어서 TCP 이동">핸들</button>
            <button class="chip-btn" data-act="reach" aria-pressed="false" title="관절 한계 안에서 플랜지가 닿는 범위 (자기 충돌은 보지 않음)">범위</button>
          </div>
          ${gripRows ? `<div class="grippers">${gripRows}</div>` : ''}
          <dl class="tcp-pose num">
            <dt>X</dt><dd data-v="x"></dd><dt>Y</dt><dd data-v="y"></dd><dt>Z</dt><dd data-v="z"></dd>
            <dt>Rx</dt><dd data-v="rx"></dd><dt>Ry</dt><dd data-v="ry"></dd><dt>Rz</dt><dd data-v="rz"></dd>
          </dl>
          <div class="jog">
            ${jog('x', 'X')}${jog('rx', 'Rx')}
            ${jog('y', 'Y')}${jog('ry', 'Ry')}
            ${jog('z', 'Z')}${jog('rz', 'Rz')}
          </div>
          <div class="tcp-opts">
            <div class="seg" role="group" aria-label="좌표계">
              <button data-frame="world" class="${ui.frame === 'world' ? 'on' : ''}">월드</button>
              <button data-frame="tool" class="${ui.frame === 'tool' ? 'on' : ''}">툴</button>
            </div>
            <select data-opt="step" aria-label="간격">
              ${STEPS.map((s, i) => `<option value="${i}"${i === ui.step ? ' selected' : ''}>${s.label}</option>`).join('')}
            </select>
          </div>
          <div class="tcp-opts">
            <label class="check"><input type="checkbox" data-opt="lock"${ui.lock ? ' checked' : ''}> 이동할 때 자세 유지</label>
            <label class="offset">오프셋 Z <input type="number" data-opt="offset" value="${ui.offset}" step="1"> mm</label>
          </div>
          <div class="handle-modes seg" role="group" aria-label="핸들" hidden>
            <button data-mode="translate" class="${ui.mode === 'translate' ? 'on' : ''}">이동</button>
            <button data-mode="rotate" class="${ui.mode === 'rotate' ? 'on' : ''}">회전</button>
          </div>
          <div class="tcp-status"></div>
          <div class="tcp-sing" hidden></div>
        </div>
        <div class="program">
          <div class="prog-head">
            <span class="section-title">프로그램</span>
            <select class="routine-pick" data-opt="routine" aria-label="프로그램 고르기" title="이 로봇의 프로그램: 메인과 서브루틴"></select>
            <button class="icon-mini" data-act="routine-new" title="새 서브루틴">+</button>
            <button class="icon-mini" data-act="routine-rename" title="이름 바꾸기">✎</button>
            <button class="icon-mini" data-act="routine-delete" title="이 서브루틴 지우기">✕</button>
            <span class="prog-cycle num"></span>
          </div>
          <div class="prog-actions">
            <button class="chip-btn" data-act="teach" data-kind="joint" title="현재 자세를 관절 이동(MoveJ) 지점으로 추가">+ MoveJ</button>
            <button class="chip-btn" data-act="teach" data-kind="linear" title="현재 자세를 직선 이동(MoveL) 지점으로 추가">+ MoveL</button>
            <button class="chip-btn" data-act="teach" data-kind="circular" title="원호 이동(MoveC): 경유점 자세에서 한 번, 끝 자세에서 한 번 누릅니다">+ MoveC</button>
            <button class="chip-btn" data-act="edge" aria-pressed="false" title="장면에서 부품의 모서리 가까이를 눌러, 그 모서리를 따라가는 경로(디버링·실링·용접)를 만듭니다">모서리</button>
            <select class="step-pick" data-opt="newstep" aria-label="동작 추가" title="고른 단계 다음에 동작을 넣습니다">
              <option value="">+ 동작</option>
              <option value="grip">그리퍼 잡기</option>
              <option value="release">그리퍼 놓기</option>
              <option value="command">장비 명령</option>
              <option value="wait">신호 대기</option>
              <option value="delay">시간 대기</option>
              <option value="next">팔레트 다음 칸</option>
              <option value="drive">AMR 주행</option>
              <option value="turn">포지셔너 회전</option>
              <option value="call">서브루틴 호출</option>
              <option value="signal_set">PLC 신호 쓰기</option>
              <option value="signal_wait">PLC 신호 대기</option>
            </select>
          </div>
          <div class="copy-bar" hidden></div>
          <div class="edge-bar" hidden></div>
          <ol class="points"></ol>
          <div class="run-bar">
            <button class="chip-btn play" data-act="play" aria-pressed="false">▶ 실행</button>
            <select data-opt="clock" aria-label="배속" title="시뮬레이션 배속">
              ${SPEEDS.map((v) => `<option value="${v}">${v}배속</option>`).join('')}
            </select>
            <span class="run-state"></span>
          </div>
          <div class="prog-foot">
            <label class="check" title="마지막 지점에서 첫 지점으로 돌아와 반복합니다"><input type="checkbox" data-opt="close"> 순환 재생</label>
            <button class="text-btn quiet" data-act="export" data-vendor="ur" title="UR 컨트롤러용 URScript 파일로 내려받기">URScript</button>
            <button class="text-btn quiet" data-act="export" data-vendor="abb" title="ABB 컨트롤러용 RAPID 모듈로 내려받기">RAPID</button>
            <button class="text-btn quiet" data-act="export" data-vendor="doosan" title="두산 로보틱스 컨트롤러용 DRL 프로그램으로 내려받기">DRL</button>
            <button class="text-btn quiet" data-act="export" data-vendor="fanuc" title="FANUC 컨트롤러용 TP 프로그램(LS)으로 내려받기">TP</button>
            <button class="text-btn quiet danger" data-act="clear">모두 지우기</button>
          </div>
        </div>`
            : ''
        }
      </section>`;
  }

  // Each robot's list of points, as its program now is.
  renderPrograms() {
    const { escapeHtml } = this.ctx;
    const seconds = (s) => `${s.toFixed(2)} s`;
    const problems = (m) =>
      (m?.problems || []).map((p) => `<div class="pprob">⚠ ${escapeHtml(p)}</div>`).join('');
    for (const section of this.host.querySelectorAll('.robot')) {
      const list = section.querySelector('.points');
      if (!list) continue;
      const program = this.programs[section.dataset.root];
      const r = this.robots.find((x) => x.root === section.dataset.root);
      const ui = r ? this.uiOf(r) : {};
      const points = program?.steps || [];
      const moves = points.filter((p) => p.type === 'move');
      // With actions the cycle is what a run measures; this is the moves'.
      section.querySelector('.prog-cycle').textContent =
        program?.cycle != null && moves.length > 1 ? `이동 ${seconds(program.cycle)}` : '';
      section.querySelector('[data-opt="close"]').checked = !!program?.close;
      // Its programs: the main one, then its routines.
      const routines = program?.routines ?? ['메인'];
      const pick = section.querySelector('[data-opt="routine"]');
      if (document.activeElement !== pick) {
        pick.innerHTML = routines
          .map((n, i) => `<option value="${i}">${escapeHtml(n)}${i === 0 && n !== '메인' ? ' (메인)' : ''}</option>`)
          .join('');
        pick.value = String(program?.active ?? 0);
      }
      section.querySelector('[data-act="routine-delete"]').hidden = !program?.active;
      section.querySelector('[data-act="clear"]').hidden = !points.length;
      // The language of the robot's controller, or every one when it is not
      // known whose the robot is.
      section
        .querySelectorAll('[data-act="export"]')
        .forEach((b) => (b.hidden = !points.length || (r.vendor != null && b.dataset.vendor !== r.vendor)));
      if (!points.length) {
        list.innerHTML =
          '<li class="points-empty">로봇을 원하는 자세로 옮긴 뒤 <b>+ MoveJ</b> 또는 <b>+ MoveL</b>로 지점을 추가하세요.</li>';
        continue;
      }
      const firstMove = points.findIndex((p) => p.type === 'move');
      // A span picked with Shift: copied as one for another machine.
      const span =
        ui.cursor == null
          ? null
          : ui.anchor != null
            ? [Math.min(ui.anchor, ui.cursor), Math.max(ui.anchor, ui.cursor)]
            : [ui.cursor, ui.cursor];
      const inRange = (i) => !!span && span[1] > span[0] && i >= span[0] && i <= span[1];
      this.renderCopyBar(section, span);
      const rows = points.map((p, i) => {
        if (p.type !== 'move') return this.actionRow(r, p, i, i === ui.cursor, inRange(i));
        const linear = p.kind !== 'joint';
        const circular = p.kind === 'circular';
        const speed = linear ? Math.round(p.speed * 1000) : Math.round(p.speed * 100);
        const bad = p.move?.problems?.length;
        // The first point is where the program starts: nothing moves into it,
        // unless the program comes back round to it.
        const into = i > firstMove || program.close;
        return `
          <li class="point${bad ? ' bad' : ''}${i === ui.cursor ? ' cursor' : ''}${inRange(i) ? ' inrange' : ''}" data-index="${i}" draggable="true">
            <span class="pidx num">${i + 1}</span>
            <span class="pname">${escapeHtml(p.name)}${p.blend ? ` <span class="pblend" title="블렌드 반경: 멈추지 않고 지나감">◠${Math.round(p.blend * 1000)}</span>` : ''}</span>
            <button class="kind-chip${linear ? ' linear' : ''}" data-act="kind" title="${circular ? '원호 이동 (MoveC) · 눌러서 직선 이동으로' : linear ? '직선 이동 (MoveL) · 눌러서 관절 이동으로' : '관절 이동 (MoveJ) · 눌러서 직선 이동으로'}">${circular ? 'C' : linear ? 'L' : 'J'}</button>
            <input class="pspeed num" type="number" data-opt="speed" value="${speed}" min="1" step="${linear ? 10 : 5}"${into ? '' : ' disabled'} aria-label="속도">
            <span class="unit">${linear ? 'mm/s' : '%'}</span>
            <span class="ptime num">${p.move ? seconds(p.move.duration) : i === firstMove ? '시작' : ''}</span>
            <span class="pacts">
              <button class="icon-mini${p.frame ? ' on' : ''}" data-act="frame" title="${p.frame ? '기준 해제: 월드 좌표로' : '가까운 장비를 기준으로: 장비가 옮겨지면 따라가고, 팔레트면 실행할 때 지금 칸으로 갑니다'}">⌖</button>
              <button class="icon-mini" data-act="goto" title="이 지점으로 이동">→</button>
              <button class="icon-mini" data-act="retouch" title="현재 자세로 다시 지정">⟳</button>
              <button class="icon-mini" data-act="delete" title="삭제">✕</button>
            </span>
            ${p.frame ? this.frameLine(p) : ''}
            ${circular && i === ui.cursor ? `<div class="pslot"><span>경유점</span> <button class="icon-mini" data-act="goto-via" title="경유점으로 이동">→</button><button class="icon-mini" data-act="retouch-via" title="현재 자세를 경유점으로">⟳</button></div>` : ''}
            ${i === ui.cursor ? `<div class="pslot"><span>블렌드</span> <input class="pblend-in num" type="number" data-opt="blend" value="${Math.round((p.blend || 0) * 1000)}" min="0" max="500" step="10" aria-label="블렌드 반경"> mm <span class="muted">${points[i + 1]?.type === 'move' ? '0이면 멈춤' : '다음이 동작이라 멈춤'}</span></div>` : ''}
            ${problems(p.move)}
          </li>`;
      });
      if (program.close && program.back) {
        rows.push(`
          <li class="point back${program.back.problems.length ? ' bad' : ''}">
            <span class="pidx num">↩</span>
            <span class="pname muted">${escapeHtml(moves[0].name)}(으)로 복귀 · ${moves[0].kind === 'linear' ? 'L' : 'J'}</span>
            <span class="ptime num">${seconds(program.back.duration)}</span>
            ${problems(program.back)}
          </li>`);
      }
      list.innerHTML = rows.join('');
      this.showRun(section, this.runOf(section.dataset.root));
    }
  }

  // An action's row: what it is, and what it is for, to be changed in
  // place.
  actionRow(r, step, i, cursor, inrange) {
    const { escapeHtml } = this.ctx;
    const pick = (field, options, value) =>
      `<select data-field="${field}">${options
        .map(([v, label]) => `<option value="${escapeHtml(v)}"${v === value ? ' selected' : ''}>${escapeHtml(label)}</option>`)
        .join('')}</select>`;
    const grippers = (r ? this.grippersOf(r) : []).map((g) => [g.object, this.ctx.records.get(g.object)?.name ?? g.name]);
    // Machines take commands; a conveyor's sensor is waited on too.
    const machines = this.machines();
    const sensed = this.cell.components.filter((c) => c.kind in SIGNALS_OF).map((c) => [c.object, c.name]);
    const kind = this.cell.components.find((c) => c.object === step.machine)?.kind ?? 'cnc';
    // One that has gone from the cell still shows by its name.
    const keep = (list, uid, name) => (list.some(([v]) => v === uid) ? list : [[uid, `${name} (없음)`], ...list]);
    let icon = '';
    let body = '';
    switch (step.type) {
      case 'grip':
      case 'release':
        icon = step.type === 'grip' ? '잡기' : '놓기';
        body = pick('gripper', keep(grippers, step.gripper, step.gripper_name), step.gripper);
        break;
      case 'command':
        icon = '명령';
        body = pick('machine', keep(machines, step.machine, step.machine_name), step.machine) + pick('command', COMMANDS_OF[kind] ?? COMMANDS, step.command);
        break;
      case 'wait':
        icon = '대기';
        body = pick('machine', keep(sensed, step.machine, step.machine_name), step.machine) + pick('signal', SIGNALS_OF[kind] ?? SIGNALS, step.signal);
        break;
      case 'delay':
        icon = '대기';
        body = `<input class="pspeed num" type="number" data-field="seconds" value="${step.seconds}" min="0" step="0.5"><span class="unit">s</span>`;
        break;
      case 'next':
        icon = '칸';
        body = pick('pallet', keep(this.pallets(), step.pallet, step.pallet_name), step.pallet) + '<span class="unit">다음 칸</span>';
        break;
      case 'call': {
        icon = '호출';
        const program = this.programs[r?.root];
        const others = (program?.routines ?? []).filter((n, i) => i !== program.active).map((n) => [n, n]);
        body =
          pick('routine', keep(others, step.routine, step.routine), step.routine) +
          `<input class="pspeed num" type="number" data-field="times" value="${step.times}" min="1" max="1000" step="1" aria-label="반복 횟수"><span class="unit">회</span>`;
        break;
      }
      case 'drive':
        icon = '주행';
        body =
          pick('amr', keep(this.ofKind('amr'), step.amr, step.amr_name), step.amr) +
          '<span class="unit">→</span>' +
          pick('stop', keep(this.ofKind('amr_stop'), step.stop, step.stop_name), step.stop);
        break;
      case 'signal_set':
      case 'signal_wait': {
        icon = 'PLC';
        const list = this.plcSignals(step.type === 'signal_wait');
        body =
          `<span class="unit">${step.type === 'signal_set' ? '쓰기' : '대기'}</span>` +
          pick('name', keep(list, step.name, step.name), step.name) +
          `<span class="unit">=</span><input class="pspeed num" type="number" data-field="value" value="${step.value}" step="1" aria-label="값">`;
        break;
      }
      case 'turn':
        icon = '회전';
        body =
          pick('positioner', keep(this.ofKind('positioner'), step.positioner, step.positioner_name), step.positioner) +
          `<input class="pspeed num" type="number" data-field="degrees" value="${Math.round(step.degrees * 10) / 10}" min="-360" max="360" step="15" aria-label="각도"><span class="unit">°</span>`;
        break;
    }
    return `
          <li class="point action${cursor ? ' cursor' : ''}${inrange ? ' inrange' : ''}" data-index="${i}" data-type="${step.type}" draggable="true">
            <span class="pidx num">${i + 1}</span>
            <span class="act-icon act-${step.type}">${icon}</span>
            <span class="act-body">${body}</span>
            <span class="pacts"><button class="icon-mini" data-act="delete" title="삭제">✕</button></span>
          </li>`;
  }

  // The action in row `row`, as its fields have it now.
  actionOf(row) {
    const step = { type: row.dataset.type };
    for (const field of row.querySelectorAll('[data-field]')) {
      step[field.dataset.field] = ['seconds', 'times', 'degrees', 'value'].includes(field.dataset.field) ? Number(field.value) : field.value;
    }
    // Another machine picked: what it takes and says, if the old does not fit.
    const kind = this.cell.components.find((c) => c.object === step.machine)?.kind;
    const fit = (value, list) => (list && !list.some(([v]) => v === value) ? list[0][0] : value);
    if (step.command) step.command = fit(step.command, COMMANDS_OF[kind]);
    if (step.signal) step.signal = fit(step.signal, SIGNALS_OF[kind]);
    return step;
  }

  // A point's frame, to be changed in place: the world, a machine, or a
  // pallet (place by place).
  frameLine(p) {
    const { escapeHtml } = this.ctx;
    const options = [
      ['', '월드 (기준 없음)'],
      ...this.machines().map(([u, n]) => [`m:${u}`, n]),
      ...this.pallets().map(([u, n]) => [`p:${u}`, `${n} · 칸마다`]),
    ];
    const value = p.pallet ? `p:${p.pallet}` : `m:${p.frame}`;
    if (!options.some(([v]) => v === value)) options.push([value, `${p.frame_name || '?'} (없음)`]);
    return `<div class="pslot"><span>⌖ 기준</span> <select data-opt="frame">${options
      .map(([v, label]) => `<option value="${escapeHtml(v)}"${v === value ? ' selected' : ''}>${escapeHtml(label)}</option>`)
      .join('')}</select></div>`;
  }

  // Under the add buttons, while a step or a span is picked: move it up or
  // down, or copy the span for another machine.
  renderCopyBar(section, span) {
    const { escapeHtml } = this.ctx;
    const bar = section.querySelector('.copy-bar');
    bar.hidden = !span;
    if (!span) return;
    const machines = this.machines();
    const copy =
      span[1] > span[0] && machines.length > 1
        ? `<select data-opt="copyto" aria-label="복제할 CNC"><option value="">CNC 골라 복제…</option>${machines
            .map(([u, n]) => `<option value="${escapeHtml(u)}">${escapeHtml(n)}용으로 복제</option>`)
            .join('')}</select>`
        : span[1] > span[0]
          ? ''
          : '<span class="muted">Shift+클릭으로 구간 선택</span>';
    // Several at once: their moves' speed and blend, set together.
    const many =
      span[1] > span[0]
        ? `<span class="many-edit">
            <label title="구간의 MoveJ 속도를 한꺼번에">J<input type="number" class="num" data-many="joint" min="1" max="100" step="5" placeholder="%"></label>
            <label title="구간의 MoveL·MoveC 속도를 한꺼번에">L<input type="number" class="num" data-many="linear" min="1" max="5000" step="10" placeholder="mm/s"></label>
            <label title="구간 이동의 블렌드 반경을 한꺼번에">R<input type="number" class="num" data-many="blend" min="0" max="200" step="5" placeholder="mm"></label>
          </span>`
        : '';
    bar.innerHTML = `<span>${span[0] === span[1] ? `${span[0] + 1}단계` : `${span[0] + 1}–${span[1] + 1}단계`}</span>
      <button class="icon-mini" data-act="shift-up" title="한 칸 위로 (끌어서 옮길 수도 있습니다)">↑</button>
      <button class="icon-mini" data-act="shift-down" title="한 칸 아래로">↓</button>
      <button class="icon-mini" data-act="steps-copy" title="복사 (Ctrl+C)">⧉</button>
      <button class="icon-mini" data-act="steps-paste" title="뒤에 붙여넣기 (Ctrl+V). 다른 로봇의 프로그램에도 붙일 수 있습니다">⎘</button>
      ${copy}${many}`;
  }

  // The span picked in robot `r`'s program: [first, last], or null.
  spanOf(r) {
    const ui = this.uiOf(r);
    if (ui.cursor == null) return null;
    return ui.anchor != null ? [Math.min(ui.anchor, ui.cursor), Math.max(ui.anchor, ui.cursor)] : [ui.cursor, ui.cursor];
  }

  // The picked steps kept to paste, here or into another robot's program.
  copySteps(r) {
    const span = this.spanOf(r);
    if (!span) return false;
    this.programOp(r, { op: 'steps_copy', index: span[0], to: span[1] }, (a) =>
      this.ctx.toast(`${a.copied}단계를 복사했습니다 (Ctrl+V로 붙여넣기)`),
    );
    return true;
  }

  // What was copied, after the step picked (or at the end), then picked.
  pasteSteps(r) {
    const ui = this.uiOf(r);
    const span = this.spanOf(r);
    const after = span ? span[1] : undefined;
    this.programOp(r, { op: 'steps_paste', after }, (a) => {
      const first = after == null ? (this.programs[r.root]?.steps.length ?? 0) : after + 1;
      ui.cursor = first + a.pasted - 1;
      ui.anchor = a.pasted > 1 ? first : null;
      this.ctx.toast(`${a.pasted}단계를 붙여 넣었습니다`);
      this.renderPrograms();
    });
  }

  // Keys for the program last clicked in: Ctrl+C, Ctrl+V. Whether taken.
  stepKey(k) {
    const r = this.robots.find((r) => r.root === this.keysFor);
    if (!r) return false;
    if (k === 'c') return this.copySteps(r);
    if (k === 'v') {
      this.pasteSteps(r);
      return true;
    }
    return false;
  }

  // The cell's components of one kind, by identity and name.
  ofKind(kind) {
    return this.cell.components.filter((c) => c.kind === kind).map((c) => [c.object, c.name]);
  }

  // The machines a robot tends: CNC and injection moulding machines.
  machines() {
    return this.cell.components.filter((c) => c.kind === 'cnc' || c.kind === 'molder').map((c) => [c.object, c.name]);
  }

  // The machine a point is at: the one whose chuck is nearest, within reach
  // of a robot working at it.
  machineNear(at) {
    let best = null;
    for (const c of this.cell.components) {
      if (c.kind !== 'cnc' || !c.seat) continue;
      const d = Math.hypot(c.seat[0] - at[0], c.seat[1] - at[1], c.seat[2] - at[2]);
      if (d < 1.0 && (!best || d < best.d)) best = { d, object: c.object };
    }
    return best?.object ?? null;
  }

  // The cell's pallets, by identity and name.
  pallets() {
    const side = { infeed: '투입', outfeed: '배출' };
    return this.cell.components
      .filter((c) => side[c.kind])
      .map((c) => [c.object, `${side[c.kind]} ${c.name}`]);
  }

  // The pallet a point stands over: the one with a place nearest it, seen
  // from above, within a pallet's reach.
  palletNear(at) {
    let best = null;
    for (const c of this.cell.components) {
      if (c.kind !== 'infeed' && c.kind !== 'outfeed') continue;
      for (const s of c.slots || []) {
        const d = Math.hypot(s[0] - at[0], s[1] - at[1]);
        if (d < 0.3 && (!best || d < best.d)) best = { d, object: c.object };
      }
    }
    return best?.object ?? null;
  }

  // A new action of `type`, for the robot's first grip point or the cell's
  // first machine.
  newAction(r, type) {
    const gripper = this.grippersOf(r)[0]?.object;
    const machine = this.machines()[0]?.[0];
    const pallet = this.pallets()[0]?.[0];
    switch (type) {
      case 'grip':
      case 'release':
        return gripper ? { type, gripper } : null;
      case 'command':
        return machine ? { type, machine, command: 'open' } : null;
      case 'wait':
        return machine ? { type, machine, signal: 'done' } : null;
      case 'delay':
        return { type, seconds: 1 };
      case 'next':
        return pallet ? { type, pallet } : null;
      case 'call': {
        // Another of the robot's programs: one that does not call itself.
        const program = this.programs[r.root];
        const routine = (program?.routines ?? []).find((n, i) => i !== program.active);
        return routine ? { type, routine, times: 1 } : null;
      }
      case 'drive': {
        const amr = this.ofKind('amr')[0]?.[0];
        const stop = this.ofKind('amr_stop')[0]?.[0];
        return amr && stop ? { type, amr, stop } : null;
      }
      case 'turn': {
        const positioner = this.ofKind('positioner')[0]?.[0];
        return positioner ? { type, positioner, degrees: 180 } : null;
      }
      case 'signal_set':
      case 'signal_wait': {
        const name = this.plcSignals(type === 'signal_wait')[0]?.[0];
        return name ? { type, name, value: 1 } : null;
      }
    }
    return null;
  }

  // The PLC signals a program can wait on (the PLC's, tied to nothing),
  // or set (the cell's, tied to nothing), as [name, name].
  plcSignals(input) {
    return (this.cell.signals ?? []).filter((s) => s.input === input && !s.link).map((s) => [s.name, s.name]);
  }

  // Each program's tool path, and its taught points, over the scene.
  drawPaths() {
    for (const child of [...this.paths.children]) {
      child.removeFromParent();
      child.geometry.dispose();
      child.material.dispose();
    }
    for (const program of Object.values(this.programs)) {
      if (program.trace.length >= 6) {
        const geometry = new THREE.BufferGeometry();
        geometry.setAttribute('position', new THREE.Float32BufferAttribute(program.trace, 3));
        const line = new THREE.Line(
          geometry,
          new THREE.LineBasicMaterial({ color: 0x5aa9ff, transparent: true, opacity: 0.9, depthTest: false }),
        );
        line.renderOrder = 9;
        this.paths.add(line);
      }
      const at = program.steps.filter((p) => p.type === 'move' && p.at).flatMap((p) => p.at);
      if (at.length) {
        const geometry = new THREE.BufferGeometry();
        geometry.setAttribute('position', new THREE.Float32BufferAttribute(at, 3));
        const dots = new THREE.Points(
          geometry,
          new THREE.PointsMaterial({ color: 0xffffff, size: 7, sizeAttenuation: false, depthTest: false }),
        );
        dots.renderOrder = 10;
        this.paths.add(dots);
      }
    }
  }

  // A request about a robot's program, with the tool its lines are drawn
  // with. In order, and none dropped: each changes the program.
  programOp(r, request, then) {
    const ui = this.uiOf(r);
    const body = { robot: r.root, tool: this.toolOf(r), offset: [0, 0, ui.offset / 1000], ...request };
    this.queue = this.queue.then(async () => {
      try {
        const answer = await post(body);
        if (!answer.ok) this.ctx.toast(answer.error || '요청을 처리하지 못했습니다');
        else then?.(answer);
      } catch (err) {
        this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
      }
    });
  }

  // The numbers: joints, and each tool where it is now.
  refresh() {
    const selected = this.ctx.selected();
    for (const r of this.robots) {
      const section = this.host.querySelector(`.robot[data-root="${r.root}"]`);
      if (!section) continue;
      for (const j of r.joints) {
        const row = section.querySelector(`.joint[data-uid="${j.uid}"]`);
        if (!row) continue;
        row.querySelector('.jval').textContent = j.moves ? jointValue(j) + (j.out_of_limits ? ' ⚠' : '') : '고정';
        row.classList.toggle('bad', j.out_of_limits);
        row.classList.toggle('selected', j.uid === selected);
        row.title = j.out_of_limits ? '한계를 벗어났습니다' : '';
        const track = row.querySelector('.track');
        if (!track || track.dataset.dragging) continue;
        const { limited, lo, hi, a, b } = spanOf(j);
        row.classList.toggle('unlimited', !limited);
        const at = (v) => `${(Math.min(Math.max((v - a) / (b - a), 0), 1) * 100).toFixed(2)}%`;
        const fill = track.querySelector('.fill');
        fill.style.display = limited ? '' : 'none';
        fill.style.left = at(lo);
        fill.style.width = `calc(${at(hi)} - ${at(lo)})`;
        let pos = j.position;
        if (!limited && !j.slides) pos = Math.atan2(Math.sin(pos), Math.cos(pos));
        track.querySelector('.mark').style.left = at(pos);
        row.querySelector('.lo').textContent = limited ? limitText(j, j.lower) : '';
        row.querySelector('.hi').textContent = limited ? limitText(j, j.upper) : '';
      }
      const tcp = this.tcpOf(r);
      const box = section.querySelector('.tcp');
      if (!tcp || !box) continue;
      const e = new THREE.Euler().setFromQuaternion(tcp.quaternion, 'ZYX');
      const v = { x: mm(tcp.position.x), y: mm(tcp.position.y), z: mm(tcp.position.z), rx: deg(e.x), ry: deg(e.y), rz: deg(e.z) };
      for (const [k, text] of Object.entries(v)) box.querySelector(`[data-v="${k}"]`).textContent = text;
      for (const row of box.querySelectorAll('.grip-row')) {
        const g = this.cell.components.find((c) => c.object === row.dataset.gripper);
        row.querySelector('.grip-state').textContent = g?.holding ? '부품 잡음' : '비어 있음';
        row.classList.toggle('holding', !!g?.holding);
      }
      const ui = this.uiOf(r);
      const status = box.querySelector('.tcp-status');
      status.textContent = ui.status;
      status.classList.toggle('bad', ui.bad);
      // Near a singularity, as posed: the joints that would race there.
      const sing = box.querySelector('.tcp-sing');
      const d = r.dexterity;
      sing.hidden = !d?.near;
      if (d?.near) {
        sing.textContent = `⚠ 특이점 근접: ${d.joints.join('·')} 관절이 급하게 돌 수 있음`;
        sing.title = `자코비안 최소 특이값 ${(d.least * 1000).toFixed(1)} mm/rad (경고 기준 10 mm/rad)`;
      }
      if (this.reach?.root === r.root) this.placeReach(r);
      if (this.handleRoot === r.root && !this.dragging) {
        this.proxy.position.copy(tcp.position);
        this.proxy.quaternion.copy(tcp.quaternion);
        this.proxy.updateMatrixWorld();
      }
    }
  }

  // Whether a click in the scene picks an edge.
  picking() {
    return this.edge != null;
  }

  // An edge of object `uid` picked near `point`: drawn, and offered to be
  // followed.
  async pickEdge(uid, point) {
    const r = this.robots.find((x) => x.root === this.edge?.root);
    if (!r) return;
    const ui = this.uiOf(r);
    const request = {
      op: 'edge',
      robot: r.root,
      tool: this.toolOf(r),
      offset: [0, 0, ui.offset / 1000],
      object: uid,
      point: [point.x, point.y, point.z],
    };
    let answer;
    try {
      answer = await post(request);
    } catch (err) {
      this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
      return;
    }
    if (!answer.ok) {
      this.ctx.toast(answer.error || '모서리를 찾지 못했습니다');
      return;
    }
    if (this.edge.line) this.dropEdgeLine();
    const geometry = new THREE.BufferGeometry().setFromPoints(answer.points.map((p) => new THREE.Vector3(...p)));
    const line = new THREE.Line(geometry, new THREE.LineBasicMaterial({ color: 0xff9a3c, depthTest: false }));
    line.renderOrder = 11;
    this.ctx.scene.add(line);
    this.edge.line = line;
    this.edge.picked = { object: uid, point: request.point };
    const bar = this.host.querySelector(`.robot[data-root="${r.root}"] .edge-bar`);
    bar.hidden = false;
    bar.innerHTML = `
      <span class="edge-what">모서리 ${(answer.length * 1000).toFixed(0)} mm${answer.closed ? ' (닫힘)' : ''}</span>
      <label>속도 <input class="pspeed num" type="number" data-edge="speed" value="50" min="1" max="1000" step="5"><span class="unit">mm/s</span></label>
      <label>접근 <input class="pspeed num" type="number" data-edge="approach" value="50" min="0" max="500" step="10"><span class="unit">mm</span></label>
      <button class="text-btn" data-act="edge-add">프로그램에 추가</button>
      <button class="text-btn quiet" data-act="edge-cancel">취소</button>`;
  }

  dropEdgeLine() {
    this.ctx.scene.remove(this.edge.line);
    this.edge.line.geometry.dispose();
    this.edge.line.material.dispose();
    this.edge.line = null;
  }

  stopEdge() {
    if (!this.edge) return;
    if (this.edge.line) this.dropEdgeLine();
    const bar = this.host.querySelector(`.robot[data-root="${this.edge.root}"] .edge-bar`);
    if (bar) {
      bar.hidden = true;
      bar.innerHTML = '';
    }
    this.setEdgeButton(this.edge.root, false);
    this.edge = null;
  }

  setEdgeButton(root, on) {
    this.host.querySelector(`.robot[data-root="${root}"] [data-act="edge"]`)?.setAttribute('aria-pressed', String(on));
  }

  // Where robot `r` reaches: KYNEX's section of it, swept round the first
  // joint, see-through, with the section's edge drawn where it is cut by
  // the base frame's XZ plane.
  async showReach(r) {
    this.hideReach();
    let answer;
    try {
      answer = await post({ op: 'reach', robot: r.root });
    } catch (err) {
      this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
      return;
    }
    if (!answer.ok) {
      this.ctx.toast(answer.error || '도달 범위를 구하지 못했습니다');
      return;
    }
    const points = answer.section.map(([rr, z]) => new THREE.Vector2(rr, z));
    const [lo, hi] = answer.turn;
    const sweep = Math.min(hi - lo, Math.PI * 2);
    // A lathe turns about its Y; turned onto Z, its angle 0 lies along -Y,
    // so the first joint's angle a is the lathe's a + 90°.
    const shell = new THREE.LatheGeometry(points, 96, lo + Math.PI / 2, sweep);
    shell.rotateX(Math.PI / 2);
    const group = new THREE.Group();
    group.matrixAutoUpdate = false;
    group.add(
      new THREE.Mesh(
        shell,
        new THREE.MeshBasicMaterial({ color: 0x4aa3ff, transparent: true, opacity: 0.1, depthWrite: false, side: THREE.DoubleSide }),
      ),
    );
    const edge = new THREE.LineBasicMaterial({ color: 0x4aa3ff, transparent: true, opacity: 0.8 });
    for (const side of [1, -1]) {
      const line = points.map((p) => new THREE.Vector3(side * p.x, 0, p.y));
      group.add(new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(line), edge));
    }
    this.ctx.scene.add(group);
    // The base frame from the robot's root, to follow it when moved.
    const root = this.ctx.records.get(r.root);
    const base = new THREE.Matrix4().fromArray(answer.base);
    const offset = root ? new THREE.Matrix4().fromArray(root.matrix).invert().multiply(base) : new THREE.Matrix4();
    this.reach = { root: r.root, group, offset };
    this.placeReach(r);
    this.ctx.toast(`도달 범위: 플랜지 최대 ${(answer.farthest * 1000).toFixed(0)} mm`);
    this.host
      .querySelector(`.robot[data-root="${r.root}"] [data-act="reach"]`)
      ?.setAttribute('aria-pressed', 'true');
    this.ctx.render?.();
  }

  placeReach(r) {
    const root = this.ctx.records.get(r.root);
    if (!this.reach || !root) return;
    this.reach.group.matrix.fromArray(root.matrix).multiply(this.reach.offset);
    this.reach.group.matrixWorldNeedsUpdate = true;
  }

  hideReach() {
    if (!this.reach) return;
    this.ctx.scene.remove(this.reach.group);
    this.reach.group.traverse((o) => {
      o.geometry?.dispose();
      o.material?.dispose();
    });
    this.host
      .querySelector(`.robot[data-root="${this.reach.root}"] [data-act="reach"]`)
      ?.setAttribute('aria-pressed', 'false');
    this.reach = null;
    this.ctx.render?.();
  }

  // The tool point's world pose: the tool link's matrix, then the offset
  // along its Z, as KYNEX works it out.
  tcpOf(r) {
    const tool = this.toolOf(r);
    const rec = tool && this.ctx.records.get(tool);
    if (!rec) return null;
    const ui = this.uiOf(r);
    const m = new THREE.Matrix4().fromArray(rec.matrix);
    m.multiply(new THREE.Matrix4().makeTranslation(0, 0, ui.offset / 1000));
    const position = new THREE.Vector3(), quaternion = new THREE.Quaternion(), scale = new THREE.Vector3();
    m.decompose(position, quaternion, scale);
    return { position, quaternion };
  }

  robotOf(el) {
    const root = el.closest('.robot')?.dataset.root;
    return this.robots.find((r) => r.root === root);
  }

  // ---- requests -------------------------------------------------------------

  send(r, request) {
    let lane = this.lanes.get(r.root);
    if (!lane) this.lanes.set(r.root, (lane = { busy: false, next: null }));
    if (lane.busy) {
      lane.next = request;
      return;
    }
    this.run(r, lane, request);
  }

  async run(r, lane, request) {
    lane.busy = true;
    try {
      const answer = await post(request);
      const ui = this.uiOf(r);
      if (!answer.ok) {
        this.ctx.toast(answer.error || '요청을 처리하지 못했습니다');
      } else if (request.op === 'ik') {
        ui.bad = !answer.reached;
        ui.status = answer.reached
          ? '도달'
          : `도달하지 못함 · 위치 ${mm(answer.position_error)} mm` +
            (request.rotation ? ` · 자세 ${deg(answer.rotation_error)}°` : '');
      } else if (request.op === 'release') {
        ui.status = '';
        ui.bad = false;
      }
      this.refresh();
    } catch (err) {
      this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
    }
    lane.busy = false;
    if (lane.next) {
      const next = lane.next;
      lane.next = null;
      this.run(r, lane, next);
    }
  }

  ik(r, position, quaternion) {
    const ui = this.uiOf(r);
    this.send(r, {
      op: 'ik',
      robot: r.root,
      tool: this.toolOf(r),
      offset: [0, 0, ui.offset / 1000],
      position: position.toArray(),
      rotation: quaternion ? quaternion.toArray() : null,
    });
  }

  // ---- joints ---------------------------------------------------------------

  // Steps dragged in the list to another place: the span picked, if the
  // step taken is in it, else that step alone. Dropped on the upper half of
  // a step, before it; on the lower half, after it.
  wireDrag() {
    const rowOf = (e) => e.target.closest?.('.point[data-index]');
    this.host.addEventListener('dragstart', (e) => {
      const row = rowOf(e);
      const r = row && this.robotOf(row);
      if (!r) return;
      const i = Number(row.dataset.index);
      const span = this.spanOf(r);
      this.dragSteps = { root: r.root, span: span && i >= span[0] && i <= span[1] ? span : [i, i] };
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/kynex-steps', String(i));
    });
    this.host.addEventListener('dragend', () => {
      this.dragSteps = null;
      this.host.querySelectorAll('.drop-before, .drop-after').forEach((el) => el.classList.remove('drop-before', 'drop-after'));
    });
    const place = (e) => {
      const row = rowOf(e);
      const r = row && this.robotOf(row);
      if (!this.dragSteps || !r || r.root !== this.dragSteps.root) return null;
      const box = row.getBoundingClientRect();
      const after = e.clientY > box.top + box.height / 2;
      return { row, r, before: Number(row.dataset.index) + (after ? 1 : 0), after };
    };
    this.host.addEventListener('dragover', (e) => {
      const at = place(e);
      this.host.querySelectorAll('.drop-before, .drop-after').forEach((el) => el.classList.remove('drop-before', 'drop-after'));
      if (!at) return;
      e.preventDefault();
      at.row.classList.add(at.after ? 'drop-after' : 'drop-before');
    });
    this.host.addEventListener('drop', (e) => {
      const at = place(e);
      if (!at) return;
      e.preventDefault();
      const [from, to] = this.dragSteps.span;
      if (at.before >= from && at.before <= to + 1) return;
      const ui = this.uiOf(at.r);
      this.programOp(at.r, { op: 'steps_move', index: from, to, before: at.before }, () => {
        // The steps where they went, still picked.
        const first = at.before > to ? at.before - (to - from + 1) : at.before;
        ui.cursor = first + (to - from);
        ui.anchor = to > from ? first : null;
        this.renderPrograms();
      });
    });
  }

  pointerDown(e) {
    // Ctrl+C and Ctrl+V are the program's while it was last clicked in.
    this.keysFor = e.target.closest('.points, .copy-bar') ? this.robotOf(e.target)?.root ?? null : null;
    const track = e.target.closest('.track');
    if (track) return this.dragJoint(e, track);
    const jog = e.target.closest('[data-jog]');
    if (jog) return this.holdJog(e, jog);
  }

  dragJoint(e, track) {
    const r = this.robotOf(track);
    const uid = track.closest('.joint').dataset.uid;
    const j = r?.joints.find((x) => x.uid === uid);
    if (!j || e.button !== 0) return;
    e.preventDefault();
    track.setPointerCapture(e.pointerId);
    track.dataset.dragging = '1';
    const { limited, lo, hi, a, b } = spanOf(j);
    const move = (ev) => {
      const rect = track.getBoundingClientRect();
      const t = Math.min(Math.max((ev.clientX - rect.left) / rect.width, 0), 1);
      let v = a + t * (b - a);
      if (limited) v = Math.min(Math.max(v, lo), hi);
      // Where the pointer is, at once; the joint follows when KYNEX answers.
      track.querySelector('.mark').style.left = `${(((v - a) / (b - a)) * 100).toFixed(2)}%`;
      this.send(r, { op: 'jog', robot: r.root, joint: uid, position: v });
    };
    const up = () => {
      delete track.dataset.dragging;
      track.removeEventListener('pointermove', move);
      track.removeEventListener('pointerup', up);
      track.removeEventListener('pointercancel', up);
    };
    track.addEventListener('pointermove', move);
    track.addEventListener('pointerup', up);
    track.addEventListener('pointercancel', up);
    move(e);
  }

  // ---- the pendant ----------------------------------------------------------

  holdJog(e, button) {
    const r = this.robotOf(button);
    const tcp = r && this.tcpOf(r);
    if (!tcp || e.button !== 0) return;
    e.preventDefault();
    // Steps add up on a target of their own: the tool's reported pose lags
    // a request behind, and stepping from it would lose steps.
    const target = { position: tcp.position.clone(), quaternion: tcp.quaternion.clone() };
    const axis = button.dataset.jog;
    const dir = Number(button.dataset.dir);
    const step = () => this.jogStep(r, target, axis, dir);
    step();
    let timer = setTimeout(function again() {
      step();
      timer = setTimeout(again, REPEAT_EVERY);
    }, REPEAT_DELAY);
    button.classList.add('down');
    const stop = () => {
      clearTimeout(timer);
      button.classList.remove('down');
      window.removeEventListener('pointerup', stop);
      window.removeEventListener('pointercancel', stop);
    };
    window.addEventListener('pointerup', stop);
    window.addEventListener('pointercancel', stop);
  }

  jogStep(r, target, axis, dir) {
    const ui = this.uiOf(r);
    const s = STEPS[ui.step];
    const turn = axis.startsWith('r');
    const unit = AXES[turn ? axis[1] : axis].clone();
    // In the tool's frame, the axis turns with the tool.
    if (ui.frame === 'tool') unit.applyQuaternion(target.quaternion);
    if (turn) {
      target.quaternion.premultiply(new THREE.Quaternion().setFromAxisAngle(unit, dir * s.rad)).normalize();
      this.ik(r, target.position, target.quaternion);
    } else {
      target.position.addScaledVector(unit, dir * s.m);
      this.ik(r, target.position, ui.lock ? target.quaternion : null);
    }
  }

  // ---- the handle -----------------------------------------------------------

  showHandle(root) {
    this.handleRoot = root;
    if (root) {
      this.handle.attach(this.proxy);
      this.ctx.scene.add(this.handleHelper);
      const r = this.robots.find((x) => x.root === root);
      this.handle.setMode(this.uiOf(r).mode);
    } else {
      this.handle.detach();
      this.handleHelper.removeFromParent();
    }
    for (const section of this.host.querySelectorAll('.robot')) {
      const on = section.dataset.root === root;
      section.querySelector('[data-act="handle"]')?.setAttribute('aria-pressed', String(on));
      const modes = section.querySelector('.handle-modes');
      if (modes) modes.hidden = !on;
    }
    this.refresh();
  }

  pulled() {
    const r = this.robots.find((x) => x.root === this.handleRoot);
    if (!r) return;
    const ui = this.uiOf(r);
    const keep = ui.mode === 'rotate' || ui.lock;
    this.ik(r, this.proxy.position, keep ? this.proxy.quaternion : null);
  }

  // ---- buttons and options --------------------------------------------------

  clicked(e) {
    const r = this.robotOf(e.target);
    if (!r) return;
    const ui = this.uiOf(r);
    const act = e.target.closest('[data-act]')?.dataset.act;
    const row = e.target.closest('.point[data-index]');
    const index = row ? Number(row.dataset.index) : null;
    // A click on a step's row (not its controls) picks it: what is taught
    // or added next goes after it.
    if (row && !e.target.closest('button, input, select')) {
      if (e.shiftKey && ui.cursor != null && ui.cursor !== index) {
        ui.anchor = index;
      } else {
        ui.cursor = ui.cursor === index ? null : index;
        ui.anchor = null;
      }
      this.renderPrograms();
      return;
    }
    if (act === 'session') {
      this.send(r, { op: 'release', robot: r.root });
    } else if (act === 'handle') {
      this.showHandle(this.handleRoot === r.root ? null : r.root);
    } else if (act === 'edge') {
      if (this.edge?.root === r.root) this.stopEdge();
      else {
        this.stopEdge();
        this.edge = { root: r.root, picked: null, line: null };
        this.setEdgeButton(r.root, true);
        this.ctx.toast('부품의 모서리 가까이를 누르세요');
      }
    } else if (act === 'edge-cancel') {
      this.stopEdge();
    } else if (act === 'edge-add') {
      const picked = this.edge?.picked;
      if (!picked) return;
      const bar = e.target.closest('.edge-bar');
      const speed = Number(bar.querySelector('[data-edge="speed"]').value) / 1000;
      const approach = Number(bar.querySelector('[data-edge="approach"]').value) / 1000;
      this.programOp(
        r,
        { op: 'edge_path', object: picked.object, point: picked.point, speed, approach, after: ui.cursor ?? undefined },
        (a) => {
          if (ui.cursor != null) ui.cursor += a.added;
          this.ctx.toast(
            a.missed
              ? `모서리 경로 ${a.added}개 지점을 넣었습니다. ${a.missed}개는 닿지 않습니다`
              : `모서리 경로 ${a.added}개 지점을 넣었습니다`,
          );
          this.stopEdge();
        },
      );
    } else if (act === 'reach') {
      if (this.reach?.root === r.root) this.hideReach();
      else this.showReach(r);
    } else if (act === 'grip' || act === 'release') {
      const gripper = e.target.closest('[data-gripper]').dataset.gripper;
      this.programOp(r, { op: act, gripper });
    } else if (act === 'teach') {
      const kind = e.target.closest('[data-kind]').dataset.kind;
      this.programOp(r, { op: 'teach', kind, after: ui.cursor ?? undefined }, (a) => {
        if (a.via) return this.ctx.toast('경유점을 저장했습니다. 끝 자세로 옮긴 뒤 + MoveC를 다시 누르세요');
        if (ui.cursor != null) ui.cursor += 1;
      });
    } else if (act === 'play') {
      const run = this.runOf(r.root);
      const going = run && !['done', 'alarm'].includes(run.state);
      this.programOp(r, { op: going ? 'stop' : 'play' });
    } else if (act === 'export') {
      this.programOp(r, { op: 'export', vendor: e.target.closest('[data-vendor]').dataset.vendor }, (answer) => {
        // Handed to the browser as a file to keep.
        const link = document.createElement('a');
        link.href = URL.createObjectURL(new Blob([answer.script], { type: 'text/plain' }));
        link.download = answer.file;
        link.click();
        setTimeout(() => URL.revokeObjectURL(link.href), 1000);
        this.ctx.toast(`${answer.file}을(를) 내려받았습니다`);
      });
    } else if (act === 'clear') {
      if (confirm('이 로봇의 프로그램 지점을 모두 지울까요?')) this.programOp(r, { op: 'clear' });
    } else if (act === 'routine-new') {
      const name = prompt('새 서브루틴 이름', '');
      if (name === null) return;
      ui.cursor = null;
      ui.anchor = null;
      this.programOp(r, { op: 'routine_new', name });
    } else if (act === 'routine-rename') {
      const program = this.programs[r.root];
      const name = prompt('프로그램 이름', program?.routines?.[program.active] ?? '');
      if (name) this.programOp(r, { op: 'routine_rename', name });
    } else if (act === 'routine-delete') {
      const program = this.programs[r.root];
      const name = program?.routines?.[program.active];
      if (name && confirm(`서브루틴 ${name}을(를) 지울까요? 이것을 부르는 단계는 실행할 때 알람이 됩니다.`)) {
        ui.cursor = null;
        ui.anchor = null;
        this.programOp(r, { op: 'routine_delete' });
      }
    } else if (act === 'steps-copy') {
      this.copySteps(r);
    } else if (act === 'steps-paste') {
      this.pasteSteps(r);
    } else if (act === 'shift-up' || act === 'shift-down') {
      const up = act === 'shift-up';
      const from = Math.min(ui.anchor ?? ui.cursor, ui.cursor);
      const to = Math.max(ui.anchor ?? ui.cursor, ui.cursor);
      this.programOp(r, { op: 'shift', from, to, up }, () => {
        const by = up ? -1 : 1;
        ui.cursor += by;
        if (ui.anchor != null) ui.anchor += by;
        this.renderPrograms();
      });
    } else if (act === 'frame' && index != null) {
      const point = this.programs[r.root]?.steps[index];
      if (point?.frame) this.programOp(r, { op: 'edit', index, frame: null });
      else if (point?.at) {
        const pallet = this.palletNear(point.at);
        const machine = pallet ? null : this.machineNear(point.at);
        if (pallet) this.programOp(r, { op: 'edit', index, pallet });
        else if (machine) this.programOp(r, { op: 'edit', index, frame: machine });
        else this.ctx.toast('이 지점 가까이에 CNC나 팔레트가 없습니다. 아래 줄에서 직접 고를 수도 있습니다');
      }
    } else if ((act === 'goto-via' || act === 'retouch-via') && index != null) {
      this.programOp(r, { op: act === 'goto-via' ? 'goto' : 'retouch', index, via: true });
    } else if (act === 'kind' && index != null) {
      const point = this.programs[r.root]?.steps[index];
      // C goes to L, L to J, J to L.
      if (point) this.programOp(r, { op: 'edit', index, kind: point.kind === 'linear' ? 'joint' : 'linear' });
    } else if (['goto', 'retouch', 'delete'].includes(act) && index != null) {
      if (act === 'delete' && ui.cursor != null && ui.cursor >= index) ui.cursor = ui.cursor > 0 ? ui.cursor - 1 : null;
      this.programOp(r, { op: act, index });
    }
    const frame = e.target.closest('[data-frame]');
    if (frame) {
      ui.frame = frame.dataset.frame;
      frame.parentElement.querySelectorAll('button').forEach((b) => b.classList.toggle('on', b === frame));
    }
    const mode = e.target.closest('[data-mode]');
    if (mode) {
      ui.mode = mode.dataset.mode;
      mode.parentElement.querySelectorAll('button').forEach((b) => b.classList.toggle('on', b === mode));
      if (this.handleRoot === r.root) this.handle.setMode(ui.mode);
    }
  }

  optionChanged(e) {
    const r = this.robotOf(e.target);
    if (!r) return;
    const ui = this.uiOf(r);
    const opt = e.target.dataset.opt;
    if (opt === 'step') ui.step = Number(e.target.value);
    if (opt === 'lock') ui.lock = e.target.checked;
    if (opt === 'offset') {
      const v = Number(e.target.value);
      ui.offset = Number.isFinite(v) ? v : 0;
      this.refresh();
      // The program's lines are timed for the tool point: again for this one.
      if (this.programs[r.root]) this.programOp(r, { op: 'program' });
    }
    if (opt === 'close') this.programOp(r, { op: 'close', close: e.target.checked });
    if (opt === 'tool') {
      ui.tool = e.target.value;
      this.render();
      // The program's lines are drawn with the tool point: again for this one.
      if (this.programs[r.root]) this.programOp(r, { op: 'program' });
    }
    if (opt === 'clock') this.programOp(r, { op: 'speed', speed: Number(e.target.value) });
    if (opt === 'newstep' && e.target.value) {
      const type = e.target.value;
      const step = this.newAction(r, type);
      e.target.value = '';
      if (!step) {
        return this.ctx.toast(
          type === 'call'
            ? '부를 서브루틴이 없습니다. 프로그램 옆 + 로 서브루틴을 만드세요'
            : type.startsWith('signal')
              ? `${type === 'signal_set' ? 'KYNEX → PLC' : 'PLC → KYNEX'} 방향의 연결 없는 신호가 없습니다. 툴바의 PLC에서 신호를 만드세요`
              : '이 동작에 쓸 그리퍼, 장비, 팔레트, AMR이나 정지 위치가 셀에 없습니다. 먼저 셀 역할을 정하세요',
        );
      }
      this.programOp(r, { op: 'add', step, after: ui.cursor ?? undefined });
      if (ui.cursor != null) ui.cursor += 1;
    }
    if (e.target.dataset.field) {
      const row = e.target.closest('.point.action');
      this.programOp(r, { op: 'edit', index: Number(row.dataset.index), step: this.actionOf(row) });
    }
    if (opt === 'routine') {
      ui.cursor = null;
      ui.anchor = null;
      this.programOp(r, { op: 'routine_select', index: Number(e.target.value) });
    }
    if (opt === 'blend') {
      const index = Number(e.target.closest('.point').dataset.index);
      const v = Number(e.target.value);
      if (Number.isFinite(v) && v >= 0) this.programOp(r, { op: 'edit', index, blend: v / 1000 });
    }
    if (opt === 'frame') {
      const index = Number(e.target.closest('.point').dataset.index);
      const [kind, uid] = e.target.value.split(':');
      this.programOp(r, kind === 'p' ? { op: 'edit', index, pallet: uid } : { op: 'edit', index, frame: uid || null });
    }
    // Several steps' speed or blend, set together.
    const many = e.target.dataset.many;
    if (many) {
      const span = this.spanOf(r);
      const v = Number(e.target.value);
      if (!span || !Number.isFinite(v) || v < 0 || e.target.value === '') return;
      const request =
        many === 'blend'
          ? { op: 'edit', index: span[0], to: span[1], blend: v / 1000 }
          : { op: 'edit', index: span[0], to: span[1], only: many, speed: many === 'joint' ? v / 100 : v / 1000 };
      this.programOp(r, request, (a) => this.ctx.toast(`${a.edited}개 이동을 바꿨습니다`));
      return;
    }
    if (opt === 'copyto' && e.target.value) {
      const [from, to] = [Math.min(ui.anchor, ui.cursor), Math.max(ui.anchor, ui.cursor)];
      this.programOp(r, { op: 'copy', from, to, machine: e.target.value }, (a) => {
        this.ctx.toast(`${a.added}단계를 복제했습니다. 복제한 지점은 그 CNC를 기준으로 합니다`);
        ui.anchor = null;
        ui.cursor = to + a.added;
        this.renderPrograms();
      });
    }
    if (opt === 'speed') {
      const index = Number(e.target.closest('.point').dataset.index);
      const point = this.programs[r.root]?.steps[index];
      const v = Number(e.target.value);
      if (point && Number.isFinite(v) && v > 0) {
        this.programOp(r, { op: 'edit', index, speed: point.kind === 'linear' ? v / 1000 : v / 100 });
      }
    }
  }
}
