const invoke = window.__TAURI__.core.invoke;
const listen = window.__TAURI__.event.listen;

// Tauri 2's built-in data-tauri-drag-region handler only matches the exact event.target,
// so child elements (chip text, etc.) don't trigger drag. This adds closest() semantics.
document.addEventListener('mousedown', (e) => {
  if (e.button !== 0) return;
  if (e.target.closest('button, input, textarea, select, a, .no-drag')) return;
  if (!e.target.closest('[data-tauri-drag-region]')) return;
  invoke('plugin:window|start_dragging').catch((err) => console.error('start_dragging failed', err));
});

const deskGrid = document.getElementById('deskGrid');
const chipAgents = document.getElementById('chipAgents');
const chipWorking = document.getElementById('chipWorking');
const chipIdle = document.getElementById('chipIdle');
const chipDone = document.getElementById('chipDone');
const refreshButton = document.getElementById('refreshButton');
const closeButton = document.getElementById('closeButton');
const hookBanner = document.getElementById('hookBanner');
const installHookButton = document.getElementById('installHookButton');
const dismissBannerButton = document.getElementById('dismissBannerButton');
const selectedName = document.getElementById('selectedName');
const selectedSub = document.getElementById('selectedSub');
const selectedPrompt = document.getElementById('selectedPrompt');
const selectedBranch = document.getElementById('selectedBranch');
const selectedIdle = document.getElementById('selectedIdle');
const selectedTurns = document.getElementById('selectedTurns');
const activityLog = document.getElementById('activityLog');

let selectedSessionId = null;
let lastSessions = [];

function formatIdle(seconds) {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

function formatTime(ms) {
  if (!ms) return '';
  const d = new Date(ms);
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

const WORKING_VARIANTS = [
  './assets/desks/desk-working.svg',
  './assets/desks/desk-character-variant-indigo.svg',
  './assets/desks/desk-character-variant-mint.svg',
  './assets/desks/desk-character-variant-rose.svg',
];

function hashStr(s) {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  }
  return Math.abs(h);
}

function deskArtSrc(session) {
  if (session.status === 'done') return './assets/desks/desk-done.svg';
  if (session.status === 'idle') return './assets/desks/desk-idle.svg';
  if (session.status === 'working') {
    const idx = hashStr(session.sessionId || '') % WORKING_VARIANTS.length;
    return WORKING_VARIANTS[idx];
  }
  return './assets/desks/desk-empty.svg';
}

function statusLabel(status) {
  if (status === 'working') return '작업 중';
  if (status === 'idle') return '대기';
  if (status === 'done') return 'DONE';
  return 'EMPTY';
}

const STATUS_TOOLTIPS = {
  working: 'WORKING — 진행 중 (도구 실행 중 또는 <5분 내 활동)',
  idle: 'IDLE — 사용자의 다음 입력 대기 중 (Claude가 답을 마치고 멈춤)',
  done: 'DONE — Stop hook 또는 task_complete 신호로 명시적 종료',
};

function branchDisplay(s) {
  if (!s.gitBranch) return '';
  if (s.isDetached || s.gitBranch === 'HEAD') {
    return '<span class="desk__branch desk__branch--detached">(detached)</span>';
  }
  const worktree = s.isWorktree ? ' <span class="desk__worktree" title="git worktree">🌿</span>' : '';
  return `<div class="desk__branch">${escapeHtml(s.gitBranch)}${worktree}</div>`;
}

function renderDesks(sessions) {
  deskGrid.innerHTML = '';
  if (sessions.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'desk desk--empty';
    empty.innerHTML = `<div class="desk__badge desk__badge--empty">Empty</div><div class="desk__name">No active sessions in last 24h</div>`;
    deskGrid.appendChild(empty);
    return;
  }

  // Group by projectRoot (git toplevel) into rooms.
  const rooms = new Map();
  const standalone = [];
  for (const s of sessions) {
    if (s.projectRoot) {
      if (!rooms.has(s.projectRoot)) rooms.set(s.projectRoot, []);
      rooms.get(s.projectRoot).push(s);
    } else {
      standalone.push(s);
    }
  }
  // Render rooms with multiple desks; singletons stay as standalone for cleaner layout.
  const multi = [];
  for (const [root, list] of rooms.entries()) {
    if (list.length > 1) multi.push([root, list]);
    else standalone.push(list[0]);
  }
  multi.sort((a, b) => b[1][0].lastActivityAtMs - a[1][0].lastActivityAtMs);
  standalone.sort((a, b) => b.lastActivityAtMs - a.lastActivityAtMs);

  for (const [, list] of multi) {
    const room = document.createElement('div');
    room.className = 'room';
    const first = list[0];
    room.innerHTML = `
      <div class="room__header">
        <span class="room__name">${escapeHtml(first.projectName)}</span>
        <span class="room__count">${list.length} agents</span>
      </div>
      <div class="room__desks"></div>
    `;
    const roomDesks = room.querySelector('.room__desks');
    for (const s of list) roomDesks.appendChild(buildDeskEl(s));
    deskGrid.appendChild(room);
  }
  for (const s of standalone) {
    deskGrid.appendChild(buildDeskEl(s));
  }
}

function buildDeskEl(s) {
  const desk = document.createElement('div');
  const provider = s.provider || 'claude';
  desk.className = `desk desk--${s.status} desk--${provider}`;
  if (s.sessionId === selectedSessionId) desk.classList.add('desk--selected');
  desk.dataset.sessionId = s.sessionId;
  const prompt = s.lastUserPrompt ? `<div class="desk__prompt">${escapeHtml(s.lastUserPrompt)}</div>` : `<div class="desk__prompt" style="color:#99a2b6">—</div>`;
  desk.innerHTML = `
    <div class="desk__badge desk__badge--${s.status}" title="${STATUS_TOOLTIPS[s.status] || ''}">${statusLabel(s.status)}</div>
    <div class="desk__provider desk__provider--${provider}">${provider}</div>
    <div class="desk__art"><img src="${deskArtSrc(s)}" alt="${s.status}"></div>
    <div class="desk__name">${escapeHtml(s.projectName)}</div>
    ${branchDisplay(s)}
    ${prompt}
    <div class="desk__idle">${formatIdle(s.idleSeconds)} idle</div>
  `;
  desk.addEventListener('click', () => {
    selectedSessionId = s.sessionId;
    renderState({ sessions: lastSessions });
    renderSelected();
  });
  desk.addEventListener('dblclick', () => {
    if (!s.cwd) return;
    invoke('focus_session_window', { cwd: s.cwd })
      .then((ok) => { if (!ok) console.warn('no matching IDE window for', s.cwd); })
      .catch((err) => console.error('focus_session_window failed', err));
  });
  desk.title = 'Click to select · Double-click to focus IDE';
  return desk;
}

function escapeHtml(s) {
  if (s == null) return '';
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

function renderChips(sessions) {
  chipAgents.textContent = sessions.length;
  chipWorking.textContent = sessions.filter((s) => s.status === 'working').length;
  chipIdle.textContent = sessions.filter((s) => s.status === 'idle').length;
  chipDone.textContent = sessions.filter((s) => s.status === 'done').length;
}

function renderSelected() {
  const s = lastSessions.find((x) => x.sessionId === selectedSessionId);
  if (!s) {
    selectedName.textContent = '—';
    selectedSub.textContent = 'Click a desk to inspect';
    selectedPrompt.textContent = '—';
    selectedBranch.textContent = '—';
    selectedIdle.textContent = '—';
    selectedTurns.textContent = '—';
    return;
  }
  selectedName.textContent = s.projectName;
  selectedSub.textContent = `${(s.provider || 'claude').toUpperCase()} · ${statusLabel(s.status)} · ${s.cwd || ''}`;
  selectedPrompt.textContent = s.lastUserPrompt || '—';
  selectedBranch.textContent = s.gitBranch || '—';
  selectedIdle.textContent = formatIdle(s.idleSeconds);
  selectedTurns.textContent = String(s.turnCount);
}

function renderLog(log) {
  activityLog.innerHTML = '';
  for (const entry of log) {
    const li = document.createElement('li');
    li.className = `log__item log__item--${entry.kind}`;
    li.innerHTML = `
      <div class="log__time">${formatTime(entry.timestampMs)}</div>
      <div class="log__head">${escapeHtml(entry.projectName)}</div>
      <div class="log__text">${escapeHtml(entry.text)}</div>
    `;
    activityLog.appendChild(li);
  }
}

function renderState(state) {
  if (state.sessions) {
    lastSessions = state.sessions;
    if (!selectedSessionId && lastSessions.length > 0) {
      selectedSessionId = lastSessions[0].sessionId;
    }
    renderDesks(lastSessions);
    renderChips(lastSessions);
    renderSelected();
  }
  if (state.log) {
    renderLog(state.log);
  }
}

async function refresh() {
  try {
    const state = await invoke('get_agent_office_state');
    renderState(state);
  } catch (err) {
    console.error('get_agent_office_state failed', err);
  }
}

async function refreshHookStatus() {
  try {
    const summary = await invoke('connector_status');
    const claude = (summary.providers || []).find((p) => p.provider === 'claude');
    const dismissed = sessionStorage.getItem('officeBannerDismissed') === '1';
    console.debug('[agent-office] claude hook status', claude);
    if (claude && !claude.installed && !dismissed) {
      hookBanner.hidden = false;
    } else {
      hookBanner.hidden = true;
    }
  } catch (err) {
    console.error('connector_status failed', err);
  }
}

installHookButton.addEventListener('click', async () => {
  installHookButton.disabled = true;
  installHookButton.textContent = '설치 중…';
  try {
    await invoke('install_connector', { provider: 'claude' });
    hookBanner.hidden = true;
  } catch (err) {
    console.error('install failed', err);
    installHookButton.textContent = '실패 — 재시도';
    installHookButton.disabled = false;
    return;
  }
});

dismissBannerButton.addEventListener('click', () => {
  hookBanner.hidden = true;
  sessionStorage.setItem('officeBannerDismissed', '1');
});

refreshButton.addEventListener('click', () => {
  refresh();
  refreshHookStatus();
});
closeButton.addEventListener('click', () => {
  invoke('close_agent_office').catch((err) => console.error('close failed', err));
});

listen('agent-office:update', (e) => renderState(e.payload));
refresh();
refreshHookStatus();
