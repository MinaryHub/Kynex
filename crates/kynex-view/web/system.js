// What is KYNEX's own rather than the scene's: its updates (the badge by
// the version, and its menu), and the viewer shared with tablets and phones
// on the network (도구 ▸ 태블릿·모바일에서 열기). Both are for the page of the
// computer KYNEX runs on; a tablet's page sees neither.

import { showMenu } from './menu.js';
import { tFor, tr } from './i18n.js';

const t = tFor('page');

// This computer's page, not one opened from another device.
export const here = ['127.0.0.1', 'localhost', '[::1]', '::1'].includes(location.hostname);

async function ask(body) {
  const response = await fetch('/robot', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  return response.json();
}

export class SystemPanel {
  constructor({ toast }) {
    this.toast = toast;
    this.update = null;
    this.share = null;
    this.badge = document.getElementById('update-btn');
    this.badge.addEventListener('click', () => {
      const r = this.badge.getBoundingClientRect();
      showMenu(r.left, r.bottom + 6, this.updateEntries());
    });
    this.panel = document.getElementById('share-panel');
    this.on = document.getElementById('share-on');
    this.on.addEventListener('change', () => this.setShared(this.on.checked));
    document.getElementById('share-copy').addEventListener('click', () => this.copy());
    document.getElementById('share-new-key').addEventListener('click', () => this.newKey());
  }

  // ---- updates ---------------------------------------------------------------

  setUpdate(u) {
    const before = this.update?.state;
    this.update = u;
    const label = {
      available: t('새 버전 v{v}', { v: u.latest }),
      downloading: t('업데이트 내려받는 중…'),
      ready: t('업데이트 v{v} 준비됨', { v: u.latest }),
      installing: t('업데이트 설치 중…'),
    }[u.state];
    this.badge.hidden = !here || !label;
    if (label) this.badge.textContent = label;
    this.badge.classList.toggle('busy', u.state === 'downloading' || u.state === 'installing');
    this.badge.title = u.state === 'ready' ? t('다음에 KYNEX를 시작할 때 설치됩니다. 눌러서 지금 설치') : t('KYNEX 업데이트');
    // Told once as it comes, not again on every reconnect.
    if (!here || !before || before === u.state) return;
    if (u.state === 'ready') this.toast(t('KYNEX v{v}를 내려받았습니다. 다음에 시작할 때 설치되며, 버전 옆 단추로 지금 설치할 수 있습니다', { v: u.latest }));
    if (u.state === 'latest' && this.checking) this.toast(t('최신 버전입니다 (v{v})', { v: u.current }));
    if (u.state === 'error' && this.checking) this.toast(t('업데이트: {error}', { error: tr(u.error) }));
    if (u.state !== 'checking') this.checking = false;
  }

  // What the update menu says and offers, as things stand.
  updateEntries() {
    const u = this.update ?? {};
    const newer = !!u.latest && ['available', 'downloading', 'ready', 'error'].includes(u.state) && u.latest !== u.current;
    const status = {
      idle: t('아직 확인하지 않았습니다'),
      checking: t('확인하는 중…'),
      latest: t('최신 버전입니다'),
      available: t('새 버전이 있습니다: v{v}', { v: u.latest }),
      downloading: t('v{v} 내려받는 중…', { v: u.latest }),
      ready: t('v{v} 준비됨 · 다음 시작 때 설치', { v: u.latest }),
      installing: t('설치하는 중… KYNEX가 곧 다시 시작합니다'),
      error: tr(u.error) || t('업데이트하지 못했습니다'),
    }[u.state] ?? '';
    const entries = [{ title: t('KYNEX v{v}', { v: u.current ?? '' }) }, { label: status, disabled: true }];
    if (u.kind === 'dev') {
      entries.push({ label: t('개발 빌드는 업데이트하지 않습니다'), disabled: true });
      return entries;
    }
    entries.push({ sep: true });
    if (newer && (u.kind === 'setup' || u.kind === 'portable')) {
      entries.push({ label: t('지금 업데이트하고 다시 시작'), run: () => this.install() });
    }
    if (u.kind === 'manual') {
      entries.push({ label: t('이 설치(MSI 등)는 자동으로 바꾸지 않습니다'), disabled: true });
    }
    if (newer || u.kind === 'manual') {
      entries.push({ label: t('릴리스 페이지 열기'), run: () => window.open(u.page, '_blank', 'noopener') });
    }
    entries.push({ label: t('지금 확인'), disabled: u.state === 'checking' || u.state === 'downloading', run: () => this.check() });
    // On: a new version fetched ahead, installed as KYNEX next starts.
    entries.push({ label: t('자동 업데이트'), checked: !!u.auto, run: () => this.setAuto(!u.auto) });
    return entries;
  }

  async check() {
    this.checking = true;
    try {
      const answer = await ask({ op: 'update', action: 'check' });
      if (!answer.ok) this.toast(tr(answer.error));
    } catch (err) {
      this.toast(t('KYNEX에 요청하지 못했습니다: {error}', { error: err.message }));
    }
  }

  async install() {
    if (!confirm(t('KYNEX를 업데이트하고 다시 시작합니다. 실행 중인 시뮬레이션은 멈추고, 프로젝트는 그대로 다시 열립니다. 계속할까요?'))) return;
    try {
      const answer = await ask({ op: 'update', action: 'install' });
      if (!answer.ok) return this.toast(tr(answer.error));
      this.toast(t('업데이트를 설치합니다. 잠시 뒤 KYNEX가 다시 시작하면 이 화면도 다시 열립니다'));
    } catch (err) {
      this.toast(t('KYNEX에 요청하지 못했습니다: {error}', { error: err.message }));
    }
  }

  async setAuto(on) {
    try {
      const answer = await ask({ op: 'update', action: 'auto', on });
      if (!answer.ok) return this.toast(tr(answer.error));
      this.toast(on ? t('자동 업데이트를 켰습니다: 새 버전을 미리 내려받아 다음 시작 때 설치합니다') : t('자동 업데이트를 껐습니다'));
    } catch (err) {
      this.toast(t('KYNEX에 요청하지 못했습니다: {error}', { error: err.message }));
    }
  }

  // ---- shared with tablets ---------------------------------------------------

  setShare(s) {
    this.share = s;
    this.on.checked = !!s.on;
    this.on.disabled = !!s.lan;
    document.getElementById('share-body').hidden = !s.on || !s.url;
    const url = document.getElementById('share-url');
    url.textContent = s.url ?? '';
    url.href = s.url ?? '#';
    const qr = document.getElementById('share-qr');
    if (s.qr) qr.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(s.qr)}`;
    else qr.removeAttribute('src');
    const error = document.getElementById('share-error');
    error.hidden = !s.error;
    error.textContent = tr(s.error) ?? '';
  }

  toggle() {
    this.panel.hidden = !this.panel.hidden;
  }

  async setShared(on) {
    try {
      const answer = await ask({ op: 'share', on });
      if (!answer.ok) {
        this.on.checked = !on;
        return this.toast(tr(answer.error));
      }
      if (on) this.toast(t('이제 같은 네트워크의 태블릿·휴대폰에서 열 수 있습니다'));
    } catch (err) {
      this.on.checked = !on;
      this.toast(t('KYNEX에 요청하지 못했습니다: {error}', { error: err.message }));
    }
  }

  async newKey() {
    if (!confirm(t('새 접속 코드를 만들면 지금까지 연 기기는 새 QR 코드로 다시 열어야 합니다. 계속할까요?'))) return;
    try {
      const answer = await ask({ op: 'share', action: 'new_key' });
      if (!answer.ok) return this.toast(tr(answer.error));
      this.toast(t('새 접속 코드를 만들었습니다'));
    } catch (err) {
      this.toast(t('KYNEX에 요청하지 못했습니다: {error}', { error: err.message }));
    }
  }

  async copy() {
    const url = this.share?.url;
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
      this.toast(t('주소를 복사했습니다'));
    } catch {
      this.toast(url);
    }
  }

  // The tools menu's entries for this: none on a tablet's page.
  toolsEntries(close) {
    if (!here) return [];
    return [
      { sep: true },
      {
        label: t('태블릿·모바일에서 열기'),
        checked: !!this.share?.on,
        run: () => {
          close();
          this.toggle();
        },
      },
      { label: t('KYNEX 업데이트'), sub: this.updateEntries() },
    ];
  }
}
