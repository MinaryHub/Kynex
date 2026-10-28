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
import { tFor, tr } from './i18n.js';

const t = tFor('panels');

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
  move: [t('이동'), '#5aa9ff'],
  grip: [t('그리퍼'), '#ff8a3d'],
  wait: [t('신호 대기'), '#f2b53c'],
  pause: [t('시간 대기'), '#8a93a1'],
  alarm: [t('알람'), '#ff5f5f'],
  cut: [t('가공'), '#3fcf8e'],
  door: [t('문 개폐'), '#7d8fa6'],
  blocked: [t('언로드 대기'), '#e0a43a'],
  drive: [t('주행'), '#9b7bff'],
  walk: [t('보행'), '#c08bff'],
  down: [t('고장'), '#d9534f'],
  maint: [t('보전'), '#4f8fd9'],
  rest: [t('휴식'), '#556070'],
  charge: [t('충전'), '#d4b83a'],
  off: [t('근무 외'), '#3a3f48'],
  work: [t('작업'), '#2fbfa0'],
  idle: [t('쉼'), '#2f353f'],
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
    document.getElementById('tl-close').addEventListener('click', () => this.ctx.toggle(false));
    for (const b of this.box.querySelectorAll('[data-tl-tab]')) {
      b.addEventListener('click', () => this.showTab(b.dataset.tlTab));
    }
    document.getElementById('sc-save').addEventListener('click', () => {
      const name = document.getElementById('sc-name');
      this.send({ action: 'save', name: name.value }, () => {
        this.ctx.toast(t('시나리오를 저장했습니다'));
        name.value = '';
      });
    });
    document.getElementById('sc-name').addEventListener('keydown', (e) => {
      if (e.key === 'Enter') document.getElementById('sc-save').click();
      e.stopPropagation();
    });
    document.getElementById('sc-run').addEventListener('click', () => {
      // Nothing saved: said so, not a button that does nothing.
      if (!this.scenarios.list.length) return this.ctx.toast(t('먼저 현재 구성 저장으로 시나리오를 저장하세요'));
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
        this.send({ action: 'load', index }, () => this.ctx.toast(t('시나리오의 장비, 셀 역할, 프로그램을 불러왔습니다')));
      }
    });
    document.getElementById('st-csv').addEventListener('click', () => {
      if (!this.data?.lanes?.length) return this.ctx.toast(t('먼저 프로그램을 실행하세요'));
      download(`kynex-${t('통계')}-${stamp()}.csv`, csv(this.data, ACTIVITIES, this.flowState), 'text/csv;charset=utf-8');
    });
    document.getElementById('st-report').addEventListener('click', () => {
      if (!this.data?.lanes?.length) return this.ctx.toast(t('먼저 프로그램을 실행하세요'));
      const win = window.open('', '_blank');
      if (!win) return this.ctx.toast(t('새 창을 열 수 없습니다. 팝업 차단을 확인하세요'));
      win.document.write(reportHtml(this.data, ACTIVITIES, this.ctx.escapeHtml, t('KYNEX 통계 보고서'), this.flowState));
      win.document.close();
    });
    document.getElementById('sc-csv').addEventListener('click', () => this.scenariosCsv());
    // A sweep: one setting of one component at several values.
    this.components = [];
    document.getElementById('sw-object').addEventListener('change', () => this.sweepProps());
    document.getElementById('sw-run').addEventListener('click', () => {
      const object = document.getElementById('sw-object').value;
      const field = this.sweepFields(object).find((f) => f[0] === document.getElementById('sw-prop').value);
      if (!field) return this.ctx.toast(t('탐색할 컴포넌트와 설정을 고르세요'));
      const shown = document
        .getElementById('sw-values')
        .value.split(/[,\s]+/)
        .filter(Boolean)
        .map(Number);
      if (!shown.length || shown.some((v) => !Number.isFinite(v))) return this.ctx.toast(t('값을 쉼표로 나눠 넣으세요 (예: 20, 30, 40)'));
      if (shown.length > 12) return this.ctx.toast(t('값은 12개까지입니다'));
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
    // The process flow as it stands: run, stopped and reset by the run
    // badge over the view and the toolbar's 처음으로, not here.
    this.flowState = null;
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
          if (!a.ok) this.ctx.toast(tr(a.error) || t('설정을 바꾸지 못했습니다'));
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
    this.render();
  }

  async send(request, then) {
    try {
      const answer = await post({ op: 'scenario', ...request });
      if (!answer.ok) this.ctx.toast(tr(answer.error) || t('요청을 처리하지 못했습니다'));
      else then?.(answer);
    } catch (err) {
      this.ctx.toast(t('KYNEX에 요청하지 못했습니다: {error}', { error: err.message }));
    }
  }

  // The simulation's clock: its speed is shown on the run badge.
  setClock() {}

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
    box.innerHTML = list.map((c) => `<option value="${escapeHtml(c.object)}">${escapeHtml(tr(c.name))}</option>`).join('');
    if (list.some((c) => c.object === was)) box.value = was;
    this.sweepProps();
  }

  sweepProps() {
    const box = document.getElementById('sw-prop');
    const was = box.value;
    const { escapeHtml } = this.ctx;
    const fields = this.sweepFields(document.getElementById('sw-object').value);
    box.innerHTML = fields.map(([k, label, unit]) => `<option value="${k}">${escapeHtml(tr(label))}${unit ? ` (${escapeHtml(tr(unit))})` : ''}</option>`).join('');
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
      box.innerHTML = `<p class="muted tl-empty">${t('탐색 실행 중…')}</p>`;
      return;
    }
    if (!swept) {
      box.innerHTML = '';
      return;
    }
    if (swept.error) {
      box.innerHTML = `<p class="sc-alarm">${escapeHtml(tr(swept.error))}</p>`;
      return;
    }
    const scale = swept.scale || 1;
    const runs = (swept.results ?? []).map((r) => ({ ...r, shown: Math.round(r.value * scale * 1000) / 1000 }));
    const best = Math.max(0, ...runs.filter((r) => r.ok !== false).map((r) => r.per_hour));
    const unit = swept.unit ? ` (${escapeHtml(tr(swept.unit))})` : '';
    const rows = runs
      .map((r) =>
        r.ok === false
          ? `<tr><td>${r.shown}</td><td colspan="5" class="sc-alarm">${escapeHtml(tr(r.alarm || ''))}</td></tr>`
          : `<tr class="${r.per_hour === best && runs.length > 1 ? 'best' : ''}"><td>${r.shown}</td>
             <td>${r.per_hour.toFixed(1)}${r.reps > 1 ? `<small> ±${r.per_hour_ci.toFixed(1)}</small>` : ''}</td>
             <td>${r.lead_mean != null ? `${r.lead_mean.toFixed(0)} s` : '—'}</td>
             <td>${r.bottleneck ? `${escapeHtml(tr(r.bottleneck.name))} ${pct(r.bottleneck.busy)}` : '—'}</td>
             <td>${r.per_part != null ? t('{n}원', { n: Math.round(r.per_part).toLocaleString('ko-KR') }) : '—'}</td>
             <td class="sc-alarm">${r.alarm ? escapeHtml(tr(r.alarm)) : ''}</td></tr>`,
      )
      .join('');
    box.innerHTML = `<h3 class="sw-title">${escapeHtml(tr(swept.name))} · ${escapeHtml(tr(swept.label))}${unit}</h3>
      <table class="sc-table"><thead><tr><th>${escapeHtml(tr(swept.label))}${unit}</th><th>${t('시간당')}</th><th>${t('리드타임')}</th><th>${t('병목')}</th><th>${t('개당 원가')}</th><th>${t('알람')}</th></tr></thead><tbody>${rows}</tbody></table>
      ${sweepChart(runs)}`;
  }

  renderScenarios() {
    const { escapeHtml } = this.ctx;
    const { list, compared, comparing, sweeping } = this.scenarios;
    this.renderSweep();
    const stats = document.getElementById('tl-stats');
    stats.textContent = comparing ? t('비교 실행 중…') : sweeping ? t('탐색 실행 중…') : t('시나리오 {n}개', { n: list.length });
    const run = document.getElementById('sc-run');
    run.disabled = comparing || sweeping;
    run.textContent = comparing ? t('실행 중…') : t('▶ 모두 실행');
    const box = document.getElementById('sc-list');
    if (!list.length) {
      box.innerHTML =
        `<p class="muted tl-empty">${t('장비를 배치하고 프로그램을 만든 뒤 <b>현재 구성 저장</b>을 누르세요. CNC 대수나 그리퍼를 바꿔 다시 저장하고 <b>모두 실행</b>으로 비교합니다.')}</p>`;
      return;
    }
    const byName = new Map(compared.map((r) => [r.name, r]));
    const best = Math.max(0, ...compared.map((r) => r.per_hour || 0));
    const rows = list
      .map((s, i) => {
        const r = byName.get(s.name);
        const cells = r
          ? r.ok === false
            ? `<td colspan="8"></td><td class="sc-alarm">${escapeHtml(tr(r.alarm || ''))}</td>`
            : `<td>${r.parts}</td>
               <td class="sc-rate"><span class="sc-bar-rate" style="width:${best ? Math.round((r.per_hour / best) * 60) : 0}px"></span>${r.per_hour.toFixed(1)}${r.reps > 1 ? `<small> ±${r.per_hour_ci.toFixed(1)}</small>` : ''}</td>
               <td title="${escapeHtml(r.machine_lanes.map((m) => `${tr(m.name)} ${pct(m.utilization)}`).join(', '))}">${pct(mean(r.machine_lanes, 'utilization'))}</td>
               <td>${r.robot_lanes.length ? pct(mean(r.robot_lanes, 'work')) : '—'}</td>
               <td>${r.lead_mean != null ? `${r.lead_mean.toFixed(0)} s${r.reps > 1 && r.lead_ci != null ? `<small> ±${r.lead_ci.toFixed(0)}</small>` : ''}` : '—'}</td>
               <td>${r.bottleneck ? `${escapeHtml(tr(r.bottleneck.name))} ${pct(r.bottleneck.busy)}` : '—'}</td>
               <td>${r.investment ? t('{n}만원', { n: Math.round(r.investment).toLocaleString('ko-KR') }) : '—'}</td>
               <td>${r.per_part != null ? t('{n}원', { n: Math.round(r.per_part).toLocaleString('ko-KR') }) : '—'}</td>
               <td class="sc-alarm">${r.alarm ? escapeHtml(tr(r.alarm)) : ''}</td>`
          : `<td colspan="9" class="muted">${t('아직 실행하지 않음')}</td>`;
        const top = r && r.ok !== false && best > 0 && r.per_hour === best && compared.length > 1;
        return `<tr data-index="${i}" class="${top ? 'best' : ''}">
          <td>${escapeHtml(tr(s.name))}</td><td>${s.machines}</td>${cells}
          <td class="sc-acts"><button data-sc="load" title="${t('이 구성으로 되돌리기')}">${t('불러오기')}</button><button data-sc="remove" title="${t('삭제')}">✕</button></td>
        </tr>`;
      })
      .join('');
    box.innerHTML = `<table class="sc-table">
      <thead><tr><th>${t('시나리오')}</th><th>${t('장비')}</th><th>${t('완성')}</th><th>${t('시간당')}</th><th>${t('장비 가동률')}</th><th>${t('로봇 작업')}</th><th>${t('리드타임')}</th><th>${t('병목')}</th><th>${t('투자비')}</th><th>${t('개당 원가')}</th><th>${t('알람')}</th><th></th></tr></thead>
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
      if (!answer.ok) this.ctx.toast(tr(answer.error) || t('요청을 처리하지 못했습니다'));
    } catch (err) {
      this.ctx.toast(t('KYNEX에 요청하지 못했습니다: {error}', { error: err.message }));
    }
  }

  // The `cell` event's process flow: shown when the cell has one.
  setFlow(cell) {
    const any = (cell.components ?? []).some((c) => c.flow);
    const f = cell.flow;
    this.flowState = f;
    this.components = cell.components ?? [];
    document.getElementById('flow-bar').hidden = !any;
    if (!any || !f) return;
    const lead = f.lead_mean != null ? t(' · 리드타임 평균 {mean} s (최소 {min}, 최대 {max})', { mean: f.lead_mean.toFixed(1), min: f.lead_min.toFixed(1), max: f.lead_max.toFixed(1) }) : '';
    const amr = f.carrying ? t(' · 운반 {n}건', { n: f.carrying }) : '';
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
    const now = f.resting ? t(' · 휴식 중') : f.warming ? t(' · 워밍업 중') : '';
    const by = (f.products ?? []).length > 1 ? ` · ${f.products.map((p) => `${tr(p.product)} ${p.sunk}`).join(' ')}` : '';
    const o = f.orders;
    const late = o && o.late_mean != null ? t(', 평균 지연 {n}분', { n: (o.late_mean / 60).toFixed(1) }) : '';
    const ord = o ? t(' · 주문 {total}건 중 {done} 완료, 납기 준수 {onTime}', { total: o.total, done: o.done, onTime: o.on_time }) + late : '';
    const problem = f.problem ? ` · ⚠ ${tr(f.problem)}` : '';
    document.getElementById('flow-state').textContent =
      t('배출 {sunk}개', { sunk: f.sunk }) + by + t(' · 시간당 {rate}개 · 공정 중 {wip}개', { rate: f.per_hour.toFixed(1), wip: f.wip }) + `${amr}${lead}${ord}${now}${problem}`;
  }

  // The scenarios compared, as CSV.
  scenariosCsv() {
    const { list, compared } = this.scenarios;
    if (!compared?.length) return this.ctx.toast(t('먼저 시나리오를 실행하세요'));
    const q = (v) => {
      const s = csvText(v);
      return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
    };
    const byName = new Map(compared.map((r) => [r.name, r]));
    const lines = [[t('시나리오'), t('장비'), t('완성'), t('시간당'), t('시간당 ±95%'), t('반복'), t('장비 가동률 (%)'), t('로봇 작업 (%)'), t('로봇 대기 (%)'), t('리드타임 (s)'), t('리드타임 ±95%'), t('병목'), t('병목 작업 (%)'), t('투자비 (만원)'), t('개당 원가 (원)'), t('알람')].map(q).join(',')];
    for (const s of list) {
      const r = byName.get(s.name);
      if (!r) continue;
      const ok = r.ok !== false;
      lines.push(
        [
          tr(s.name),
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
          ok && r.bottleneck ? tr(r.bottleneck.name) : '',
          ok && r.bottleneck ? (r.bottleneck.busy * 100).toFixed(1) : '',
          ok ? (r.investment ?? 0) : '',
          ok && r.per_part != null ? r.per_part.toFixed(1) : '',
          tr(r.alarm || ''),
        ]
          .map(q)
          .join(','),
      );
    }
    download(`kynex-${t('시나리오')}-${stamp()}.csv`, `\ufeff${lines.join('\r\n')}\r\n`, 'text/csv;charset=utf-8');
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
      body.innerHTML = `<p class="muted tl-empty">${t('프로그램을 실행하면 장비별 상태와 생산 추이가 여기에 나옵니다.')}</p>`;
      return;
    }
    const f = figures(d, ACTIVITIES);
    stats.textContent = t('경과 {time} · 완성 {parts}개 · 시간당 {rate}개', { time: clock(d.elapsed), parts: f.parts, rate: f.perHour.toFixed(1) });
    body.innerHTML = `
      <div class="st-cards">${summary(f, this.flowState)
        .map(([k, v]) => `<div><span>${escapeHtml(k)}</span><b class="num">${escapeHtml(v)}</b></div>`)
        .join('')}</div>
      <div class="st-cols">
        <section><h3>${t('장비별 상태')}</h3>${stateChart(f, ACTIVITIES, escapeHtml)}</section>
        <section><h3>${t('누적 완성 수')} <span class="muted">· ${t('점선: 평균 속도')}</span></h3>${throughputChart(f)}</section>
      </div>
      <section><h3>${t('작업·막힘·대기')} <span class="muted">· ${t('작업이 가장 많은 곳이 병목')}</span></h3>${laneTable(f, escapeHtml)}</section>
      ${costCards(this.flowState, escapeHtml)}
      ${seriesCharts(this.flowState)}
      ${oeeTable(this.flowState, escapeHtml)}`;
  }

  render() {
    if (this.tab === 'scenarios') return this.renderScenarios();
    if (this.tab === 'stats') return this.renderStats();
    const { escapeHtml } = this.ctx;
    const d = this.data;
    const stats = document.getElementById('tl-stats');
    const lanes = document.getElementById('tl-lanes');
    if (!d || !d.lanes.length) {
      stats.textContent = '';
      lanes.innerHTML = `<p class="muted tl-empty">${t('프로그램을 실행하면 로봇과 장비가 한 일이 여기에 쌓입니다.')}</p>`;
      return;
    }
    stats.textContent = t('경과 {time} · 완성 {parts}개 · 시간당 {rate}개', { time: clock(d.elapsed), parts: d.parts, rate: d.per_hour.toFixed(1) });
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
              ? t('가동률 <b>{pct}</b>', { pct: pct((s.drive || 0) + (s.turn || 0) + (s.walk || 0) + (s.work || 0)) })
              : t('가동률 <b>{pct}</b>', { pct: pct(s.cut) })
            : t('작업 <b>{work}</b> · 대기 {wait}', { work: pct((s.move || 0) + (s.grip || 0)), wait: pct((s.wait || 0) + (s.pause || 0)) });
        return `<div class="tl-lane"><span class="tl-name" title="${escapeHtml(tr(l.name))}">${escapeHtml(tr(l.name))}</span><div class="tl-track">${bars}</div><span class="tl-share">${share}</span></div>`;
      })
      .join('');
  }
}
