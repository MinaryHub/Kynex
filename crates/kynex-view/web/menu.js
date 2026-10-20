// A menu that opens where the pointer is — on a right click in the 3D
// view, on the tree, on the flow chart — and goes when something is
// chosen, when anything outside it is pressed, or on Esc. An entry may
// open a second column of entries beside it (a submenu).
//
// Entries: { label, hint?, danger?, disabled?, checked?, sub?: [...], run?() }
// or { sep: true } or { title: '...' }.

let open = null;

export function closeMenu() {
  if (!open) return;
  open.remove();
  open = null;
}

export function menuOpen() {
  return open != null;
}

export function showMenu(x, y, entries) {
  closeMenu();
  const box = document.createElement('div');
  box.className = 'ctx-menu';
  box.setAttribute('role', 'menu');
  box.appendChild(column(entries, box));
  document.body.appendChild(box);
  open = box;
  placeAt(box.firstElementChild, x, y);
}

function column(entries, root) {
  const col = document.createElement('div');
  col.className = 'ctx-col';
  for (const entry of entries) {
    if (entry.sep) {
      col.appendChild(Object.assign(document.createElement('div'), { className: 'ctx-sep' }));
      continue;
    }
    if (entry.title != null) {
      const t = document.createElement('div');
      t.className = 'ctx-title';
      t.textContent = entry.title;
      col.appendChild(t);
      continue;
    }
    const row = document.createElement('button');
    row.type = 'button';
    row.className = 'ctx-item';
    row.setAttribute('role', 'menuitem');
    if (entry.danger) row.classList.add('danger');
    if (entry.disabled) row.disabled = true;
    const label = document.createElement('span');
    label.className = 'ctx-label';
    label.textContent = (entry.checked ? '✓ ' : '') + entry.label;
    row.appendChild(label);
    if (entry.hint) {
      const hint = document.createElement('span');
      hint.className = 'ctx-hint';
      hint.textContent = entry.hint;
      row.appendChild(hint);
    }
    if (entry.sub) {
      row.classList.add('has-sub');
      const more = document.createElement('span');
      more.className = 'ctx-more';
      more.textContent = '▸';
      row.appendChild(more);
      const show = () => {
        for (const other of root.querySelectorAll('.ctx-col.sub')) {
          if (other.dataset.owner !== row.dataset.id) other.remove();
        }
        for (const other of col.querySelectorAll('.ctx-item.open')) other.classList.remove('open');
        if (root.querySelector(`.ctx-col.sub[data-owner="${row.dataset.id}"]`)) return;
        row.classList.add('open');
        const sub = column(entry.sub.length ? entry.sub : [{ label: '없음', disabled: true }], root);
        sub.classList.add('sub');
        sub.dataset.owner = row.dataset.id;
        root.appendChild(sub);
        const r = row.getBoundingClientRect();
        placeAt(sub, r.right - 2, r.top - 6, r.left + 2);
      };
      row.dataset.id = `m${Math.random().toString(36).slice(2, 8)}`;
      row.addEventListener('pointerenter', show);
      row.addEventListener('click', show);
    } else {
      row.addEventListener('pointerenter', () => {
        // Over a plain entry, any open submenu of this column goes.
        if (col.classList.contains('sub')) return;
        for (const other of root.querySelectorAll('.ctx-col.sub')) other.remove();
        for (const other of col.querySelectorAll('.ctx-item.open')) other.classList.remove('open');
      });
      row.addEventListener('click', () => {
        closeMenu();
        entry.run?.();
      });
    }
    col.appendChild(row);
  }
  return col;
}

// `col` at (x, y) on the screen, kept inside the window: a submenu that
// would run off the right edge opens to the left of `leftEdge` instead.
function placeAt(col, x, y, leftEdge = null) {
  col.style.left = '0px';
  col.style.top = '0px';
  const w = col.offsetWidth;
  const h = col.offsetHeight;
  let left = x;
  let top = y;
  if (left + w > window.innerWidth - 6) left = leftEdge != null ? leftEdge - w : Math.max(6, window.innerWidth - w - 6);
  if (top + h > window.innerHeight - 6) top = Math.max(6, window.innerHeight - h - 6);
  col.style.left = `${Math.max(6, left)}px`;
  col.style.top = `${Math.max(6, top)}px`;
}

document.addEventListener(
  'pointerdown',
  (e) => {
    if (open && !open.contains(e.target)) closeMenu();
  },
  true,
);
window.addEventListener(
  'keydown',
  (e) => {
    if (e.key === 'Escape' && open) {
      closeMenu();
      e.preventDefault();
      e.stopImmediatePropagation();
    }
  },
  true,
);
window.addEventListener('blur', closeMenu);
window.addEventListener('resize', closeMenu);
