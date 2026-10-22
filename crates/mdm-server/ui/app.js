// mdm web UI — OmniGet-style shell: hero paste bar with a download card,
// grouped download cards, category sidebar, settings. Talks to the local
// REST + WS API on the same origin.
"use strict";

const $ = (id) => document.getElementById(id);

const CATS = {
  video: { title: "Video", kinds: ["video"] },
  audio: { title: "Audio", kinds: ["audio"] },
  torrents: { title: "Torrents", kinds: ["torrent", "magnet"] },
  files: { title: "Files", kinds: ["direct"] },
};

const GROUPS = [
  { g: "running", label: "Downloading" },
  { g: "queued", label: "Queued" },
  { g: "paused", label: "Paused" },
  { g: "done", label: "Finished" },
  { g: "failed", label: "Failed" },
];

const KIND_ICONS = {
  video: '<svg viewBox="0 0 24 24"><rect width="24" height="24" fill="none"/><path d="M10.5 15.5v-7l6 3.5z" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/><rect x="3" y="4" width="18" height="16" rx="3" fill="none" stroke="currentColor" stroke-width="1.8"/></svg>',
  audio: '<svg viewBox="0 0 24 24"><path d="M4 14a8 8 0 0 1 16 0" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/><rect x="3" y="13" width="4.6" height="7" rx="2.3" fill="none" stroke="currentColor" stroke-width="1.8"/><rect x="16.4" y="13" width="4.6" height="7" rx="2.3" fill="none" stroke="currentColor" stroke-width="1.8"/></svg>',
  torrent: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M5.5 4v7.5a6.5 6.5 0 0 0 13 0V4" stroke-linecap="round"/><path d="M5.5 4h4.8V9H5.5zM13.7 4h4.8V9h-4.8z" fill="currentColor" stroke-width="1"/></svg>',
  magnet: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M5.5 4v7.5a6.5 6.5 0 0 0 13 0V4" stroke-linecap="round"/><path d="M5.5 4h4.8V9H5.5zM13.7 4h4.8V9h-4.8z" fill="currentColor" stroke-width="1"/></svg>',
  direct: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6 3.5h7.5L19 9v11.5H6zM13.5 3.7V9H19"/></svg>',
  generic: '<svg viewBox="0 0 24 24"><path d="M12 4v10m-4.5-4 4.5 4.5L16.5 10M5 19.5h14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>',
};

const ACT_ICONS = {
  pause: '<svg viewBox="0 0 24 24"><path d="M9 5.5v13M15 5.5v13" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>',
  resume: '<svg viewBox="0 0 24 24"><path d="M8 5.5v13l10-6.5z" fill="currentColor"/></svg>',
  rm: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4.5 6.5h15M9.5 4h5M6.5 6.5l1 13h9l1-13M10 10.5v5.5M14 10.5v5.5"/></svg>',
  copy: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><rect x="4" y="4" width="12" height="12" rx="2.5"/><path d="M8.5 20h9a2.5 2.5 0 0 0 2.5-2.5v-9"/></svg>',
};

const jobs = new Map();   // id -> job (latest)
const cards = new Map();  // id -> { el, refs }
let kindFilter = null;    // null | 'video'|'audio'|'torrents'|'files'
let stateFilter = "all";  // all | active | done | failed
let searchQuery = "";
let qualityFmt = "";      // quality segment format value
let cfgCache = null;      // last known config

// thumbnail sources; the <img> falls back to the kind icon on error.
function ytId(url) {
  const m = String(url).match(/(?:youtube\.com\/(?:watch\?v=|shorts\/|embed\/)|youtu\.be\/)([A-Za-z0-9_-]{6,15})/);
  return m ? m[1] : null;
}

function thumbSrcs(job) {
  const out = [];
  const host = safeHost(job.url);
  if (job.kind === "video" || job.kind === "audio") {
    const id = ytId(job.url);
    if (id) out.push(`https://i.ytimg.com/vi/${id}/mqdefault.jpg`);
    if (host) out.push(`https://icons.duckduckgo.com/ip3/${host}.ico`);
  } else if (job.kind === "direct") {
    if (host) out.push(`https://icons.duckduckgo.com/ip3/${host}.ico`);
  }
  return out;
}

function thumb(kind, job) {
  const box = mk("div", "thumb");
  const srcs = job ? thumbSrcs(job) : [];
  if (srcs.length) {
    const img = mk("img", "thumb-img");
    let i = 0;
    img.src = srcs[0];
    img.referrerPolicy = "no-referrer";
    img.loading = "lazy";
    img.alt = "";
    img.addEventListener("load", () => box.classList.add("filled"));
    img.onerror = () => {
      i += 1;
      if (i < srcs.length) img.src = srcs[i];
      else img.remove();
    };
    box.appendChild(img);
  }
  if (!srcs.length || srcs[0]) {
    const ic = icon(KIND_ICONS[job.kind] || KIND_ICONS.generic);
    ic.classList.add("thumb-fallback");
    box.appendChild(ic);
  }
  return box;
}

// ---------- tiny helpers ----------

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

function fmtPct(p) {
  return `${Math.min(100, Math.max(0, p)).toFixed(0)}%`;
}

function fmtElapsed(sec) {
  const s = Math.floor(sec);
  if (s >= 3600) return `${Math.floor(s / 3600)}:${String(Math.floor(s / 60) % 60).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
  return `${Math.floor(s / 60)}.${String(s % 60).padStart(2, "0")}`;
}

const seenRunning = new Map(); // id -> started-at ms (this page load)

function elapsedSec(job) {
  const s = seenRunning.get(job.id);
  return s ? Math.max(0, (Date.now() - s) / 1000) : 0;
}

function basename(path) {
  const parts = String(path).split("/");
  return parts[parts.length - 1] || String(path);
}

function displayName(job) {
  return job.title || (job.output_path ? basename(job.output_path) : "Untitled");
}

function safeHost(url) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch (_) {
    return "";
  }
}

// Matches mdm-core's JobKind::detect, plus magnet label handling.
function detectKind(input) {
  const u = input.trim();
  if (u.startsWith("magnet:")) return "magnet";
  if (u.toLowerCase().split("?")[0].endsWith(".torrent")) return "torrent";
  return "video";
}

function siteLabel(job) {
  if (job.kind === "magnet") return "Magnet";
  if (job.kind === "torrent") return "Torrent file";
  const host = safeHost(job.url);
  if (!host) return "";
  const seg = host.split(".");
  const brand = (seg.length >= 2 ? seg[seg.length - 2] : seg[0]);
  const nice = { youtube: "YouTube", instagram: "Instagram", tiktok: "TikTok", reddit: "Reddit", twitter: "Twitter" };
  return nice[brand.toLowerCase()] || host;
}

function fmtChip(job) {
  const f = (job.format || "").trim();
  if (!f || f === "best") return null;
  if (f === "bestaudio/best" || f === "bestaudio") return "audio only";
  if (f === "bestvideo/best" || f === "bestvideo") return "video · no sound";
  const m = f.match(/height<=?(\d+)/);
  if (m) return `≤${m[1]}p`;
  return f.length > 18 ? `${f.slice(0, 16)}…` : f;
}

function notice(msg, isError) {
  const el = $("notice");
  el.textContent = msg;
  el.classList.toggle("error", !!isError);
  el.classList.remove("hidden");
  clearTimeout(notice.timer);
  notice.timer = setTimeout(() => el.classList.add("hidden"), isError ? 8000 : 3500);
}

function mk(tag, cls, text) {
  const el = document.createElement(tag);
  if (cls) el.className = cls;
  if (text !== undefined) el.textContent = text;
  return el;
}

function icon(html) {
  const wrap = mk("span");
  wrap.innerHTML = html;
  return wrap.firstElementChild;
}

// ---------- cards ----------

function buildCard(job) {
  const el = mk("div", "job-card");
  el.dataset.g = "";

  const thumbEl = thumb(job.kind, job);

  const main = mk("div", "job-main");

  const top = mk("div", "job-top");
  const title = mk("div", "job-title");
  const link = mk("a", null, displayName(job));
  link.href = job.url;
  link.target = "_blank";
  link.rel = "noopener";
  title.appendChild(link);
  title.title = displayName(job);
  const pill = mk("span", `state ${job.state}`, job.state);
  const acts = mk("div", "job-acts");
  top.appendChild(title);
  top.appendChild(acts);
  top.appendChild(pill);

  const meta = mk("div", "job-meta");
  const site = siteLabel(job);
  if (site) meta.appendChild(mk("span", null, site));
  meta.appendChild(icon('<svg viewBox="0 0 24 24"><circle cx="6" cy="12" r="1.6" fill="currentColor"/><circle cx="12" cy="12" r="1.6" fill="currentColor"/><circle cx="18" cy="12" r="1.6" fill="currentColor"/></svg>'));
  meta.appendChild(mk("span", "engine-tag", job.engine));
  const fmtC = fmtChip(job);
  if (fmtC) meta.appendChild(mk("span", "meta-chip", fmtC));
  const sizeC = mk("span", "meta-chip");
  meta.appendChild(sizeC);

  // stepper only while queued/running
  const stepper = mk("div", "stepper");
  const steps = ["queue", "download", "finish"].map(s => {
    const w = mk("span", "step", s === "queue" ? "Queue" : s === "download" ? "Download" : "Finish");
    w.dataset.step = s;
    w.prepend(mk("i"));
    stepper.appendChild(w);
    return w;
  });
  stepper.appendChild(mk("span", "sep", "·"));
  const stage = mk("span", null);
  stepper.appendChild(stage);

  const barRow = mk("div", "bar-row");
  const bar = mk("div", "bar");
  const fill = mk("i");
  bar.appendChild(fill);
  const pct = mk("span", "pct", "0%");
  barRow.appendChild(bar);
  barRow.appendChild(pct);

  const note = mk("div", "job-note");

  main.appendChild(top);
  main.appendChild(meta);
  main.appendChild(stepper);
  main.appendChild(barRow);
  main.appendChild(note);
  el.appendChild(thumbEl);
  el.appendChild(main);

  return { el, refs: { link, title, pill, acts, sizeC, bar, fill, pct, note, barRow, stepper, steps, stage, meta }, lastThumb: "" };
}

function setAction(refs, job) {
  refs.acts.replaceChildren();
  const act = (html, cls, method, path, okMsg) => {
    const b = mk("button", `icon-btn${cls ? ` ${cls}` : ""}`);
    b.appendChild(icon(html));
    b.addEventListener("click", async () => {
      try {
        const res = await fetch(path, { method });
        if (res.ok) {
          if (okMsg) notice(okMsg);
        } else {
          notice((await res.text()) || `error ${res.status}`, true);
        }
      } catch (e) {
        notice(`request failed: ${e}`, true);
      }
    });
    refs.acts.appendChild(b);
  };
  if (job.state === "queued" || job.state === "running") {
    act(ACT_ICONS.pause, "", "POST", `/api/jobs/${job.id}/pause`, "Paused");
  } else if (job.state === "paused") {
    act(ACT_ICONS.resume, "", "POST", `/api/jobs/${job.id}/resume`, "Resumed");
  }
  if (job.output_path && job.state === "done") {
    const copy = mk("button", "icon-btn");
    copy.title = job.output_path;
    copy.appendChild(icon(ACT_ICONS.copy));
    copy.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(job.output_path);
        notice("Path copied");
      } catch (_) {
        notice(job.output_path);
      }
    });
    refs.acts.appendChild(copy);
  }
  act(ACT_ICONS.rm, "danger", "DELETE", `/api/jobs/${job.id}`, "Removed");
}

function groupOf(state) { return state; }

function renderCard(job) {
  let c = cards.get(job.id);
  if (!c) {
    c = buildCard(job);
    cards.set(job.id, c);
  }
  const r = c.refs;

  // thumbnail refresh (yt/favicons) only when kind/url actually changed
  const tKey = `${job.kind}|${thumbSrcs(job).join("|")}`;
  if (c.lastThumb !== tKey) {
    c.lastThumb = tKey;
    c.el.firstElementChild.replaceWith(thumb(job.kind, job));
  }

  r.link.textContent = displayName(job);
  r.title.title = displayName(job);
  r.pill.textContent = job.state;
  r.pill.className = `state ${job.state}`;
  r.sizeC.textContent = job.state === "done" && job.output_path
    ? fmtBytes(job.downloaded_bytes ?? job.total_bytes ?? null)
    : (job.total_bytes !== null && job.total_bytes !== undefined
      ? `${fmtBytes(job.downloaded_bytes ?? 0)} / ${fmtBytes(job.total_bytes)}`
      : (job.downloaded_bytes !== null && job.downloaded_bytes !== undefined ? fmtBytes(job.downloaded_bytes) : ""));

  // stepper
  const showStepper = job.state === "queued" || job.state === "running";
  r.stepper.classList.toggle("hidden", !showStepper);
  if (showStepper) {
    for (const s of r.steps) {
      s.classList.toggle("on", s.dataset.step === (job.state === "queued" ? "queue" : "download"));
      s.classList.toggle("past", s.dataset.step === "queue" && job.state === "running");
    }
    const activeStage = job.kind === "video" || job.kind === "audio"
      ? (job.progress > 0 ? "Running…" : "Starting engine…")
      : "Downloading";
    r.stage.textContent = activeStage;
  }

  // running-elapsed bookkeeping
  if (job.state === "running") {
    if (!seenRunning.has(job.id)) seenRunning.set(job.id, Date.now());
  } else {
    seenRunning.delete(job.id);
  }

  // bar
  r.bar.className = `bar ${job.state}`;
  r.fill.style.width = `${Math.min(100, Math.max(0, job.progress || 0))}%`;
  r.pct.textContent = fmtPct(job.progress || 0);
  r.barRow.classList.toggle("hidden", job.state === "done" || job.state === "failed");

  // note: error / output path
  if (job.error) {
    r.note.textContent = job.error;
    r.note.className = "job-note mono bad";
    r.note.title = job.error;
  } else if (job.state === "running") {
    r.note.textContent = `· ${fmtElapsed(elapsedSec(job))} elapsed`;
    r.note.className = "job-note";
  } else if (job.state === "done" && job.output_path) {
    r.note.textContent = job.output_path;
    r.note.className = "job-note mono";
    r.note.title = job.output_path;
  } else if (job.state === "paused") {
    r.note.textContent = "Paused — resume to continue.";
    r.note.className = "job-note";
  } else {
    r.note.textContent = "";
  }

  setAction(r, job);

  // move to its group bucket
  const g = groupOf(job.state);
  if (c.el.dataset.g !== g) {
    c.el.dataset.g = g;
    groupBucket(g).appendChild(c.el);
  }
}

const groupBuckets = new Map();

function groupBucket(state) {
  const id = state;
  if (!groupBuckets.has(id)) {
    const wrap = mk("div", "group");
    wrap.dataset.g = state;
    wrap.appendChild(mk("div", "group-label", GROUPS.find(x => x.g === state)?.label || state));
    const bucket = mk("div", "cards");
    wrap.appendChild(bucket);
    $("cards").appendChild(wrap);
    groupBuckets.set(id, bucket);
  }
  return groupBuckets.get(id);
}

function removeCard(id) {
  const c = cards.get(id);
  if (c) {
    c.el.remove();
    cards.delete(id);
  }
  jobs.delete(id);
  sync();
}

// ---------- filters + counts ----------

function visible(job) {
  if (kindFilter && !CATS[kindFilter].kinds.includes(job.kind)) return false;
  if (stateFilter === "active" && job.state !== "running" && job.state !== "queued") return false;
  if (stateFilter === "done" && job.state !== "done") return false;
  if (stateFilter === "failed" && job.state !== "failed") return false;
  if (searchQuery) {
    const hay = `${displayName(job)} ${job.url}`.toLowerCase();
    if (!hay.includes(searchQuery)) return false;
  }
  return true;
}

function sync() {
  for (const [id, job] of jobs) {
    const c = cards.get(id);
    if (c) c.el.classList.toggle("hidden", !visible(job));
  }
  for (const [g, bucket] of groupBuckets) {
    const wrap = bucket.parentElement;
    const any = [...jobs.values()].some(j => groupOf(j.state) === g && visible(j));
    wrap.classList.toggle("hidden", !any);
  }

  const list = [...jobs.values()];
  const running = list.filter(j => j.state === "running");
  const active = list.filter(j => j.state === "running" || j.state === "queued");

  const chipDefs = [
    ["n-all", list.length],
    ["n-active", active.length],
    ["n-done", list.filter(j => j.state === "done").length],
    ["n-failed", list.filter(j => j.state === "failed").length],
  ];
  for (const [id, n] of chipDefs) $(id).textContent = n;
  for (const [id, n] of [["stat-running", running.length], ["stat-queued", active.length - running.length]]) {
    $(id).textContent = n;
  }
  $("stat-done").textContent = list.filter(j => j.state === "done").length;
  const badge = $("badge-active");
  badge.classList.toggle("hidden", active.length === 0);
  badge.textContent = active.length;

  $("kind-note").classList.toggle("hidden", !kindFilter);
  if (kindFilter) {
    $("kind-note").textContent = `Showing ${CATS[kindFilter].title.toLowerCase()} downloads. Pick a category in the sidebar to change.`;
  }

  const anyVisible = list.some(visible);
  $("empty").classList.toggle("hidden", anyVisible);
  $("empty").textContent = stateFilter === "all"
    ? "Nothing here yet — add a link from Home."
    : "No downloads match this filter.";

  // header summary
  const saved = list
    .filter(j => j.state === "done")
    .reduce((a, j) => a + (j.downloaded_bytes || j.total_bytes || 0), 0);
  const summary = $("dl-summary");
  summary.classList.toggle("hidden", list.length === 0);
  summary.textContent = `${list.length} download${list.length === 1 ? "" : "s"} · ${fmtBytes(saved)} saved`;

  // footer
  const footer = $("dl-footer");
  if (running.length) {
    footer.classList.remove("hidden");
    const aggSpeed = running.reduce((a, j) => a + (j.speed_bps > 0 ? j.speed_bps : 0), 0);
    const doneB = running.reduce((a, j) => a + (j.downloaded_bytes > 0 ? j.downloaded_bytes : 0), 0);
    const pct = running.reduce((a, j) => a + Math.min(100, Math.max(0, j.progress || 0)), 0) / running.length;
    $("dl-footer-text").textContent = `${running.length} downloading · ${fmtBytes(doneB)} saved · ${fmtSpeed(aggSpeed)}`;
    $("overall-bar").style.width = `${pct.toFixed(1)}%`;
    $("overall-pct").textContent = fmtPct(pct);
  } else {
    footer.classList.add("hidden");
  }
}

// ---------- torrent preview ----------

let curTorrentInfo = null;
let fileSel = new Set();
const torrentInfoCache = new Map(); // path -> Promise<info|null>

function loadTorrentInfo(path) {
  if (!torrentInfoCache.has(path)) {
    torrentInfoCache.set(
      path,
      fetch("/api/torrent-info", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ path }),
      }).then(r => (r.ok ? r.json() : null)),
    );
  }
  return torrentInfoCache.get(path);
}

function renderTorrentFiles() {
  const box = $("t-files");
  box.replaceChildren();
  fileSel.clear();
  if (!curTorrentInfo) {
    $("torrent-block").classList.add("hidden");
    return;
  }
  $("torrent-block").classList.remove("hidden");
  curTorrentInfo.files.forEach((f, i) => {
    const row = mk("label", "t-row");
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = true;
    cb.dataset.idx = String(i + 1);
    row.appendChild(cb);
    row.appendChild(mk("span", "t-name", f.name || `file ${i + 1}`));
    row.appendChild(mk("span", "t-size", fmtBytes(f.size)));
    fileSel.add(i + 1);
    box.appendChild(row);
  });
  updateTorrentCounts();
}

function updateTorrentCounts() {
  if (!curTorrentInfo) return;
  const total = curTorrentInfo.files.length;
  $("t-count").textContent = `${fileSel.size} of ${total} file${total === 1 ? "" : "s"}`;
  const kindLabel = "torrent";
  $("add-btn").textContent =
    fileSel.size === total ? `Download ${total} file${total === 1 ? "" : "s"}`
      : fileSel.size === 0 ? "Add torrent (no files selected)"
      : `Download ${fileSel.size} file${fileSel.size === 1 ? "" : "s"}`;
  void kindLabel;
}

$("t-files").addEventListener("change", (e) => {
  const cb = e.target.closest("input[type=checkbox]");
  if (!cb) return;
  const idx = Number(cb.dataset.idx);
  if (cb.checked) fileSel.add(idx);
  else fileSel.delete(idx);
  updateTorrentCounts();
});
$("t-all").addEventListener("click", () => {
  for (const cb of $("t-files").querySelectorAll("input[type=checkbox]")) {
    cb.checked = true;
    fileSel.add(Number(cb.dataset.idx));
  }
  updateTorrentCounts();
});
$("t-none").addEventListener("click", () => {
  for (const cb of $("t-files").querySelectorAll("input[type=checkbox]")) {
    cb.checked = false;
  }
  fileSel.clear();
  updateTorrentCounts();
});

// ---------- add card (home) ----------

function refreshPreview() {
  const url = $("url").value.trim();
  const hasUrl = url.length > 0;
  $("clear-btn").classList.toggle("hidden", !hasUrl);
  document.querySelector(".hero").classList.toggle("focused", hasUrl);
  $("add-card").classList.toggle("hidden", !hasUrl);
  curTorrentInfo = null;
  fileSel.clear();
  $("torrent-block").classList.toggle("hidden", true);
  if (!hasUrl) return;

  const isTorrentPath = /\.torrent$/i.test(url.split("?")[0]);
  const kind = isTorrentPath ? "torrent" : ($("kind").value || detectKind(url));
  $("pv-site").textContent =
    kind === "magnet" ? "Magnet link"
    : kind === "torrent" ? "BitTorrent"
    : (siteLabel({ kind, url }) || "Link");
  $("pv-type").textContent = { video: "Video", audio: "Audio", direct: "File", torrent: "Torrent", magnet: "Magnet" }[kind];
  $("add-btn").textContent = `Download ${{ video: "video", audio: "audio", direct: "file", torrent: "torrent", magnet: "magnet" }[kind]}`;

  if (kind === "torrent" && (url.startsWith("/") || url.startsWith("~"))) {
    loadTorrentInfo(url).then(info => {
      // ignore stale responses after path changed
      if (info && $("url").value.trim() === url) {
        curTorrentInfo = info;
        renderTorrentFiles();
      }
    });
  }
}

function loadOutDir() {
  fetch("/api/config").then(r => (r.ok ? r.json() : null)).then(cfg => {
    cfgCache = cfg;
    if (cfg) $("out-dir-label").textContent = cfg.output_dir || "~/Downloads/mdm";
  }).catch(() => {});
}

$("url").addEventListener("input", refreshPreview);
$("kind").addEventListener("change", refreshPreview);
$("url").addEventListener("keydown", (e) => { if (e.key === "Escape") { $("url").value = ""; refreshPreview(); } });
$("clear-btn").addEventListener("click", () => { $("url").value = ""; $("url").focus(); refreshPreview(); });
$("quality-seg").addEventListener("click", (e) => {
  const b = e.target.closest(".seg-btn");
  if (!b) return;
  qualityFmt = b.dataset.fmt;
  for (const s of $("quality-seg").children) s.classList.toggle("on", s === b);
  if ($("format").value) $("format").value = qualityFmt;
});
$("out-dir-label").addEventListener("click", () => {
  if (!cfgCache) return;
  $("out-dir-label").classList.add("hidden");
  const input = $("out-dir-input");
  input.classList.remove("hidden");
  $("out-dir-save").classList.remove("hidden");
  input.value = cfgCache.output_dir || "";
  input.focus();
});
$("out-dir-save").addEventListener("click", async () => {
  const input = $("out-dir-input");
  try {
    const res = await fetch("/api/config", {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ output_dir: input.value.trim() }),
    });
    if (!res.ok) {
      notice((await res.text()) || `error ${res.status}`, true);
      return;
    }
    cfgCache = await res.json();
    $("out-dir-label").textContent = cfgCache.output_dir;
    notice("Output folder updated");
  } catch (e) {
    notice(`request failed: ${e}`, true);
  } finally {
    input.classList.add("hidden");
    $("out-dir-save").classList.add("hidden");
    $("out-dir-label").classList.remove("hidden");
  }
});

$("paste-btn").addEventListener("click", async () => {
  const input = $("url");
  try {
    const text = await navigator.clipboard.readText();
    if (text) input.value = text.trim();
  } catch (_) { /* clipboard blocked: user can type */ }
  input.focus();
  refreshPreview();
});

$("add-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const url = $("url").value.trim();
  if (!url) return;
  const kind = $("kind").value;
  const format = ($("format").value.trim() || qualityFmt) || undefined;
  const body = { url };
  if (kind) body.kind = kind;
  if (format) body.format = format;
  if (curTorrentInfo && fileSel.size > 0 && fileSel.size < curTorrentInfo.files.length) {
    body.select_files = [...fileSel].sort((a, b) => a - b);
  }
  const btn = $("add-btn");
  btn.disabled = true;
  try {
    const res = await fetch("/api/jobs", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(format ? { url, kind: kind || undefined, format } : { url, kind: kind || undefined }),
    });
    if (res.ok) {
      notice("Added to queue");
      $("url").value = "";
      refreshPreview();
      healthSoon();
    } else {
      notice((await res.text()) || `error ${res.status}`, true);
    }
  } catch (err) {
    notice(`request failed: ${err}`, true);
  } finally {
    btn.disabled = false;
  }
});

// ---------- settings ----------

async function loadSettings() {
  try {
    const res = await fetch("/api/config");
    if (!res.ok) throw new Error(res.status);
    const cfg = await res.json();
    cfgCache = cfg;
    $("set-output-dir").value = cfg.output_dir || "";
    $("set-max-concurrent").value = cfg.max_concurrent;
    $("set-max-retries").value = cfg.max_retries;
    $("set-seed-minutes").value = cfg.seed_minutes;
  } catch (e) {
    notice(`cannot load settings: ${e}`, true);
  }
}

$("settings-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const body = {
    output_dir: $("set-output-dir").value.trim(),
    max_concurrent: Number($("set-max-concurrent").value) || undefined,
    max_retries: Number($("set-max-retries").value) || 0,
    seed_minutes: Number($("set-seed-minutes").value) || 0,
  };
  try {
    const res = await fetch("/api/config", {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (res.ok) {
      cfgCache = await res.json();
      notice("Settings saved");
    } else {
      notice((await res.text()) || `error ${res.status}`, true);
    }
  } catch (err) {
    notice(`request failed: ${err}`, true);
  }
});

// ---------- router ----------

function applyRoute() {
  const hash = location.hash || "#/home";
  const parts = hash.replace(/^#\/?/, "").split("/");
  let view = parts[0] || "home";

  kindFilter = null;
  let target = "#/home";
  if (view === "downloads") {
    const cat = parts[1];
    if (cat && CATS[cat]) {
      kindFilter = cat;
      target = `#/downloads/${cat}`;
    } else if (cat) {
      view = "home";
    } else {
      target = "#/downloads";
    }
  } else if (view === "settings") {
    target = "#/settings";
  } else {
    view = "home";
  }

  const views = { home: "view-home", downloads: "view-downloads", settings: "view-settings" };
  for (const [v, id] of Object.entries(views)) $(id).hidden = v !== view;

  for (const btn of document.querySelectorAll("[data-nav], [data-kind]")) {
    const dest = btn.dataset.kind || btn.dataset.nav;
    btn.classList.toggle("active", dest === target);
  }

  if (view === "settings") loadSettings();
  sync();
}

function go(path) {
  location.hash = path;
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
  ws.onopen = () => { reconnectDelay = 1000; pollHealth(); };
  ws.onclose = () => {
    setHealth(false, "reconnecting…");
    setTimeout(connect, reconnectDelay);
    reconnectDelay = Math.min(reconnectDelay * 2, 10000);
  };
  ws.onerror = () => ws.close();
  ws.onmessage = (m) => {
    let ev;
    try { ev = JSON.parse(m.data); } catch (_) { return; }
    if (ev.type === "snapshot") {
      for (const [, c] of cards) c.el.remove();
      cards.clear();
      groupBuckets.clear();
      $("cards").replaceChildren();
      jobs.clear();
      for (const job of ev.jobs || []) { jobs.set(job.id, job); renderCard(job); }
      sync();
    } else if (ev.type === "job_updated" && ev.job) {
      jobs.set(ev.job.id, ev.job);
      renderCard(ev.job);
      sync();
      healthSoon();
    } else if (ev.type === "job_removed" && ev.id) {
      removeCard(ev.id);
    } else if (ev.type === "queue_idle") {
      healthSoon();
    }
  };
}

let healthTimer = 0;
function healthSoon() {
  clearTimeout(healthTimer);
  healthTimer = setTimeout(pollHealth, 400);
}

async function pollHealth() {
  try {
    const res = await fetch("/api/health");
    if (!res.ok) throw new Error(res.status);
    const h = await res.json();
    const parts = [];
    if (h.running > 0) parts.push(`${h.running} running`);
    if (h.queued > 0) parts.push(`${h.queued} queued`);
    setHealth(true, parts.length ? parts.join(" · ") : "idle");
  } catch (_) { /* WS banner covers it */ }
}

// ---------- bindings ----------

for (const btn of document.querySelectorAll("[data-nav]")) {
  btn.addEventListener("click", () => go(btn.dataset.nav));
}
for (const btn of document.querySelectorAll(".nav-item.cat")) {
  btn.addEventListener("click", () => go(btn.dataset.kind));
}
for (const btn of document.querySelectorAll(".stat")) {
  btn.addEventListener("click", () => go("#/downloads"));
}
$("go-add").addEventListener("click", () => {
  go("#/home");
  setTimeout(() => $("url").focus(), 50);
});
$("state-chips").addEventListener("click", (e) => {
  const chip = e.target.closest(".chip");
  if (!chip) return;
  stateFilter = chip.dataset.state;
  for (const c of $("state-chips").children) c.classList.toggle("on", c === chip);
  sync();
});
$("search").addEventListener("input", (e) => {
  searchQuery = e.target.value.trim().toLowerCase();
  sync();
});
$("collapse").addEventListener("click", () => document.querySelector(".app").classList.toggle("collapsed"));
window.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.shiftKey && (e.key === "S" || e.key === "s")) {
    e.preventDefault();
    document.querySelector(".app").classList.toggle("collapsed");
  }
});
window.addEventListener("hashchange", applyRoute);

// ---------- start ----------

setInterval(() => {
  for (const [id, job] of jobs) {
    if (job.state !== "running") continue;
    const c = cards.get(id);
    if (c) c.refs.note.textContent = `· ${fmtElapsed(elapsedSec(job))} elapsed`;
  }
}, 1000);

async function refreshInitial() {
  try {
    const res = await fetch("/api/jobs");
    if (res.ok) {
      for (const job of await res.json()) {
        jobs.set(job.id, job);
        renderCard(job);
      }
      sync();
    }
  } catch (_) { /* WS snapshot will fill in */ }
}

refreshInitial();
loadOutDir();
applyRoute();
connect();
pollHealth();
setInterval(pollHealth, 5000);
