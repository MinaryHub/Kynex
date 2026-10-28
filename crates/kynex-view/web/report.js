// A layout report, for a review or a proposal: the cell from above and from
// an angle, its equipment where it stands, its robots and their programs,
// what each piece is to the process — a page to print, or keep as a PDF.

import * as THREE from 'three';
import { tFor, tr, getLang } from './i18n.js';

const t = tFor('panels');

// The scene drawn through `camera` at `w` × `h`, as an image.
function snapshot(renderer, scene, camera, w, h) {
  const size = renderer.getSize(new THREE.Vector2());
  const ratio = renderer.getPixelRatio();
  renderer.setPixelRatio(1);
  renderer.setSize(w, h, false);
  renderer.render(scene, camera);
  const url = renderer.domElement.toDataURL('image/png');
  renderer.setPixelRatio(ratio);
  renderer.setSize(size.x, size.y, false);
  return url;
}

// The camera over `box`: from above, square on, everything in; or from the
// front left, above.
function cameraFor(box, view, aspect) {
  const centre = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3());
  if (view === 'top') {
    const half = Math.max(size.x / aspect, size.y) / 2 + 0.3;
    const cam = new THREE.OrthographicCamera(-half * aspect, half * aspect, half, -half, 0.01, 100);
    cam.up.set(0, 1, 0);
    cam.position.set(centre.x, centre.y, box.max.z + 10);
    cam.lookAt(centre.x, centre.y, centre.z);
    cam.updateProjectionMatrix();
    return cam;
  }
  const cam = new THREE.PerspectiveCamera(35, aspect, 0.05, 200);
  cam.up.set(0, 0, 1);
  const reach = size.length() / 2 / Math.tan((35 * Math.PI) / 360) * 1.1;
  cam.position.copy(centre).add(new THREE.Vector3(1, -1.35, 0.95).normalize().multiplyScalar(reach));
  cam.lookAt(centre);
  cam.updateProjectionMatrix();
  return cam;
}

const mm = (v) => (v * 1000).toFixed(0);
const VENDOR = { ur: 'Universal Robots', abb: 'ABB', doosan: 'Doosan Robotics', fanuc: 'FANUC' };
const deg = (v) => ((v * 180) / Math.PI).toFixed(1);

// Opens the report in a window of its own. `ctx` has the renderer, the
// scene and the cell's bounds (`bounds`), the library (`library`), the
// robots and their programs (`robots`, `programs`), the cell (`cell`), the
// project's name, and `hide`/`show` for what is not to be drawn.
export function openReport(ctx) {
  const { escapeHtml: esc } = ctx;
  const box = ctx.bounds();
  if (box.isEmpty()) return ctx.toast(t('보고서에 넣을 장면이 없습니다'));
  const win = window.open('', '_blank');
  if (!win) return ctx.toast(t('새 창을 열 수 없습니다: 팝업 차단을 풀어 주세요'));
  ctx.hide();
  const top = snapshot(ctx.renderer, ctx.scene, cameraFor(box, 'top', 1.5), 1500, 1000);
  const iso = snapshot(ctx.renderer, ctx.scene, cameraFor(box, 'iso', 1.5), 1500, 1000);
  ctx.show();
  const size = box.getSize(new THREE.Vector3());

  const templates = new Map(ctx.library.templates.map((x) => [x.key, x]));
  const equipment = ctx.library.items
    .map((i) => {
      const tpl = templates.get(i.template);
      const params = (tpl?.params ?? []).map((p) => `${esc(tr(p.label))} ${p.choices?.length ? esc(tr(p.choices[Math.round(i.params[p.key] ?? p.default)] ?? '')) : mm(i.params[p.key] ?? p.default)}`).join(', ');
      return `<tr><td>${esc(tr(i.name))}</td><td>${esc(tr(tpl?.name ?? i.template))}</td><td class="n">${mm(i.at[0])}, ${mm(i.at[1])}, ${mm(i.at[2])}</td><td class="n">${deg(i.yaw)}</td><td>${params || '—'}</td></tr>`;
    })
    .join('');
  const robots = ctx.robots
    .map((r) => {
      const p = ctx.programs[r.root];
      const moves = (p?.steps ?? []).filter((s) => s.type === 'move').length;
      const actions = (p?.steps ?? []).length - moves;
      const cycle = p?.cycle != null ? `${p.cycle.toFixed(2)} s` : '—';
      const routines = (p?.routines ?? []).map((n) => esc(tr(n))).join(', ') || '—';
      return `<tr><td>${esc(tr(r.name))}</td><td>${VENDOR[r.vendor] ?? '—'}</td><td class="n">${r.joints.filter((j) => j.moves).length}</td><td>${routines}</td><td class="n">${moves}</td><td class="n">${actions}</td><td class="n">${cycle}</td></tr>`;
    })
    .join('');
  const KIND = {
    cnc: t('CNC 장비'), infeed: t('투입 팔레트'), outfeed: t('배출 팔레트'), gripper: t('그리퍼'), fence: t('안전 펜스'),
    molder: t('사출 성형기'), conveyor: t('컨베이어'), amr: 'AMR', amr_stop: t('AMR 정지 위치'), positioner: t('포지셔너'),
  };
  const roles = ctx.cell.components
    .map((c) => {
      const p = c.props;
      const what = {
        cnc: t('가공 {cycle} s, 문 {door} s', { cycle: p.cycle, door: p.door }),
        molder: t('성형 {cycle} s, 가드 {door} s', { cycle: p.cycle, door: p.door }),
        infeed: t('{rows} × {cols}, 간격 {pitch} mm', { rows: p.rows, cols: p.cols, pitch: mm(p.pitch) }),
        outfeed: t('{rows} × {cols}, 간격 {pitch} mm', { rows: p.rows, cols: p.cols, pitch: mm(p.pitch) }),
        gripper: t('개폐 {s} s', { s: p.actuate }),
        conveyor: `${mm(p.speed)} mm/s`,
        amr: `${mm(p.speed)} mm/s`,
        positioner: `${deg(p.speed)} °/s`,
      }[c.kind] ?? '';
      const io = Object.entries(c.io ?? {}).map(([k, n]) => `${esc(k)}→${n}`).join(', ');
      return `<tr><td>${esc(tr(c.name))}</td><td>${KIND[c.kind] ?? esc(c.kind)}</td><td>${what}</td><td>${io || '—'}</td></tr>`;
    })
    .join('');
  const en = getLang() === 'en';
  const when = new Date().toLocaleString(en ? 'en-US' : 'ko-KR');
  win.document.write(`<!doctype html><html lang="${en ? 'en' : 'ko'}"><head><meta charset="utf-8">
<title>${esc(t('KYNEX 배치 보고서 — {project}', { project: ctx.project }))}</title>
<style>
  body { font: 13px/1.5 system-ui, 'Malgun Gothic', sans-serif; color: #1b1e23; margin: 24px 32px; }
  h1 { font-size: 20px; margin: 0 0 2px; } h2 { font-size: 15px; margin: 22px 0 8px; border-bottom: 1px solid #ccd; padding-bottom: 3px; }
  .meta { color: #667; margin-bottom: 14px; }
  .views { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  figure { margin: 0; } figure img { width: 100%; border: 1px solid #ccd; border-radius: 4px; background: #15181d; }
  figcaption { color: #667; font-size: 12px; margin-top: 3px; }
  table { border-collapse: collapse; width: 100%; } th, td { border-bottom: 1px solid #e2e4ea; padding: 4px 6px; text-align: left; vertical-align: top; }
  th { background: #f2f3f6; font-weight: 600; } td.n { font-variant-numeric: tabular-nums; white-space: nowrap; }
  .print { position: fixed; top: 16px; right: 24px; padding: 6px 14px; }
  @media print { .print { display: none; } body { margin: 10mm; } h2 { break-after: avoid; } tr { break-inside: avoid; } }
</style></head><body>
<button class="print" onclick="print()">${t('인쇄 / PDF로 저장')}</button>
<h1>${esc(t('배치 보고서 — {project}', { project: ctx.project }))}</h1>
<div class="meta">${when} · KYNEX · ${t('셀 크기 {x} × {y} × {z} mm (장비가 차지한 범위)', { x: mm(size.x), y: mm(size.y), z: mm(size.z) })}</div>
<div class="views">
  <figure><img src="${top}" alt="${t('위에서 본 배치')}"><figcaption>${t('위에서 (그리드 한 칸 = 뷰어 그리드)')}</figcaption></figure>
  <figure><img src="${iso}" alt="${t('등각으로 본 배치')}"><figcaption>${t('등각')}</figcaption></figure>
</div>
<h2>${t('장비')} (${ctx.library.items.length})</h2>
<table><tr><th>${t('이름')}</th><th>${t('종류')}</th><th>${t('위치 X, Y, Z (mm)')}</th><th>${t('회전 (°)')}</th><th>${t('크기 (mm)')}</th></tr>${equipment || `<tr><td colspan="5">${t('라이브러리로 놓은 장비가 없습니다')}</td></tr>`}</table>
<h2>${t('로봇')} (${ctx.robots.length})</h2>
<table><tr><th>${t('로봇')}</th><th>${t('제조사')}</th><th>${t('축')}</th><th>${t('프로그램')}</th><th>${t('이동')}</th><th>${t('동작')}</th><th>${t('이동 시간 (작업 중 프로그램)')}</th></tr>${robots || `<tr><td colspan="7">${t('로봇이 없습니다')}</td></tr>`}</table>
<h2>${t('셀 역할')} (${ctx.cell.components.length})</h2>
<table><tr><th>${t('이름')}</th><th>${t('역할')}</th><th>${t('설정')}</th><th>I/O</th></tr>${roles || `<tr><td colspan="4">${t('셀 역할이 없습니다')}</td></tr>`}</table>
</body></html>`);
  win.document.close();
}
