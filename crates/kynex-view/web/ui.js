// What every panel does the same way: asking KYNEX (`POST /robot`), and
// telling whether the person is typing in a box that is about to be drawn
// anew. One place, so that a refusal reads the same wherever it comes from.

import { tFor } from './i18n.js';

const t = tFor('page');

// Why KYNEX answered as it did, when it is not an answer: in words, not
// "Unexpected token" from reading a refusal page as JSON.
function failure(status) {
  return (
    {
      403: t('이 기기에서는 할 수 없습니다 (공유가 꺼졌거나 접속 코드가 바뀌었습니다)'),
      413: t('보내려는 것이 너무 큽니다'),
      503: t('KYNEX가 바빠 제때 답하지 못했습니다. 잠시 뒤 다시 하세요'),
    }[status] ?? `HTTP ${status}`
  );
}

// `request` asked of KYNEX; its answer, or an error thrown with why.
export async function post(request) {
  const response = await fetch('/robot', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error(failure(response.status));
  try {
    return await response.json();
  } catch {
    throw new Error(t('KYNEX의 답을 읽지 못했습니다'));
  }
}

// The same, never thrown: `{ok: false, error}` when it did not go.
export async function ask(request) {
  try {
    return await post(request);
  } catch (err) {
    return { ok: false, error: String(err.message ?? err) };
  }
}

// Whether a field in `box` is being typed in or chosen from: drawn anew,
// what was typed is lost and a tablet's keyboard closes.
export function editing(box) {
  const at = document.activeElement;
  return !!box && !!at && at !== document.body && box.contains(at) && at.matches('input, select, textarea');
}
