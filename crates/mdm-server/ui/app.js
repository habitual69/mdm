// mdm web UI — talks to the local REST + WS API on the same origin.
"use strict";

const $ = (id) => document.getElementById(id);

const rows = new Map(); // id -> { tr, name, url, kind, state, barFill, bar, pct, size, speed, eta, err, actions }

// ---------- formatting ----------

function fmtBytes(n) {
  if (n === null || n === undefined) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let v = n;
  let u = 0;
  while (Math.abs(v) >= 1024 && u < units.length - 1) { v /= 1024; u += 1; }
  const digits = v >= 100 || u === 0 ? 0 : 1;
  return `${v.toFixed(digits)} ${units[u]}`;
}

function fmtSpeed(bps) {
  if (bps === null || bps === undefined) return "—";
  return `${fmtBytes(bps)}/s`;
}

function fmtEta(sec) {
  if (sec === null || sec === undefined) return "—";
  const s = Math.max(0, Math.floor(sec));
  if (s >= 3600) {
    return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
  }
  if (s >= 60) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${s}s`;
}

function fmtPct(p) {
  return `${Math.min(100, Math.max(0, p)).toFixed(1)}%`;
}

function basename(path) {
  const parts = String(path).split("/");
  return parts[parts.length - 1] || String(path);
}

// ---------- notice ----------

let noticeTimer = 0;

function showNotice(msg, isError) {
  const el = $("notice");
  el.textContent = msg;
  el.classList.toggle("error", !!isError);
  el.classList.remove("hidden");
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => el.classList.add("hidden"), isError ? 8000 : 4000);
}

// ---------- rendering ----------

function displayName(job) {
  return job.title || (job.output_path ? basename(job.output_path) : job.url);
}

function buildRow(job) {
  const tr = document.createElement("tr");
  const mk = (cls) => {
    const td = document.createElement("td");
    if (cls) td.className = cls;
    tr.appendChild(td);
    return td;
  };

  const name = document.createElement("span");
  name.className = "name";
  const url = document.createElement("a");
  url.className = "url";
  url.target = "_blank";
  url.rel = "noopener";
  const cName = mk("c-name");
  cName.appendChild(name);
  cName.appendChild(url);

  const kind = mk("c-kind");
  const state = document.createElement("span");
  state.className = "state";
  mk("c-state").appendChild(state);

  const bar = mk("c-prog");
  bar.className = "c-prog";
  const barFill = document.createElement("i");
  const barBox = document.createElement("div");
  barBox.className = "bar";
  barBox.appendChild(barFill);
  const pct = document.createElement("div");
  pct.className = "pct";
  const err = document.createElement("div");
  err.className = "err-msg hidden";
  bar.appendChild(barBox);
  bar.appendChild(pct);
  bar.appendChild(err);

  const size = mk("c-size");
  const speed = mk("c-speed");
  const eta = mk("c-eta");
  const actions = mk("c-actions");

  $("queue-body").appendChild(tr);
  return {
    tr, name, url, kind, state, barFill, barBox, pct, err, size, speed, eta, actions,
  };
}

function actionButtons(job) {
  const cell = rows.get(job.id).actions;
  cell.replaceChildren();
  const add = (label, cls, fn) => {
    const b = document.createElement("button");
    b.textContent = label;
    if (cls) b.className = cls;
    b.addEventListener("click", () => fn(job.id));
    cell.appendChild(b);
  };
  if (job.state === "queued" || job.state === "running") {
    add("pause", "", (id) => post(`/api/jobs/${id}/pause`));
  } else if (job.state === "paused") {
    add("resume", "", (id) => post(`/api/jobs/${id}/resume`));
  }
  add("rm", "danger", (id) => del(`/api/jobs/${id}`));
  if (job.state === "done" && job.output_path) {
    // Local downloads: just surface the path; opening it is a desktop concern.
    cell.title = job.output_path;
  }
}

function renderRow(job) {
  let r = rows.get(job.id);
  if (!r) {
    r = buildRow(job);
    rows.set(job.id, r);
  }
  r.name.textContent = displayName(job);
  r.name.title = displayName(job);
  r.url.textContent = job.url;
  r.url.href = job.url;
  r.kind.textContent = job.kind;
  r.state.textContent = job.state;
  r.state.className = `state ${job.state}`;

  r.barBox.className = `bar ${job.state}`;
  r.barFill.style.width = `${Math.min(100, Math.max(0, job.progress || 0))}%`;
  r.pct.textContent = job.state === "done" ? "100%" : fmtPct(job.progress || 0);
  if (job.error) {
    r.err.textContent = job.error;
    r.err.classList.remove("hidden");
    r.err.title = job.error;
  } else {
    r.err.classList.add("hidden");
  }

  if (job.total_bytes !== null && job.total_bytes !== undefined) {
    r.size.textContent = `${fmtBytes(job.downloaded_bytes ?? 0)} / ${fmtBytes(job.total_bytes)}`;
  } else if (job.downloaded_bytes !== null && job.downloaded_bytes !== undefined) {
    r.size.textContent = fmtBytes(job.downloaded_bytes);
  } else {
    r.size.textContent = "—";
  }
  r.speed.textContent = job.state === "running" ? fmtSpeed(job.speed_bps) : "—";
  r.eta.textContent = job.state === "running" ? fmtEta(job.eta_sec) : "—";
  actionButtons(job);
}

function renderAll(jobs) {
  const seen = new Set();
  for (const job of jobs) {
    seen.add(job.id);
    renderRow(job);
  }
  for (const [id, r] of rows) {
    if (!seen.has(id)) {
      r.tr.remove();
      rows.delete(id);
    }
  }
  $("empty").classList.toggle("hidden", rows.size > 0);
}

function removeJob(id) {
  const r = rows.get(id);
  if (r) {
    r.tr.remove();
    rows.delete(id);
  }
  $("empty").classList.toggle("hidden", rows.size > 0);
}

// ---------- API ----------

async function post(path) {
  try {
    const res = await fetch(path, { method: "POST" });
    if (!res.ok) showNotice(await res.text(), true);
  } catch (e) {
    showNotice(`request failed: ${e}`, true);
  }
}

async function del(path) {
  try {
    const res = await fetch(path, { method: "DELETE" });
    if (!res.ok) showNotice(await res.text(), true);
  } catch (e) {
    showNotice(`request failed: ${e}`, true);
  }
}

async function refresh() {
  try {
    const res = await fetch("/api/jobs");
    if (res.ok) renderAll(await res.json());
  } catch (_) { /* offline; WS status shows it */ }
}

// ---------- websocket ----------

let ws = null;
let reconnectDelay = 1000;

function setHealth(ok, text) {
  $("health-dot").className = `dot ${ok ? "ok" : "bad"}`;
  $("health-text").textContent = text;
}

function connect() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  ws = new WebSocket(`${proto}://${location.host}/api/events`);
  ws.onopen = () => {
    reconnectDelay = 1000;
    setHealth(true, "connected");
  };
  ws.onclose = () => {
    setHealth(false, "reconnecting…");
    setTimeout(connect, reconnectDelay);
    reconnectDelay = Math.min(reconnectDelay * 2, 10000);
  };
  ws.onerror = () => ws.close();
  ws.onmessage = (m) => {
    let ev;
    try {
      ev = JSON.parse(m.data);
    } catch (_) {
      return;
    }
    if (ev.type === "snapshot" || ev.type === "job_updated") {
      if (ev.type === "snapshot") {
        for (const [, r] of rows) r.tr.remove();
        rows.clear();
        renderAll(ev.jobs || []);
      } else if (ev.job) {
        renderRow(ev.job);
        $("empty").classList.toggle("hidden", rows.size > 0);
      }
      healthSoon();
    } else if (ev.type === "job_removed") {
      removeJob(ev.id);
      healthSoon();
    } else if (ev.type === "queue_idle") {
      healthSoon();
    }
  };
}

// ---------- health poll ----------

async function pollHealth() {
  try {
    const res = await fetch("/api/health");
    if (!res.ok) throw new Error(res.status);
    const h = await res.json();
    const parts = [];
    if (h.running > 0) parts.push(`${h.running} running`);
    if (h.queued > 0) parts.push(`${h.queued} queued`);
    setHealth(true, parts.length ? parts.join(" · ") : "connected");
  } catch (_) {
    // WS reconnect banner already communicates state.
  }
}

let healthTimer = 0;
function healthSoon() {
  clearTimeout(healthTimer);
  healthTimer = setTimeout(pollHealth, 400);
}

// ---------- add form ----------

$("add-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const btn = $("add-btn");
  const url = $("url").value.trim();
  if (!url) return;
  const kind = $("kind").value;
  const format = $("format").value;
  const body = {};
  if (kind) body.kind = kind;
  if (format) body.format = format;
  btn.disabled = true;
  try {
    const res = await fetch("/api/jobs", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ url, ...body }),
    });
    if (res.ok) {
      $("url").value = "";
      showNotice("added to queue");
      healthSoon();
    } else {
      showNotice((await res.text()) || `error ${res.status}`, true);
    }
  } catch (err) {
    showNotice(`request failed: ${err}`, true);
  } finally {
    btn.disabled = false;
  }
});

// ---------- start ----------

refresh();
connect();
pollHealth();
setInterval(pollHealth, 5000);
