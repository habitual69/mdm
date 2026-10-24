// mDm IDM-style Download Confirmation Dialog Window Logic
"use strict";

const $ = (id) => document.getElementById(id);

function fmtBytes(n) {
  if (!n) return "Unknown";
  const bytes = Number(n);
  if (isNaN(bytes) || bytes <= 0) return "Unknown";
  const units = ["B", "KB", "MB", "GB"];
  let v = bytes, u = 0;
  while (Math.abs(v) >= 1024 && u < units.length - 1) { v /= 1024; u += 1; }
  return `${v.toFixed(v >= 100 || u === 0 ? 0 : 1)} ${units[u]}`;
}

const EXT_CATEGORY = {
  exe: "programs", msi: "programs", apk: "programs", appimage: "programs", deb: "programs", rpm: "programs", dmg: "programs",
  zip: "compressed", rar: "compressed", "7z": "compressed", tar: "compressed", gz: "compressed", bz2: "compressed", xz: "compressed", iso: "compressed",
  mp4: "videos", mkv: "videos", webm: "videos", avi: "videos", mov: "videos", flv: "videos", ts: "videos",
  mp3: "audio", flac: "audio", wav: "audio", m4a: "audio", aac: "audio", ogg: "audio", opus: "audio",
  pdf: "documents", doc: "documents", docx: "documents", txt: "documents", xls: "documents", xlsx: "documents", epub: "documents",
};

// Parse URL params
const params = new URLSearchParams(location.search);
const rawUrl = params.get("url") || "";
let rawFilename = params.get("filename") || "";
const rawSize = params.get("size") || "";

if (!rawFilename && rawUrl) {
  try {
    const u = new URL(rawUrl);
    rawFilename = u.pathname.split("/").pop() || "download";
  } catch {
    rawFilename = rawUrl.split("/").pop() || "download";
  }
}

// Clean filename
rawFilename = decodeURIComponent(rawFilename).split("?")[0];
const ext = (rawFilename.split(".").pop() || "").toLowerCase();
const detectedCat = EXT_CATEGORY[ext] || "general";

$("dlg-url").value = rawUrl;
$("dlg-filename").value = rawFilename;
$("dlg-category").value = detectedCat;
$("dlg-size").textContent = `Size: ${fmtBytes(rawSize)}`;

// Check daemon connection
chrome.runtime.sendMessage({ action: "CHECK_STATUS" }, (res) => {
  const st = $("dlg-status");
  if (res?.connected) {
    st.textContent = `● mDm Connected (${res.version || "online"})`;
    st.className = "dlg-daemon-status is-connected";
  } else {
    st.textContent = "● mDm Offline (Launch mDm desktop app)";
    st.className = "dlg-daemon-status is-error";
  }
});

// Load preferences
chrome.storage.sync.get({ showDownloadDialog: true }, (cfg) => {
  $("dlg-chk-show").checked = cfg.showDownloadDialog;
});

function startDownload() {
  const url = $("dlg-url").value.trim();
  const filename = $("dlg-filename").value.trim();
  const cat = $("dlg-category").value;
  const showDlg = $("dlg-chk-show").checked;

  chrome.storage.sync.set({ showDownloadDialog: showDlg });

  let kind = "direct";
  if (cat === "videos") kind = "video";
  else if (cat === "audio") kind = "audio";

  const btn = $("btn-start");
  btn.textContent = "Starting…";
  btn.disabled = true;

  chrome.runtime.sendMessage(
    {
      action: "SEND_TO_MDM",
      payload: {
        url,
        kind,
        title: filename,
      },
    },
    (res) => {
      if (res?.ok) {
        window.close();
      } else {
        alert(res?.error || "Could not start download in mDm.");
        btn.textContent = "Start Download";
        btn.disabled = false;
      }
    }
  );
}

$("btn-start").addEventListener("click", startDownload);
$("btn-cancel").addEventListener("click", () => window.close());
$("btn-close").addEventListener("click", () => window.close());

window.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    startDownload();
  } else if (e.key === "Escape") {
    e.preventDefault();
    window.close();
  }
});
