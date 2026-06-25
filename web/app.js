// nockd dashboard — vanilla ES module, a thin client of the /api/v1 Control API.
// No build step; served embedded from the nockd binary (design/API-INTEGRATION.md §6).

const app = document.getElementById('app');
let dispose = () => {};

// ---- helpers ----
const $ = (html) => { const t = document.createElement('template'); t.innerHTML = html.trim(); return t.content.firstElementChild; };
const esc = (s) => String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
// Strip ANSI/VT100 escape sequences (used for status grep parity; logs render color).
const stripAnsi = (s) => String(s ?? '').replace(/\x1b\[[0-9;]*[A-Za-z]/g, '');

// Highlight NockApp log verbs on top of whatever ANSI coloring the line carries.
const colorVerbs = (s) => s.replace(/\b(poke|peek|chain|snap)\b/g, (m) => `<span class="v-${m}">${m}</span>`);

// Log ANSI palette — a small expansion of the Bauhaus set, scoped to the log panel only
// (readable on the dark ink ground). Standard SGR fg codes → harmonious hues.
const ANSI_FG = {
  30: '#6b6557', 31: '#e0654c', 32: '#8fb56a', 33: '#efc02a',
  34: '#6f8fd0', 35: '#c98fd0', 36: '#6fc6c0', 37: '#f3ede1',
  90: '#9a937f', 91: '#e0654c', 92: '#a6cf86', 93: '#f5d35e',
  94: '#8aa6e0', 95: '#d6a6dd', 96: '#8fd6d0', 97: '#ffffff',
};

// Convert a log line's ANSI SGR sequences into styled spans, applying verb highlighting
// within each run. Plain (non-ANSI) lines still get verb coloring.
function ansiToHtml(line) {
  line = String(line ?? '');
  let cur = { color: null, bold: false, dim: false };
  const open = () => {
    const s = [];
    if (cur.color) s.push(`color:${cur.color}`);
    if (cur.bold) s.push('font-weight:700');
    if (cur.dim) s.push('opacity:.65');
    // ANSI italic (SGR 3) is intentionally NOT rendered: snake_case identifiers like
    // `new_heaviest_chain` in italic read as Markdown `_emphasis_`. Keep color/bold/dim.
    return s.length ? `<span style="${s.join(';')}">` : '<span>';
  };
  const re = /\x1b\[([0-9;]*)m/g;
  let html = '', last = 0, m;
  const emit = (text) => { if (text) html += open() + colorVerbs(esc(text)) + '</span>'; };
  while ((m = re.exec(line)) !== null) {
    emit(line.slice(last, m.index));
    const codes = m[1] === '' ? [0] : m[1].split(';').map(Number);
    for (const c of codes) {
      if (c === 0) cur = { color: null, bold: false, dim: false };
      else if (c === 1) cur.bold = true;
      else if (c === 2) cur.dim = true;
      else if (c === 22) { cur.bold = false; cur.dim = false; }
      else if (c === 39) cur.color = null;
      else if (ANSI_FG[c]) cur.color = ANSI_FG[c];
    }
    last = re.lastIndex;
  }
  emit(line.slice(last));
  return html;
}
const STATUSES = ['running', 'degraded', 'crashing', 'stopped'];
const statusClass = (s) => (STATUSES.includes(s) ? s : 'stopped');
const glyph = (s) => `<span class="glyph ${statusClass(s)}"></span>`;
const shortHash = (h) => (h ? `${h.slice(0, 4)}…${h.slice(-2)}` : '—');
const metricStr = (a) => (a.status_line ? `${a.status_label ? a.status_label + ' ' : ''}${a.status_line}` : '');
// Build attestation indicator (DESIGN VERIFY): verified ● blue, drift ▼ red, unverified ■ ink.
const vfy = (v) => v === 'verified' ? `<span class="vfy ok">✓ verified</span>`
  : v === 'drift' ? `<span class="vfy bad">▼ drift</span>`
  : `<span class="vfy muted">unverified</span>`;
// An HTTP app's "open app" link, derived from its declared port + the host you're viewing
// nockd on (never a stored URL). null if the app serves no port.
const appLink = (a) => (a.port ? `${location.protocol}//${location.hostname}:${a.port}/` : null);
// App icon <img>, served from /icon (cache-busted by updated_at so a redeploy refreshes it).
// Empty string when the app has no icon — callers fall back to the status glyph.
const appIcon = (a, cls = '') => a.has_icon
  ? `<img class="appicon ${cls}" src="/api/v1/apps/${encodeURIComponent(a.name)}/icon?v=${a.updated_at}" alt="" loading="lazy">`
  : '';

// Human-readable byte size (RSS). Binary units, one decimal past KB.
function fmtBytes(n) {
  if (n == null) return '—';
  if (n < 1024) return `${n} B`;
  const u = ['KB', 'MB', 'GB', 'TB'];
  let v = n / 1024, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(1)} ${u[i]}`;
}

function fmtUptime(s) {
  if (s == null) return '—';
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

async function getJSON(path) {
  const r = await fetch(path);
  if (!r.ok) throw new Error(`${r.status} ${await r.text()}`);
  return r.json();
}
const post = (path) => fetch(path, { method: 'POST' });

function setActiveNav(hash) {
  const route = hash === '#/' || hash.startsWith('#/app/') ? 'fleet' : hash.slice(2);
  document.querySelectorAll('#nav a').forEach((a) => a.classList.toggle('active', a.dataset.route === route));
}

function banner(msg) {
  let b = document.getElementById('disc');
  if (!b) { b = $(`<div id="disc" class="disconnect"></div>`); document.body.prepend(b); }
  b.textContent = msg;
}
function clearBanner() { document.getElementById('disc')?.remove(); }

// ---- Fleet view (table + tiles) ----
function fleetView() {
  let mode = localStorage.getItem('nockd.fleetMode') || 'table';
  let timer = null, es = null;

  async function render() {
    let apps;
    try { apps = await getJSON('/api/v1/apps'); clearBanner(); }
    catch (e) { banner(`daemon unreachable — ${e.message}`); return; }

    const count = (s) => apps.filter((a) => a.status === s).length;
    const stopped = count('stopped');
    app.innerHTML = '';
    app.append($(`
      <div class="stats">
        <div class="stat"><div class="num">${apps.length}</div><div class="lab">Apps</div></div>
        <div class="stat blue"><div class="num">${count('running')}</div><div class="lab">Running</div></div>
        <div class="stat yellow"><div class="num">${count('degraded')}</div><div class="lab">Degraded</div></div>
        <div class="stat red"><div class="num">${count('crashing')}</div><div class="lab">Crashing · ${stopped} stopped</div></div>
      </div>`));

    const summary = `${apps.length} apps · ${count('running')} running · ${count('degraded')} degraded · ${count('crashing')} crashing · ${stopped} stopped`;
    const bar = $(`
      <div class="toolbar">
        <div class="summary">${summary}</div>
        <div class="seg">
          <button data-m="table" class="${mode === 'table' ? 'active' : ''}">Table</button>
          <button data-m="tiles" class="${mode === 'tiles' ? 'active' : ''}">Tiles</button>
        </div>
      </div>`);
    bar.querySelectorAll('button').forEach((b) =>
      b.onclick = () => { mode = b.dataset.m; localStorage.setItem('nockd.fleetMode', mode); render(); });
    app.append(bar);

    app.append(mode === 'table' ? fleetTable(apps) : fleetTiles(apps));

    app.append($(`
      <div class="legend">
        <span class="item">${glyph('running')} Running</span>
        <span class="item">${glyph('degraded')} Degraded</span>
        <span class="item">${glyph('crashing')} Crashing</span>
        <span class="item">${glyph('stopped')} Stopped</span>
        <span class="recon">Reconciler · live</span>
      </div>`));
  }

  function fleetTable(apps) {
    const wrap = $(`<table class="table"><thead><tr>
      <th></th><th>App</th><th>Artifact</th><th>Endpoint</th><th>Uptime</th>
      <th>Rst</th><th>Health</th><th>Verified</th><th>Metric</th><th>Status</th></tr></thead><tbody></tbody></table>`);
    const tb = wrap.querySelector('tbody');
    if (!apps.length) { app.append($(`<div class="empty-tile">No apps deployed. Use <b>nockd deploy</b>.</div>`)); return wrap; }
    for (const a of apps) {
      const idle = a.status === 'stopped';
      const rst = a.restart_count > 3 ? `<span class="rst-hot">${a.restart_count}</span>` : a.restart_count;
      const tr = $(`<tr class="${idle ? 'idle' : ''}">
        <td>${glyph(a.status)}</td>
        <td class="cell-app">${appIcon(a)}${esc(a.name)}${appLink(a) ? ` <a class="relay-mini" href="${esc(appLink(a))}" target="_blank" rel="noopener" title="Open ${esc(appLink(a))}">↗</a>` : ''}</td>
        <td class="mono">${shortHash(a.artifact_hash)}</td>
        <td class="mono">${esc(a.endpoint_name || '—')}</td>
        <td class="mono">${fmtUptime(a.uptime_s)}</td>
        <td class="mono">${rst}</td>
        <td class="mono muted">${esc(a.health)}</td>
        <td>${vfy(a.verified)}</td>
        <td class="mono">${a.status_line ? esc(metricStr(a)) : '—'}</td>
        <td><span class="status-word ${statusClass(a.status)}">${esc(a.status)}</span></td>
      </tr>`);
      // Let the relay link open in a new tab without also navigating the row to detail.
      tr.onclick = (e) => { if (e.target.closest('a')) return; location.hash = `#/app/${encodeURIComponent(a.name)}`; };
      tb.append(tr);
    }
    return wrap;
  }

  function fleetTiles(apps) {
    const grid = $(`<div class="tiles"></div>`);
    for (const a of apps) {
      const idle = a.status === 'stopped';
      const t = $(`<div class="tile ${idle ? 'idle' : ''}">
        <div class="band ${statusClass(a.status)}">
          <span class="left">${glyph(a.status)} ${esc(a.status)}</span>
          <span>${a.status_line ? esc(a.status_line) : (a.status === 'crashing' ? `${a.restart_count} rst` : fmtUptime(a.uptime_s))}</span>
        </div>
        <div class="body">
          <div class="tname">${appIcon(a)}${esc(a.name)}${appLink(a) ? ` <a class="relay-mini" href="${esc(appLink(a))}" target="_blank" rel="noopener" title="Open ${esc(appLink(a))}">↗</a>` : ''}</div>
          <div class="meta">${esc(a.artifact_hash ? a.artifact_hash.slice(0, 18) + '…' : '—')}</div>
          <div class="meta">${esc(a.endpoint_name || 'no endpoint')}</div>
          <div class="tfoot"><span>up ${fmtUptime(a.uptime_s)}</span>${vfy(a.verified)}</div>
        </div>
      </div>`);
      // Let the relay ↗ open in a new tab without navigating the tile to detail.
      t.onclick = (e) => { if (e.target.closest('a')) return; location.hash = `#/app/${encodeURIComponent(a.name)}`; };
      grid.append(t);
    }
    grid.append($(`<div class="tile deploy"><span class="plus">+ DEPLOY APP</span></div>`));
    return grid;
  }

  render();
  timer = setInterval(render, 2500);
  // SSE: refresh promptly when the daemon emits a new event.
  try { es = new EventSource('/api/v1/events'); es.onmessage = () => render(); es.onerror = () => {}; } catch (_) {}
  return () => { clearInterval(timer); es && es.close(); };
}

// ---- App detail ----
function detailView(name) {
  let es = null, evTimer = null;

  async function render() {
    let a;
    try { a = await getJSON(`/api/v1/apps/${encodeURIComponent(name)}`); clearBanner(); }
    catch (e) { banner(`unreachable — ${e.message}`); return; }

    app.innerHTML = '';
    const head = $(`
      <div class="detail-head">
        <a class="back" href="#/">‹ FLEET</a>
        ${glyph(a.status)}
        ${appIcon(a, 'big')}
        <h1>${esc(a.name)}</h1>
        <span class="sub">${esc(a.restart_policy)} · up ${fmtUptime(a.uptime_s)} · pid ${a.pid ?? '—'}${a.status_line ? ' · ' + esc(metricStr(a)) : ''}</span>
        <div class="actions">
          ${appLink(a) ? `<a class="btn relay" href="${esc(appLink(a))}" target="_blank" rel="noopener" title="Open ${esc(appLink(a))}">Open app ↗</a>` : ''}
          <button class="btn" data-act="reload" title="${a.manifest_path ? `Re-read ${esc(a.manifest_path)} and re-apply its config (no rebuild)` : 'Deploy once with `nockd deploy -f nockd.toml` to enable Reload'}">Reload</button>
          <button class="btn" data-act="rollback" data-confirm="Roll back ${esc(a.name)} to the previous artifact (${a.prev_artifact ? shortHash(a.prev_artifact) : '—'})?" title="${a.prev_artifact ? `Revert to ${shortHash(a.prev_artifact)} and restart` : 'No previous artifact to roll back to yet'}">Rollback</button>
          <button class="btn" data-act="restart">Restart</button>
          <button class="btn" data-act="start">Start</button>
          <button class="btn danger" data-act="stop">Stop</button>
        </div>
      </div>`);
    head.querySelectorAll('[data-act]').forEach((b) =>
      b.onclick = async () => {
        // Confirm destructive/state-changing actions (rollback) before firing.
        if (b.dataset.confirm && !confirm(b.dataset.confirm)) return;
        const r = await post(`/api/v1/apps/${encodeURIComponent(name)}/${b.dataset.act}`);
        // Surface failures (e.g. Reload with no manifest, Rollback with no history).
        if (!r.ok) { banner(`${b.dataset.act} failed — ${await r.text()}`); setTimeout(clearBanner, 6000); }
        setTimeout(render, 400);
      });
    app.append(head);

    const body = $(`<div class="grid2">
      <div class="col">
        <div class="panel log" id="log">
          <div class="following"><span class="dot"></span> FOLLOWING · LIVE LOG</div>
          <pre id="logpre"></pre>
        </div>
        <div class="panel"><h3>Event timeline</h3><div class="timeline" id="tl"></div></div>
      </div>
      <div class="col">
        <div class="panel">
          <h3>Artifact</h3>
          <div class="kv"><span class="k">current</span> ${esc(a.artifact_hash || '—')}</div>
          ${a.kernel_hash ? `<div class="kv"><span class="k">kernel</span> ${esc(a.kernel_hash)}</div>` : `<div class="kv muted">kernel embedded in binary</div>`}
          <div class="kv"><span class="k">verified</span> <span class="tag" style="color:var(--ink-muted)">${esc(a.verified)}</span></div>
          ${a.prev_artifact ? `<div class="kv"><span class="k">rollback to</span> ${esc(shortHash(a.prev_artifact))}</div>` : ''}
          <div class="hist" id="hist"><div class="kv muted">deploy history…</div></div>
        </div>
        <div class="panel attach">
          <h3>Attachment</h3>
          <div class="kv" style="color:var(--cream)"><span class="k" style="color:rgba(243,237,225,.7)">endpoint</span> ${esc(a.endpoint_name || '— none —')}</div>
          <div class="kv" style="color:var(--cream)"><span class="k" style="color:rgba(243,237,225,.7)">health</span> ${esc(a.health)}</div>
          <div class="kv" style="color:var(--cream)"><span class="k" style="color:rgba(243,237,225,.7)">page</span> ${appLink(a) ? `<a class="relay-inline" href="${esc(appLink(a))}" target="_blank" rel="noopener">${esc(appLink(a))} ↗</a>` : '— none —'}</div>
        </div>
        <div class="panel"><h3>Resources</h3>
          <div class="kv"><span class="k">cpu</span> ${a.cpu_pct != null ? a.cpu_pct.toFixed(1) + '%' : '—'}</div>
          <div class="kv"><span class="k">rss</span> ${a.rss_bytes != null ? fmtBytes(a.rss_bytes) : '—'}</div>
        </div>
      </div>
    </div>`);
    app.append(body);

    await renderTimeline();
    renderHistory();
    startLogs();
  }

  async function renderHistory() {
    const el = document.getElementById('hist');
    if (!el) return;
    let hist = [];
    try { hist = await getJSON(`/api/v1/apps/${encodeURIComponent(name)}/history`); } catch (_) {}
    el.innerHTML = '';
    if (!hist.length) { el.append($(`<div class="kv muted">no deploy history</div>`)); return; }
    for (const h of hist) {
      const when = new Date(h.deployed_at * 1000).toLocaleString();
      const vcls = h.verified_status === 'verified' ? 'ok' : h.verified_status === 'drift' ? 'bad' : 'muted';
      el.append($(`<div class="hrow${h.current ? ' cur' : ''}">
        <span class="hdot ${vcls}"></span>
        <span class="mono">${esc(shortHash(h.artifact_hash))}</span>
        ${h.current ? '<span class="hcur">current</span>' : ''}
        <span class="hwhen">${esc(when)}</span>
      </div>`));
    }
  }

  async function renderTimeline() {
    const tl = document.getElementById('tl');
    if (!tl) return;
    let events = [];
    try { events = await getJSON(`/api/v1/apps/${encodeURIComponent(name)}/events`); } catch (_) {}
    tl.innerHTML = '';
    if (!events.length) { tl.append($(`<div class="kv muted">no events yet</div>`)); return; }
    for (const ev of events) {
      const when = new Date(ev.ts * 1000).toLocaleTimeString();
      tl.append($(`<div class="ev ${esc(ev.kind)}">
        <span class="evdot"></span>
        <div><div class="et">${esc(when)}</div>
          <span class="ek">${esc(ev.kind)}</span> <span class="ed">${esc(ev.detail)}</span></div>
      </div>`));
    }
  }

  function startLogs() {
    es && es.close();
    const pre = document.getElementById('logpre');
    const box = document.getElementById('log');
    if (!pre) return;
    pre.textContent = '';
    es = new EventSource(`/api/v1/apps/${encodeURIComponent(name)}/logs`);
    es.onmessage = (m) => {
      const line = document.createElement('div');
      line.innerHTML = ansiToHtml(m.data);
      pre.append(line);
      box.scrollTop = box.scrollHeight;
    };
    es.onerror = () => {};
  }

  render();
  evTimer = setInterval(renderTimeline, 4000);
  return () => { es && es.close(); clearInterval(evTimer); };
}

// ---- Endpoints registry ----
function endpointsView() {
  const LAG_THRESHOLD = 800;
  const epStatus = (e) => (!e.reachable ? 'crashing' : ((e.lag_ms ?? 0) > LAG_THRESHOLD ? 'degraded' : 'running'));
  const epLabel = (s) => ({ running: 'REACHABLE', degraded: 'HIGH LAG', crashing: 'UNREACHABLE' }[s]);
  let timer = null;

  async function add() {
    const name = prompt('Endpoint name (e.g. mainnet-rpc):');
    if (!name) return;
    const url = prompt('Public-gRPC URL (e.g. http://host:5555):');
    if (!url) return;
    try {
      await fetch('/api/v1/endpoints', {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ name, url, kind: 'remote' }),
      });
    } catch (_) {}
    render();
  }
  async function remove(name) {
    if (!confirm(`Remove endpoint ${name}?`)) return;
    try { await fetch(`/api/v1/endpoints/${encodeURIComponent(name)}`, { method: 'DELETE' }); } catch (_) {}
    render();
  }

  async function render() {
    let eps;
    try { eps = await getJSON('/api/v1/endpoints'); clearBanner(); }
    catch (e) { banner(`daemon unreachable — ${e.message}`); return; }

    const reach = eps.filter((e) => epStatus(e) === 'running').length;
    const lag = eps.filter((e) => epStatus(e) === 'degraded').length;
    const down = eps.filter((e) => epStatus(e) === 'crashing').length;

    app.innerHTML = '';
    app.append($(`
      <div class="stats">
        <div class="stat"><div class="num">${eps.length}</div><div class="lab">Endpoints</div></div>
        <div class="stat blue"><div class="num">${reach}</div><div class="lab">Reachable</div></div>
        <div class="stat yellow"><div class="num">${lag}</div><div class="lab">High lag</div></div>
        <div class="stat red"><div class="num">${down}</div><div class="lab">Unreachable</div></div>
      </div>`));

    const bar = $(`<div class="toolbar"><div class="summary">Named Nockchain RPC targets — apps attach by name</div></div>`);
    const addBtn = $(`<button class="btn primary">+ ADD ENDPOINT</button>`);
    addBtn.onclick = add;
    bar.append(addBtn);
    app.append(bar);

    if (!eps.length) {
      app.append($(`<div class="empty-tile">No endpoints. Add one above, or <b>nockd endpoint add &lt;name&gt; &lt;url&gt;</b>.</div>`));
      return;
    }

    const grid = $(`<div class="tiles"></div>`);
    for (const e of eps) {
      const s = epStatus(e);
      const lagPct = Math.min(((e.lag_ms ?? 0) / LAG_THRESHOLD) * 100, 100);
      const barFill = !e.reachable
        ? `background:repeating-linear-gradient(45deg,var(--red),var(--red) 4px,var(--track) 4px,var(--track) 8px)`
        : `width:${lagPct}%;background:${s === 'degraded' ? 'var(--yellow)' : 'var(--blue)'}`;
      const chips = e.attached_apps.length
        ? e.attached_apps.map((a) => `<span class="tag">${esc(a)}</span>`).join(' ')
        : `<span class="muted mono">— no instances attached —</span>`;
      const tile = $(`<div class="tile">
        <div class="band ${s}">
          <span class="left">${glyph(s)} ${epLabel(s)}</span>
          <span>${e.reachable ? (e.lag_ms != null ? e.lag_ms + 'ms' : '') : 'timeout'}</span>
        </div>
        <div class="body">
          <div class="tname">${esc(e.name)} <span class="tag" style="font-size:10px">${esc(e.kind)}</span></div>
          <div class="meta">${esc(e.url)}</div>
          ${e.height != null ? `<div class="meta">block ${e.height}${e.behind ? ` · ${e.behind} behind` : ' · tip'}</div>` : ''}
          <div style="height:8px;background:var(--track);margin:6px 0"><div style="height:100%;${barFill}"></div></div>
          <div class="tfoot"><span>ATTACHED · ${e.attached_apps.length} APPS</span><span class="rm" style="cursor:pointer">✕</span></div>
          <div style="display:flex;gap:6px;flex-wrap:wrap">${chips}</div>
        </div>
      </div>`);
      tile.querySelector('.rm').onclick = () => remove(e.name);
      grid.append(tile);
    }
    app.append(grid);
  }

  render();
  timer = setInterval(render, 4000);
  return () => clearInterval(timer);
}

// ---- Placeholder views (screens that land with their backend feature) ----
function placeholderView(title, sub) {
  app.innerHTML = '';
  app.append($(`<div class="placeholder">
    <div class="big">${esc(title)}</div>
    <div class="sub">${esc(sub)}</div>
  </div>`));
  return () => {};
}

// ---- Router ----
function setView(factory) { dispose(); dispose = factory() || (() => {}); }

function route() {
  const h = location.hash || '#/';
  setActiveNav(h);
  if (h.startsWith('#/app/')) return setView(() => detailView(decodeURIComponent(h.slice(6))));
  switch (h) {
    case '#/endpoints': return setView(endpointsView);
    case '#/secrets': return setView(() => placeholderView('SECRETS', 'Metadata-only secrets management lands with the encrypted secrets store (DESIGN OQ5). Values are never rendered.'));
    case '#/verify': return setView(() => placeholderView('VERIFY', 'Reproducible-build verification lands with signed attestations (DESIGN OQ2 / strict-both).'));
    default: return setView(fleetView);
  }
}

document.getElementById('host').textContent = location.host || 'localhost';
window.addEventListener('hashchange', route);
route();
