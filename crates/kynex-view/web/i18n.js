// The page's language: English unless Korean is chosen (the header's
// EN / 한 button), kept in the browser. The page is written in Korean;
// `t` gives each phrase in the language chosen, from the tables in
// `i18n/`, and `tr` the same for what KYNEX itself says (its errors, its
// equipment's names), which may have names and numbers in it.

import page from './i18n/en-page.js';
import cell from './i18n/en-cell.js';
import robot from './i18n/en-robot.js';
import panels from './i18n/en-panels.js';
import server, { patterns } from './i18n/en-server.js';

const KEY = 'kynex.lang';

function read() {
  try {
    return localStorage.getItem(KEY) === 'ko' ? 'ko' : 'en';
  } catch {
    return 'en';
  }
}

const lang = read();
const EN = { ...server, ...panels, ...robot, ...cell, ...page };
// What KYNEX says with names and numbers in it: [regex, English with $1…].
const PATTERNS = patterns.map(([re, to]) => [new RegExp(`^${re}$`), to]);

export function getLang() {
  return lang;
}

// The other language, chosen: the page drawn again in it.
export function setLang(to) {
  try {
    localStorage.setItem(KEY, to);
  } catch {}
  location.reload();
}

const TABLES = { page, cell, robot, panels, server };

function fill(out, vars) {
  return vars ? out.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : out;
}

// Phrase `ko` in the language chosen; `{name}` in it filled from `vars`.
export function t(ko, vars) {
  return fill(lang === 'en' ? (EN[ko] ?? ko) : ko, vars);
}

// `t` for one part of the page, its own table first: one Korean word may
// be two English ones — 배출 a flow's Sink in the cell's inspector, parts
// Out in the statistics.
export function tFor(name) {
  const own = TABLES[name] ?? {};
  return (ko, vars) => fill(lang === 'en' ? (own[ko] ?? EN[ko] ?? ko) : ko, vars);
}

// What KYNEX said, in the language chosen: a phrase of the tables, or one
// of the patterns, its names and numbers kept; each part of a message
// joined by " · " or " — " on its own. As it was if nothing fits.
export function tr(text) {
  if (lang !== 'en' || text == null || !/[가-힣]/.test(String(text))) return text;
  const s = String(text);
  // KYNEX's own words first.
  if (server[s] != null) return server[s];
  if (EN[s] != null) return EN[s];
  for (const [re, to] of PATTERNS) {
    if (re.test(s)) return s.replace(re, (...m) => to.replace(/\$(\d)/g, (_, i) => tr(m[Number(i)] ?? '')));
  }
  for (const sep of [' — ', ' · ', ': ']) {
    if (s.includes(sep)) return s.split(sep).map(tr).join(sep);
  }
  // A name and its number: 벨트 컨베이어 2.
  const numbered = s.match(/^(.*\D)\s(\d+)$/);
  const base = numbered && (server[numbered[1]] ?? EN[numbered[1]]);
  if (base != null) return `${base} ${numbered[2]}`;
  return s;
}

// The page's own text — index.html's words and its titles, placeholders
// and labels — in the language chosen, once, as it opens.
export function translatePage(root = document.body) {
  if (lang !== 'en') return;
  document.documentElement.lang = 'en';
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes = [];
  while (walker.nextNode()) nodes.push(walker.currentNode);
  for (const n of nodes) {
    const raw = n.nodeValue;
    const s = raw.trim();
    if (!s || !/[가-힣]/.test(s)) continue;
    const en = EN[s];
    if (en != null) n.nodeValue = raw.replace(s, en);
  }
  for (const el of root.querySelectorAll('[title], [placeholder], [aria-label]')) {
    for (const a of ['title', 'placeholder', 'aria-label']) {
      const v = el.getAttribute(a);
      if (v && EN[v] != null) el.setAttribute(a, EN[v]);
    }
  }
}
