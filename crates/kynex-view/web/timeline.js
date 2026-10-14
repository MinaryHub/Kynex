// The timeline: what the robot and each machine did while the program ran,
// as a Gantt chart over the run, with how much of it each machine was
// cutting, how much the robot worked and waited, and the parts made an
// hour. KYNEX records it (the `timeline` event); this draws it.
//
// Beside it, scenarios: the cell saved as it stands, several times over —
// one CNC machine, two, another gripper — run for hours away from the view
// and compared (the `scenarios` event).

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
    document.getElementById('sc-run').addEventListener('click', () =>
      this.send({ action: 'run', seconds: Number(document.getElementById('sc-hours').value) * 3600 }),
    );
    document.getElementById('sc-list').addEventListener('click', (e) => {
      const b = e.target.closest('[data-sc]');
      if (!b) return;
      const index = Number(b.closest('[data-index]').dataset.index);
      if (b.dataset.sc === 'remove') this.send({ action: 'remove', index });
      if (b.dataset.sc === 'load') {
        this.send({ action: 'load', index }, () => this.ctx.toast('시나리오의 장비, 셀 역할, 프로그램을 불러왔습니다'));
      }
    });
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

  // The `scenarios` event.
  setScenarios(data) {
    this.scenarios = data;
    if (!this.box.hidden) this.render();
  }

  renderScenarios() {
    const { escapeHtml } = this.ctx;
    const { list, compared, comparing } = this.scenarios;
    const stats = document.getElementById('tl-stats');
    stats.textContent = comparing ? '비교 실행 중…' : `시나리오 ${list.length}개`;
    document.getElementById('sc-run').disabled = comparing || !list.length;
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
            ? `<td colspan="5"></td><td class="sc-alarm">${escapeHtml(r.alarm || '')}</td>`
            : `<td>${r.parts}</td>
               <td class="sc-rate"><span class="sc-bar-rate" style="width:${best ? Math.round((r.per_hour / best) * 60) : 0}px"></span>${r.per_hour.toFixed(1)}</td>
               <td title="${escapeHtml(r.machine_lanes.map((m) => `${m.name} ${pct(m.utilization)}`).join(', '))}">${pct(mean(r.machine_lanes, 'utilization'))}</td>
               <td>${pct(mean(r.robot_lanes, 'work'))}</td>
               <td>${pct(mean(r.robot_lanes, 'wait'))}</td>
               <td class="sc-alarm">${r.alarm ? escapeHtml(r.alarm) : ''}</td>`
          : '<td colspan="6" class="muted">아직 실행하지 않음</td>';
        const top = r && r.ok !== false && best > 0 && r.per_hour === best && compared.length > 1;
        return `<tr data-index="${i}" class="${top ? 'best' : ''}">
          <td>${escapeHtml(s.name)}</td><td>${s.machines}</td>${cells}
          <td class="sc-acts"><button data-sc="load" title="이 구성으로 되돌리기">불러오기</button><button data-sc="remove" title="삭제">✕</button></td>
        </tr>`;
      })
      .join('');
    box.innerHTML = `<table class="sc-table">
      <thead><tr><th>시나리오</th><th>CNC</th><th>완성</th><th>시간당</th><th>CNC 가동률</th><th>로봇 작업</th><th>로봇 대기</th><th>알람</th><th></th></tr></thead>
      <tbody>${rows}</tbody></table>`;
  }

  // The `timeline` event.
  setTimeline(data) {
    this.data = data;
    if (!this.box.hidden) this.render();
  }

  render() {
    if (this.tab === 'scenarios') return this.renderScenarios();
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
            ? `가동률 <b>${pct(s.cut)}</b>`
            : `작업 <b>${pct((s.move || 0) + (s.grip || 0))}</b> · 대기 ${pct((s.wait || 0) + (s.pause || 0))}`;
        return `<div class="tl-lane"><span class="tl-name" title="${escapeHtml(l.name)}">${escapeHtml(l.name)}</span><div class="tl-track">${bars}</div><span class="tl-share">${share}</span></div>`;
      })
      .join('');
  }
}
