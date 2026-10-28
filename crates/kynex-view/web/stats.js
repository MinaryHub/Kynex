// Statistics of a run, from the timeline KYNEX records: how each robot and
// machine spent its time, parts made over time and the time between them —
// as charts on the page, a CSV file for a spreadsheet, or a page to print
// or keep as a PDF.

import { tFor, tr, getLang } from './i18n.js';

const t = tFor('panels');

// What counts as at work: moving, gripping, machining, driving, turning,
// walking, working. Waiting, blocked and idle do not.
const BUSY = ['move', 'grip', 'cut', 'drive', 'turn', 'walk', 'work'];

// What a timeline says, worked out: each lane's seconds and share per
// activity and at work, the moments parts were made (a machine's cut that
// ended), and the bottleneck: what was at work the most.
export function figures(d, activities) {
  const elapsed = Math.max(d?.elapsed || 0, 0);
  const lanes = (d?.lanes || []).map((l) => {
    const totals = l.totals || {};
    const order = Object.keys(activities).filter((a) => a in totals);
    for (const a of Object.keys(totals)) if (!order.includes(a)) order.push(a);
    const shares = l.shares || {};
    const busy = BUSY.reduce((s, a) => s + (shares[a] || 0), 0);
    return { key: l.key, name: l.name, group: l.group, totals, shares, order, busy };
  });
  const bottleneck = lanes.reduce((best, l) => (l.busy > (best?.busy ?? 0) ? l : best), null);
  // A cut followed by something else is a part made, when it ended.
  const made = [];
  for (const l of d?.lanes || []) {
    l.spans.forEach(([, to, a], i) => {
      if (a === 'cut' && i + 1 < l.spans.length) made.push({ at: to, lane: l.name });
    });
  }
  made.sort((a, b) => a.at - b.at);
  const gaps = made.slice(1).map((m, i) => m.at - made[i].at);
  const sum = gaps.reduce((s, g) => s + g, 0);
  return {
    elapsed,
    parts: made.length,
    perHour: elapsed > 0 ? (made.length / elapsed) * 3600 : 0,
    lanes,
    made,
    bottleneck,
    gaps: gaps.length
      ? { mean: sum / gaps.length, min: Math.min(...gaps), max: Math.max(...gaps), count: gaps.length }
      : null,
  };
}

// A cell's text, never a formula: a name a spreadsheet would run as one
// (=, +, -, @ first) is kept as text with a leading '. Numbers stay numbers.
export function csvText(v) {
  const s = String(v ?? '');
  if (/^[=+\-@\t\r]/.test(s) && !/^-?\d+(\.\d+)?$/.test(s)) return `'${s}`;
  return s;
}

const pct = (x) => `${Math.round((x || 0) * 100)}%`;
const secs = (s) => (s >= 60 ? t('{m}분 {s}초', { m: Math.floor(s / 60), s: Math.round(s % 60) }) : t('{s}초', { s: s.toFixed(1) }));

// Each lane's time as one bar, its activities side by side in their colours.
export function stateChart(f, activities, escapeHtml) {
  return f.lanes
    .map((l) => {
      const parts = l.order
        .filter((a) => (l.shares[a] || 0) > 0)
        .map((a) => {
          const [label, color] = activities[a] || [a, '#555'];
          const w = (l.shares[a] * 100).toFixed(2);
          const text = l.shares[a] >= 0.08 ? pct(l.shares[a]) : '';
          return `<span style="width:${w}%;background:${color}" title="${escapeHtml(label)} ${pct(l.shares[a])} · ${secs(l.totals[a] || 0)}">${text}</span>`;
        })
        .join('');
      return `<div class="st-row"><span class="st-name" title="${escapeHtml(tr(l.name))}">${escapeHtml(tr(l.name))}</span><div class="st-bar">${parts}</div></div>`;
    })
    .join('');
}

// Parts made against time: a step for each, from the start to now.
export function throughputChart(f, { width = 560, height = 160 } = {}) {
  const pad = { l: 34, r: 10, t: 10, b: 22 };
  const w = width - pad.l - pad.r;
  const h = height - pad.t - pad.b;
  const T = Math.max(f.elapsed, 1e-9);
  const N = Math.max(f.parts, 1);
  const x = (t) => pad.l + (t / T) * w;
  const y = (n) => pad.t + h - (n / N) * h;
  let path = `M${x(0)},${y(0)}`;
  f.made.forEach((m, i) => {
    path += ` H${x(m.at).toFixed(1)} V${y(i + 1).toFixed(1)}`;
  });
  path += ` H${x(T).toFixed(1)}`;
  // The pace a whole run would keep: from the start to now, straight.
  const pace = f.parts ? `<line x1="${x(0)}" y1="${y(0)}" x2="${x(T)}" y2="${y(f.parts)}" class="st-pace"/>` : '';
  // Seconds for a short run, minutes for a longer one: six ticks or so.
  const short = T < 120;
  const unit = short ? 1 : 60;
  const span = T / unit;
  const step = (short ? [1, 2, 5, 10, 15, 30] : [1, 2, 5, 10, 15, 30, 60, 120, 240, 480]).find((s) => span / s <= 6) || 960;
  let ticks = '';
  for (let m = 0; m <= span + 1e-9; m += step) {
    ticks += `<line x1="${x(m * unit)}" y1="${pad.t + h}" x2="${x(m * unit)}" y2="${pad.t + h + 4}" class="st-axis"/><text x="${x(m * unit)}" y="${height - 6}" text-anchor="middle">${short ? t('{n}초', { n: m }) : t('{n}분', { n: m })}</text>`;
  }
  const ny = [0, Math.ceil(N / 2), N];
  for (const n of [...new Set(ny)]) {
    ticks += `<text x="${pad.l - 6}" y="${y(n) + 4}" text-anchor="end">${n}</text><line x1="${pad.l}" y1="${y(n)}" x2="${pad.l + w}" y2="${y(n)}" class="st-grid"/>`;
  }
  return `<svg class="st-chart" viewBox="0 0 ${width} ${height}" width="100%" role="img" aria-label="${t('시간에 따른 누적 완성 수')}">
    ${ticks}
    <line x1="${pad.l}" y1="${pad.t + h}" x2="${pad.l + w}" y2="${pad.t + h}" class="st-axis"/>
    ${pace}
    <path d="${path}" class="st-line"/>
  </svg>`;
}

// Whether a process flow ran: its figures say more than machines' cuts.
const flowed = (flow) => flow && (flow.sunk > 0 || flow.on || flow.wip > 0);

// The summary as figures to show: a process flow's, if one ran — parts
// out, an hour, in process, lead time — else the machines'; and the
// bottleneck.
export function summary(f, flow) {
  const n = (v) => t('{n}개', { n: v });
  const out = [[t('경과 시간'), secs(f.elapsed)]];
  if (flowed(flow)) {
    out.push(
      [t('배출'), n(flow.sunk)],
      [t('시간당'), n(flow.per_hour.toFixed(1))],
      [t('공정 중 (WIP)'), n(flow.wip)],
      [t('리드타임 평균'), flow.lead_mean != null ? secs(flow.lead_mean) : '—'],
      [t('리드타임 최소 / 최대'), flow.lead_min != null ? `${secs(flow.lead_min)} / ${secs(flow.lead_max)}` : '—'],
    );
  } else {
    out.push(
      [t('완성'), n(f.parts)],
      [t('시간당'), n(f.perHour.toFixed(1))],
      [t('완성 간격 (평균)'), f.gaps ? secs(f.gaps.mean) : '—'],
      [t('간격 최소 / 최대'), f.gaps ? `${secs(f.gaps.min)} / ${secs(f.gaps.max)}` : '—'],
    );
  }
  out.push([t('병목'), f.bottleneck ? `${tr(f.bottleneck.name)} (${pct(f.bottleneck.busy)})` : '—']);
  return out;
}

// One line over time: `points` [t, value], against `t` from 0 to its
// last, the value from 0 to its most.
function lineChart(points, { width = 520, height = 130, unit = '' } = {}) {
  if (points.length < 2) return '';
  const pad = { l: 34, r: 10, t: 8, b: 20 };
  const w = width - pad.l - pad.r;
  const h = height - pad.t - pad.b;
  const T = Math.max(points[points.length - 1][0], 1e-9);
  const V = Math.max(1e-9, ...points.map((p) => p[1]));
  const x = (t) => pad.l + (t / T) * w;
  const y = (v) => pad.t + h - (v / V) * h;
  const path = points.map((p, i) => `${i ? 'L' : 'M'}${x(p[0]).toFixed(1)},${y(p[1]).toFixed(1)}`).join(' ');
  const minutes = T / 60;
  const step = [1, 2, 5, 10, 15, 30, 60, 120, 240, 480].find((s) => minutes / s <= 6) || 960;
  let ticks = '';
  for (let m = 0; m <= minutes + 1e-9; m += step) {
    ticks += `<text x="${x(m * 60)}" y="${height - 5}" text-anchor="middle">${t('{n}분', { n: m })}</text>`;
  }
  ticks += `<text x="${pad.l - 6}" y="${y(V) + 4}" text-anchor="end">${Math.round(V)}${unit}</text><text x="${pad.l - 6}" y="${y(0) + 4}" text-anchor="end">0</text><line x1="${pad.l}" y1="${y(V)}" x2="${pad.l + w}" y2="${y(V)}" class="st-grid"/>`;
  return `<svg class="st-chart" viewBox="0 0 ${width} ${height}" width="100%">${ticks}<line x1="${pad.l}" y1="${y(0)}" x2="${pad.l + w}" y2="${y(0)}" class="st-axis"/><path d="${path}" class="st-line"/></svg>`;
}

// A process flow over time: its parts in process, and parts out an hour
// (over each stretch between samples, smoothed over a few).
export function seriesCharts(flow) {
  const s = flow?.series ?? [];
  if (s.length < 3) return '';
  const wip = s.map(([t, w]) => [t, w]);
  const rate = [];
  const k = Math.max(1, Math.round(s.length / 30));
  for (let i = k; i < s.length; i++) {
    const dt = s[i][0] - s[i - k][0];
    if (dt > 0) rate.push([s[i][0], ((s[i][2] - s[i - k][2]) / dt) * 3600]);
  }
  return `<div class="st-cols"><section><h3>${t('공정 중 (WIP) 추이')}</h3>${lineChart(wip, { unit: t('개') })}</section><section><h3>${t('시간당 배출 추이')}</h3>${lineChart(rate, { unit: '' })}</section></div>`;
}

// A sweep's parts an hour at each value tried, with its 95 % interval.
export function sweepChart(runs, { width = 520, height = 150 } = {}) {
  const ok = runs.filter((r) => r.ok !== false);
  if (ok.length < 2) return '';
  const pad = { l: 40, r: 12, t: 10, b: 22 };
  const w = width - pad.l - pad.r;
  const h = height - pad.t - pad.b;
  const top = Math.max(1e-9, ...ok.map((r) => r.per_hour + (r.per_hour_ci || 0)));
  const x = (i) => pad.l + (ok.length === 1 ? w / 2 : (i / (ok.length - 1)) * w);
  const y = (v) => pad.t + h - (v / top) * h;
  const path = ok.map((r, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)},${y(r.per_hour).toFixed(1)}`).join(' ');
  const marks = ok
    .map((r, i) => {
      const ci = r.per_hour_ci || 0;
      const bar = ci > 0 ? `<line x1="${x(i)}" y1="${y(r.per_hour + ci)}" x2="${x(i)}" y2="${y(Math.max(0, r.per_hour - ci))}" class="st-axis"/>` : '';
      return `${bar}<circle cx="${x(i)}" cy="${y(r.per_hour)}" r="3" class="st-dot"/><text x="${x(i)}" y="${height - 6}" text-anchor="middle">${r.shown}</text>`;
    })
    .join('');
  return `<svg class="st-chart" viewBox="0 0 ${width} ${height}" width="100%"><text x="${pad.l - 6}" y="${y(top) + 4}" text-anchor="end">${Math.round(top)}</text><text x="${pad.l - 6}" y="${y(0) + 4}" text-anchor="end">0</text><line x1="${pad.l}" y1="${y(0)}" x2="${pad.l + w}" y2="${y(0)}" class="st-axis"/><path d="${path}" class="st-line"/>${marks}</svg>`;
}

// What the line costs: invested, an hour (write-off and wages), a part, and
// the years to pay it back.
export function costCards(flow, escapeHtml) {
  const c = flow?.cost;
  if (!c || (!c.investment && !c.wages)) return '';
  const won = (x) => t('{n}원', { n: (Math.round(x) || 0).toLocaleString('ko-KR') });
  const cards = [
    [t('투자비'), t('{n}만원', { n: Math.round(c.investment).toLocaleString('ko-KR') })],
    [t('시간당 비용'), t('{total} (감가 {writeOff} + 인건비 {wages})', { total: won(c.per_hour), writeOff: won(c.write_off), wages: won(c.wages) })],
    [t('개당 원가'), c.per_part != null ? won(c.per_part) : '—'],
    [t('회수 기간'), c.payback != null ? t('{n}년', { n: c.payback.toFixed(1) }) : c.margin ? t('벌어 오는 돈이 인건비보다 적음') : t('부품당 이익을 넣으면')],
  ];
  return `<section><h3>${t('비용')} <span class="muted">· ${t('연간 {hours}시간, {years}년 감가', { hours: c.hours, years: c.years })}</span></h3><div class="st-cards">${cards
    .map(([k, v]) => `<div><span>${escapeHtml(k)}</span><b class="num">${escapeHtml(v)}</b></div>`)
    .join('')}</div></section>`;
}

// Each station's OEE — availability × performance × quality — and its
// parts good and scrapped, when a process flow ran.
export function oeeTable(flow, escapeHtml) {
  const rows = (flow?.stations ?? []).filter((s) => s.good + s.bad > 0);
  if (!rows.length) return '';
  return `<section><h3>${t('스테이션 OEE')} <span class="muted">· ${t('가용성 × 성능 × 품질')}</span></h3><table class="st-table"><thead><tr><th>${t('스테이션')}</th><th>OEE</th><th>${t('가용성')}</th><th>${t('성능')}</th><th>${t('품질')}</th><th>${t('양품')}</th><th>${t('불량')}</th></tr></thead><tbody>${rows
    .map(
      (s) =>
        `<tr><td>${escapeHtml(tr(s.name))}</td><td><b>${pct(s.oee)}</b></td><td>${pct(s.availability)}</td><td>${pct(s.performance)}</td><td>${pct(s.quality)}</td><td>${s.good}</td><td>${s.bad}</td></tr>`,
    )
    .join('')}</tbody></table></section>`;
}

// Each lane's shares: at work, blocked, waiting, idle — the table.
export function laneTable(f, escapeHtml) {
  const row = (l) => {
    const s = l.shares;
    const wait = (s.wait || 0) + (s.pause || 0);
    const top = f.bottleneck && l.key === f.bottleneck.key;
    return `<tr${top ? ' class="st-top"' : ''}><td>${escapeHtml(tr(l.name))}${top ? ` <span class="st-tag">${t('병목')}</span>` : ''}</td><td>${pct(l.busy)}</td><td>${pct(s.blocked)}</td><td>${pct(wait)}</td><td>${pct(s.idle)}</td></tr>`;
  };
  return `<table class="st-table"><thead><tr><th>${t('장비·사람')}</th><th>${t('작업')}</th><th>${t('막힘')}</th><th>${t('대기')}</th><th>${t('쉼')}</th></tr></thead><tbody>${[...f.lanes]
    .sort((a, b) => b.busy - a.busy)
    .map(row)
    .join('')}</tbody></table>`;
}

// The whole of it as CSV: the summary, each lane's seconds and share per
// activity, the parts made, and every span — for a spreadsheet.
export function csv(d, activities, flow) {
  const f = figures(d, activities);
  const q = (v) => {
    const s = csvText(v);
    return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  const row = (...cells) => cells.map(q).join(',');
  const label = (a) => activities[a]?.[0] ?? a;
  const used = [...new Set(f.lanes.flatMap((l) => l.order))];
  const lines = [
    row(t('KYNEX 통계')),
    row(t('경과 시간 (s)'), f.elapsed.toFixed(2)),
    row(t('완성 (개)'), f.parts),
    row(t('시간당 (개)'), f.perHour.toFixed(2)),
    row(t('완성 간격 평균 (s)'), f.gaps ? f.gaps.mean.toFixed(2) : ''),
    row(t('완성 간격 최소 (s)'), f.gaps ? f.gaps.min.toFixed(2) : ''),
    row(t('완성 간격 최대 (s)'), f.gaps ? f.gaps.max.toFixed(2) : ''),
    row(t('병목'), f.bottleneck ? tr(f.bottleneck.name) : '', f.bottleneck ? (f.bottleneck.busy * 100).toFixed(1) : ''),
    ...(flowed(flow)
      ? [
          row(t('공정 흐름 배출 (개)'), flow.sunk),
          row(t('공정 흐름 시간당 (개)'), flow.per_hour.toFixed(2)),
          row(t('공정 중 (개)'), flow.wip),
          row(t('리드타임 평균 (s)'), flow.lead_mean != null ? flow.lead_mean.toFixed(2) : ''),
          row(t('리드타임 최소 (s)'), flow.lead_min != null ? flow.lead_min.toFixed(2) : ''),
          row(t('리드타임 최대 (s)'), flow.lead_max != null ? flow.lead_max.toFixed(2) : ''),
          row(t('불량 (개)'), flow.scrapped ?? 0),
          ...(flow.cost && (flow.cost.investment || flow.cost.wages)
            ? [
                row(t('투자비 (만원)'), flow.cost.investment),
                row(t('시간당 비용 (원)'), flow.cost.per_hour.toFixed(0)),
                row(t('개당 원가 (원)'), flow.cost.per_part != null ? flow.cost.per_part.toFixed(1) : ''),
                row(t('회수 기간 (년)'), flow.cost.payback != null ? flow.cost.payback.toFixed(2) : ''),
              ]
            : []),
          ...(flow.orders
            ? [
                row(t('주문 (건)'), flow.orders.total),
                row(t('주문 완료 (건)'), flow.orders.done),
                row(t('납기 준수 (건)'), flow.orders.on_time),
                row(t('평균 지연 (s)'), flow.orders.late_mean != null ? flow.orders.late_mean.toFixed(1) : ''),
              ]
            : []),
          '',
          row(t('스테이션'), 'OEE (%)', t('가용성 (%)'), t('성능 (%)'), t('품질 (%)'), t('양품'), t('불량')),
          ...(flow.stations ?? []).map((s) =>
            row(tr(s.name), (s.oee * 100).toFixed(1), (s.availability * 100).toFixed(1), (s.performance * 100).toFixed(1), (s.quality * 100).toFixed(1), s.good, s.bad),
          ),
        ]
      : []),
    '',
    row(t('장비'), t('구분'), t('작업 (%)'), ...used.flatMap((a) => [`${label(a)} (s)`, `${label(a)} (%)`])),
    ...f.lanes.map((l) =>
      row(
        tr(l.name),
        l.group === 'machine' ? t('장비') : t('로봇'),
        (l.busy * 100).toFixed(1),
        ...used.flatMap((a) => [(l.totals[a] || 0).toFixed(2), ((l.shares[a] || 0) * 100).toFixed(1)]),
      ),
    ),
    '',
    row(t('완성 시각 (s)'), t('장비')),
    ...f.made.map((m) => row(m.at.toFixed(2), tr(m.lane))),
    '',
    row(t('장비'), t('시작 (s)'), t('끝 (s)'), t('시간 (s)'), t('상태')),
    ...(d?.lanes || []).flatMap((l) =>
      l.spans.map(([from, to, a]) => row(tr(l.name), from.toFixed(2), to.toFixed(2), (to - from).toFixed(2), label(a))),
    ),
  ];
  // With its byte order mark, so a spreadsheet reads the Korean as such.
  return `﻿${lines.join('\r\n')}\r\n`;
}

// A page of it to print, or keep as a PDF.
export function reportHtml(d, activities, escapeHtml, title, flow) {
  const f = figures(d, activities);
  const legend = Object.entries(activities)
    .filter(([a]) => f.lanes.some((l) => (l.shares[a] || 0) > 0))
    .map(([, [label, color]]) => `<span><i style="background:${color}"></i>${escapeHtml(label)}</span>`)
    .join('');
  const used = [...new Set(f.lanes.flatMap((l) => l.order))];
  const rows = f.lanes
    .map(
      (l) =>
        `<tr><td>${escapeHtml(tr(l.name))}</td>${used.map((a) => `<td>${pct(l.shares[a])}<small> ${secs(l.totals[a] || 0)}</small></td>`).join('')}</tr>`,
    )
    .join('');
  const en = getLang() === 'en';
  const when = new Date().toLocaleString(en ? 'en-US' : 'ko-KR');
  return `<!doctype html><html lang="${en ? 'en' : 'ko'}"><head><meta charset="utf-8"><title>${escapeHtml(title)}</title>
<style>
  body { font: 13px/1.5 system-ui, 'Malgun Gothic', sans-serif; color: #1b1f24; margin: 24px; }
  h1 { font-size: 20px; margin: 0 0 2px; } h2 { font-size: 15px; margin: 22px 0 8px; }
  .muted { color: #6b7380; }
  .cards { display: flex; flex-wrap: wrap; gap: 10px; }
  .cards div { border: 1px solid #d8dde4; border-radius: 6px; padding: 8px 12px; min-width: 120px; }
  .cards b { display: block; font-size: 17px; }
  .st-row { display: flex; align-items: center; gap: 8px; margin: 4px 0; }
  .st-name { width: 160px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st-bar { flex: 1; display: flex; height: 18px; border-radius: 3px; overflow: hidden; background: #eef1f4; }
  .st-bar span { display: flex; align-items: center; justify-content: center; color: #fff; font-size: 10.5px; -webkit-print-color-adjust: exact; print-color-adjust: exact; }
  .legend { display: flex; flex-wrap: wrap; gap: 12px; margin-top: 6px; font-size: 12px; }
  .legend i { display: inline-block; width: 10px; height: 10px; border-radius: 2px; margin-right: 4px; -webkit-print-color-adjust: exact; print-color-adjust: exact; }
  .st-chart text { font-size: 10px; fill: #6b7380; }
  .st-line { fill: none; stroke: #2f7de1; stroke-width: 2; }
  .st-pace { stroke: #9aa3ad; stroke-dasharray: 4 4; }
  .st-axis { stroke: #9aa3ad; } .st-grid { stroke: #e6e9ed; }
  table { border-collapse: collapse; width: 100%; } th, td { border-bottom: 1px solid #e3e7ec; padding: 4px 6px; text-align: left; }
  td small { color: #6b7380; }
  .st-top td { font-weight: 600; }
  .st-tag { font-size: 11px; color: #b4441f; border: 1px solid #b4441f; border-radius: 3px; padding: 0 4px; }
  .print { position: fixed; top: 16px; right: 24px; padding: 6px 14px; }
  @media print { .print { display: none; } body { margin: 10mm; } h2 { break-after: avoid; } tr { break-inside: avoid; } }
</style></head><body>
<button class="print" onclick="print()">${t('인쇄 / PDF로 저장')}</button>
<h1>${escapeHtml(title)}</h1>
<p class="muted">${t('KYNEX 시뮬레이션 통계')} · ${escapeHtml(when)}</p>
<div class="cards">${summary(f, flow)
    .map(([k, v]) => `<div><span class="muted">${escapeHtml(k)}</span><b>${escapeHtml(v)}</b></div>`)
    .join('')}</div>
<h2>${t('장비별 상태')}</h2>
${stateChart(f, activities, escapeHtml)}
<div class="legend">${legend}</div>
<h2>${t('누적 완성 수')}</h2>
${throughputChart(f, { width: 680, height: 200 })}
<p class="muted">${t('점선: 처음부터 지금까지의 평균 속도')}</p>
<h2>${t('작업·막힘·대기')}</h2>
${laneTable(f, escapeHtml)}
${oeeTable(flow, escapeHtml).replace('<section><h3>', '<h2>').replace('</h3>', '</h2>').replace('</section>', '')}
<h2>${t('장비별 시간')}</h2>
<table><thead><tr><th>${t('장비')}</th>${used.map((a) => `<th>${escapeHtml(activities[a]?.[0] ?? a)}</th>`).join('')}</tr></thead><tbody>${rows}</tbody></table>
</body></html>`;
}
