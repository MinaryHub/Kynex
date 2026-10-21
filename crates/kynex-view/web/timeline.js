// The timeline: what the robot and each machine did while the program ran,
// as a Gantt chart over the run, with how much of it each machine was
// cutting, how much the robot worked and waited, and the parts made an
// hour. KYNEX records it (the `timeline` event); this draws it.
//
// Beside it, scenarios: the cell saved as it stands, several times over —
// one CNC machine, two, another gripper — run for hours away from the view
// and compared (the `scenarios` event).

import { costCards, csv, csvText, figures, laneTable, oeeTable, reportHtml, seriesCharts, stateChart, summary, sweepChart, throughputChart } from './stats.js';
import { FIELDS } from './cell.js';
import { FlowChart } from './flowchart.js';

// Hands `text` to the browser as a file called `name`.
function download(name, text, type) {
  const link = document.createElement('a');
  link.href = URL.createObjectURL(new Blob([text], { type }));
  link.download = name;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(link.href), 1000);
}

// Today and now, for a file's name: 20261016-1530.
function stamp() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`;
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

const ACTIVITIES = {
  move: ['이동', '#5aa9ff'],
  grip: ['그리퍼', '#ff8a3d'],
  wait: ['신호 대기', '#f2b53c'],
  pause: ['시간 대기', '#8a93a1'],
  alarm: ['알람', '#ff5f5f'],
  cut: ['가공', '#3fcf8e'],
  door: ['문 개폐', '#7d8fa6'],
  blocked: ['언로드 대기', '#e0a43a'],
  drive: ['주행', '#9b7bff'],
  walk: ['보행', '#c08bff'],
  down: ['고장', '#d9534f'],
  maint: ['보전', '#4f8fd9'],
  rest: ['휴식', '#556070'],
  charge: ['충전', '#d4b83a'],
  off: ['근무 외', '#3a3f48'],
  work: ['작업', '#2fbfa0'],
  idle: ['쉼', '#2f353f'],
};

const pct = (v) => `${Math.round((v || 0) * 100)}%`;

function clock(s) {
  const m = Math.floor(s / 60);
  return `${m}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
}

const mean = (list, key) => (list?.length ? list.reduce((s, x) => s + (x[key] || 0), 0) / list.length : 0);

export class TimelinePanel {
  // ctx: { escapeHtml, toast, toggle(on) }
  constructor(ctx) {
    this.ctx = ctx;
    this.box = document.getElementById('timeline');
    this.data = null;
    this.scenarios = { list: [], compared: [], comparing: false };
    this.tab = 'timeline';
    this.chart = new FlowChart(document.getElementById('tl-view-flowchart'), {
      escapeHtml: ctx.escapeHtml,
      toast: ctx.toast,
      select: (uid) => ctx.select?.(uid),
      showMenu: ctx.showMenu,
      equipmentMenu: ctx.equipmentMenu,
      linkMenu: ctx.linkMenu,
    });
    document.getElementById('tl-close').addEventListener('click', () => this.ctx.toggle(false));
    for (const b of this.box.querySelectorAll('[data-tl-tab]')) {
      b.addEventListener('click', () => this.showTab(b.dataset.tlTab));
    }
    document.getElementById('sc-save').addEventListener('click', () => {
      const name = document.getElementById('sc-name');
      this.send({ action: 'save', name: name.value }, () => {
        this.ctx.toast('시나리오를 저장했습니다');
        name.value = '';
      });
    });
    document.getElementById('sc-name').addEventListener('keydown', (e) => {
      if (e.key === 'Enter') document.getElementById('sc-save').click();
      e.stopPropagation();
    });
    document.getElementById('sc-run').addEventListener('click', () => {
      // Nothing saved: said so, not a button that does nothing.
      if (!this.scenarios.list.length) return this.ctx.toast('먼저 현재 구성 저장으로 시나리오를 저장하세요');
      this.send({
        action: 'run',
        seconds: Number(document.getElementById('sc-hours').value) * 3600,
        reps: Number(document.getElementById('sc-reps').value),
      });
    });
    document.getElementById('sc-list').addEventListener('click', (e) => {
      const b = e.target.closest('[data-sc]');
      if (!b) return;
      const index = Number(b.closest('[data-index]').dataset.index);
      if (b.dataset.sc === 'remove') this.send({ action: 'remove', index });
      if (b.dataset.sc === 'load') {
        this.send({ action: 'load', index }, () => this.ctx.toast('시나리오의 장비, 셀 역할, 프로그램을 불러왔습니다'));
      }
    });
    document.getElementById('st-csv').addEventListener('click', () => {
      if (!this.data?.lanes?.length) return this.ctx.toast('먼저 프로그램을 실행하세요');
      download(`kynex-통계-${stamp()}.csv`, csv(this.data, ACTIVITIES, this.flowState), 'text/csv;charset=utf-8');
    });
    document.getElementById('st-report').addEventListener('click', () => {
      if (!this.data?.lanes?.length) return this.ctx.toast('먼저 프로그램을 실행하세요');
      const win = window.open('', '_blank');
      if (!win) return this.ctx.toast('새 창을 열 수 없습니다. 팝업 차단을 확인하세요');
      win.document.write(reportHtml(this.data, ACTIVITIES, this.ctx.escapeHtml, 'KYNEX 통계 보고서', this.flowState));
      win.document.close();
    });
    document.getElementById('sc-csv').addEventListener('click', () => this.scenariosCsv());
    // A sweep: one setting of one component at several values.
    this.components = [];
    document.getElementById('sw-object').addEventListener('change', () => this.sweepProps());
    document.getElementById('sw-run').addEventListener('click', () => {
      const object = document.getElementById('sw-object').value;
      const field = this.sweepFields(object).find((f) => f[0] === document.getElementById('sw-prop').value);
      if (!field) return this.ctx.toast('탐색할 컴포넌트와 설정을 고르세요');
      const shown = document
        .getElementById('sw-values')
        .value.split(/[,\s]+/)
        .filter(Boolean)
        .map(Number);
      if (!shown.length || shown.some((v) => !Number.isFinite(v))) return this.ctx.toast('값을 쉼표로 나눠 넣으세요 (예: 20, 30, 40)');
      if (shown.length > 12) return this.ctx.toast('값은 12개까지입니다');
      const [prop, label, unit, scale] = field;
      this.send({
        action: 'sweep',
        object,
        prop,
        label,
        unit,
        scale,
        values: shown.map((v) => v / scale),
        seconds: Number(document.getElementById('sc-hours').value) * 3600,
        reps: Number(document.getElementById('sc-reps').value),
      });
    });
    // The process flow: run, stop, reset.
    this.flowState = null;
    document.getElementById('flow-run').addEventListener('click', () =>
      this.flowOp(this.flowState?.on ? 'stop' : 'start'),
    );
    document.getElementById('flow-reset').addEventListener('click', () => this.flowOp('reset'));
    // How fast the factory runs: the simulation's clock, shared with the
    // robots' programs.
    document.getElementById('flow-speed').addEventListener('change', async (e) => {
      try {
        const answer = await post({ op: 'speed', speed: Number(e.target.value) });
        if (!answer.ok) this.ctx.toast(answer.error || '배속을 바꾸지 못했습니다');
      } catch (err) {
        this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
      }
    });
    // The flow's settings, all of them each time: the calendar and the
    // cost's figures.
    for (const id of ['flow-work', 'flow-rest', 'flow-warmup', 'cost-hours', 'cost-years', 'cost-margin']) {
      document.getElementById(id).addEventListener('change', () => {
        const n = (id) => Number(document.getElementById(id).value) || 0;
        post({
          op: 'flow_settings',
          work: n('flow-work'),
          rest: n('flow-rest'),
          warmup: n('flow-warmup') * 60,
          hours: n('cost-hours'),
          years: n('cost-years'),
          margin: n('cost-margin'),
        }).then((a) => {
          if (!a.ok) this.ctx.toast(a.error || '설정을 바꾸지 못했습니다');
        });
      });
    }
    document.getElementById('tl-legend').innerHTML = Object.entries(ACTIVITIES)
      .map(([, [label, color]]) => `<span><i style="background:${color}"></i>${label}</span>`)
      .join('');
  }

  show(on) {
    this.box.hidden = !on;
    if (on) this.render();
  }

  showTab(tab) {
    this.tab = tab;
    for (const b of this.box.querySelectorAll('[data-tl-tab]')) b.classList.toggle('on', b.dataset.tlTab === tab);
    document.getElementById('tl-view-timeline').hidden = tab !== 'timeline';
    document.getElementById('tl-view-scenarios').hidden = tab !== 'scenarios';
    document.getElementById('tl-view-stats').hidden = tab !== 'stats';
    document.getElementById('tl-view-flowchart').hidden = tab !== 'flowchart';
    this.render();
  }

  async send(request, then) {
    try {
      const answer = await post({ op: 'scenario', ...request });
      if (!answer.ok) this.ctx.toast(answer.error || '요청을 처리하지 못했습니다');
      else then?.(answer);
    } catch (err) {
      this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
    }
  }

  // The simulation's clock as KYNEX has it: the speed shown, unless it is
  // being chosen.
  setClock(clock) {
    const speed = document.getElementById('flow-speed');
    if (!clock || document.activeElement === speed) return;
    const v = String(clock.speed);
    if (![...speed.options].some((o) => o.value === v)) speed.add(new Option(`${v}배속`, v));
    speed.value = v;
  }

  // The `scenarios` event.
  setScenarios(data) {
    this.scenarios = data;
    if (!this.box.hidden) this.render();
  }

  // The settings of component `object` a sweep may vary: numbers, not
  // choices.
  sweepFields(object) {
    const c = this.components.find((c) => c.object === object);
    return ((c && FIELDS[c.kind]) || []).filter((f) => !f[4]);
  }

  // The sweep's components: those with settings to vary.
  sweepObjects() {
    const box = document.getElementById('sw-object');
    const list = this.components.filter((c) => this.sweepFields(c.object).length);
    const key = list.map((c) => `${c.object}:${c.name}`).join('|');
    if (key === this.sweepKey) return;
    this.sweepKey = key;
    const was = box.value;
    const { escapeHtml } = this.ctx;
    box.innerHTML = list.map((c) => `<option value="${escapeHtml(c.object)}">${escapeHtml(c.name)}</option>`).join('');
    if (list.some((c) => c.object === was)) box.value = was;
    this.sweepProps();
  }

  sweepProps() {
    const box = document.getElementById('sw-prop');
    const was = box.value;
    const { escapeHtml } = this.ctx;
    const fields = this.sweepFields(document.getElementById('sw-object').value);
    box.innerHTML = fields.map(([k, label, unit]) => `<option value="${k}">${escapeHtml(label)}${unit ? ` (${escapeHtml(unit)})` : ''}</option>`).join('');
    if (fields.some((f) => f[0] === was)) box.value = was;
  }

  // The last sweep: a row and a point for each value.
  renderSweep() {
    const { escapeHtml } = this.ctx;
    const { sweeping, swept, comparing } = this.scenarios;
    this.sweepObjects();
    document.getElementById('sw-run').disabled = !!(comparing || sweeping);
    const box = document.getElementById('sw-list');
    if (sweeping) {
      box.innerHTML = '<p class="muted tl-empty">탐색 실행 중…</p>';
      return;
    }
    if (!swept) {
      box.innerHTML = '';
      return;
    }
    if (swept.error) {
      box.innerHTML = `<p class="sc-alarm">${escapeHtml(swept.error)}</p>`;
      return;
    }
    const scale = swept.scale || 1;
    const runs = (swept.results ?? []).map((r) => ({ ...r, shown: Math.round(r.value * scale * 1000) / 1000 }));
    const best = Math.max(0, ...runs.filter((r) => r.ok !== false).map((r) => r.per_hour));
    const unit = swept.unit ? ` (${escapeHtml(swept.unit)})` : '';
    const rows = runs
      .map((r) =>
        r.ok === false
          ? `<tr><td>${r.shown}</td><td colspan="5" class="sc-alarm">${escapeHtml(r.alarm || '')}</td></tr>`
          : `<tr class="${r.per_hour === best && runs.length > 1 ? 'best' : ''}"><td>${r.shown}</td>
             <td>${r.per_hour.toFixed(1)}${r.reps > 1 ? `<small> ±${r.per_hour_ci.toFixed(1)}</small>` : ''}</td>
             <td>${r.lead_mean != null ? `${r.lead_mean.toFixed(0)} s` : '—'}</td>
             <td>${r.bottleneck ? `${escapeHtml(r.bottleneck.name)} ${pct(r.bottleneck.busy)}` : '—'}</td>
             <td>${r.per_part != null ? `${Math.round(r.per_part).toLocaleString('ko-KR')}원` : '—'}</td>
             <td class="sc-alarm">${r.alarm ? escapeHtml(r.alarm) : ''}</td></tr>`,
      )
      .join('');
    box.innerHTML = `<h3 class="sw-title">${escapeHtml(swept.name)} · ${escapeHtml(swept.label)}${unit}</h3>
      <table class="sc-table"><thead><tr><th>${escapeHtml(swept.label)}${unit}</th><th>시간당</th><th>리드타임</th><th>병목</th><th>개당 원가</th><th>알람</th></tr></thead><tbody>${rows}</tbody></table>
      ${sweepChart(runs)}`;
  }

  renderScenarios() {
    const { escapeHtml } = this.ctx;
    const { list, compared, comparing, sweeping } = this.scenarios;
    this.renderSweep();
    const stats = document.getElementById('tl-stats');
    stats.textContent = comparing ? '비교 실행 중…' : sweeping ? '탐색 실행 중…' : `시나리오 ${list.length}개`;
    const run = document.getElementById('sc-run');
    run.disabled = comparing || sweeping;
    run.textContent = comparing ? '실행 중…' : '▶ 모두 실행';
    const box = document.getElementById('sc-list');
    if (!list.length) {
      box.innerHTML =
        '<p class="muted tl-empty">장비를 배치하고 프로그램을 만든 뒤 <b>현재 구성 저장</b>을 누르세요. CNC 대수나 그리퍼를 바꿔 다시 저장하고 <b>모두 실행</b>으로 비교합니다.</p>';
      return;
    }
    const byName = new Map(compared.map((r) => [r.name, r]));
    const best = Math.max(0, ...compared.map((r) => r.per_hour || 0));
    const rows = list
      .map((s, i) => {
        const r = byName.get(s.name);
        const cells = r
          ? r.ok === false
            ? `<td colspan="8"></td><td class="sc-alarm">${escapeHtml(r.alarm || '')}</td>`
            : `<td>${r.parts}</td>
               <td class="sc-rate"><span class="sc-bar-rate" style="width:${best ? Math.round((r.per_hour / best) * 60) : 0}px"></span>${r.per_hour.toFixed(1)}${r.reps > 1 ? `<small> ±${r.per_hour_ci.toFixed(1)}</small>` : ''}</td>
               <td title="${escapeHtml(r.machine_lanes.map((m) => `${m.name} ${pct(m.utilization)}`).join(', '))}">${pct(mean(r.machine_lanes, 'utilization'))}</td>
               <td>${r.robot_lanes.length ? pct(mean(r.robot_lanes, 'work')) : '—'}</td>
               <td>${r.lead_mean != null ? `${r.lead_mean.toFixed(0)} s${r.reps > 1 && r.lead_ci != null ? `<small> ±${r.lead_ci.toFixed(0)}</small>` : ''}` : '—'}</td>
               <td>${r.bottleneck ? `${escapeHtml(r.bottleneck.name)} ${pct(r.bottleneck.busy)}` : '—'}</td>
               <td>${r.investment ? `${Math.round(r.investment).toLocaleString('ko-KR')}만원` : '—'}</td>
               <td>${r.per_part != null ? `${Math.round(r.per_part).toLocaleString('ko-KR')}원` : '—'}</td>
               <td class="sc-alarm">${r.alarm ? escapeHtml(r.alarm) : ''}</td>`
          : '<td colspan="9" class="muted">아직 실행하지 않음</td>';
        const top = r && r.ok !== false && best > 0 && r.per_hour === best && compared.length > 1;
        return `<tr data-index="${i}" class="${top ? 'best' : ''}">
          <td>${escapeHtml(s.name)}</td><td>${s.machines}</td>${cells}
          <td class="sc-acts"><button data-sc="load" title="이 구성으로 되돌리기">불러오기</button><button data-sc="remove" title="삭제">✕</button></td>
        </tr>`;
      })
      .join('');
    box.innerHTML = `<table class="sc-table">
      <thead><tr><th>시나리오</th><th>장비</th><th>완성</th><th>시간당</th><th>장비 가동률</th><th>로봇 작업</th><th>리드타임</th><th>병목</th><th>투자비</th><th>개당 원가</th><th>알람</th><th></th></tr></thead>
      <tbody>${rows}</tbody></table>`;
  }

  // The `timeline` event.
  setTimeline(data) {
    this.data = data;
    if (!this.box.hidden) this.render();
  }

  async flowOp(action) {
    try {
      const answer = await post({ op: 'flow', action });
      if (!answer.ok) this.ctx.toast(answer.error || '요청을 처리하지 못했습니다');
    } catch (err) {
      this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
    }
  }

  // The `cell` event's process flow: shown when the cell has one.
  setFlow(cell) {
    const any = (cell.components ?? []).some((c) => c.flow);
    const f = cell.flow;
    this.flowState = f;
    this.components = cell.components ?? [];
    if (this.tab === 'flowchart' && !this.box.hidden) this.chart.render(this.components);
    document.getElementById('flow-bar').hidden = !any;
    if (!any || !f) return;
    document.getElementById('flow-run').textContent = f.on ? '■ 정지' : '▶ 실행';
    const lead = f.lead_mean != null ? ` · 리드타임 평균 ${f.lead_mean.toFixed(1)} s (최소 ${f.lead_min.toFixed(1)}, 최대 ${f.lead_max.toFixed(1)})` : '';
    const amr = f.carrying ? ` · 운반 ${f.carrying}건` : '';
    // The calendar's fields, as KYNEX has them, unless being typed in.
    const set = (id, v) => {
      const el = document.getElementById(id);
      if (document.activeElement !== el) el.value = v ? Math.round(v * 100) / 100 : '';
    };
    set('flow-work', f.settings?.work);
    set('flow-rest', f.settings?.rest);
    set('flow-warmup', (f.settings?.warmup ?? 0) / 60);
    set('cost-hours', f.settings?.hours);
    set('cost-years', f.settings?.years);
    set('cost-margin', f.settings?.margin);
    const now = f.resting ? ' · 휴식 중' : f.warming ? ' · 워밍업 중' : '';
    const by = (f.products ?? []).length > 1 ? ` · ${f.products.map((p) => `${p.product} ${p.sunk}`).join(' ')}` : '';
    const o = f.orders;
    const ord = o ? ` · 주문 ${o.total}건 중 ${o.done} 완료, 납기 준수 ${o.on_time}${o.late_mean != null ? `, 평균 지연 ${(o.late_mean / 60).toFixed(1)}분` : ''}` : '';
    const problem = f.problem ? ` · ⚠ ${f.problem}` : '';
    document.getElementById('flow-state').textContent = `배출 ${f.sunk}개${by} · 시간당 ${f.per_hour.toFixed(1)}개 · 공정 중 ${f.wip}개${amr}${lead}${ord}${now}${problem}`;
  }

  // The scenarios compared, as CSV.
  scenariosCsv() {
    const { list, compared } = this.scenarios;
    if (!compared?.length) return this.ctx.toast('먼저 시나리오를 실행하세요');
    const q = (v) => {
      const s = csvText(v);
      return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
    };
    const byName = new Map(compared.map((r) => [r.name, r]));
    const lines = [['시나리오', '장비', '완성', '시간당', '시간당 ±95%', '반복', '장비 가동률 (%)', '로봇 작업 (%)', '로봇 대기 (%)', '리드타임 (s)', '리드타임 ±95%', '병목', '병목 작업 (%)', '투자비 (만원)', '개당 원가 (원)', '알람'].map(q).join(',')];
    for (const s of list) {
      const r = byName.get(s.name);
      if (!r) continue;
      const ok = r.ok !== false;
      lines.push(
        [
          s.name,
          s.machines,
          ok ? r.parts : '',
          ok ? r.per_hour.toFixed(2) : '',
          ok && r.per_hour_ci != null ? r.per_hour_ci.toFixed(2) : '',
          ok ? (r.reps ?? 1) : '',
          ok ? (mean(r.machine_lanes, 'utilization') * 100).toFixed(1) : '',
          ok ? (mean(r.robot_lanes, 'work') * 100).toFixed(1) : '',
          ok ? (mean(r.robot_lanes, 'wait') * 100).toFixed(1) : '',
          ok && r.lead_mean != null ? r.lead_mean.toFixed(1) : '',
          ok && r.lead_ci != null ? r.lead_ci.toFixed(1) : '',
          ok && r.bottleneck ? r.bottleneck.name : '',
          ok && r.bottleneck ? (r.bottleneck.busy * 100).toFixed(1) : '',
          ok ? (r.investment ?? 0) : '',
          ok && r.per_part != null ? r.per_part.toFixed(1) : '',
          r.alarm || '',
        ]
          .map(q)
          .join(','),
      );
    }
    download(`kynex-시나리오-${stamp()}.csv`, `\ufeff${lines.join('\r\n')}\r\n`, 'text/csv;charset=utf-8');
  }

  // The statistics of the last run: figures, each one's time by state,
  // and parts made against time.
  renderStats() {
    const { escapeHtml } = this.ctx;
    const d = this.data;
    const body = document.getElementById('st-body');
    const stats = document.getElementById('tl-stats');
    if (!d || !d.lanes.length) {
      stats.textContent = '';
      body.innerHTML = '<p class="muted tl-empty">프로그램을 실행하면 장비별 상태와 생산 추이가 여기에 나옵니다.</p>';
      return;
    }
    const f = figures(d, ACTIVITIES);
    stats.textContent = `경과 ${clock(d.elapsed)} · 완성 ${f.parts}개 · 시간당 ${f.perHour.toFixed(1)}개`;
    body.innerHTML = `
      <div class="st-cards">${summary(f, this.flowState)
        .map(([k, v]) => `<div><span>${escapeHtml(k)}</span><b class="num">${escapeHtml(v)}</b></div>`)
        .join('')}</div>
      <div class="st-cols">
        <section><h3>장비별 상태</h3>${stateChart(f, ACTIVITIES, escapeHtml)}</section>
        <section><h3>누적 완성 수 <span class="muted">· 점선: 평균 속도</span></h3>${throughputChart(f)}</section>
      </div>
      <section><h3>작업·막힘·대기 <span class="muted">· 작업이 가장 많은 곳이 병목</span></h3>${laneTable(f, escapeHtml)}</section>
      ${costCards(this.flowState, escapeHtml)}
      ${seriesCharts(this.flowState)}
      ${oeeTable(this.flowState, escapeHtml)}`;
  }

  render() {
    if (this.tab === 'flowchart') return this.chart.render(this.components ?? []);
    if (this.tab === 'scenarios') return this.renderScenarios();
    if (this.tab === 'stats') return this.renderStats();
    const { escapeHtml } = this.ctx;
    const d = this.data;
    const stats = document.getElementById('tl-stats');
    const lanes = document.getElementById('tl-lanes');
    if (!d || !d.lanes.length) {
      stats.textContent = '';
      lanes.innerHTML = '<p class="muted tl-empty">프로그램을 실행하면 로봇과 장비가 한 일이 여기에 쌓입니다.</p>';
      return;
    }
    stats.textContent = `경과 ${clock(d.elapsed)} · 완성 ${d.parts}개 · 시간당 ${d.per_hour.toFixed(1)}개`;
    const span = Math.max(d.elapsed, 1e-9);
    lanes.innerHTML = d.lanes
      .map((l) => {
        const bars = l.spans
          // An alarm stops the run where it happens: it shows as a mark.
          .filter(([from, to, a]) => to > from || a === 'alarm')
          .map(([from, to, a]) => {
            const [label, color] = ACTIVITIES[a] || [a, '#555'];
            const mark = a === 'alarm' ? ';min-width:3px;transform:translateX(-3px)' : '';
            return `<span style="left:${((from / span) * 100).toFixed(3)}%;width:${(((to - from) / span) * 100).toFixed(3)}%;background:${color}${mark}" title="${escapeHtml(label)} ${(to - from).toFixed(1)} s"></span>`;
          })
          .join('');
        const s = l.shares;
        const share =
          l.group === 'machine'
            ? s.drive != null || s.turn != null || s.walk != null || s.work != null
              ? `가동률 <b>${pct((s.drive || 0) + (s.turn || 0) + (s.walk || 0) + (s.work || 0))}</b>`
              : `가동률 <b>${pct(s.cut)}</b>`
            : `작업 <b>${pct((s.move || 0) + (s.grip || 0))}</b> · 대기 ${pct((s.wait || 0) + (s.pause || 0))}`;
        return `<div class="tl-lane"><span class="tl-name" title="${escapeHtml(l.name)}">${escapeHtml(l.name)}</span><div class="tl-track">${bars}</div><span class="tl-share">${share}</span></div>`;
      })
      .join('');
  }
}
