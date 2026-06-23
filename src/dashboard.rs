//! Minimal browser dashboard (DESIGN §9), embedded in the binary. Phase 0 is a read-only
//! fleet view + live logs that polls the same control API the CLI uses. Streaming (SSE),
//! actions, and auth are Phase 1.

pub const INDEX_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>nockd</title>
<style>
  :root { color-scheme: dark; }
  body { font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace;
         margin: 0; background: #0b0f14; color: #d7e0ea; }
  header { padding: 14px 20px; border-bottom: 1px solid #1c2530; display: flex;
           align-items: baseline; gap: 12px; }
  header h1 { font-size: 16px; margin: 0; letter-spacing: .5px; }
  header .sub { color: #6b7a8d; }
  main { padding: 20px; }
  table { border-collapse: collapse; width: 100%; }
  th, td { text-align: left; padding: 8px 10px; border-bottom: 1px solid #161e28; }
  th { color: #6b7a8d; font-weight: 600; }
  .pill { padding: 2px 8px; border-radius: 10px; font-size: 12px; }
  .running { background: #11331f; color: #5fe39a; }
  .stopped { background: #2a2f36; color: #9fb0c2; }
  .crashed, .backoff { background: #3a1717; color: #ff8d8d; }
  .mono { color: #7fa7d4; }
  .row { cursor: pointer; }
  .row:hover td { background: #0f1620; }
  #logs { margin-top: 18px; background: #06090d; border: 1px solid #161e28;
          border-radius: 6px; padding: 12px; white-space: pre-wrap;
          max-height: 50vh; overflow: auto; display: none; }
  #logs h3 { margin: 0 0 8px; color: #6b7a8d; font-size: 12px; }
  .empty { color: #6b7a8d; padding: 24px 0; }
</style>
</head>
<body>
<header>
  <h1>nockd</h1>
  <span class="sub">NockApp deployment — fleet</span>
</header>
<main>
  <table id="apps">
    <thead><tr>
      <th>app</th><th>status</th><th>kernel</th><th>endpoint</th>
      <th>restart</th><th>pid</th><th>restarts</th>
    </tr></thead>
    <tbody id="tbody"></tbody>
  </table>
  <div id="empty" class="empty">No apps deployed yet. Try <code>nockd deploy</code>.</div>
  <div id="logs"><h3 id="logs-title"></h3><div id="logs-body"></div></div>
</main>
<script>
let selected = null;

function short(h) { return h ? h.slice(0, 12) : "—"; }

async function refresh() {
  let apps = [];
  try { apps = await (await fetch("/api/apps")).json(); } catch (e) { return; }
  const tbody = document.getElementById("tbody");
  document.getElementById("empty").style.display = apps.length ? "none" : "block";
  tbody.innerHTML = "";
  for (const a of apps) {
    const rt = a.runtime || {};
    const state = rt.state || a.desired_status || "stopped";
    const tr = document.createElement("tr");
    tr.className = "row";
    tr.onclick = () => { selected = a.name; refreshLogs(); };
    tr.innerHTML =
      `<td>${a.name}</td>` +
      `<td><span class="pill ${state}">${state}</span></td>` +
      `<td class="mono">${short(a.kernel_hash)}</td>` +
      `<td class="mono">${a.endpoint || "—"}</td>` +
      `<td>${a.restart_policy}</td>` +
      `<td>${rt.pid || "—"}</td>` +
      `<td>${rt.restarts || 0}</td>`;
    tbody.appendChild(tr);
  }
}

async function refreshLogs() {
  if (!selected) return;
  const box = document.getElementById("logs");
  box.style.display = "block";
  document.getElementById("logs-title").textContent = "logs — " + selected;
  try {
    const txt = await (await fetch(`/api/apps/${selected}/logs?lines=300`)).text();
    const body = document.getElementById("logs-body");
    body.textContent = txt || "(no output yet)";
    box.scrollTop = box.scrollHeight;
  } catch (e) {}
}

refresh(); refreshLogs();
setInterval(() => { refresh(); refreshLogs(); }, 2000);
</script>
</body>
</html>
"##;
