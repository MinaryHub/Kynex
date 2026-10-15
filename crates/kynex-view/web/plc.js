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
  // ctx: { toast }
  constructor(ctx) {
    this.ctx = ctx;
    this.panel = document.getElementById('plc-panel');
    this.host = document.getElementById('plc-host');
    this.port = document.getElementById('plc-port');
    this.unit = document.getElementById('plc-unit');
    this.button = document.getElementById('plc-connect');
    this.status = document.getElementById('plc-status');
    this.dot = document.querySelector('.toolbar .plc-dot');
    this.state = null;
    this.button.addEventListener('click', () => this.send(!this.state?.settings?.on));
  }

  toggle() {
    this.panel.hidden = !this.panel.hidden;
  }

  async send(on) {
    const port = Number(this.port.value);
    const unit = Number(this.unit.value);
    if (!(Number.isInteger(port) && port >= 1 && port <= 65535)) return this.ctx.toast('포트는 1–65535입니다');
    if (!(Number.isInteger(unit) && unit >= 0 && unit <= 255)) return this.ctx.toast('유닛은 0–255입니다');
    try {
      const answer = await post({ op: 'plc_link', host: this.host.value.trim(), port, unit, on });
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
      [this.host, s.host],
      [this.port, s.port],
      [this.unit, s.unit],
    ]) {
      if (document.activeElement !== input) input.value = v;
    }
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
    if (state.warning) text += ` · ${state.warning}`;
    this.status.textContent = text;
    this.status.className = `plc-status ${cls}`;
    this.dot.hidden = !s.on;
    this.dot.classList.toggle('ok', s.on && state.connected);
  }
}
