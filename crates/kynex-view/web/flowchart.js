// The process flow as a chart: each piece of the flow — source, station,
// buffer, conveyor, sink — a box, in columns by how far down the flow it
// is, an arrow for each link. Links are made by dragging from a box's dot
// (●) onto another box, and undone with the ✕ on an arrow; a box clicked
// selects its equipment. A right click on a box or an arrow opens the
// same menu as in the 3D view.

const KIND = {
  source: ['소재 공급', '#f2a33c'],
  station: ['공정', '#5aa9ff'],
  buffer: ['버퍼', '#8fd18a'],
  conveyor: ['컨베이어', '#b9a4f5'],
  sink: ['배출', '#9aa3ad'],
};

const W = 156;
const H = 46;
const COL = 210;
const ROW = 64;
const PAD = 24;

async function post(request) {
  const response = await fetch('/robot', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

export class FlowChart {
  // box: where it is drawn; ctx: { escapeHtml, toast, select(uid) }
  constructor(box, ctx) {
    this.box = box;
    this.ctx = ctx;
    this.key = '';
    this.drag = null;
    this.nodes = [];
    box.addEventListener('pointerdown', (e) => this.down(e));
    box.addEventListener('click', (e) => this.click(e));
    box.addEventListener('contextmenu', (e) => this.menu(e));
  }

  // The flow's pieces of `components`, laid out in columns: each one
  // column after the furthest of those that send to it.
  layout(components) {
    const flow = components.filter((c) => c.flow && KIND[c.kind]);
    const ids = new Set(flow.map((c) => c.object));
    const next = new Map(flow.map((c) => [c.object, (c.flow.next ?? []).filter((n) => ids.has(n))]));
    const depth = new Map(flow.map((c) => [c.object, 0]));
    // Pushed on along each link, as often as there are pieces: a loop
    // goes no further than that.
    for (let pass = 0; pass < flow.length; pass++) {
      let moved = false;
      for (const c of flow) {
        for (const n of next.get(c.object)) {
          if (depth.get(n) < depth.get(c.object) + 1) {
            depth.set(n, depth.get(c.object) + 1);
            moved = true;
          }
        }
      }
      if (!moved) break;
    }
    // In a column, as they stand across the floor.
    const columns = new Map();
    for (const c of flow) {
      const d = depth.get(c.object);
      if (!columns.has(d)) columns.set(d, []);
      columns.get(d).push(c);
    }
    const at = (c) => c.flow?.at ?? [0, 0, 0];
    const nodes = [];
    for (const [d, list] of [...columns].sort((a, b) => a[0] - b[0])) {
      list.sort((a, b) => at(b)[1] - at(a)[1] || at(a)[0] - at(b)[0]);
      list.forEach((c, row) => nodes.push({ c, x: PAD + d * COL, y: PAD + row * ROW, next: next.get(c.object) }));
    }
    return nodes;
  }

  // Drawn anew when what it shows has changed, not while a link is dragged.
  render(components) {
    if (this.drag) return;
    const nodes = this.layout(components);
    const key = JSON.stringify(nodes.map((n) => [n.c.object, n.c.name, n.c.kind, n.next, n.x, n.y]));
    if (key === this.key) return;
    this.key = key;
    this.nodes = nodes;
    const esc = this.ctx.escapeHtml;
    if (!nodes.length) {
      this.box.innerHTML =
        '<p class="muted tl-empty">공정 흐름 역할이 있는 설비가 없습니다. 라이브러리의 <b>공정 흐름</b> 장비(소재 공급·공정 스테이션·버퍼·배출)를 놓거나, 설비에 셀 역할을 주세요.</p>';
      return;
    }
    const byId = new Map(nodes.map((n) => [n.c.object, n]));
    const width = Math.max(...nodes.map((n) => n.x)) + W + PAD;
    const height = Math.max(...nodes.map((n) => n.y)) + H + PAD;
    const edges = nodes
      .flatMap((n) =>
        n.next.map((to) => {
          const m = byId.get(to);
          if (!m) return '';
          const [x1, y1, x2, y2] = [n.x + W, n.y + H / 2, m.x, m.y + H / 2];
          const bend = Math.max(40, Math.abs(x2 - x1) / 2);
          const d = `M${x1},${y1} C${x1 + bend},${y1} ${x2 - bend},${y2} ${x2},${y2}`;
          // Its middle, for the ✕ that undoes it.
          const [mx, my] = [(x1 + x2) / 2, (y1 + y2) / 2];
          return `<g class="fc-edge" data-from="${esc(n.c.object)}" data-to="${esc(to)}"><path d="${d}" class="fc-line" marker-end="url(#fc-arrow)"/><path d="${d}" class="fc-hit"/>
            <g class="fc-cut" data-from="${esc(n.c.object)}" data-to="${esc(to)}" transform="translate(${mx},${my})"><title>연결 끊기</title><circle r="8"/><path d="M-3,-3 L3,3 M3,-3 L-3,3"/></g></g>`;
        }),
      )
      .join('');
    const boxes = nodes
      .map((n) => {
        const [label, colour] = KIND[n.c.kind];
        const f = n.c.flow;
        const state = n.c.kind === 'station' ? (f.down ? '고장' : f.working ? '작업 중' : f.out ? '막힘' : '대기') : n.c.kind === 'sink' ? `배출 ${f.made}` : '';
        const name = n.c.name.length > 14 ? `${n.c.name.slice(0, 13)}…` : n.c.name;
        const dot = n.c.kind === 'sink' ? '' : `<circle class="fc-dot" data-dot="${esc(n.c.object)}" cx="${n.x + W}" cy="${n.y + H / 2}" r="7"><title>끌어서 다음 공정 상자에 놓으면 연결</title></circle>`;
        return `<g class="fc-node" data-node="${esc(n.c.object)}">
          <title>${esc(n.c.name)} — 눌러서 선택</title>
          <rect x="${n.x}" y="${n.y}" width="${W}" height="${H}" rx="7" style="--kind:${colour}"/>
          <rect x="${n.x}" y="${n.y}" width="5" height="${H}" rx="2" fill="${colour}"/>
          <text x="${n.x + 14}" y="${n.y + 19}" class="fc-name">${esc(name)}</text>
          <text x="${n.x + 14}" y="${n.y + 35}" class="fc-kind">${label}${state ? ` · ${state}` : ''}</text>
          ${dot}</g>`;
      })
      .join('');
    this.box.innerHTML = `<p class="fc-hint muted">오른쪽 점(●)을 끌어 다음 공정 상자에 놓으면 연결 · 화살표의 ✕로 끊기 · 상자를 누르면 설비 선택 · 오른쪽 클릭: 연결 메뉴</p>
      <svg class="fc-svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
        <defs><marker id="fc-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0 L10,5 L0,10 z" class="fc-head"/></marker></defs>
        ${edges}${boxes}<path class="fc-drag" hidden/></svg>`;
  }

  // A drag begun on a box's dot: a line from it, following the pointer,
  // until it is let go on another box.
  down(e) {
    const dot = e.target.closest('[data-dot]');
    if (!dot) return;
    e.preventDefault();
    const svg = this.box.querySelector('svg');
    const line = svg.querySelector('.fc-drag');
    const from = this.nodes.find((n) => n.c.object === dot.dataset.dot);
    const point = (ev) => {
      const r = svg.getBoundingClientRect();
      return [ev.clientX - r.left, ev.clientY - r.top];
    };
    this.drag = { from };
    const move = (ev) => {
      const [x, y] = point(ev);
      const [x1, y1] = [from.x + W, from.y + H / 2];
      line.setAttribute('d', `M${x1},${y1} C${x1 + 50},${y1} ${x - 50},${y} ${x},${y}`);
      line.hidden = false;
      for (const g of svg.querySelectorAll('.fc-node.target')) g.classList.remove('target');
      document.elementFromPoint(ev.clientX, ev.clientY)?.closest('.fc-node')?.classList.add('target');
    };
    const up = async (ev) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      line.hidden = true;
      this.drag = null;
      this.key = '';
      const to = document.elementFromPoint(ev.clientX, ev.clientY)?.closest('[data-node]')?.dataset.node;
      if (!to || to === from.c.object) return;
      if (from.next.includes(to)) return this.ctx.toast('이미 이어져 있습니다');
      await this.link(from.c.object, [...from.next, to]);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }

  // A right click: on a box, its equipment's menu; on an arrow, the link's.
  menu(e) {
    if (!this.ctx.showMenu) return;
    const edge = e.target.closest('.fc-edge');
    const node = e.target.closest('[data-node]');
    if (!edge && !node) return;
    e.preventDefault();
    const entries = edge
      ? this.ctx.linkMenu?.({ from: edge.dataset.from, to: edge.dataset.to })
      : this.ctx.equipmentMenu?.(node.dataset.node);
    if (entries) this.ctx.showMenu(e.clientX, e.clientY, entries);
  }

  click(e) {
    const cut = e.target.closest('.fc-cut');
    if (cut) {
      const from = this.nodes.find((n) => n.c.object === cut.dataset.from);
      if (from) this.link(from.c.object, from.next.filter((n) => n !== cut.dataset.to));
      return;
    }
    if (e.target.closest('[data-dot]')) return;
    const node = e.target.closest('[data-node]');
    if (node) this.ctx.select(node.dataset.node);
  }

  async link(object, next) {
    try {
      const answer = await post({ op: 'flow_next', object, next });
      if (!answer.ok) this.ctx.toast(answer.error || '잇지 못했습니다');
    } catch (err) {
      this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
    }
  }
}
