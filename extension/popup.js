// mDm Chrome Extension — Popup Logic
"use strict";

const $ = (id) => document.getElementById(id);

function fmtBytes(n) {
  if (!n) return "";
  const units = ["B", "KB", "MB", "GB"];
  let v = n, u = 0;
  while (Math.abs(v) >= 1024 && u < units.length - 1) { v /= 1024; u += 1; }
  return `${v.toFixed(v >= 100 || u === 0 ? 0 : 1)} ${units[u]}`;
}

function fmtSpeed(bps) {
  return bps ? `${fmtBytes(bps)}/s` : "0 B/s";
}

// --- Check Daemon Status & Update UI ---------------------------------------

async function refreshStatus() {
  chrome.runtime.sendMessage({ action: "CHECK_STATUS" }, (res) => {
    const pill = $("status-pill");
    const txt = $("status-text");
    const actCard = $("activity-card");

    if (chrome.runtime.lastError || !res || !res.connected) {
      pill.className = "status-pill is-error";
      txt.textContent = "mDm Offline";
      actCard.style.display = "none";
      return;
    }

    pill.className = "status-pill is-connected";
    txt.textContent = `Connected (${res.version || "online"})`;
    actCard.style.display = "flex";

    // Fetch active downloads stats
    chrome.runtime.sendMessage({ action: "GET_ACTIVE_JOBS" }, (jobRes) => {
      if (jobRes?.ok && Array.isArray(jobRes.jobs)) {
        const running = jobRes.jobs.filter((j) => j.state === "running");
        const downSpeed = running.reduce((a, j) => a + (j.speed_bps || 0), 0);
        const upSpeed = running.reduce((a, j) => a + (j.upload_speed_bps || 0), 0);

        $("act-active").textContent = `${running.length} / ${jobRes.jobs.length}`;
        $("act-down").textContent = fmtSpeed(downSpeed);
        $("act-up").textContent = fmtSpeed(upSpeed);
      }
    });
  });
}

// --- Active Tab Media Sniffer List -----------------------------------------

async function loadTabMedia() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab) return;

  // Prefill quick URL if current tab is a media or video platform
  const tabUrl = tab.url || "";
  if (tabUrl.startsWith("http") && (
      tabUrl.includes("youtube.com") || tabUrl.includes("youtu.be") ||
      tabUrl.includes("twitter.com") || tabUrl.includes("x.com") ||
      tabUrl.includes("reddit.com") || tabUrl.includes("tiktok.com") ||
      tabUrl.includes("vimeo.com") || tabUrl.includes("twitch.tv")
  )) {
    if (!$("quick-url").value) {
      $("quick-url").value = tabUrl;
    }
  }

  chrome.runtime.sendMessage({ action: "GET_TAB_MEDIA", tabId: tab.id }, (res) => {
    const list = $("media-list");
    const countEl = $("media-count");
    if (!res?.ok || !res.items || res.items.length === 0) {
      countEl.textContent = "0 found";
      return;
    }

    const items = res.items;
    countEl.textContent = `${items.length} found`;
    list.innerHTML = "";

    items.forEach((item) => {
      const row = document.createElement("div");
      row.className = "media-item";

      const urlClean = item.url.split("?")[0];
      const filename = urlClean.split("/").pop() || item.url;
      const sizeStr = item.size ? fmtBytes(item.size) : "";

      row.innerHTML = `
        <div class="media-item-info">
          <span class="media-item-badge">${item.ext || "MEDIA"}</span>
          <span class="media-item-url" title="${item.url}">${filename}</span>
          ${sizeStr ? `<span class="media-item-size">(${sizeStr})</span>` : ""}
        </div>
        <button type="button" class="btn-item-dl" title="Send to mDm">Download</button>
      `;

      row.querySelector(".btn-item-dl").addEventListener("click", () => {
        sendDownload(item.url, item.type, null, filename);
      });

      list.appendChild(row);
    });
  });
}

// --- Quick Download Actions ------------------------------------------------

function sendDownload(url, kind, format, title) {
  const btn = $("btn-download");
  const origText = btn.textContent;
  btn.textContent = "Sending…";

  chrome.runtime.sendMessage(
    {
      action: "SEND_TO_MDM",
      payload: { url, kind, format, title },
    },
    (res) => {
      if (chrome.runtime.lastError || !res?.ok) {
        btn.textContent = "Failed ✕";
        alert(res?.error || "Cannot connect to mDm desktop app at 127.0.0.1:5757.");
      } else {
        btn.textContent = "Sent ✓";
        $("quick-url").value = "";
      }
      setTimeout(() => { btn.textContent = origText; }, 1800);
      refreshStatus();
    }
  );
}

$("quick-form").addEventListener("submit", (e) => {
  e.preventDefault();
  const url = $("quick-url").value.trim();
  if (!url) return;

  const fmtVal = $("quick-format").value;
  let format = null;
  let kind = "video";

  if (fmtVal === "1080p") {
    format = "bv*[height<=1080]+ba/b[height<=1080]/b";
  } else if (fmtVal === "720p") {
    format = "bv*[height<=720]+ba/b[height<=720]/b";
  } else if (fmtVal === "audio") {
    format = "bestaudio";
    kind = "audio";
  }

  sendDownload(url, kind, format, null);
});

$("btn-paste")?.addEventListener("click", async () => {
  try {
    const text = await navigator.clipboard.readText();
    if (text) $("quick-url").value = text.trim();
  } catch (_) {}
});

$("btn-open-app")?.addEventListener("click", () => {
  fetch("http://127.0.0.1:5757/api/window/maximize", { method: "POST" }).catch(() => {});
});

// --- Capture & File Extension Settings ------------------------------------

let defaultExts = "";

function loadSettings() {
  chrome.runtime.sendMessage({ action: "GET_SETTINGS" }, (res) => {
    if (!res?.ok) return;
    defaultExts = res.defaultExtensions || "";
    const cfg = res.settings;
    $("cfg-intercept").checked = cfg.interceptDownloads !== false;
    $("cfg-dialog").checked = cfg.showDownloadDialog !== false;
    $("cfg-extensions").value = cfg.captureExtensions || defaultExts;
  });
}

$("btn-toggle-cfg")?.addEventListener("click", () => {
  const p = $("cfg-panel");
  const isHidden = p.style.display === "none";
  p.style.display = isHidden ? "flex" : "none";
  if (isHidden) loadSettings();
});

$("btn-reset-ext")?.addEventListener("click", () => {
  if (defaultExts) $("cfg-extensions").value = defaultExts;
});

$("btn-save-cfg")?.addEventListener("click", () => {
  const btn = $("btn-save-cfg");
  const settings = {
    interceptDownloads: $("cfg-intercept").checked,
    showDownloadDialog: $("cfg-dialog").checked,
    captureExtensions: $("cfg-extensions").value.trim().toUpperCase(),
  };

  chrome.runtime.sendMessage({ action: "SAVE_SETTINGS", settings }, (res) => {
    btn.textContent = "Saved ✓";
    setTimeout(() => {
      btn.textContent = "Save Extension Settings";
      $("cfg-panel").style.display = "none";
    }, 1200);
  });
});

// --- Init ------------------------------------------------------------------

refreshStatus();
loadTabMedia();
loadSettings();
setInterval(refreshStatus, 3000);
