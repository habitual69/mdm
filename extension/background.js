// mDm (Media Download Manager) — Background Service Worker
"use strict";

const MDM_API_BASE = "http://127.0.0.1:5757";

// In-memory store for media detected on tabs: tabId -> Map(url -> mediaItem)
const tabMedia = new Map();

// --- IDM-Style Automatic Browser Download Interceptor -----------------------

const DEFAULT_CAPTURE_EXTENSIONS = 
  "EXE ZIP RAR 7Z ISO TAR GZ BZ2 XZ MP4 MKV AVI MOV WEBM FLV TS MP3 FLAC WAV M4A AAC OGG OPUS PDF DOC DOCX XLS XLSX PPT PPTX APK DEB RPM DMG MSI APPIMAGE TORRENT";

let interceptEnabled = true;
let showDialogEnabled = true;
let captureExtensionSet = new Set(DEFAULT_CAPTURE_EXTENSIONS.toLowerCase().split(/\s+/));
const inFlightUrls = new Set();

// Load settings from storage
chrome.storage.sync.get(
  {
    interceptDownloads: true,
    showDownloadDialog: true,
    captureExtensions: DEFAULT_CAPTURE_EXTENSIONS,
  },
  (cfg) => {
    interceptEnabled = cfg.interceptDownloads !== false;
    showDialogEnabled = cfg.showDownloadDialog !== false;
    updateCaptureExtensionSet(cfg.captureExtensions);
  }
);

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "sync") {
    if (changes.interceptDownloads) interceptEnabled = changes.interceptDownloads.newValue;
    if (changes.showDownloadDialog) showDialogEnabled = changes.showDownloadDialog.newValue;
    if (changes.captureExtensions) updateCaptureExtensionSet(changes.captureExtensions.newValue);
  }
});

function updateCaptureExtensionSet(extString) {
  const parts = String(extString || DEFAULT_CAPTURE_EXTENSIONS)
    .toLowerCase()
    .replace(/[.,;]/g, " ")
    .trim()
    .split(/\s+/);
  captureExtensionSet = new Set(parts.filter(Boolean));
}

function extractExtension(url, filename) {
  let s = (filename || "").trim();
  if (!s && url) {
    try {
      s = new URL(url).pathname.split("/").pop() || "";
    } catch {
      s = url.split("?")[0].split("/").pop() || "";
    }
  }
  const clean = s.split(/[?#]/)[0];
  const m = clean.match(/\.([a-z0-9]{1,8})$/i);
  return m ? m[1].toLowerCase() : "";
}

function openDownloadDialog({ url, filename, size, referrer }) {
  const dialogUrl = chrome.runtime.getURL(
    `dialog.html?url=${encodeURIComponent(url)}&filename=${encodeURIComponent(filename || "")}&size=${encodeURIComponent(size || 0)}`
  );

  const w = 520;
  const h = 320;

  chrome.windows.getCurrent((curWin) => {
    const left = curWin && curWin.left !== undefined ? Math.round(curWin.left + (curWin.width - w) / 2) : 200;
    const top = curWin && curWin.top !== undefined ? Math.round(curWin.top + (curWin.height - h) / 2) : 150;

    chrome.windows.create(
      {
        url: dialogUrl,
        type: "popup",
        width: w,
        height: h,
        left: Math.max(0, left),
        top: Math.max(0, top),
        focused: true,
      },
      (win) => {
        if (chrome.runtime.lastError || !win) {
          chrome.tabs.create({ url: dialogUrl });
        }
      }
    );
  });
}

// Intercept browser downloads when filename and mime are determined
chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  if (!interceptEnabled) {
    suggest();
    return;
  }

  // Skip blob, data URLs, or extension URLs
  if (!item.url || item.url.startsWith("blob:") || item.url.startsWith("data:") || item.url.startsWith("chrome-extension:")) {
    suggest();
    return;
  }

  // Prevent re-intercepting downloads created or triggered by mDm
  if (inFlightUrls.has(item.url) || (item.finalUrl && inFlightUrls.has(item.finalUrl))) {
    suggest();
    return;
  }

  const targetUrl = item.finalUrl || item.url;
  const filename = item.filename || "";
  const ext = extractExtension(targetUrl, filename);
  const mime = (item.mime || "").toLowerCase();

  const isMatchedExt = ext && captureExtensionSet.has(ext);
  const isMatchedMime = mime.includes("application/x-msdownload") ||
                        (mime.includes("application/octet-stream") && ext && captureExtensionSet.has(ext)) ||
                        mime.includes("application/zip") ||
                        mime.includes("application/x-7z-compressed") ||
                        mime.includes("application/x-rar-compressed");

  if (!isMatchedExt && !isMatchedMime) {
    suggest();
    return;
  }

  // Match found! Cancel native Chrome download
  chrome.downloads.cancel(item.id, () => {
    chrome.downloads.erase({ id: item.id });
  });

  // Track in-flight to prevent duplicate intercept loops
  inFlightUrls.add(item.url);
  if (item.finalUrl) inFlightUrls.add(item.finalUrl);
  setTimeout(() => {
    inFlightUrls.delete(item.url);
    if (item.finalUrl) inFlightUrls.delete(item.finalUrl);
  }, 10000);

  if (showDialogEnabled) {
    openDownloadDialog({
      url: targetUrl,
      filename: filename,
      size: item.fileSize || 0,
      referrer: item.referrer || "",
    });
  } else {
    let kind = "direct";
    if (mime.includes("video") || ["mp4", "mkv", "webm", "avi", "mov", "flv"].includes(ext)) {
      kind = "video";
    } else if (mime.includes("audio") || ["mp3", "flac", "wav", "m4a", "aac", "ogg"].includes(ext)) {
      kind = "audio";
    }
    sendDownloadToMdm({ url: targetUrl, title: filename, kind }).then((res) => {
      if (res.ok) {
        showNotification("mDm Intercepted Download", `Downloading: ${filename || targetUrl}`);
      }
    });
  }
});

// --- Context Menus ---------------------------------------------------------

chrome.runtime.onInstalled.addListener(() => {
  setupContextMenus();
});

function setupContextMenus() {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "mdm_download_link",
      title: "Download with mDm",
      contexts: ["link"],
    });
    chrome.contextMenus.create({
      id: "mdm_download_media",
      title: "Download media with mDm",
      contexts: ["video", "audio", "image"],
    });
    chrome.contextMenus.create({
      id: "mdm_download_selection",
      title: "Download '%s' with mDm",
      contexts: ["selection"],
    });
    chrome.contextMenus.create({
      id: "mdm_download_page",
      title: "Download this video/page with mDm",
      contexts: ["page"],
    });
  });
}

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  let url = info.linkUrl || info.srcUrl || info.selectionText || info.pageUrl;
  if (!url) return;
  url = url.trim();

  // If selection starts without protocol, prepend https://
  if (info.selectionText && !/^https?:\/\//i.test(url)) {
    url = `https://${url}`;
  }

  const res = await sendDownloadToMdm({ url, title: tab?.title });
  if (res.ok) {
    showNotification("mDm Download Started", `Sent to mDm: ${tab?.title || url}`);
  } else {
    showNotification("mDm Download Failed", res.error || "Could not connect to mDm daemon at 127.0.0.1:5757");
  }
});

// --- Stream Sniffer (Web Requests) -----------------------------------------

const MEDIA_REGEX = /\.(mp4|m3u8|mpd|webm|m4s|ts|flv|m4a|mp3|ogg|wav)($|\?)/i;

chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0 || details.method !== "GET") return;

    let isMedia = MEDIA_REGEX.test(details.url);
    let contentType = "";
    let contentLength = 0;

    for (const h of details.responseHeaders || []) {
      const name = h.name.toLowerCase();
      if (name === "content-type") {
        contentType = h.value || "";
        if (contentType.startsWith("video/") || contentType.startsWith("audio/") ||
            contentType.includes("mpegurl") || contentType.includes("dash+xml")) {
          isMedia = true;
        }
      } else if (name === "content-length") {
        contentLength = parseInt(h.value, 10) || 0;
      }
    }

    if (!isMedia) return;

    let items = tabMedia.get(details.tabId);
    if (!items) {
      items = new Map();
      tabMedia.set(details.tabId, items);
    }

    // Ignore tiny chunks or duplicate segment requests
    if (items.has(details.url)) return;
    if (contentLength > 0 && contentLength < 32768 && !details.url.includes(".m3u8") && !details.url.includes(".mpd")) {
      return;
    }

    const extMatch = details.url.match(/\.([a-z0-9]{2,5})($|\?)/i);
    const ext = extMatch ? extMatch[1].toUpperCase() : (contentType.split("/")[1] || "VIDEO").toUpperCase();

    items.set(details.url, {
      url: details.url,
      type: contentType.startsWith("audio") ? "audio" : "video",
      ext,
      contentType,
      size: contentLength,
      timestamp: Date.now(),
    });

    // Update extension badge with count
    updateBadge(details.tabId, items.size);
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"]
);

function updateBadge(tabId, count) {
  if (count > 0) {
    chrome.action.setBadgeText({ tabId, text: String(count) });
    chrome.action.setBadgeBackgroundColor({ tabId, color: "#e8934a" });
  } else {
    chrome.action.setBadgeText({ tabId, text: "" });
  }
}

chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === "loading") {
    tabMedia.delete(tabId);
    updateBadge(tabId, 0);
  }
});

chrome.tabs.onRemoved.addListener((tabId) => {
  tabMedia.delete(tabId);
});

// --- API Helpers -----------------------------------------------------------

async function checkMdmHealth() {
  try {
    const res = await fetch(`${MDM_API_BASE}/api/health`, { method: "GET" });
    if (!res.ok) return { connected: false };
    const data = await res.json();
    return {
      connected: true,
      version: data.version,
      queued: data.queued,
      running: data.running,
    };
  } catch (_) {
    return { connected: false };
  }
}

async function getNetscapeCookiesForUrl(url) {
  if (!chrome.cookies) return "";
  try {
    const parsed = new URL(url);
    const cookies = await chrome.cookies.getAll({ url });
    const parts = parsed.hostname.split(".");
    let domainPattern = parsed.hostname;
    if (parts.length >= 2) {
      domainPattern = "." + parts.slice(-2).join(".");
    }
    const domainCookies = await chrome.cookies.getAll({ domain: domainPattern });
    const map = new Map();
    for (const c of [...cookies, ...domainCookies]) {
      map.set(`${c.domain}|${c.path}|${c.name}`, c);
    }
    if (map.size === 0) return "";
    const lines = ["# Netscape HTTP Cookie File"];
    for (const c of map.values()) {
      const flag = c.domain.startsWith(".") ? "TRUE" : "FALSE";
      const path = c.path || "/";
      const secure = c.secure ? "TRUE" : "FALSE";
      const exp = c.expirationDate ? Math.round(c.expirationDate) : 0;
      lines.push(`${c.domain}\t${flag}\t${path}\t${secure}\t${exp}\t${c.name}\t${c.value}`);
    }
    return lines.join("\n");
  } catch (e) {
    console.warn("[mDm] Cookie extraction failed:", e);
    return "";
  }
}

async function sendDownloadToMdm({ url, kind, format, title, cookies }) {
  try {
    let finalKind = kind;
    if (!finalKind) {
      const checkStr = `${title || ""} ${url}`.toLowerCase();
      if (checkStr.match(/\.(exe|deb|rpm|apk|msi|appimage|zip|7z|tar|gz|bz2|xz|iso|img|bin|pdf|doc|docx|csv|xls|xlsx)(\?|&|\s|$)/)) {
        finalKind = "direct";
      }
    }
    let cookieData = cookies;
    if (!cookieData) {
      cookieData = await getNetscapeCookiesForUrl(url);
    }
    const payload = {
      url: url.trim(),
      kind: finalKind || undefined,
      format: format || undefined,
      title: title || undefined,
      cookies: cookieData || undefined,
    };
    const res = await fetch(`${MDM_API_BASE}/api/jobs`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });

    if (res.ok) {
      const job = await res.json();
      return { ok: true, job };
    } else {
      const text = await res.text();
      return { ok: false, error: text || `HTTP ${res.status}` };
    }
  } catch (err) {
    return {
      ok: false,
      error: "mDm desktop app is not reachable. Please ensure mDm is running on 127.0.0.1:5757.",
    };
  }
}

async function fetchMediaInfo(url) {
  try {
    const res = await fetch(`${MDM_API_BASE}/api/media-info`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url }),
    });
    if (res.ok) {
      const data = await res.json();
      return { ok: true, data };
    } else {
      return { ok: false, error: await res.text() };
    }
  } catch (err) {
    return { ok: false, error: String(err) };
  }
}

function showNotification(title, message) {
  chrome.notifications.create({
    type: "basic",
    iconUrl: "icons/icon128.png",
    title,
    message,
    priority: 1,
  });
}

// --- Message Dispatcher ----------------------------------------------------

chrome.runtime.onMessage.addListener((req, sender, sendResponse) => {
  if (req.action === "INTERCEPT_LINK_CLICK") {
    const { url, filename, title } = req.payload;
    if (showDialogEnabled) {
      openDownloadDialog({
        url,
        filename: filename || "",
        size: 0,
        referrer: sender.tab?.url || "",
      });
    } else {
      sendDownloadToMdm({ url, title: filename || title, kind: "direct" }).then((res) => {
        if (res.ok) {
          showNotification("mDm Download Started", `Downloading: ${filename || url}`);
        }
      });
    }
    sendResponse({ ok: true });
    return false;
  }
  if (req.action === "SEND_TO_MDM") {
    sendDownloadToMdm(req.payload).then(sendResponse);
    return true; // async reply
  }
  if (req.action === "CHECK_STATUS") {
    checkMdmHealth().then(sendResponse);
    return true;
  }
  if (req.action === "GET_MEDIA_INFO") {
    fetchMediaInfo(req.url).then(sendResponse);
    return true;
  }
  if (req.action === "GET_TAB_MEDIA") {
    const tabId = req.tabId || sender.tab?.id;
    const items = tabMedia.get(tabId);
    sendResponse({
      ok: true,
      items: items ? Array.from(items.values()) : [],
    });
    return false;
  }
  if (req.action === "GET_ACTIVE_JOBS") {
    fetch(`${MDM_API_BASE}/api/jobs`)
      .then((r) => (r.ok ? r.json() : []))
      .then((jobs) => sendResponse({ ok: true, jobs }))
      .catch((err) => sendResponse({ ok: false, error: String(err) }));
    return true;
  }
  if (req.action === "GET_SETTINGS") {
    chrome.storage.sync.get(
      {
        interceptDownloads: true,
        showDownloadDialog: true,
        captureExtensions: DEFAULT_CAPTURE_EXTENSIONS,
      },
      (cfg) => {
        sendResponse({ ok: true, settings: cfg, defaultExtensions: DEFAULT_CAPTURE_EXTENSIONS });
      }
    );
    return true;
  }
  if (req.action === "SAVE_SETTINGS") {
    chrome.storage.sync.set(req.settings, () => {
      sendResponse({ ok: true });
    });
    return true;
  }
});
