// The AI chat: a conversation with Claude, GPT or Gemini that works the
// simulation through tools — reading the scene and its process flow,
// placing, moving and removing equipment, setting roles and links, running
// the flow — each a request to KYNEX (`/robot`) as the page's own panels
// make them, so that what it does can be undone as theirs can.
//
// The page calls the provider itself, with the user's own key, kept in this
// browser only (localStorage); nothing goes through KYNEX but the tools.

import { tFor } from './i18n.js';

const t = tFor('panels');

// Each provider: its name, the model it starts with (changeable), and where
// its keys are made.
const PROVIDERS = {
  claude: { name: 'Claude', model: 'claude-opus-5-5', keys: 'https://platform.claude.com/settings/keys' },
  openai: { name: 'GPT', model: 'gpt-5', keys: 'https://platform.openai.com/api-keys' },
  gemini: { name: 'Gemini', model: 'gemini-2.5-pro', keys: 'https://aistudio.google.com/apikey' },
};

// How many rounds of tool use one message may take before it stops.
const MAX_ROUNDS = 24;

const SYSTEM = `You operate KYNEX, a factory and robot-cell simulator, for its user through the tools given.

The scene is a layout of equipment items placed from a library of templates, each with an id, a name, a position in metres (x, y on the floor, z up) and a turn (yaw, degrees about z). Items carry roles in a process flow: sources put parts in, stations work on them for a cycle time (seconds), buffers and racks hold them, sinks take them out, conveyors carry them. Each flow component sends its parts on to the components linked after it, carried straight (carrier 0), by AMR (1, along the AMR stops' lanes), by an operator (2) or by a robot (3). The flow runs on a clock whose speed can be raised.

Read the state before changing things, refer to items by the names the state gives, and check the state again after a change that matters. Changes can be undone by the user with Undo, so act on clear requests without asking for confirmation; ask only when a request is genuinely ambiguous. When a tool returns an error, read it and try a corrected call or explain what blocked it. Reply in the user's language, briefly: say what you did and what you saw, not how you went about it.`;

// The tools, as JSON Schema; each provider's request shapes them its way.
const TOOLS = [
  {
    name: 'get_state',
    description:
      'The scene and the process flow now: project, whether the flow runs and its figures (parts out, per hour, bottleneck), the equipment items (id, name, template, position, yaw, parameters) and the flow components (name, kind, props, what each links to, the equipment it stands for). Large scenes are cut to the first items; filter by name to see the rest.',
    parameters: {
      type: 'object',
      properties: {
        name_contains: { type: 'string', description: 'Only items and components whose name contains this text.' },
      },
    },
  },
  {
    name: 'list_templates',
    description: 'The library templates that can be placed: key, name, group and parameters (key, label, default, range, choices).',
    parameters: {
      type: 'object',
      properties: {
        group_contains: { type: 'string', description: 'Only templates whose group or name contains this text.' },
      },
    },
  },
  {
    name: 'place_equipment',
    description: 'Places a library template as a new item and names it. Returns its id.',
    parameters: {
      type: 'object',
      properties: {
        template: { type: 'string', description: 'Template key, from list_templates.' },
        name: { type: 'string', description: 'Name for the new item.' },
        x: { type: 'number', description: 'Metres.' },
        y: { type: 'number', description: 'Metres.' },
        z: { type: 'number', description: 'Metres, default 0.' },
        yaw_deg: { type: 'number', description: 'Turn about z, degrees, default 0.' },
        params: { type: 'string', description: 'JSON object of template parameters, e.g. {"length": 3}. Optional.' },
      },
      required: ['template', 'x', 'y'],
    },
  },
  {
    name: 'move_equipment',
    description: 'Moves or turns an item. Coordinates left out stay as they are.',
    parameters: {
      type: 'object',
      properties: {
        item: { type: 'string', description: 'Item name or id.' },
        x: { type: 'number' },
        y: { type: 'number' },
        z: { type: 'number' },
        yaw_deg: { type: 'number' },
      },
      required: ['item'],
    },
  },
  {
    name: 'set_equipment_params',
    description: "Changes an item's template parameters (size, cycle time, levels...).",
    parameters: {
      type: 'object',
      properties: {
        item: { type: 'string', description: 'Item name or id.' },
        params: { type: 'string', description: 'JSON object of parameters to change.' },
      },
      required: ['item', 'params'],
    },
  },
  {
    name: 'remove_equipment',
    description: 'Removes an item from the scene.',
    parameters: {
      type: 'object',
      properties: { item: { type: 'string', description: 'Item name or id.' } },
      required: ['item'],
    },
  },
  {
    name: 'set_flow_role',
    description:
      'Sets the role of a component and its props: kind source, station, buffer, sink or conveyor; props such as feed (s), cycle (s), capacity, carrier (0 straight, 1 AMR, 2 operator, 3 robot), rule (0 in turn, 1 fewest waiting, 2 random, 3 by share), scrap (share), mtbf/mttr (s), speed (m/s).',
    parameters: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'Component or item name.' },
        kind: { type: 'string', description: 'source, station, buffer, sink or conveyor.' },
        props: { type: 'string', description: 'JSON object of props, e.g. {"cycle": 30, "carrier": 1}.' },
      },
      required: ['component', 'kind'],
    },
  },
  {
    name: 'link_flow',
    description: 'Links a flow component to the components its parts go on to. mode "set" replaces its links, "add" adds to them.',
    parameters: {
      type: 'object',
      properties: {
        from: { type: 'string', description: 'Component name.' },
        to: { type: 'array', items: { type: 'string' }, description: 'Names of the components it sends to.' },
        mode: { type: 'string', description: '"set" (default) or "add".' },
      },
      required: ['from', 'to'],
    },
  },
  {
    name: 'run_flow',
    description: 'Starts the run, stops it where it is, or takes it back to the start (all parts taken away).',
    parameters: {
      type: 'object',
      properties: { action: { type: 'string', description: 'start, stop or reset.' } },
      required: ['action'],
    },
  },
  {
    name: 'set_speed',
    description: "Sets the simulation clock's speed, times real time (0.1 to several hundred).",
    parameters: {
      type: 'object',
      properties: { speed: { type: 'number' } },
      required: ['speed'],
    },
  },
  {
    name: 'wait',
    description: 'Lets the simulation run for some real seconds (at most 30) before going on, e.g. to see how a change does.',
    parameters: {
      type: 'object',
      properties: { seconds: { type: 'number' } },
      required: ['seconds'],
    },
  },
  {
    name: 'kynex_request',
    description:
      'Sends any other request to KYNEX as JSON, for what the other tools do not cover. Examples: {"op":"flow_products","object":"<id>","mix":[1,1]} (a source\'s product mix), {"op":"flow_products","object":"<id>","to":"<id>","products":[0]} (a link taking certain products), {"op":"amr_lane","from":"<stop id>","to":"<stop id>","way":"both"}, {"op":"flow_settings","warmup":600}, {"op":"rename","item":3,"name":"..."}. Object ids are the "object" values get_state shows.',
    parameters: {
      type: 'object',
      properties: { request: { type: 'string', description: 'The request, a JSON object with its "op".' } },
      required: ['request'],
    },
  },
];

const store = {
  get(key, fallback) {
    try {
      return localStorage.getItem(`kynex.chat.${key}`) ?? fallback;
    } catch {
      return fallback;
    }
  },
  set(key, value) {
    try {
      localStorage.setItem(`kynex.chat.${key}`, value);
    } catch {}
  },
};

async function post(body) {
  const response = await fetch('/robot', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  return response.json();
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const rad = (deg) => (deg * Math.PI) / 180;
const deg = (r) => Math.round(((r * 180) / Math.PI) * 10) / 10;
const round = (v) => Math.round(v * 1000) / 1000;

// A JSON object given as a string (or already an object), or an error.
function objectOf(v) {
  if (v == null || v === '') return {};
  if (typeof v === 'object') return v;
  const parsed = JSON.parse(v);
  if (typeof parsed !== 'object' || Array.isArray(parsed)) throw new Error('not a JSON object');
  return parsed;
}

export class ChatPanel {
  // ctx: library() the library event, cell() the cell event, toast.
  constructor(ctx) {
    this.ctx = ctx;
    this.el = document.getElementById('chat');
    this.log = this.el.querySelector('.chat-log');
    this.input = this.el.querySelector('.chat-input');
    this.sendBtn = this.el.querySelector('[data-chat="send"]');
    this.provider = store.get('provider', 'claude');
    this.history = [];
    this.busy = false;
    this.stopped = false;
    this.renderSettings();
    this.el.addEventListener('click', (e) => this.onClick(e));
    this.el.addEventListener('change', (e) => this.onChange(e));
    this.input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
        e.preventDefault();
        this.send();
      }
      e.stopPropagation();
    });
  }

  // The provider, its model and key: what the settings show.
  renderSettings() {
    const p = PROVIDERS[this.provider];
    const box = this.el.querySelector('.chat-settings');
    box.querySelector('[data-chat="provider"]').value = this.provider;
    box.querySelector('[data-chat="model"]').value = store.get(`${this.provider}.model`, p.model);
    box.querySelector('[data-chat="key"]').value = store.get(`${this.provider}.key`, '');
    const link = box.querySelector('[data-chat="keys"]');
    link.href = p.keys;
    link.textContent = t('{name} API 키 발급', { name: p.name });
    this.el.querySelector('.chat-who').textContent = `${p.name} · ${store.get(`${this.provider}.model`, p.model)}`;
    if (!store.get(`${this.provider}.key`, '')) box.open = true;
  }

  onChange(e) {
    const what = e.target.dataset.chat;
    if (what === 'provider') {
      this.provider = e.target.value;
      store.set('provider', this.provider);
      // Another provider's turns are not this one's to read on.
      this.clear();
      this.renderSettings();
    } else if (what === 'model') {
      store.set(`${this.provider}.model`, e.target.value.trim() || PROVIDERS[this.provider].model);
      this.renderSettings();
    } else if (what === 'key') {
      store.set(`${this.provider}.key`, e.target.value.trim());
    }
  }

  onClick(e) {
    const what = e.target.closest('[data-chat]')?.dataset.chat;
    if (what === 'send') this.busy ? (this.stopped = true) : this.send();
    else if (what === 'clear') this.clear();
  }

  clear() {
    this.history = [];
    this.log.innerHTML = `<p class="muted chat-empty">${t('시뮬레이션에 대해 묻거나 시켜 보세요. 예: "공정 흐름을 시작하고 병목을 알려줘", "컨베이어를 3 m로 늘려줘"')}</p>`;
  }

  add(kind, text) {
    this.log.querySelector('.chat-empty')?.remove();
    const div = document.createElement('div');
    div.className = `chat-msg ${kind}`;
    div.textContent = text;
    this.log.appendChild(div);
    this.log.scrollTop = this.log.scrollHeight;
    return div;
  }

  setBusy(on) {
    this.busy = on;
    this.stopped = false;
    this.sendBtn.textContent = on ? t('중지') : t('보내기');
    this.el.classList.toggle('busy', on);
  }

  async send() {
    const text = this.input.value.trim();
    if (!text || this.busy) return;
    const key = store.get(`${this.provider}.key`, '');
    if (!key) {
      this.el.querySelector('.chat-settings').open = true;
      return this.ctx.toast(t('API 키를 먼저 넣으세요'));
    }
    this.input.value = '';
    this.add('user', text);
    this.history.push({ role: 'user', text });
    this.setBusy(true);
    const thinking = this.add('note', t('생각하는 중…'));
    try {
      await this.converse(key, thinking);
    } catch (err) {
      this.add('error', String(err.message ?? err));
    } finally {
      thinking.remove();
      this.setBusy(false);
    }
  }

  // Asks the provider, runs the tools it calls, and asks again, until it
  // answers without a tool, is stopped, or has taken too many rounds.
  async converse(key, thinking) {
    const model = store.get(`${this.provider}.model`, PROVIDERS[this.provider].model);
    const ask = { claude: askClaude, openai: askOpenAI, gemini: askGemini }[this.provider];
    for (let round = 0; round < MAX_ROUNDS; round++) {
      if (this.stopped) return this.add('note', t('중지했습니다'));
      const reply = await ask(key, model, this.history);
      this.history.push(reply.turn);
      if (reply.text) this.add('assistant', reply.text);
      if (reply.notice) this.add('note', reply.notice);
      if (!reply.calls.length) return;
      const results = [];
      for (const call of reply.calls) {
        if (this.stopped) {
          results.push({ call, result: { ok: false, error: 'stopped by the user' } });
          continue;
        }
        thinking.textContent = t('도구 실행: {name}', { name: call.name });
        const line = this.add('tool', `▸ ${call.name} ${brief(call.args)}`);
        let result;
        try {
          result = await this.run(call.name, call.args ?? {});
        } catch (err) {
          result = { ok: false, error: String(err.message ?? err) };
        }
        if (result && result.ok === false) line.classList.add('failed');
        results.push({ call, result });
      }
      this.history.push({ role: 'tool', results });
      thinking.textContent = t('생각하는 중…');
    }
    this.add('note', t('도구를 너무 여러 번 써서 멈췄습니다. 이어서 시키면 계속합니다'));
  }

  // --- The tools -------------------------------------------------------

  items() {
    return this.ctx.library()?.items ?? [];
  }

  components() {
    return this.ctx.cell()?.components ?? [];
  }

  // An item by its id or name: exact first, then the one name containing it.
  item(ref) {
    const items = this.items();
    const s = String(ref ?? '').trim();
    const byId = /^\d+$/.test(s) ? items.find((i) => String(i.id) === s) : null;
    const found =
      byId ?? items.find((i) => i.name === s) ?? items.find((i) => i.name.toLowerCase() === s.toLowerCase());
    if (found) return found;
    const some = items.filter((i) => i.name.toLowerCase().includes(s.toLowerCase()));
    if (some.length === 1) return some[0];
    throw new Error(some.length ? `"${s}" matches ${some.length} items: ${some.slice(0, 8).map((i) => i.name).join(', ')}` : `no item "${s}"`);
  }

  // A flow component (or any role) by its name, or by its item's.
  component(ref) {
    const s = String(ref ?? '').trim();
    const comps = this.components();
    const direct = comps.find((c) => c.object === s || c.name === s);
    if (direct) return direct;
    const item = this.item(s);
    const root = BigInt(item.root);
    const under = comps.filter((c) => {
      const d = BigInt(c.object) - root;
      return d >= 0n && d < 1n << 24n;
    });
    const flowish = under.find((c) => ['source', 'station', 'buffer', 'sink', 'conveyor'].includes(c.kind));
    const found = flowish ?? under[0];
    if (!found) throw new Error(`"${s}" has no role`);
    return found;
  }

  nameOf(object) {
    const c = this.components().find((x) => x.object === object);
    if (!c) return object;
    if (c.name !== 'belt') return c.name;
    // A conveyor's role is named for its belt: its item's name instead.
    const item = this.items().find((i) => {
      const d = BigInt(object) - BigInt(i.root);
      return d >= 0n && d < 1n << 24n;
    });
    return item?.name ?? c.name;
  }

  // After a change, the page's state once KYNEX has told it.
  async settle() {
    await sleep(450);
  }

  async run(name, a) {
    switch (name) {
      case 'get_state':
        return this.state(a.name_contains);
      case 'list_templates': {
        const want = (a.group_contains ?? '').toLowerCase();
        const list = (this.ctx.library()?.templates ?? [])
          .filter((tpl) => !want || `${tpl.group} ${tpl.name} ${tpl.key}`.toLowerCase().includes(want))
          .map((tpl) => ({
            key: tpl.key,
            name: tpl.name,
            group: tpl.group,
            params: tpl.params.map((p) => (p.choices?.length ? { key: p.key, label: p.label, default: p.default, choices: p.choices } : { key: p.key, label: p.label, default: p.default, min: p.min, max: p.max, unit: p.unit })),
          }));
        return { ok: true, templates: list };
      }
      case 'place_equipment': {
        const r = await post({
          op: 'place',
          template: a.template,
          at: [a.x, a.y, a.z ?? 0],
          yaw: rad(a.yaw_deg ?? 0),
          params: objectOf(a.params),
        });
        if (!r.ok) return r;
        if (a.name) await post({ op: 'rename', item: r.item, name: a.name });
        await this.settle();
        return { ok: true, item: r.item, name: a.name };
      }
      case 'move_equipment': {
        const item = this.item(a.item);
        const at = [a.x ?? item.at[0], a.y ?? item.at[1], a.z ?? item.at[2]];
        const r = await post({ op: 'move', item: item.id, at, yaw: a.yaw_deg != null ? rad(a.yaw_deg) : item.yaw });
        await this.settle();
        return r;
      }
      case 'set_equipment_params': {
        const item = this.item(a.item);
        const r = await post({ op: 'resize', item: item.id, params: objectOf(a.params) });
        await this.settle();
        return r;
      }
      case 'remove_equipment': {
        const item = this.item(a.item);
        const r = await post({ op: 'remove', item: item.id });
        await this.settle();
        return r;
      }
      case 'set_flow_role': {
        const c = this.component(a.component);
        const r = await post({ op: 'role', object: c.object, kind: a.kind, props: objectOf(a.props) });
        await this.settle();
        return r;
      }
      case 'link_flow': {
        const from = this.component(a.from);
        const to = (a.to ?? []).map((n) => this.component(n).object);
        const before = a.mode === 'add' ? (from.flow?.next ?? []) : [];
        const next = [...new Set([...before, ...to])];
        const r = await post({ op: 'flow_next', object: from.object, next });
        await this.settle();
        return r;
      }
      case 'run_flow': {
        const r =
          a.action === 'reset' ? await post({ op: 'reset_run' }) : await post({ op: 'flow', action: a.action === 'stop' ? 'stop' : 'start' });
        await this.settle();
        return r;
      }
      case 'set_speed':
        return post({ op: 'speed', speed: a.speed });
      case 'wait': {
        const s = Math.min(Math.max(Number(a.seconds) || 1, 0.5), 30);
        for (let k = 0; k < s * 4 && !this.stopped; k++) await sleep(250);
        return { ok: true, waited: s };
      }
      case 'kynex_request':
        return post(objectOf(a.request));
      default:
        return { ok: false, error: `no tool ${name}` };
    }
  }

  // The scene and flow, cut short where large.
  state(filter) {
    const want = (filter ?? '').toLowerCase();
    const keep = (name) => !want || String(name).toLowerCase().includes(want);
    const cell = this.ctx.cell() ?? {};
    const items = this.items().filter((i) => keep(i.name));
    const flowKinds = ['source', 'station', 'buffer', 'sink', 'conveyor', 'amr', 'amr_stop', 'operator'];
    const comps = this.components().filter((c) => flowKinds.includes(c.kind) && keep(this.nameOf(c.object)));
    const f = cell.flow ?? {};
    const LIMIT = 150;
    return {
      ok: true,
      project: this.ctx.project?.() ?? null,
      flow: {
        running: !!f.on,
        elapsed_s: f.elapsed != null ? Math.round(f.elapsed) : null,
        parts_out: f.sunk,
        per_hour: f.per_hour != null ? Math.round(f.per_hour * 10) / 10 : null,
        wip: f.wip,
        scrapped: f.scrapped,
        problem: f.problem ?? null,
        stations: (f.stations ?? []).map((s) => ({ name: s.name, busy: s.busy != null ? Math.round(s.busy * 100) / 100 : undefined, good: s.good, oee: s.oee != null ? Math.round(s.oee * 100) / 100 : undefined })),
      },
      items_total: items.length,
      items: items.slice(0, LIMIT).map((i) => ({
        id: i.id,
        name: i.name,
        template: i.template,
        at: i.at.map(round),
        yaw_deg: deg(i.yaw),
        params: i.params,
        locked: i.locked || undefined,
      })),
      components_total: comps.length,
      components: comps.slice(0, LIMIT).map((c) => ({
        object: c.object,
        name: this.nameOf(c.object),
        kind: c.kind,
        props: pickProps(c),
        next: c.flow?.next?.map((o) => this.nameOf(o)),
        equipment: c.host ? this.nameOf(c.host) : undefined,
        waiting: c.flow ? (c.flow.queue ?? 0) : undefined,
      })),
    };
  }
}

// The props worth showing for a component's kind.
function pickProps(c) {
  const p = c.props ?? {};
  const keys = {
    source: ['feed', 'look', 'capacity', 'kanban', 'carrier', 'rule', 'policy'],
    station: ['cycle', 'takes', 'makes', 'capacity', 'carrier', 'rule', 'scrap', 'mtbf', 'mttr', 'join', 'look'],
    buffer: ['capacity', 'carrier', 'rule', 'policy'],
    conveyor: ['speed', 'feed', 'drain', 'carrier', 'rule'],
    sink: [],
    amr: ['speed', 'battery'],
    amr_stop: ['charger'],
    operator: ['speed'],
  }[c.kind] ?? [];
  return Object.fromEntries(keys.filter((k) => p[k] != null).map((k) => [k, p[k]]));
}

function brief(args) {
  const s = JSON.stringify(args ?? {});
  return s.length > 120 ? `${s.slice(0, 117)}…` : s;
}

// The result of a tool, as text for the model.
function resultText(result) {
  const s = JSON.stringify(result ?? { ok: true });
  return s.length > 60000 ? `${s.slice(0, 60000)}… (cut)` : s;
}

async function failed(response, who) {
  let detail = '';
  try {
    const body = await response.json();
    detail = body.error?.message ?? body.error ?? JSON.stringify(body);
  } catch {}
  return new Error(`${who} ${response.status}: ${detail || response.statusText}`);
}

// --- Claude (Messages API) ----------------------------------------------
//
// The history kept neutral: user text, the provider's own assistant turns
// (kept whole, to send back unchanged — Claude's thinking blocks with
// them), and tool results.

async function askClaude(key, model, history) {
  const messages = [];
  for (const h of history) {
    if (h.role === 'user') messages.push({ role: 'user', content: h.text });
    else if (h.role === 'assistant') messages.push({ role: 'assistant', content: h.claude });
    else if (h.role === 'tool')
      messages.push({
        role: 'user',
        content: h.results.map(({ call, result }) => ({
          type: 'tool_result',
          tool_use_id: call.id,
          content: resultText(result),
          is_error: result?.ok === false || undefined,
        })),
      });
  }
  // A model declined by its safeguards is answered by another, where the
  // model takes it.
  const fallback = /^claude-(opus-5-5|opus-5|fable-5-1|sonnet-5-5)$/.test(model);
  const headers = {
    'content-type': 'application/json',
    'x-api-key': key,
    'anthropic-version': '2026-10-21',
    'anthropic-dangerous-direct-browser-access': 'true',
  };
  if (fallback) headers['anthropic-beta'] = 'server-side-fallback-2026-07-01';
  const body = {
    model,
    max_tokens: 16000,
    system: SYSTEM,
    tools: TOOLS.map((tool) => ({ name: tool.name, description: tool.description, input_schema: tool.parameters })),
    messages,
  };
  if (fallback) body.fallbacks = 'default';
  const response = await fetch('https://api.anthropic.com/v1/messages', { method: 'POST', headers, body: JSON.stringify(body) });
  if (!response.ok) throw await failed(response, 'Claude');
  const m = await response.json();
  const content = m.content ?? [];
  const text = content.filter((b) => b.type === 'text').map((b) => b.text).join('\n').trim();
  let notice = null;
  if (m.stop_reason === 'refusal') notice = t('모델이 이 요청을 거절했습니다');
  else if (m.stop_reason === 'max_tokens') notice = t('답이 길어 잘렸습니다');
  const calls =
    m.stop_reason === 'tool_use'
      ? content.filter((b) => b.type === 'tool_use').map((b) => ({ id: b.id, name: b.name, args: b.input }))
      : [];
  return { turn: { role: 'assistant', claude: content }, text, calls, notice };
}

// --- GPT (Chat Completions) ---------------------------------------------

async function askOpenAI(key, model, history) {
  const messages = [{ role: 'system', content: SYSTEM }];
  for (const h of history) {
    if (h.role === 'user') messages.push({ role: 'user', content: h.text });
    else if (h.role === 'assistant') messages.push(h.openai);
    else if (h.role === 'tool')
      for (const { call, result } of h.results) messages.push({ role: 'tool', tool_call_id: call.id, content: resultText(result) });
  }
  const response = await fetch('https://api.openai.com/v1/chat/completions', {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: `Bearer ${key}` },
    body: JSON.stringify({
      model,
      messages,
      tools: TOOLS.map((tool) => ({ type: 'function', function: { name: tool.name, description: tool.description, parameters: tool.parameters } })),
    }),
  });
  if (!response.ok) throw await failed(response, 'GPT');
  const r = await response.json();
  const choice = r.choices?.[0] ?? {};
  const msg = choice.message ?? { role: 'assistant', content: '' };
  const calls = (msg.tool_calls ?? []).map((c) => {
    let args = {};
    try {
      args = JSON.parse(c.function?.arguments || '{}');
    } catch {
      args = { _invalid: c.function?.arguments };
    }
    return { id: c.id, name: c.function?.name, args };
  });
  const notice = choice.finish_reason === 'length' ? t('답이 길어 잘렸습니다') : msg.refusal ? msg.refusal : null;
  return { turn: { role: 'assistant', openai: msg }, text: (msg.content ?? '').trim(), calls, notice };
}

// --- Gemini (generateContent) -------------------------------------------

async function askGemini(key, model, history) {
  const contents = [];
  for (const h of history) {
    if (h.role === 'user') contents.push({ role: 'user', parts: [{ text: h.text }] });
    // The model's turn as it gave it: its thought signatures with it.
    else if (h.role === 'assistant') contents.push(h.gemini);
    else if (h.role === 'tool')
      contents.push({
        role: 'user',
        parts: h.results.map(({ call, result }) => ({ functionResponse: { name: call.name, response: { result: resultText(result) } } })),
      });
  }
  const response = await fetch(`https://generativelanguage.googleapis.com/v1beta/models/${encodeURIComponent(model)}:generateContent`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-goog-api-key': key },
    body: JSON.stringify({
      systemInstruction: { parts: [{ text: SYSTEM }] },
      contents,
      tools: [{ functionDeclarations: TOOLS.map((tool) => ({ name: tool.name, description: tool.description, parameters: tool.parameters })) }],
    }),
  });
  if (!response.ok) throw await failed(response, 'Gemini');
  const r = await response.json();
  const cand = r.candidates?.[0];
  const content = cand?.content ?? { role: 'model', parts: [] };
  if (!content.role) content.role = 'model';
  const parts = content.parts ?? [];
  const text = parts.filter((p) => p.text && !p.thought).map((p) => p.text).join('\n').trim();
  const calls = parts.filter((p) => p.functionCall).map((p, k) => ({ id: `g${k}`, name: p.functionCall.name, args: p.functionCall.args ?? {} }));
  let notice = null;
  if (!cand) notice = r.promptFeedback?.blockReason ? t('모델이 이 요청을 거절했습니다') : t('답이 없습니다');
  else if (cand.finishReason === 'MAX_TOKENS') notice = t('답이 길어 잘렸습니다');
  else if (cand.finishReason && !['STOP', 'MAX_TOKENS'].includes(cand.finishReason) && !calls.length && !text) notice = `${t('답이 없습니다')} (${cand.finishReason})`;
  return { turn: { role: 'assistant', gemini: content }, text, calls, notice };
}
