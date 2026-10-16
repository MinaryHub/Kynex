// The PLC link: where the PLC is, connecting and letting go, and how the
// link is going (the `plc` event). The cell's machines are wired to its
// coils in the inspector (cell.js).

async function post(request) {
  const response = await fetch('/robot', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

export class PlcPanel {
  // ctx: { toast, escapeHtml, robots() }
  constructor(ctx) {
    this.ctx = ctx;
    this.panel = document.getElementById('plc-panel');
    this.protocol = document.getElementById('plc-protocol');
    this.path = document.getElementById('plc-path');
    this.security = document.getElementById('plc-security');
    this.user = document.getElementById('plc-user');
    this.password = document.getElementById('plc-password');
    this.host = document.getElementById('plc-host');
    this.port = document.getElementById('plc-port');
    this.unit = document.getElementById('plc-unit');
    this.rack = document.getElementById('plc-rack');
    this.slot = document.getElementById('plc-slot');
    this.button = document.getElementById('plc-connect');
    this.status = document.getElementById('plc-status');
    this.dot = document.querySelector('.toolbar .plc-dot');
    this.state = null;
    this.button.addEventListener('click', () => this.send(!this.state?.settings?.on));
    // The server certificate trusted forgotten: the next one met is.
    document.getElementById('plc-forget').addEventListener('click', () => {
      if (confirm('믿던 서버 인증서를 잊을까요? 다음에 연결하는 서버의 인증서를 믿게 됩니다.')) this.send(this.state?.settings?.on ?? false, { trust: '' });
    });
    // OPC UA's usual port in place of Modbus's, as the protocol changes.
    this.protocol.addEventListener('change', () => {
      const usual = { modbus: '502', opcua: '4840', s7: '102', xgt: '2004' };
      if (Object.values(usual).includes(this.port.value)) this.port.value = usual[this.protocol.value];
      this.showFields();
    });
    this.showFields();
    this.cell = null;
    this.draft = null;
    this.wireSignals();
  }

  // The fields the protocol has.
  showFields() {
    for (const label of this.panel.querySelectorAll('[data-for]')) {
      label.hidden = label.dataset.for !== this.protocol.value;
    }
  }

  toggle() {
    this.panel.hidden = !this.panel.hidden;
  }

  async send(on, more = {}) {
    const port = Number(this.port.value);
    const unit = Number(this.unit.value);
    if (!(Number.isInteger(port) && port >= 1 && port <= 65535)) return this.ctx.toast('포트는 1–65535입니다');
    if (!(Number.isInteger(unit) && unit >= 0 && unit <= 255)) return this.ctx.toast('유닛은 0–255입니다');
    try {
      const answer = await post({
        op: 'plc_link',
        protocol: this.protocol.value,
        host: this.host.value.trim(),
        port,
        unit,
        rack: Number(this.rack.value) || 0,
        slot: Number(this.slot.value) || 0,
        path: this.path.value.trim(),
        security: this.security.value,
        user: this.user.value.trim(),
        // Left empty, the password KYNEX holds for this user stays.
        password: this.password.value,
        on,
        ...more,
      });
      this.password.value = '';
      if (!answer.ok) this.ctx.toast(answer.error || 'PLC 설정을 바꾸지 못했습니다');
    } catch (err) {
      this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
    }
  }

  // The `plc` event.
  setState(state) {
    this.state = state;
    const s = state.settings;
    // The fields follow KYNEX's, except the one being typed in.
    for (const [input, v] of [
      [this.protocol, s.protocol ?? 'modbus'],
      [this.host, s.host],
      [this.port, s.port],
      [this.unit, s.unit],
      [this.rack, s.rack ?? 0],
      [this.slot, s.slot ?? 1],
      [this.path, s.path ?? ''],
      [this.security, s.security ?? 'none'],
      [this.user, s.user ?? ''],
    ]) {
      if (document.activeElement !== input) input.value = v;
    }
    this.showFields();
    this.button.textContent = s.on ? '끊기' : '연결';
    let text = '연결 안 됨';
    let cls = '';
    if (s.on && state.connected) {
      text = `연결됨 · 교환 ${state.exchanges}회`;
      cls = 'ok';
    } else if (s.on) {
      text = state.error ? `연결 중… ${state.error}` : '연결 중…';
      cls = 'bad';
    }
    if (state.problems?.length) text += ` · ${state.problems.join(', ')}`;
    if (state.warning) text += ` · ${state.warning}`;
    if (s.on && state.drops) text += ` · 끊김 ${state.drops}회 (${state.dropped})`;
    this.status.textContent = text;
    this.status.className = `plc-status ${cls}`;
    this.password.placeholder = state.has_password ? '기억함 (바꿀 때만 입력)' : '';
    document.getElementById('plc-trust').textContent = s.trust || (state.server_thumbprint ?? '아직 없음 (처음 연결할 때 믿습니다)');
    document.getElementById('plc-client').textContent = state.client_thumbprint ?? '보안 연결 때 만듭니다';
    document.getElementById('plc-pki').textContent = state.pki ? `· ${state.pki}` : '';
    this.dot.hidden = !s.on;
    this.dot.classList.toggle('ok', s.on && state.connected);
  }

  // --- the signal table ---

  // The `cell` event: the signals as KYNEX has them, and their values.
  setCell(cell) {
    this.cell = cell;
    if (!this.draft) this.drawSignals(cell.signals ?? []);
    else this.drawValues();
  }

  // What a signal can be tied to, by its direction: [value, label].
  links(input) {
    const robots = this.ctx.robots().map((r) => [r.root, r.name]);
    const machines = (this.cell?.components ?? []).filter((c) => c.kind === 'cnc' || c.kind === 'molder');
    return input
      ? [
          ['', '없음 (프로그램이 대기)'],
          ...robots.map(([u, n]) => [`start:${u}`, `${n} 프로그램 시작`]),
          ...robots.map(([u, n]) => [`stop:${u}`, `${n} 프로그램 정지`]),
        ]
      : [
          ['', '없음 (프로그램이 씀)'],
          ...robots.map(([u, n]) => [`running:${u}`, `${n} 실행 중`]),
          ...robots.map(([u, n]) => [`done:${u}`, `${n} 끝남`]),
          ...machines.map((c) => [`cycles:${c.object}`, `${c.name} 가공 횟수`]),
        ];
  }

  drawSignals(signals) {
    const { escapeHtml } = this.ctx;
    const options = (list, value) =>
      list.map(([v, label]) => `<option value="${escapeHtml(v)}"${v === value ? ' selected' : ''}>${escapeHtml(label)}</option>`).join('');
    const head = '<div class="plc-row head"><span>이름</span><span>방향</span><span>형식</span><span>PLC 주소</span><span>연결</span><span>값</span><span></span></div>';
    const rows = signals.map((s, i) => {
      const links = this.links(s.input);
      if (!links.some(([v]) => v === s.link)) links.push([s.link, `${s.link} (없음)`]);
      return `<div class="plc-row" data-i="${i}">
        <input data-k="name" value="${escapeHtml(s.name)}" maxlength="40" spellcheck="false">
        <select data-k="input">${options([['1', 'PLC → KYNEX'], ['0', 'KYNEX → PLC']], s.input ? '1' : '0')}</select>
        <select data-k="number">${options([['0', '비트'], ['1', '숫자']], s.number ? '1' : '0')}</select>
        <input data-k="address" value="${escapeHtml(s.address)}" placeholder="코일·레지스터 번호 또는 노드 ID" spellcheck="false">
        <select data-k="link">${options(links, s.link)}</select>
        <span class="plc-value" data-value="${escapeHtml(s.name)}">${s.value ?? ''}</span>
        <button class="icon-mini" data-del="${i}" title="이 신호 지우기">✕</button>
      </div>`;
    });
    this.table.innerHTML = signals.length ? head + rows.join('') : '<span class="muted">신호가 없습니다</span>';
  }

  // Values as KYNEX has them, into the rows being edited.
  drawValues() {
    for (const s of this.cell?.signals ?? []) {
      const cell = [...this.table.querySelectorAll('[data-value]')].find((e) => e.dataset.value === s.name);
      if (cell) cell.textContent = s.value ?? '';
    }
  }

  // The table as it is on the page.
  readSignals() {
    return [...this.table.querySelectorAll('.plc-row[data-i]')].map((row) => {
      const v = (k) => row.querySelector(`[data-k="${k}"]`).value;
      return { name: v('name').trim(), input: v('input') === '1', number: v('number') === '1', address: v('address').trim(), link: v('link') };
    });
  }

  // The table edited: kept as a draft until applied. Redrawn only when its
  // rows or their link choices change, so a field being typed in stays.
  edited(signals, redraw) {
    this.draft = signals;
    if (redraw) this.drawSignals(signals);
    this.apply.disabled = false;
  }

  async applySignals() {
    try {
      const answer = await post({ op: 'signals', signals: this.readSignals() });
      if (!answer.ok) return this.ctx.toast(answer.error || '신호를 바꾸지 못했습니다');
      this.draft = null;
      this.apply.disabled = true;
      this.ctx.toast('PLC 신호를 적용했습니다');
    } catch (err) {
      this.ctx.toast(`KYNEX에 요청하지 못했습니다: ${err.message}`);
    }
  }

  wireSignals() {
    this.table = document.getElementById('plc-signals');
    this.apply = document.getElementById('plc-signal-apply');
    document.getElementById('plc-signal-add').addEventListener('click', () => {
      const signals = this.draft ?? this.readSignals();
      let n = signals.length + 1;
      while (signals.some((s) => s.name === `신호${n}`)) n += 1;
      this.edited([...signals, { name: `신호${n}`, input: true, number: false, address: '', link: '' }], true);
    });
    this.apply.addEventListener('click', () => this.applySignals());
    this.table.addEventListener('click', (e) => {
      const del = e.target.closest('[data-del]');
      if (!del) return;
      const signals = this.readSignals();
      signals.splice(Number(del.dataset.del), 1);
      this.edited(signals, true);
    });
    // Typing is a draft too, before the field is left.
    this.table.addEventListener('input', () => this.edited(this.readSignals(), false));
    this.table.addEventListener('change', (e) => {
      const signals = this.readSignals();
      // A direction changed: its links are the other direction's.
      const flipped = e.target.dataset.k === 'input';
      if (flipped) {
        const i = Number(e.target.closest('[data-i]').dataset.i);
        signals[i].link = '';
      }
      this.edited(signals, flipped);
    });
  }
}
