// nockd dashboard — vanilla ES module, a thin client of the /api/v1 Control API.
// No build step; served embedded from the nockd binary (design/API-INTEGRATION.md §6).

const app = document.getElementById('app');
let dispose = () => {};

// ---- helpers ----
const $ = (html) => { const t = document.createElement('template'); t.innerHTML = html.trim(); return t.content.firstElementChild; };
const esc = (s) => String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const STATUSES = ['running', 'degraded', 'crashing', 'stopped'];
const statusClass = (s) => (STATUSES.includes(s) ? s : 'stopped');
const glyph = (s) => `<span class="glyph ${statusClass(s)}"></span>`;
const shortHash = (h) => (h ? `${h.slice(0, 4)}…${h.slice(-2)}` : '—');

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
      <th>Rst</th><th>Health</th><th>Status</th></tr></thead><tbody></tbody></table>`);
    const tb = wrap.querySelector('tbody');
    if (!apps.length) { app.append($(`<div class="empty-tile">No apps deployed. Use <b>nockd deploy</b>.</div>`)); return wrap; }
    for (const a of apps) {
      const idle = a.status === 'stopped';
      const rst = a.restart_count > 3 ? `<span class="rst-hot">${a.restart_count}</span>` : a.restart_count;
      const tr = $(`<tr class="${idle ? 'idle' : ''}">
        <td>${glyph(a.status)}</td>
        <td class="cell-app">${esc(a.name)}</td>
        <td class="mono">${shortHash(a.artifact_hash)}</td>
        <td class="mono">${esc(a.endpoint_name || '—')}</td>
        <td class="mono">${fmtUptime(a.uptime_s)}</td>
        <td class="mono">${rst}</td>
        <td class="mono muted">${esc(a.health)}</td>
        <td><span class="status-word ${statusClass(a.status)}">${esc(a.status)}</span></td>
      </tr>`);
      tr.onclick = () => location.hash = `#/app/${encodeURIComponent(a.name)}`;
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
          <span>${a.status === 'crashing' ? `${a.restart_count} rst` : fmtUptime(a.uptime_s)}</span>
        </div>
        <div class="body">
          <div class="tname">${esc(a.name)}</div>
          <div class="meta">${esc(a.artifact_hash ? a.artifact_hash.slice(0, 18) + '…' : '—')}</div>
          <div class="meta">${esc(a.endpoint_name || 'no endpoint')}</div>
          <div class="tfoot"><span>${esc(a.health)}</span><span>vfy —</span></div>
        </div>
      </div>`);
      t.onclick = () => location.hash = `#/app/${encodeURIComponent(a.name)}`;
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
        <h1>${esc(a.name)}</h1>
        <span class="sub">${esc(a.restart_policy)} · up ${fmtUptime(a.uptime_s)} · pid ${a.pid ?? '—'}</span>
        <div class="actions">
          <button class="btn" data-act="restart">Restart</button>
          <button class="btn" data-act="start">Start</button>
          <button class="btn danger" data-act="stop">Stop</button>
        </div>
      </div>`);
    head.querySelectorAll('[data-act]').forEach((b) =>
      b.onclick = async () => { await post(`/api/v1/apps/${encodeURIComponent(name)}/${b.dataset.act}`); setTimeout(render, 400); });
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
        </div>
        <div class="panel attach">
          <h3>Attachment</h3>
          <div class="kv" style="color:var(--cream)"><span class="k" style="color:rgba(243,237,225,.7)">endpoint</span> ${esc(a.endpoint_name || '— none —')}</div>
          <div class="kv" style="color:var(--cream)"><span class="k" style="color:rgba(243,237,225,.7)">health</span> ${esc(a.health)}</div>
        </div>
        <div class="panel"><h3>Resources</h3><div class="kv muted">CPU / RSS sampling lands with metrics (DESIGN OQ8).</div></div>
      </div>
    </div>`);
    app.append(body);

    await renderTimeline();
    startLogs();
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
      line.innerHTML = colorVerbs(esc(m.data));
      pre.append(line);
      box.scrollTop = box.scrollHeight;
    };
    es.onerror = () => {};
  }

  function colorVerbs(line) {
    return line.replace(/\b(poke|peek|chain|snap)\b/g, (m) => `<span class="v-${m}">${m}</span>`);
  }

  render();
  evTimer = setInterval(renderTimeline, 4000);
  return () => { es && es.close(); clearInterval(evTimer); };
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
    case '#/endpoints': return setView(() => placeholderView('ENDPOINTS', 'The named Nockchain RPC registry lands with the endpoint-registry backend (DESIGN §11). Today an app carries a single endpoint string.'));
    case '#/secrets': return setView(() => placeholderView('SECRETS', 'Metadata-only secrets management lands with the encrypted secrets store (DESIGN OQ5). Values are never rendered.'));
    case '#/verify': return setView(() => placeholderView('VERIFY', 'Reproducible-build verification lands with signed attestations (DESIGN OQ2 / strict-both).'));
    default: return setView(fleetView);
  }
}

document.getElementById('host').textContent = location.host || 'localhost';
window.addEventListener('hashchange', route);
route();
