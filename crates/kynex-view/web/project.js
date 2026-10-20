// Projects: the one open (the `project` event), and making a new one,
// opening another or saving this one under another name — by name, in the
// projects folder KYNEX keeps; and the demos that come with KYNEX, each
// opened as a copy of it in that folder.

async function post(request) {
  const response = await fetch('/robot', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

export class ProjectPanel {
  // ctx: { toast, escapeHtml }
  constructor(ctx) {
    this.ctx = ctx;
    this.panel = document.getElementById('project-panel');
    this.name = document.getElementById('project-new-name');
    this.list = document.getElementById('project-list');
    this.demos = document.getElementById('project-demos');
    this.state = null;
    document.getElementById('project-new').addEventListener('click', () => {
      const name = this.name.value.trim();
      if (!name) return this.ctx.toast('새 프로젝트의 이름을 넣으세요');
      if (!confirm(`새 프로젝트 "${name}"을(를) 엽니다. 지금 프로젝트는 저장되고, 실행 중인 프로그램과 PLC 연결은 멈춥니다.`)) return;
      this.act('new', name, `새 프로젝트 "${name}"을(를) 열었습니다`);
    });
    document.getElementById('project-save-as').addEventListener('click', () => {
      const name = this.name.value.trim();
      if (!name) return this.ctx.toast('저장할 이름을 넣으세요');
      this.act('save_as', name, `"${name}"(으)로 저장했습니다. 이제부터 이 파일에 저장됩니다`);
    });
    this.name.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') document.getElementById('project-new').click();
    });
    this.list.addEventListener('click', (e) => {
      const li = e.target.closest('li[data-name]');
      if (!li || li.classList.contains('current')) return;
      const name = li.dataset.name;
      if (!confirm(`프로젝트 "${name}"을(를) 엽니다. 지금 프로젝트는 저장되고, 실행 중인 프로그램과 PLC 연결은 멈춥니다.`)) return;
      this.act('open', name, `프로젝트 "${name}"을(를) 열었습니다`);
    });
    this.demos.addEventListener('click', async (e) => {
      const li = e.target.closest('li[data-demo]');
      if (!li) return;
      const name = li.dataset.demo;
      if (!confirm(`예제 "${name}"의 사본을 프로젝트 폴더에 만들어 엽니다. 지금 프로젝트는 저장되고, 실행 중인 프로그램과 PLC 연결은 멈춥니다.`)) return;
      try {
        const answer = await post({ op: 'project', action: 'demo', name });
        if (!answer.ok) return this.ctx.toast(answer.error || '열 수 없습니다');
        this.ctx.toast(`예제를 "${answer.name}"(으)로 열었습니다. 바꾼 것은 이 사본에 저장됩니다`);
        this.refresh();
      } catch (err) {
        this.ctx.toast(`예제: ${err.message}`);
      }
    });
  }

  toggle() {
    this.panel.hidden = !this.panel.hidden;
    if (!this.panel.hidden) this.refresh();
  }

  setState(state) {
    this.state = state;
    document.getElementById('project-name').textContent = state.name ?? '';
    document.getElementById('project-path').textContent = state.path ?? '';
    document.getElementById('project-path').title = state.path ?? '';
    document.getElementById('project-dir').textContent = state.dir ? `폴더: ${state.dir}` : '';
    if (!this.panel.hidden) this.refresh();
  }

  async act(action, name, done) {
    try {
      const answer = await post({ op: 'project', action, name });
      if (!answer.ok) return this.ctx.toast(answer.error || '할 수 없습니다');
      this.name.value = '';
      this.ctx.toast(done);
      this.refresh();
    } catch (e) {
      this.ctx.toast(`프로젝트: ${e.message}`);
    }
  }

  async refresh() {
    let answer;
    try {
      answer = await post({ op: 'project', action: 'list' });
    } catch (e) {
      return this.ctx.toast(`프로젝트 목록: ${e.message}`);
    }
    if (!answer.ok) {
      this.list.innerHTML = `<li class="muted">${this.ctx.escapeHtml(answer.error || '목록을 읽지 못했습니다')}</li>`;
      return;
    }
    const esc = this.ctx.escapeHtml;
    const when = (s) => new Date(s * 1000).toLocaleString('ko-KR', { dateStyle: 'short', timeStyle: 'short' });
    this.list.innerHTML = answer.projects.length
      ? answer.projects
          .map((p) => `<li data-name="${esc(p.name)}" class="${p.current ? 'current' : ''}" title="${p.current ? '지금 열린 프로젝트' : '눌러서 열기'}"><span>${esc(p.name)}</span><span class="muted num">${p.current ? '열림 · ' : ''}${when(p.modified)}</span></li>`)
          .join('')
      : '<li class="muted">아직 저장된 프로젝트가 없습니다</li>';
    this.demos.innerHTML = (answer.demos ?? [])
      .map((d) => `<li data-demo="${esc(d.name)}" title="눌러서 사본을 만들어 열기"><span>${esc(d.name)}</span><span class="muted">${esc(d.about)}</span></li>`)
      .join('');
  }
}
