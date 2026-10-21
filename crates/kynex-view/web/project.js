// Projects: the one open (the `project` event), and making a new one,
// opening another or saving this one under another name — by name, in the
// projects folder KYNEX keeps; and the demos and samples that come with
// KYNEX, each opened as a copy of it in that folder.

async function post(request) {
  const response = await fetch('/robot', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

// A question over the page: its title, what it says, a name to type if it
// asks for one, and its choices; the choice made (and the name), or null
// if it was let go — Esc, ✕, or a click beside it.
function choose({ title, text, name = null, choices }) {
  return new Promise((resolve) => {
    const box = document.createElement('div');
    box.className = 'modal';
    box.innerHTML = `<div class="modal-card" role="dialog" aria-modal="true">
      <div class="modal-head"><b></b><button class="icon-btn small" data-close aria-label="닫기">✕</button></div>
      <p class="modal-text"></p>
      ${name !== null ? '<input class="modal-name" type="text" maxlength="60" spellcheck="false" aria-label="이름">' : ''}
      <div class="modal-actions"></div>
    </div>`;
    box.querySelector('b').textContent = title;
    box.querySelector('.modal-text').textContent = text;
    const field = box.querySelector('.modal-name');
    if (field) field.value = name;
    const actions = box.querySelector('.modal-actions');
    for (const [value, label, kind] of choices) {
      const b = document.createElement('button');
      b.className = `text-btn${kind ? ` ${kind}` : ''}`;
      b.textContent = label;
      b.addEventListener('click', () => done(value));
      actions.appendChild(b);
    }
    const done = (value) => {
      box.remove();
      window.removeEventListener('keydown', key, true);
      resolve(value === null ? null : { value, name: field?.value.trim() ?? '' });
    };
    const key = (e) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopImmediatePropagation();
        done(null);
      } else if (e.key === 'Enter' && !e.isComposing) {
        e.preventDefault();
        e.stopImmediatePropagation();
        done(choices[0][0]);
      }
    };
    window.addEventListener('keydown', key, true);
    box.addEventListener('pointerdown', (e) => {
      if (e.target === box) done(null);
    });
    box.querySelector('[data-close]').addEventListener('click', () => done(null));
    document.body.appendChild(box);
    (field ?? actions.querySelector('button'))?.focus();
    field?.select();
  });
}

export class ProjectPanel {
  // ctx: { toast, escapeHtml }
  constructor(ctx) {
    this.ctx = ctx;
    this.panel = document.getElementById('project-panel');
    this.name = document.getElementById('project-new-name');
    this.list = document.getElementById('project-list');
    this.demos = document.getElementById('project-demos');
    this.samples = document.getElementById('project-samples');
    this.state = null;
    // A new project: its name asked, and what becomes of this one.
    document.getElementById('project-new').addEventListener('click', async () => {
      const typed = this.name.value.trim();
      const taken = new Set([...this.list.querySelectorAll('li[data-name]')].map((li) => li.dataset.name));
      let n = 1;
      while (taken.has(`새 프로젝트 ${n}`)) n++;
      const asked = await choose({
        title: '새 프로젝트',
        text: `새 프로젝트의 이름을 넣으세요. 지금 프로젝트(${this.state?.name ?? ''})는 저장하고 넘어갈 수도, 마지막 저장 때로 되돌리고 넘어갈 수도 있습니다.`,
        name: typed || `새 프로젝트 ${n}`,
        choices: [
          ['keep', '저장하고 만들기', 'primary'],
          ['discard', '저장하지 않고 만들기'],
          [null, '취소', 'quiet'],
        ],
      });
      if (!asked) return;
      if (!asked.name) return this.ctx.toast('새 프로젝트의 이름을 넣으세요');
      this.act('new', asked.name, `새 프로젝트 "${asked.name}"을(를) 열었습니다`, { keep: asked.value === 'keep' });
    });
    document.getElementById('project-save').addEventListener('click', () =>
      this.act('save', '', '저장했습니다. 다른 프로젝트를 저장하지 않고 열면 이때로 돌아옵니다'),
    );
    document.getElementById('project-save-as').addEventListener('click', () => {
      const name = this.name.value.trim();
      if (!name) return this.ctx.toast('저장할 이름을 넣으세요');
      this.act('save_as', name, `"${name}"(으)로 저장했습니다. 이제부터 이 파일에 저장됩니다`);
    });
    this.name.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') document.getElementById('project-new').click();
    });
    this.list.addEventListener('click', async (e) => {
      const li = e.target.closest('li[data-name]');
      if (!li || li.classList.contains('current')) return;
      const name = li.dataset.name;
      const keep = await this.leaving(`프로젝트 "${name}"을(를) 엽니다.`);
      if (keep === null) return;
      this.act('open', name, `프로젝트 "${name}"을(를) 열었습니다`, { keep });
    });
    // A demo or a sample: opened as a copy.
    const openDemo = async (e) => {
      const li = e.target.closest('li[data-demo]');
      if (!li) return;
      const name = li.dataset.demo;
      const keep = await this.leaving(`예제 "${name}"의 사본을 프로젝트 폴더에 만들어 엽니다.`);
      if (keep === null) return;
      try {
        const answer = await post({ op: 'project', action: 'demo', name, keep });
        if (!answer.ok) return this.ctx.toast(answer.error || '열 수 없습니다');
        this.ctx.toast(`예제를 "${answer.name}"(으)로 열었습니다. 바꾼 것은 이 사본에 저장됩니다`);
        this.refresh();
      } catch (err) {
        this.ctx.toast(`예제: ${err.message}`);
      }
    };
    this.demos.addEventListener('click', openDemo);
    this.samples.addEventListener('click', openDemo);
  }

  // Whether this project is saved as another is opened (`what`): true,
  // false — put back as last saved —, or null, not opened after all.
  async leaving(what) {
    const asked = await choose({
      title: '다른 프로젝트 열기',
      text: `${what} 지금 프로젝트(${this.state?.name ?? ''})는 어떻게 할까요? 저장하지 않으면 마지막 저장 때로 돌아갑니다. 실행 중인 프로그램과 PLC 연결은 멈춥니다.`,
      choices: [
        ['keep', '저장하고 열기', 'primary'],
        ['discard', '저장하지 않고 열기'],
        [null, '취소', 'quiet'],
      ],
    });
    return asked ? asked.value === 'keep' : null;
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

  async act(action, name, done, more = {}) {
    try {
      const answer = await post({ op: 'project', action, name, ...more });
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
    this.samples.innerHTML = (answer.samples ?? [])
      .map(
        (d) =>
          `<li data-demo="${esc(d.name)}" title="눌러서 사본을 만들어 열기"><span>${esc(d.name.replace(/^샘플 /, ''))}</span><span class="muted">${esc(d.about)}</span></li>`,
      )
      .join('');
  }
}
