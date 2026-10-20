// How much the page shows: 1 a beginner's (lay out, link, give each
// machine its time, run), 2 the factory's detail (variation, breakdowns,
// products, carriers, shifts, cost, scenarios), 3 everything (robot
// programs, wiring, PLCs, real equipment). Kept in this browser.
//
// An element marked `data-min="2"` or `data-min="3"` shows from that level
// up; the inspector's settings and roles are picked here too.

const KEY = 'kynex.level';
const listeners = [];

function read() {
  try {
    const v = Number(localStorage.getItem(KEY));
    return [1, 2, 3].includes(v) ? v : 1;
  } catch {
    return 1;
  }
}

let level = read();

export function getLevel() {
  return level;
}

export function setLevel(v) {
  if (![1, 2, 3].includes(v) || v === level) return;
  level = v;
  try {
    localStorage.setItem(KEY, String(v));
  } catch {
    // Not kept: this visit only.
  }
  document.body.dataset.level = String(v);
  for (const f of listeners) f(v);
}

export function onLevel(f) {
  listeners.push(f);
}

// The settings a beginner sets: the one time or rate that makes each
// kind of equipment what it is.
const BASIC = {
  source: ['feed', 'carrier', 'capacity'],
  station: ['cycle', 'carrier'],
  buffer: ['capacity', 'carrier'],
  conveyor: ['speed', 'carrier'],
  cnc: ['cycle'],
  molder: ['cycle'],
  amr: ['speed', 'capacity'],
  operator: ['speed'],
};

// The settings of `kind` shown at this level, of `fields` ([key, …]).
export function fieldsFor(kind, fields) {
  if (level >= 2) return fields;
  const keep = BASIC[kind] ?? [];
  return fields.filter((f) => keep.includes(f[0]));
}

// The roles offered at each level, beyond the one an object has.
const ROLES = {
  1: ['', 'source', 'station', 'buffer', 'sink', 'conveyor'],
  2: ['', 'source', 'station', 'buffer', 'sink', 'conveyor', 'operator', 'amr', 'amr_stop', 'cnc', 'molder', 'infeed', 'outfeed', 'positioner'],
};

// Whether role `kind` is offered at this level.
export function roleShown(kind) {
  return level >= 3 || ROLES[level].includes(kind);
}

// The switch in the header.
export function mountLevelSwitch(box) {
  document.body.dataset.level = String(level);
  const names = [
    [1, '초급', '배치하고, 잇고, 설비마다 공정 시간만 넣어 돌려 봅니다'],
    [2, '중급', '시간 분포, 고장, 불량, 제품, 반송(AMR·작업자·로봇), 근무, 비용, 시나리오'],
    [3, '전문가', '로봇 프로그램, 입출력 배선, PLC, 실제 장비 연동까지 모두'],
  ];
  box.innerHTML = names
    .map(([v, label, title]) => `<button type="button" data-level="${v}" title="${title}" aria-pressed="${v === level}" class="${v === level ? 'on' : ''}">${label}</button>`)
    .join('');
  box.addEventListener('click', (e) => {
    const b = e.target.closest('[data-level]');
    if (b) setLevel(Number(b.dataset.level));
  });
  onLevel((v) => {
    for (const b of box.querySelectorAll('[data-level]')) {
      const on = Number(b.dataset.level) === v;
      b.classList.toggle('on', on);
      b.setAttribute('aria-pressed', String(on));
    }
  });
}
