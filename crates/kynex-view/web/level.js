// How much the page shows: 1 a beginner's (lay out, link, give each
// machine its time, run), 2 the factory's detail (variation, breakdowns,
// products, carriers, shifts, cost, scenarios), 3 everything (robot
// programs, wiring, PLCs, real equipment). The page shows everything: the
// switch between them is gone, the levels kept for what may use them.
//
// An element marked `data-min="2"` or `data-min="3"` shows from that level
// up; the inspector's settings and roles are picked here too.

const KEY = 'kynex.level';
const listeners = [];

function read() {
  return 3;
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
