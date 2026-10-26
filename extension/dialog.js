// mDm IDM-style Download Confirmation Dialog Window Logic
"use strict";

const $ = (id) => document.getElementById(id);

// --- SVG Icons for Categories (Proper vector graphics, NO emojis) ---
const CATEGORY_SVGS = {
  videos: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"/>
      <line x1="7" y1="2" x2="7" y2="22"/>
      <line x1="17" y1="2" x2="17" y2="22"/>
      <line x1="2" y1="12" x2="22" y2="12"/>
      <line x1="2" y1="7" x2="7" y2="7"/>
      <line x1="2" y1="17" x2="7" y2="17"/>
      <line x1="17" y1="17" x2="22" y2="17"/>
      <line x1="17" y1="7" x2="22" y2="7"/>
    </svg>
  `,
  audio: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M9 18V5l12-2v13"/>
      <circle cx="6" cy="18" r="3"/>
      <circle cx="18" cy="16" r="3"/>
    </svg>
  `,
  compressed: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <polyline points="21 8 21 21 3 21 3 8"/>
      <rect x="1" y="3" width="22" height="5"/>
      <line x1="10" y1="12" x2="14" y2="12"/>
    </svg>
  `,
  programs: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <rect x="4" y="4" width="16" height="16" rx="2"/>
      <rect x="9" y="9" width="6" height="6"/>
      <line x1="9" y1="1" x2="9" y2="4"/>
      <line x1="15" y1="1" x2="15" y2="4"/>
      <line x1="9" y1="20" x2="9" y2="23"/>
      <line x1="15" y1="20" x2="15" y2="23"/>
      <line x1="20" y1="9" x2="23" y2="9"/>
      <line x1="20" y1="14" x2="23" y2="14"/>
      <line x1="1" y1="9" x2="4" y2="9"/>
      <line x1="1" y1="14" x2="4" y2="14"/>
    </svg>
  `,
  documents: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
      <polyline points="14 2 14 8 20 8"/>
      <line x1="16" y1="13" x2="8" y2="13"/>
      <line x1="16" y1="17" x2="8" y2="17"/>
      <polyline points="10 9 9 9 8 9"/>
    </svg>
  `,
  general: `
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
      <polyline points="7 10 12 15 17 10"/>
      <line x1="12" y1="15" x2="12" y2="3"/>
    </svg>
  `,
};

const EXT_CATEGORY = {
  exe: "programs", msi: "programs", apk: "programs", appimage: "programs", deb: "programs", rpm: "programs", dmg: "programs", iso: "programs",
  zip: "compressed", rar: "compressed", "7z": "compressed", tar: "compressed", gz: "compressed", bz2: "compressed", xz: "compressed",
  mp4: "videos", mkv: "videos", webm: "videos", avi: "videos", mov: "videos", flv: "videos", ts: "videos", m4v: "videos",
  mp3: "audio", flac: "audio", wav: "audio", m4a: "audio", aac: "audio", ogg: "audio", opus: "audio",
  pdf: "documents", doc: "documents", docx: "documents", txt: "documents", xls: "documents", xlsx: "documents", ppt: "documents", pptx: "documents", epub: "documents",
};

function fmtBytes(n) {
  if (!n) return "Unknown";
  const bytes = Number(n);
  if (isNaN(bytes) || bytes <= 0) return "Unknown";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes, u = 0;
  while (Math.abs(v) >= 1024 && u < units.length - 1) { v /= 1024; u += 1; }
  return `${v.toFixed(v >= 100 || u === 0 ? 0 : 1)} ${units[u]}`;
}

// Parse URL params
const params = new URLSearchParams(location.search);
const rawUrl = params.get("url") || "";
let rawFilename = params.get("filename") || "";
let rawSize = parseInt(params.get("size") || "0", 10) || 0;
const rawKind = params.get("kind") || "";
const rawFormat = params.get("format") || "";
const rawReferrer = params.get("referrer") || "";

// Extract filename from URL if missing
if (!rawFilename && rawUrl) {
  try {
    const u = new URL(rawUrl);
    rawFilename = u.pathname.split("/").pop() || "download";
  } catch {
    rawFilename = rawUrl.split("?")[0].split("/").pop() || "download";
  }
}

// Clean filename safely
try {
  rawFilename = decodeURIComponent(rawFilename);
} catch (_) {}
rawFilename = (rawFilename || "").split("?")[0].trim();
if (!rawFilename) rawFilename = "download";

let baseOutputDir = "";

function updateFilePresentation(filename, category) {
  const parts = filename.split(".");
  const ext = (parts.length > 1 ? parts.pop() : "").toLowerCase();
  const cat = category || EXT_CATEGORY[ext] || "general";

  $("dlg-ext-badge").textContent = (ext ? ext.toUpperCase() : (cat ? cat.toUpperCase() : "FILE")).slice(0, 6);
  $("dlg-file-icon").innerHTML = CATEGORY_SVGS[cat] || CATEGORY_SVGS.general;
}

// Initial populate
$("dlg-url").value = rawUrl;
$("dlg-filename").value = rawFilename;

const initialExt = (rawFilename.split(".").pop() || "").toLowerCase();
let initialCat = EXT_CATEGORY[initialExt] || (rawKind === "video" ? "videos" : rawKind === "audio" ? "audio" : "general");
$("dlg-category").value = initialCat;
updateFilePresentation(rawFilename, initialCat);

// Display size
if (rawSize > 0) {
  $("dlg-size").textContent = `Size: ${fmtBytes(rawSize)}`;
  $("dlg-size-dot").className = "dlg-size-dot is-verified";
} else {
  $("dlg-size").textContent = "Size: Probing…";
  $("dlg-size-dot").className = "dlg-size-dot is-probing";

  // Probe size from background service worker
  if (rawUrl) {
    chrome.runtime.sendMessage({ action: "PROBE_URL_SIZE", url: rawUrl }, (res) => {
      if (res?.ok && res.size > 0) {
        rawSize = res.size;
        $("dlg-size").textContent = `Size: ${fmtBytes(rawSize)}`;
        $("dlg-size-dot").className = "dlg-size-dot is-verified";
        if (res.filename && $("dlg-filename").value === "download") {
          $("dlg-filename").value = res.filename;
          updateFilePresentation(res.filename, $("dlg-category").value);
        }
      } else {
        $("dlg-size").textContent = "Size: Unknown (Stream)";
        $("dlg-size-dot").className = "dlg-size-dot";
      }
    });
  }
}

// Fetch mDm daemon config and status
chrome.runtime.sendMessage({ action: "CHECK_STATUS" }, (res) => {
  const dot = $("dlg-status-dot");
  const text = $("dlg-status-text");

  if (res?.connected) {
    dot.className = "dlg-status-dot is-connected";
    text.textContent = `mDm Engine Ready (v${res.version || "0.1.0"})`;
  } else {
    dot.className = "dlg-status-dot is-error";
    text.textContent = "mDm Engine Offline (Launch mDm desktop app)";
  }
});

// Fetch default output_dir from mDm REST API
fetch("http://127.0.0.1:5757/api/config")
  .then((r) => (r.ok ? r.json() : null))
  .then((cfg) => {
    if (cfg?.output_dir) {
      baseOutputDir = cfg.output_dir;
      $("dlg-output-dir").value = baseOutputDir;
    }
  })
  .catch(() => {
    // Daemon might not be ready; leave default empty or path
  });

// Event listeners
$("dlg-filename").addEventListener("input", (e) => {
  updateFilePresentation(e.target.value, $("dlg-category").value);
});

$("dlg-category").addEventListener("change", (e) => {
  updateFilePresentation($("dlg-filename").value, e.target.value);
});

// Copy URL button
$("btn-copy-url").addEventListener("click", () => {
  const url = $("dlg-url").value;
  if (!url) return;
  navigator.clipboard.writeText(url).then(() => {
    const textSpan = $("dlg-copy-text");
    const orig = textSpan.textContent;
    textSpan.textContent = "Copied!";
    setTimeout(() => { textSpan.textContent = orig; }, 1500);
  });
});

// Load preferences
chrome.storage.sync.get({ showDownloadDialog: true }, (cfg) => {
  $("dlg-chk-show").checked = cfg.showDownloadDialog !== false;
});

function submitDownload(downloadLater = false) {
  const url = $("dlg-url").value.trim();
  const filename = $("dlg-filename").value.trim();
  const cat = $("dlg-category").value;
  const outputDir = $("dlg-output-dir").value.trim();
  const showDlg = $("dlg-chk-show").checked;

  chrome.storage.sync.set({ showDownloadDialog: showDlg });

  let kind = "direct";
  if (cat === "videos") kind = "video";
  else if (cat === "audio") kind = "audio";
  else if (rawKind === "video") kind = "video";
  else if (rawKind === "audio") kind = "audio";

  const btnStart = $("btn-start");
  const btnLater = $("btn-later");
  btnStart.disabled = true;
  btnLater.disabled = true;

  if (downloadLater) {
    btnLater.innerHTML = `<span>Saving…</span>`;
  } else {
    btnStart.innerHTML = `<span>Starting…</span>`;
  }

  chrome.runtime.sendMessage(
    {
      action: "SEND_TO_MDM",
      payload: {
        url,
        kind,
        format: rawFormat || undefined,
        title: filename,
        output_dir: outputDir || undefined,
        downloadLater,
      },
    },
    (res) => {
      if (res?.ok) {
        window.close();
      } else {
        alert(res?.error || "Could not connect to mDm desktop app at 127.0.0.1:5757.");
        btnStart.disabled = false;
        btnLater.disabled = false;
        btnStart.innerHTML = `
          <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 3v13"/><polyline points="7 11 12 16 17 11"/><line x1="5" y1="21" x2="19" y2="21"/>
          </svg>
          <span>Start Download</span>
        `;
        btnLater.innerHTML = `
          <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>
          </svg>
          <span>Download Later</span>
        `;
      }
    }
  );
}

$("btn-start").addEventListener("click", () => submitDownload(false));
$("btn-later").addEventListener("click", () => submitDownload(true));
$("btn-cancel").addEventListener("click", () => window.close());
$("btn-close").addEventListener("click", () => window.close());

window.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    submitDownload(false);
  } else if (e.key === "Escape") {
    e.preventDefault();
    window.close();
  }
});
