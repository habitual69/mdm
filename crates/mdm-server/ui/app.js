// mdm web UI — OmniGet-style shell: hero paste bar with a download card,
// grouped download cards, category sidebar, settings. Talks to the local
// REST + WS API on the same origin.
"use strict";

// Bootstrap theme and accent color instantly from localStorage
(function bootstrapAppearance() {
  try {
    const t = localStorage.getItem("mdm_theme") || "dark";
    const a = localStorage.getItem("mdm_accent") || "#e8934a";
    document.documentElement.dataset.theme = t;
    document.documentElement.style.setProperty("--accent", a);
    document.documentElement.style.setProperty("--accent-2", a);
    document.documentElement.style.setProperty("--accent-glow", `${a}33`);
  } catch (_) {}
})();

const $ = (id) => document.getElementById(id);

const COLOR_NAMES = {
  "#e8934a": "Amber Orange",
  "#10b981": "Emerald Green",
  "#3b82f6": "Electric Blue",
  "#8b5cf6": "Violet Purple",
  "#ef4444": "Crimson Ruby",
  "#ec4899": "Rose Pink",
  "#f59e0b": "Golden Sunset",
  "#06b6d4": "Cyan Teal",
};

function getColorName(hex) {
  if (!hex) return "Amber Orange";
  const lower = hex.toLowerCase();
  return COLOR_NAMES[lower] || `Custom (${lower})`;
}

function getJobCategory(job) {
  const dir = (job.output_dir || "").toLowerCase();
  if (dir.endsWith("/compress") || dir.endsWith("\\compress")) return "compress";
  if (dir.endsWith("/document") || dir.endsWith("\\document")) return "document";
  if (dir.endsWith("/videos") || dir.endsWith("\\videos")) return "video";
  if (dir.endsWith("/audios") || dir.endsWith("\\audios")) return "audio";
  if (dir.endsWith("/torrents") || dir.endsWith("\\torrents")) return "torrents";
  if (dir.endsWith("/miscellaneous") || dir.endsWith("\\miscellaneous")) return "misc";

  const text = (job.output_path || job.title || job.url || "").toLowerCase();
  const clean = text.split("?")[0].split("#")[0];
  const ext = clean.includes(".") ? clean.split(".").pop() : "";

  if (job.kind === "torrent" || job.kind === "magnet" || ext === "torrent") return "torrents";
  if (job.kind === "audio" || ["mp3", "flac", "wav", "ogg", "opus", "m4a", "aac", "wma", "alac", "mid"].includes(ext)) return "audio";
  if (job.kind === "video" || ["mp4", "mkv", "webm", "avi", "mov", "m4v", "ts", "wmv", "flv", "3gp"].includes(ext)) return "video";
  if (["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "img", "dmg", "apk", "deb", "rpm", "exe", "msi", "appimage", "bin"].includes(ext)) return "compress";
  if (["pdf", "doc", "docx", "txt", "md", "markdown", "xls", "xlsx", "csv", "ppt", "pptx", "html", "htm", "xml", "json", "epub"].includes(ext)) return "document";
  return "misc";
}

const CATS = {
  video: { title: "Video", filter: (j) => getJobCategory(j) === "video" },
  audio: { title: "Audio", filter: (j) => getJobCategory(j) === "audio" },
  compress: { title: "Compressed", filter: (j) => getJobCategory(j) === "compress" },
  document: { title: "Documents", filter: (j) => getJobCategory(j) === "document" },
  torrents: { title: "Torrents", filter: (j) => getJobCategory(j) === "torrents" },
  misc: { title: "Miscellaneous", filter: (j) => getJobCategory(j) === "misc" },
  files: { title: "Files", filter: (j) => j.kind === "direct" },
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
  folder: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2z"/></svg>',
};

// Icons for concrete file types (extension-based).
const FT_ICONS = {
  video: KIND_ICONS.video,
  audio: KIND_ICONS.audio,
  torrent: KIND_ICONS.torrent,
  image: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><circle cx="8.6" cy="9.2" r="1.7" fill="currentColor" stroke="none"/><path d="M4.5 16.8 9 12.6l3.2 3 2.6-2.5 4.7 4.4"/></svg>',
  archive: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><rect x="4.5" y="3.5" width="15" height="17" rx="2.5"/><path d="M9.5 3.5v3.4h5V3.5"/><path d="M4.5 14.5h15"/><path d="M12 17.5v2.8"/></svg>',
  doc: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6.5 3.5h7.2L18 8v12.5H6.5z"/><path d="M13.3 3.8v4.6h4.4"/><path d="M9.3 12.2h5.8M9.3 15.4h5.8"/></svg>',
  iso: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="12" cy="12" r="8.4"/><circle cx="12" cy="12" r="2.6"/><path d="M12 6.7a5.3 5.3 0 0 1 5.3 5.3"/></svg>',
  sub: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M6.8 13.4h6.4M6.8 16.2h4.2M14.8 16.2h2.4"/></svg>',
  app: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><rect x="4" y="4" width="16" height="16" rx="3"/><path d="M4.6 9h14.8"/><circle cx="7.6" cy="6.6" r="1" fill="currentColor" stroke="none"/><circle cx="10.6" cy="6.6" r="1" fill="currentColor" stroke="none"/></svg>',
  ext: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M7 3.5h6.6L18.2 8v12.5H7z"/><path d="M13.2 3.8v4.7h4.5"/></svg>',
};

// extension -> file-type icon key
const EXT_KIND = new Map([
  ["mp4", "video"], ["mkv", "video"], ["webm", "video"], ["avi", "video"],
  ["mov", "video"], ["m4v", "video"], ["ts", "video"], ["wmv", "video"],
  ["flv", "video"], ["mpg", "video"], ["mpeg", "video"], ["3gp", "video"],
  ["mp3", "audio"], ["flac", "audio"], ["wav", "audio"], ["ogg", "audio"],
  ["opus", "audio"], ["m4a", "audio"], ["aac", "audio"], ["wma", "audio"],
  ["jpg", "image"], ["jpeg", "image"], ["png", "image"], ["gif", "image"],
  ["webp", "image"], ["bmp", "image"], ["avif", "image"], ["heic", "image"],
  ["tiff", "image"], ["psd", "image"],
  ["zip", "archive"], ["rar", "archive"], ["7z", "archive"], ["tar", "archive"],
  ["gz", "archive"], ["xz", "archive"], ["bz2", "archive"], ["zst", "archive"],
  ["tgz", "archive"], ["cab", "archive"],
  ["pdf", "doc"], ["doc", "doc"], ["docx", "doc"], ["rtf", "doc"], ["txt", "doc"],
  ["md", "doc"], ["epub", "doc"], ["mobi", "doc"], ["csv", "doc"], ["xls", "doc"],
  ["xlsx", "doc"], ["ppt", "doc"], ["pptx", "doc"], ["odt", "doc"],
  ["iso", "iso"], ["img", "iso"], ["cue", "iso"], ["bin", "iso"], ["mds", "iso"],
  ["vhd", "iso"], ["vdi", "iso"], ["vmdk", "iso"],
  ["srt", "sub"], ["ass", "sub"], ["ssa", "sub"], ["sub", "sub"], ["vtt", "sub"],
  ["sbv", "sub"],
  ["torrent", "torrent"],
  ["exe", "app"], ["msi", "app"], ["appimage", "app"], ["deb", "app"],
  ["rpm", "app"], ["dmg", "app"], ["apk", "app"], ["flatpak", "app"],
]);

function extOf(s) {
  s = String(s || "").trim();
  if (!s) return "";
  try {
    const u = new URL(s);
    // Only the path carries a real extension; "https://example.io" must not
    // read like a .io file.
    if (/^(https?|ftp):$/.test(u.protocol)) s = u.pathname;
  } catch {
    // not an absolute URL — treat the string as a filename/path.
  }
  const m = s.split(/[?#]/)[0].match(/\.([a-z0-9]{1,8})$/i);
  return m ? m[1].toLowerCase() : "";
}

// Icon for a single file name / path (picker rows, saved files).
function fileIcon(name) {
  return FT_ICONS[EXT_KIND.get(extOf(name)) || "ext"] || FT_ICONS.ext;
}

// Icon for a whole download: torrent/magnet keep their glyphs; media kinds
// stay video/audio unless the URL points at a distinct file type (a .zip
// link must not render as a film frame).
function jobIcon(job) {
  if (!job) return KIND_ICONS.generic;
  if (job.kind === "magnet") return KIND_ICONS.magnet;
  if (job.kind === "torrent") return KIND_ICONS.torrent;
  const ext = extOf(job.url) || extOf(job.title) || extOf(job.output_path);
  if (ext) {
    const k = EXT_KIND.get(ext);
    if (k && FT_ICONS[k]) return FT_ICONS[k];
  }
  if (job.kind === "video" || job.kind === "audio") return KIND_ICONS[job.kind];
  return KIND_ICONS.direct;
}

const jobs = new Map();   // id -> job (latest)
const cards = new Map();  // id -> { el, refs }
let kindFilter = null;    // null | 'video'|'audio'|'torrents'|'files'
let stateFilter = "all";  // all | active | done | failed
let searchQuery = "";
let qualityFmt = "";      // quality segment format value
let cfgCache = null;      // last known config

// thumbnail sources; the <img> falls back to the kind icon on error.
function ytId(url) {
  if (!url) return null;
  try {
    const u = new URL(url);
    if (u.hostname.includes("youtube.com") || u.hostname.includes("youtu.be")) {
      if (u.hostname === "youtu.be" || u.pathname.startsWith("/shorts/") || u.pathname.startsWith("/embed/")) {
        const seg = u.pathname.split("/").filter(Boolean);
        return seg[seg.length - 1] || null;
      }
      return u.searchParams.get("v");
    }
  } catch (_) {}
  const m = String(url).match(/(?:[?&]v=|youtu\.be\/|\/shorts\/|\/embed\/)([A-Za-z0-9_-]{11})/);
  return m ? m[1] : null;
}

function thumbSrcs(job) {
  const out = [];
  const host = safeHost(job.url);
  const id = ytId(job.url);
  if (id) {
    out.push(`https://i.ytimg.com/vi/${id}/mqdefault.jpg`);
    out.push(`https://i.ytimg.com/vi/${id}/hqdefault.jpg`);
    out.push(`https://i.ytimg.com/vi/${id}/default.jpg`);
  }
  if (host) {
    out.push(`https://icons.duckduckgo.com/ip3/${host}.ico`);
  }
  return out;
}

function thumb(kind, job) {
  const box = mk("div", "thumb");
  const fallbackIcon = icon(jobIcon(job));
  fallbackIcon.classList.add("thumb-fallback");
  box.appendChild(fallbackIcon);

  const srcs = job ? thumbSrcs(job) : [];
  if (srcs.length) {
    const img = mk("img", "thumb-img");
    let i = 0;
    img.src = srcs[0];
    img.referrerPolicy = "no-referrer";
    img.loading = "lazy";
    img.alt = "";
    img.onload = () => {
      box.classList.add("filled");
    };
    if (img.complete && img.naturalWidth > 0) {
      box.classList.add("filled");
    }
    img.onerror = () => {
      i += 1;
      if (i < srcs.length) {
        img.src = srcs[i];
      } else {
        img.remove();
        box.classList.remove("filled");
      }
    };
    box.appendChild(img);
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
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function fmtEta(sec) {
  if (!sec || sec < 0 || !isFinite(sec)) return "—";
  const s = Math.floor(sec);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

function escapeHtml(s) {
  return String(s || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
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
  const lower = u.toLowerCase().split("?")[0].split("#")[0];
  if (lower.endsWith(".torrent")) return "torrent";
  const ext = lower.split(".").pop();
  const DIRECT_EXTS = new Set([
    "exe", "msi", "apk", "appimage", "deb", "rpm", "dmg", "pkg", "bin",
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "tgz", "tbz2",
    "iso", "img", "cab", "vhd", "vdi", "vmdk",
    "pdf", "doc", "docx", "rtf", "txt", "md", "epub", "mobi", "csv",
    "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp"
  ]);
  const AUDIO_EXTS = new Set(["mp3", "flac", "wav", "ogg", "opus", "m4a", "aac", "wma", "alac"]);
  if (DIRECT_EXTS.has(ext)) return "direct";
  if (AUDIO_EXTS.has(ext)) return "audio";
  if (u.toLowerCase().match(/\.(exe|deb|rpm|apk|zip|iso|msi|appimage|7z)(\?|$)/)) return "direct";
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
  const f = (job.format || "").trim().toLowerCase();
  if (job.kind === "audio") {
    if (f === "flac" || f.includes("flac")) return "FLAC";
    if (f === "mp3" || f.includes("mp3")) return "MP3";
    if (f === "m4a" || f.includes("m4a")) return "M4A";
    if (f === "opus" || f.includes("opus")) return "OPUS";
    if (f === "wav" || f.includes("wav")) return "WAV";
    if (f === "bestaudio/best" || f === "bestaudio" || f === "best" || !f) {
      const cfgFmt = (cfgCache && cfgCache.ytdlp_audio_format && cfgCache.ytdlp_audio_format !== "best")
        ? cfgCache.ytdlp_audio_format.toUpperCase()
        : "MP3";
      return cfgFmt;
    }
    return f.toUpperCase();
  }
  if (job.kind === "video") {
    const m = f.match(/height<=?(\d+)/);
    if (m) return `${m[1]}p`;
    if (f.includes("1080p")) return "1080p";
    if (f.includes("720p")) return "720p";
    if (f === "bestvideo/best" || f === "bestvideo") return "No Sound";
    if (!f || f === "best") {
      if (cfgCache && cfgCache.ytdlp_max_height > 0) return `${cfgCache.ytdlp_max_height}p`;
      return "Best Video";
    }
    return f.length > 14 ? `${f.slice(0, 12)}…` : f;
  }
  if (job.kind === "direct") {
    const ext = (job.title || job.url).split("?")[0].split(".").pop();
    if (ext && ext.length <= 5 && /^[a-z0-9]+$/i.test(ext)) {
      return ext.toUpperCase();
    }
  }
  if (!f || f === "best") return null;
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

// ---------- audio & toast notification system ----------

let audioCtx = null;
function playNotificationSound(type = "complete") {
  if (cfgCache && cfgCache.notification_sound === false) return;
  try {
    const AudioContextClass = window.AudioContext || window.webkitAudioContext;
    if (!AudioContextClass) return;
    if (!audioCtx) audioCtx = new AudioContextClass();
    if (audioCtx.state === "suspended") {
      audioCtx.resume();
    }
    const now = audioCtx.currentTime;
    const osc = audioCtx.createOscillator();
    const gain = audioCtx.createGain();
    osc.connect(gain);
    gain.connect(audioCtx.destination);

    if (type === "complete") {
      // Pleasant two-tone ascending chime (587.33 Hz D5 -> 880 Hz A5)
      osc.type = "sine";
      osc.frequency.setValueAtTime(587.33, now);
      osc.frequency.exponentialRampToValueAtTime(880.0, now + 0.12);
      gain.gain.setValueAtTime(0.14, now);
      gain.gain.exponentialRampToValueAtTime(0.001, now + 0.45);
      osc.start(now);
      osc.stop(now + 0.45);
    } else if (type === "error") {
      // Descending minor alert (330 Hz E4 -> 220 Hz A3)
      osc.type = "triangle";
      osc.frequency.setValueAtTime(330, now);
      osc.frequency.setValueAtTime(220, now + 0.15);
      gain.gain.setValueAtTime(0.18, now);
      gain.gain.exponentialRampToValueAtTime(0.001, now + 0.5);
      osc.start(now);
      osc.stop(now + 0.5);
    } else {
      osc.type = "sine";
      osc.frequency.setValueAtTime(659.25, now);
      gain.gain.setValueAtTime(0.12, now);
      gain.gain.exponentialRampToValueAtTime(0.001, now + 0.2);
      osc.start(now);
      osc.stop(now + 0.2);
    }
  } catch (_) {}
}

const TOAST_ICONS = {
  success: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>`,
  error: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>`,
  warn: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>`,
  info: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>`,
  close: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>`
};

function showToast({ type = "info", title, message, actions = [], timeout = 5000 }) {
  const container = $("toast-container");
  if (!container) return () => {};

  const toast = mk("div", `toast ${type}`);

  const icoWrap = mk("div", "toast-icon");
  icoWrap.innerHTML = TOAST_ICONS[type] || TOAST_ICONS.info;
  toast.appendChild(icoWrap);

  const body = mk("div", "toast-body");
  if (title) {
    const tEl = mk("div", "toast-title", title);
    body.appendChild(tEl);
  }
  if (message) {
    const mEl = mk("div", "toast-msg", message);
    body.appendChild(mEl);
  }

  if (actions && actions.length > 0) {
    const acts = mk("div", "toast-actions");
    for (const act of actions) {
      const btn = mk("button", "toast-btn", act.label);
      btn.type = "button";
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        try { act.onClick(); } catch (_) {}
        dismissToast(toast);
      });
      acts.appendChild(btn);
    }
    body.appendChild(acts);
  }
  toast.appendChild(body);

  const closeBtn = mk("button", "toast-close");
  closeBtn.type = "button";
  closeBtn.title = "Dismiss";
  closeBtn.innerHTML = TOAST_ICONS.close;
  closeBtn.addEventListener("click", () => dismissToast(toast));
  toast.appendChild(closeBtn);

  if (timeout > 0) {
    const prog = mk("div", "toast-progress");
    prog.style.animation = `toastProgress ${timeout}ms linear forwards`;
    toast.appendChild(prog);
  }

  container.appendChild(toast);

  let hideTimer = 0;
  if (timeout > 0) {
    hideTimer = setTimeout(() => dismissToast(toast), timeout);
  }

  function dismissToast(el) {
    if (el.dataset.dismissed) return;
    el.dataset.dismissed = "1";
    clearTimeout(hideTimer);
    el.classList.add("toast-hiding");
    setTimeout(() => el.remove(), 220);
  }

  return () => dismissToast(toast);
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

// Engine chip: yt-dlp media jobs hand the byte transfer to aria2 when
// the offload setting is on — show that in the UI.
function engineTag(job) {
  if (
    job.engine === "ytdlp" &&
    (job.kind === "video" || job.kind === "audio") &&
    cfgCache &&
    cfgCache.aria2_offload !== false
  ) {
    return "ytdlp·aria2";
  }
  return job.engine;
}

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
  meta.appendChild(mk("span", "engine-tag", engineTag(job)));
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
  const act = (html, cls, method, path, okMsg, title) => {
    const b = mk("button", `icon-btn${cls ? ` ${cls}` : ""}`);
    if (title) b.title = title;
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
    act(ACT_ICONS.pause, "", "POST", `/api/jobs/${job.id}/pause`, "Paused", "Pause download");
  } else if (job.state === "paused") {
    act(ACT_ICONS.resume, "", "POST", `/api/jobs/${job.id}/resume`, "Resumed", "Resume download");
  } else if (job.state === "failed") {
    act(ACT_ICONS.resume, "", "POST", `/api/jobs/${job.id}/resume`, "Retrying download", "Resume / Retry download");
  }
  if (job.state === "done" && (job.output_path || job.output_dir)) {
    const folder = mk("button", "icon-btn");
    folder.title = "Show in folder";
    folder.appendChild(icon(ACT_ICONS.folder));
    folder.addEventListener("click", async () => {
      try {
        const res = await fetch(`/api/jobs/${job.id}/open-folder`, { method: "POST" });
        if (res.ok) {
          notice("Opened in file manager");
        } else {
          notice("File or folder not found", true);
        }
      } catch (e) {
        notice(`Failed to open: ${e}`, true);
      }
    });
    refs.acts.appendChild(folder);
  }
  if (job.output_path && job.state === "done") {
    const copy = mk("button", "icon-btn");
    copy.title = `Copy path: ${job.output_path}`;
    copy.appendChild(icon(ACT_ICONS.copy));
    copy.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(job.output_path);
        notice("Path copied to clipboard");
      } catch (_) {
        notice(job.output_path);
      }
    });
    refs.acts.appendChild(copy);
  }
  act(ACT_ICONS.rm, "danger", "DELETE", `/api/jobs/${job.id}`, "Removed", "Delete download");
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
  } else if (job.state === "done" && (job.output_path || job.output_dir)) {
    const dispPath = job.output_path || job.output_dir;
    r.note.textContent = dispPath;
    r.note.className = "job-note mono clickable-path";
    r.note.title = "Click to show in folder";
    r.note.onclick = async () => {
      try {
        const res = await fetch(`/api/jobs/${job.id}/open-folder`, { method: "POST" });
        if (res.ok) notice("Opened in file manager");
      } catch (_) {}
    };
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
  if (kindFilter && CATS[kindFilter] && !CATS[kindFilter].filter(job)) return false;
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

  // live speed & progress indicator pills (topbar, downloads header, playlist dialog)
  const aggDown = running.reduce((a, j) => a + (j.speed_bps > 0 ? j.speed_bps : 0), 0);
  const aggUp = running.reduce((a, j) => a + (j.upload_speed_bps > 0 ? j.upload_speed_bps : 0), 0);
  const pct = running.length ? (running.reduce((a, j) => a + Math.min(100, Math.max(0, j.progress || 0)), 0) / running.length) : 0;
  const sDown = fmtSpeed(aggDown);
  const sUp = fmtSpeed(aggUp);
  const sPct = `${pct.toFixed(1)}%`;
  const sPctTxt = fmtPct(pct);
  const isActive = running.length > 0;

  if ($("tb-transfer-btn")) {
    $("tb-down-val").textContent = sDown;
    $("tb-up-val").textContent = sUp;
    $("tb-prog-fill").style.width = sPct;
    $("tb-prog-text").textContent = sPctTxt;
    $("tb-status-dot").classList.toggle("active", isActive);
  }

  if ($("dl-status-pill")) {
    $("dl-down-val").textContent = sDown;
    $("dl-up-val").textContent = sUp;
    $("dl-prog-fill").style.width = sPct;
    $("dl-prog-text").textContent = sPctTxt;
    $("dl-status-dot").classList.toggle("active", isActive);
  }

  if ($("pl-status-pill")) {
    $("pl-down-val").textContent = sDown;
    $("pl-up-val").textContent = sUp;
    $("pl-prog-fill").style.width = sPct;
    $("pl-prog-text").textContent = sPctTxt;
    $("pl-dot").classList.toggle("active", isActive);
  }
  updateActiveSheet(running);
}

function updateActiveSheet(running) {
  const sheet = $("active-sheet");
  if (!sheet) return;
  $("sheet-stats").textContent = `${running.length} active task${running.length === 1 ? "" : "s"}`;
  const list = $("sheet-list");
  const empty = $("sheet-empty");
  if (!running.length) {
    list.innerHTML = "";
    empty.classList.remove("hidden");
    return;
  }
  empty.classList.add("hidden");
  list.innerHTML = running.map(j => {
    const name = escapeHtml(j.title || j.url.split("/").pop() || j.id.slice(0, 8));
    const downSp = fmtSpeed(j.speed_bps || 0);
    const upSp = (j.upload_speed_bps && j.upload_speed_bps > 0) ? ` · ↑ ${fmtSpeed(j.upload_speed_bps)}` : "";
    const pct = Math.min(100, Math.max(0, j.progress || 0)).toFixed(1);
    const eta = j.eta_sec ? ` · ${fmtEta(j.eta_sec)}` : "";
    return `
      <div class="sheet-item" data-id="${j.id}">
        <div class="sheet-item-row">
          <span class="sheet-item-name" title="${name}">${name}</span>
          <span class="sheet-item-meta"><b class="sp-down">↓ ${downSp}</b>${upSp}<span>${pct}%</span>${eta}</span>
        </div>
        <div class="sheet-item-bar"><i style="width: ${pct}%"></i></div>
      </div>
    `;
  }).join("");
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
    row.appendChild(icon(fileIcon(f.name)));
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

function isPlaylistUrl(url) {
  if (!url) return false;
  try {
    const u = new URL(url);
    if ((u.hostname.includes("youtube.com") || u.hostname.includes("youtu.be")) && u.searchParams.has("list")) {
      return true;
    }
    if (u.pathname.includes("/playlist") || u.pathname.includes("/sets/")) {
      return true;
    }
  } catch (_) {
    if (/[?&]list=[a-zA-Z0-9_-]+/.test(url) || /\/playlist[/?]/.test(url)) {
      return true;
    }
  }
  return false;
}

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
  const isPlaylist = isPlaylistUrl(url);

  if (isPlaylist) {
    $("pv-site").textContent = "Playlist";
    $("pv-type").textContent = "Playlist";
    $("add-btn").textContent = "Inspect & Download Playlist…";
  } else {
    $("pv-site").textContent =
      kind === "magnet" ? "Magnet link"
      : kind === "torrent" ? "BitTorrent"
      : (siteLabel({ kind, url }) || "Link");
    $("pv-type").textContent = { video: "Video", audio: "Audio", direct: "File", torrent: "Torrent", magnet: "Magnet" }[kind];
    $("add-btn").textContent = `Download ${{ video: "video", audio: "audio", direct: "file", torrent: "torrent", magnet: "magnet" }[kind]}`;
  }

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

// Upload a picked .torrent file: store it via the server, then feed the
// stored path back into the normal preview flow (with per-file selection).
$("browse-btn").addEventListener("click", () => $("torrent-file").click());
$("torrent-file").addEventListener("change", async (e) => {
  const f = e.target.files && e.target.files[0];
  e.target.value = ""; // allow re-picking the same file later
  if (!f) return;
  if (!/\.torrent$/i.test(f.name)) return notice("pick a .torrent file", true);
  if (f.size > 8 * 1024 * 1024) return notice("torrent file too large (>8 MiB)", true);
  try {
    const b64 = await new Promise((res, rej) => {
      const r = new FileReader();
      r.onload = () => res(String(r.result).split(",")[1] || "");
      r.onerror = () => rej(r.error || new Error("read failed"));
      r.readAsDataURL(f);
    });
    const res = await fetch("/api/torrent-upload", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: f.name, b64 }),
    });
    if (!res.ok) throw new Error((await res.text()) || `error ${res.status}`);
    const { path } = await res.json();
    go("#/home");
    $("url").value = path;
    refreshPreview();
    notice(`Loaded ${f.name} — pick files to download`);
  } catch (err) {
    notice(`upload failed: ${err}`, true);
  }
});
$("quality-seg").addEventListener("click", (e) => {
  const b = e.target.closest(".seg-btn");
  if (!b) return;
  qualityFmt = b.dataset.fmt;
  for (const s of $("quality-seg").children) s.classList.toggle("on", s === b);
  if (qualityFmt === "bestaudio/best") {
    $("kind").value = "audio";
    const audioFmt = (cfgCache && cfgCache.ytdlp_audio_format && cfgCache.ytdlp_audio_format !== "best")
      ? cfgCache.ytdlp_audio_format
      : "mp3";
    $("format").value = audioFmt;
  } else if (qualityFmt === "bestvideo/best") {
    $("kind").value = "video";
    $("format").value = qualityFmt;
  } else {
    // Auto
    $("kind").value = "";
    $("format").value = "";
  }
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

// ---------- playlist download inspector ----------

let curPlaylist = null;
const plSelectedIndices = new Set();
let plSearchQuery = "";

function openPlaylistModal(url) {
  curPlaylist = null;
  plSelectedIndices.clear();
  plSearchQuery = "";
  $("pl-search-input").value = "";
  $("playlist-modal").classList.remove("hidden");
  $("pl-loading").classList.remove("hidden");
  $("pl-content").classList.add("hidden");
  $("pl-btn-single").classList.add("hidden");
  $("pl-btn-download").disabled = true;
  $("pl-modal-title").textContent = "Analyzing playlist…";
  $("pl-modal-sub").textContent = "Fetching video list from server…";

  // Pre-select format based on active choice on home page or config
  const homeKind = $("kind").value;
  const isAudioActive = homeKind === "audio" || (qualityFmt && qualityFmt.includes("audio"));
  if (isAudioActive) {
    const preferredAudio = (cfgCache && cfgCache.ytdlp_audio_format === "flac") ? "audio:flac" : "audio:mp3";
    $("pl-format-select").value = preferredAudio;
  } else {
    $("pl-format-select").value = "video:best";
  }

  fetch("/api/playlist-info", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ url }),
  })
    .then(async (res) => {
      if (!res.ok) throw new Error((await res.text()) || `error ${res.status}`);
      return res.json();
    })
    .then((data) => {
      if (!data.is_playlist || !data.entries || data.entries.length === 0) {
        // Not a playlist, close modal and submit normally as single job
        closePlaylistModal();
        submitSingleJob(url);
        return;
      }
      curPlaylist = data;
      $("pl-modal-title").textContent = data.title || "Playlist";
      $("pl-modal-sub").textContent = `${data.uploader ? data.uploader + " · " : ""}${data.count} items`;
      
      // Select all by default
      data.entries.forEach(e => plSelectedIndices.add(e.index));

      if (data.single_video_url) {
        $("pl-btn-single").classList.remove("hidden");
        const titleSnippet = (data.single_video_title || "Current Video").slice(0, 32);
        $("pl-btn-single").textContent = `Download Single Video (${titleSnippet}…)`;
      } else {
        $("pl-btn-single").classList.add("hidden");
      }

      $("pl-loading").classList.add("hidden");
      $("pl-content").classList.remove("hidden");
      renderPlaylistItems();
    })
    .catch((err) => {
      notice(`Playlist inspection failed: ${err.message || err}`, true);
      closePlaylistModal();
    });
}

function closePlaylistModal() {
  $("playlist-modal").classList.add("hidden");
  curPlaylist = null;
  plSelectedIndices.clear();
}

function renderPlaylistItems() {
  if (!curPlaylist) return;
  const listEl = $("pl-items-list");
  listEl.innerHTML = "";
  const filter = plSearchQuery.toLowerCase().trim();

  const visibleEntries = curPlaylist.entries.filter((e) => {
    if (!filter) return true;
    return (e.title && e.title.toLowerCase().includes(filter))
      || (e.uploader && e.uploader.toLowerCase().includes(filter));
  });

  if (visibleEntries.length === 0) {
    const empty = document.createElement("div");
    empty.style.cssText = "padding: 24px; text-align: center; color: var(--muted); font-size: 13px;";
    empty.textContent = "No videos match search filter.";
    listEl.appendChild(empty);
  } else {
    for (const entry of visibleEntries) {
      const isSelected = plSelectedIndices.has(entry.index);
      const row = document.createElement("div");
      row.className = `pl-item-row ${isSelected ? "is-selected" : ""}`;

      const cb = document.createElement("input");
      cb.type = "checkbox";
      cb.className = "pl-item-check";
      cb.checked = isSelected;
      cb.dataset.idx = entry.index;

      cb.addEventListener("click", (e) => {
        e.stopPropagation();
        togglePlaylistItem(entry.index, cb.checked);
      });

      row.addEventListener("click", () => {
        cb.checked = !cb.checked;
        togglePlaylistItem(entry.index, cb.checked);
      });

      const idxSpan = document.createElement("span");
      idxSpan.className = "pl-item-idx";
      idxSpan.textContent = `#${entry.index}`;

      const img = document.createElement("img");
      img.className = "pl-item-thumb";
      img.referrerPolicy = "no-referrer";
      const thumbUrl = entry.thumbnail || (entry.id ? `https://i.ytimg.com/vi/${entry.id}/mqdefault.jpg` : "");
      img.src = thumbUrl;
      img.alt = "";
      img.loading = "lazy";
      img.onerror = () => {
        if (entry.id && !img.src.includes("hqdefault")) {
          img.src = `https://i.ytimg.com/vi/${entry.id}/hqdefault.jpg`;
        } else {
          img.style.display = "none";
        }
      };

      const infoDiv = document.createElement("div");
      infoDiv.className = "pl-item-info";

      const titleEl = document.createElement("span");
      titleEl.className = "pl-item-title";
      titleEl.textContent = entry.title || "Untitled";
      titleEl.title = entry.title || "";

      const authorEl = document.createElement("span");
      authorEl.className = "pl-item-author";
      authorEl.textContent = entry.uploader || "";

      infoDiv.appendChild(titleEl);
      if (entry.uploader) infoDiv.appendChild(authorEl);

      row.appendChild(cb);
      row.appendChild(idxSpan);
      if (thumbUrl) row.appendChild(img);
      row.appendChild(infoDiv);

      if (entry.duration_string) {
        const durEl = document.createElement("span");
        durEl.className = "pl-item-dur";
        durEl.textContent = entry.duration_string;
        row.appendChild(durEl);
      }

      listEl.appendChild(row);
    }
  }

  updatePlaylistCounts();
}

function togglePlaylistItem(index, checked) {
  if (checked) {
    plSelectedIndices.add(index);
  } else {
    plSelectedIndices.delete(index);
  }
  updatePlaylistCounts();
  document.querySelectorAll(".pl-item-row").forEach((row) => {
    const cb = row.querySelector(".pl-item-check");
    if (cb) {
      row.classList.toggle("is-selected", cb.checked);
    }
  });
}

function updatePlaylistCounts() {
  if (!curPlaylist) return;
  const total = curPlaylist.entries.length;
  const count = plSelectedIndices.size;
  $("pl-selected-count").textContent = `${count} / ${total} selected`;
  $("pl-btn-download").textContent = `Download Selected (${count})`;
  $("pl-btn-download").disabled = count === 0;

  const allCb = $("pl-check-all");
  allCb.checked = count === total && total > 0;
  allCb.indeterminate = count > 0 && count < total;
}

$("pl-check-all").addEventListener("change", (e) => {
  if (!curPlaylist) return;
  const checked = e.target.checked;
  if (checked) {
    curPlaylist.entries.forEach(item => plSelectedIndices.add(item.index));
  } else {
    plSelectedIndices.clear();
  }
  renderPlaylistItems();
});

$("pl-search-input").addEventListener("input", (e) => {
  plSearchQuery = e.target.value;
  renderPlaylistItems();
});

$("pl-btn-close").addEventListener("click", closePlaylistModal);
$("pl-btn-cancel").addEventListener("click", closePlaylistModal);
$("playlist-modal").addEventListener("click", (e) => {
  if (e.target === $("playlist-modal")) closePlaylistModal();
});

$("pl-btn-single").addEventListener("click", () => {
  if (!curPlaylist || !curPlaylist.single_video_url) return;
  const singleUrl = curPlaylist.single_video_url;
  closePlaylistModal();
  submitSingleJob(singleUrl);
});

$("pl-btn-download").addEventListener("click", async () => {
  if (!curPlaylist || plSelectedIndices.size === 0) return;
  const fmtChoice = $("pl-format-select").value;
  let kind = "video";
  let format = "best";
  if (fmtChoice.startsWith("audio:")) {
    kind = "audio";
    format = fmtChoice.slice("audio:".length); // "flac", "mp3", "m4a"
  } else if (fmtChoice.startsWith("video:")) {
    kind = "video";
    const vFmt = fmtChoice.slice("video:".length);
    if (vFmt === "1080p") {
      format = "bv*[height<=1080]+ba/b[height<=1080][acodec!=none]/best";
    } else if (vFmt === "720p") {
      format = "bv*[height<=720]+ba/b[height<=720][acodec!=none]/best";
    } else {
      format = "best";
    }
  } else if (fmtChoice === "audio") {
    kind = "audio";
    format = (cfgCache && cfgCache.ytdlp_audio_format && cfgCache.ytdlp_audio_format !== "best") ? cfgCache.ytdlp_audio_format : "mp3";
  }

  const selectedEntries = curPlaylist.entries.filter(e => plSelectedIndices.has(e.index));
  const jobs = selectedEntries.map(e => ({
    url: e.url,
    title: e.title,
    kind,
    format: format || undefined,
  }));

  const btn = $("pl-btn-download");
  btn.disabled = true;
  btn.textContent = `Adding ${jobs.length} downloads…`;

  try {
    const res = await fetch("/api/jobs/batch", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jobs }),
    });
    if (!res.ok) throw new Error((await res.text()) || `error ${res.status}`);
    const data = await res.json();
    notice(`Added ${data.added || jobs.length} videos from playlist to queue!`);
    closePlaylistModal();
    $("url").value = "";
    refreshPreview();
    healthSoon();
    go("#/downloads");
  } catch (err) {
    notice(`Failed to add playlist: ${err}`, true);
    btn.disabled = false;
    btn.textContent = `Download Selected (${jobs.length})`;
  }
});

async function submitSingleJob(urlOverride) {
  const url = (urlOverride || $("url").value).trim();
  if (!url) return;
  let kind = $("kind").value || undefined;
  let format = ($("format").value.trim() || qualityFmt) || undefined;
  if (!kind && qualityFmt === "bestaudio/best") {
    kind = "audio";
    if (!format || format === "bestaudio/best") {
      format = (cfgCache && cfgCache.ytdlp_audio_format && cfgCache.ytdlp_audio_format !== "best") ? cfgCache.ytdlp_audio_format : "mp3";
    }
  }
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
      go("#/downloads");
    } else {
      notice((await res.text()) || `error ${res.status}`, true);
    }
  } catch (err) {
    notice(`request failed: ${err}`, true);
  } finally {
    btn.disabled = false;
  }
}

$("add-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const url = $("url").value.trim();
  if (!url) return;
  if (isPlaylistUrl(url)) {
    openPlaylistModal(url);
    return;
  }
  await submitSingleJob();
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
    $("set-speed-limit").value = cfg.speed_limit || "";
    $("set-proxy").value = cfg.proxy || "";
    $("set-write-subs").checked = !!cfg.write_subtitles;
    $("set-write-auto-subs").checked = !!cfg.write_auto_subs;
    $("set-embed-metadata").checked = cfg.embed_metadata !== false;
    $("set-embed-thumbnail").checked = cfg.embed_thumbnail !== false;
    $("set-aria2-offload").checked = cfg.aria2_offload !== false;
    $("set-aria2-fallback").checked = cfg.aria2_fallback !== false;
    $("set-aria2-conn").value = cfg.aria2_connections ?? 8;
    $("set-aria2-split").value = cfg.aria2_splits ?? 8;
    $("set-aria2-timeout").value = cfg.aria2_connect_timeout ?? 30;
    $("set-aria2-cache").value = cfg.aria2_disk_cache ?? 64;
    $("set-aria2-prealloc").checked = cfg.aria2_file_allocation !== "none";
    $("set-aria2-minsplit").value = cfg.aria2_min_split_size ?? 4;
    $("set-aria2-tries").value = cfg.aria2_max_tries ?? 3;
    $("set-aria2-retrywait").value = cfg.aria2_retry_wait ?? 5;
    $("set-aria2-slowlimit").value = cfg.aria2_lowest_speed_limit ?? 0;
    $("set-aria2-btpeers").value = cfg.aria2_bt_max_peers ?? 128;
    $("set-aria2-ua").value = cfg.aria2_user_agent || "";
    $("set-aria2-tracker").value = cfg.aria2_custom_tracker_url || "";
    $("set-ytdlp-quality").value = String(cfg.ytdlp_max_height ?? 0);
    $("set-ytdlp-merge").value = cfg.ytdlp_merge_format || "mkv";
    if ($("set-ytdlp-cookies")) $("set-ytdlp-cookies").value = cfg.ytdlp_cookies_from_browser ?? "";
    $("set-ytdlp-audio").value = cfg.ytdlp_audio_format || "mp3";
    $("set-ytdlp-frags").value = cfg.ytdlp_fragments ?? 4;
    $("set-ytdlp-embed-subs").checked = !!cfg.ytdlp_embed_subs;
    if ($("set-notifications-enabled")) {
      $("set-notifications-enabled").checked = cfg.notifications_enabled !== false;
    }
    if ($("set-autostart")) {
      $("set-autostart").checked = !!cfg.autostart;
    }
    if ($("set-notification-sound")) {
      $("set-notification-sound").checked = cfg.notification_sound !== false;
    }
    applyAppearance(cfg.theme, cfg.accent_color, cfg.widget_shape);
    loadBinsStatus();
    loadExtensionStatus();
    const dirty = $("settings-dirty");
    if (dirty) dirty.classList.add("hidden");
  } catch (e) {
    notice(`cannot load settings: ${e}`, true);
  }
}

$("settings-form").addEventListener("input", () => {
  const dirty = $("settings-dirty");
  if (dirty) dirty.classList.remove("hidden");
});
$("settings-form").addEventListener("change", () => {
  const dirty = $("settings-dirty");
  if (dirty) dirty.classList.remove("hidden");
});

$("settings-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const body = {
    output_dir: $("set-output-dir").value.trim(),
    max_concurrent: Number($("set-max-concurrent").value) || undefined,
    max_retries: Number($("set-max-retries").value) || 0,
    seed_minutes: Number($("set-seed-minutes").value) || 0,
    speed_limit: $("set-speed-limit").value.trim() || null,
    proxy: $("set-proxy").value.trim() || null,
    write_subtitles: $("set-write-subs").checked,
    write_auto_subs: $("set-write-auto-subs").checked,
    embed_metadata: $("set-embed-metadata").checked,
    embed_thumbnail: $("set-embed-thumbnail").checked,
    aria2_offload: $("set-aria2-offload").checked,
    aria2_fallback: $("set-aria2-fallback").checked,
    aria2_connections: Number($("set-aria2-conn").value) || undefined,
    aria2_splits: Number($("set-aria2-split").value) || undefined,
    aria2_connect_timeout: Number($("set-aria2-timeout").value) || undefined,
    aria2_disk_cache: Number($("set-aria2-cache").value) || 0,
    aria2_file_allocation: $("set-aria2-prealloc").checked ? "falloc" : "none",
    aria2_min_split_size: Number($("set-aria2-minsplit").value) || undefined,
    aria2_max_tries: Number($("set-aria2-tries").value) || 0,
    aria2_retry_wait: Number($("set-aria2-retrywait").value) || 0,
    aria2_lowest_speed_limit: Number($("set-aria2-slowlimit").value) || 0,
    aria2_bt_max_peers: Number($("set-aria2-btpeers").value) || 0,
    aria2_user_agent: $("set-aria2-ua").value.trim() || null,
    aria2_custom_tracker_url: $("set-aria2-tracker").value.trim() || null,
    ytdlp_max_height: Number($("set-ytdlp-quality").value) || 0,
    ytdlp_merge_format: $("set-ytdlp-merge").value,
    ytdlp_cookies_from_browser: $("set-ytdlp-cookies")?.value ?? "",
    ytdlp_audio_format: $("set-ytdlp-audio").value,
    ytdlp_fragments: Number($("set-ytdlp-frags").value) || undefined,
    ytdlp_embed_subs: $("set-ytdlp-embed-subs").checked,
    theme: $("set-theme")?.value || "dark",
    accent_color: $("set-accent-color")?.value || "#e8934a",
    widget_shape: $("set-widget-shape")?.value || "pill",
    notifications_enabled: $("set-notifications-enabled") ? $("set-notifications-enabled").checked : true,
    notification_sound: $("set-notification-sound") ? $("set-notification-sound").checked : true,
    autostart: $("set-autostart") ? $("set-autostart").checked : false,
  };
  try {
    const res = await fetch("/api/config", {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (res.ok) {
      cfgCache = await res.json();
      applyAppearance(cfgCache.theme, cfgCache.accent_color, cfgCache.widget_shape);
      const btn = $("settings-save");
      const hint = $("settings-saved");
      const dirty = $("settings-dirty");
      if (dirty) dirty.classList.add("hidden");
      btn.textContent = "Saved";
      hint.classList.remove("hidden");
      setTimeout(() => { btn.textContent = "Save"; hint.classList.add("hidden"); }, 1800);
    } else {
      notice((await res.text()) || `error ${res.status}`, true);
    }
  } catch (err) {
    notice(`request failed: ${err}`, true);
  }
});

// stepper buttons next to numeric settings
document.querySelectorAll(".stbtn").forEach((b) => {
  b.addEventListener("click", () => {
    const input = $(b.dataset.target);
    if (!input) return;
    const min = Number(input.min ?? 0);
    const max = Number(input.max ?? Number.MAX_SAFE_INTEGER);
    const cur = Number(input.value) || 0;
    const isPlus = b.textContent.includes("+");
    const next = Math.min(max, Math.max(min, cur + Number(input.step || 1) * (isPlus ? 1 : -1)));
    input.value = next;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
});

// category tabs filtering for settings
const settingsTabsWrap = $("settings-chips");
if (settingsTabsWrap) {
  settingsTabsWrap.addEventListener("wheel", (e) => {
    if (e.deltaY !== 0) {
      e.preventDefault();
      settingsTabsWrap.scrollLeft += e.deltaY;
    }
  }, { passive: false });
}

document.querySelectorAll("#settings-chips .settings-tab, #settings-chips .chip").forEach((chip) => {
  chip.addEventListener("click", () => {
    document.querySelectorAll("#settings-chips .settings-tab, #settings-chips .chip").forEach((c) => {
      c.classList.remove("on");
      c.setAttribute("aria-selected", "false");
    });
    chip.classList.add("on");
    chip.setAttribute("aria-selected", "true");
    chip.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
    const cat = chip.dataset.cat;
    $("settings-form").classList.toggle("is-filtered", cat !== "all");
    document.querySelectorAll(".settings-card[data-category]").forEach((card) => {
      card.style.display = (cat === "all" || card.dataset.category === cat) ? "" : "none";
    });
  });
});

// ---------- appearance settings & widget preview ----------

let pwSparkTimer = null;
const pwSparkData = [14, 20, 16, 24, 33, 28, 38, 44, 36, 46, 40, 52, 46, 56, 51, 60, 55, 64, 58, 70];

function renderPreviewSpark(accent) {
  const cv = $("pw-spark");
  if (!cv) return;
  const ctx = cv.getContext("2d");
  const w = cv.width;
  const h = cv.height;
  ctx.clearRect(0, 0, w, h);

  const maxVal = Math.max(...pwSparkData, 75);
  const stepX = w / (pwSparkData.length - 1);
  const color = accent || getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#e8934a";

  ctx.beginPath();
  ctx.moveTo(0, h);
  pwSparkData.forEach((val, i) => {
    const x = i * stepX;
    const y = h - (Math.min(1, val / maxVal) * (h - 4)) - 2;
    ctx.lineTo(x, y);
  });
  ctx.lineTo(w, h);
  ctx.closePath();

  const g = ctx.createLinearGradient(0, 0, 0, h);
  g.addColorStop(0, `${color}66`);
  g.addColorStop(1, `${color}00`);
  ctx.fillStyle = g;
  ctx.fill();

  ctx.beginPath();
  pwSparkData.forEach((val, i) => {
    const x = i * stepX;
    const y = h - (Math.min(1, val / maxVal) * (h - 4)) - 2;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  });
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.8;
  ctx.stroke();
}

function startPreviewSpark() {
  if (pwSparkTimer) return;
  renderPreviewSpark($("set-accent-color")?.value);
  pwSparkTimer = setInterval(() => {
    const last = pwSparkData[pwSparkData.length - 1];
    const delta = (Math.random() - 0.48) * 10;
    const next = Math.max(12, Math.min(72, last + delta));
    pwSparkData.shift();
    pwSparkData.push(next);
    renderPreviewSpark($("set-accent-color")?.value);
  }, 750);
}

function stopPreviewSpark() {
  if (pwSparkTimer) {
    clearInterval(pwSparkTimer);
    pwSparkTimer = null;
  }
}

function applyAppearance(theme, accentColor, widgetShape) {
  theme = theme || "dark";
  accentColor = accentColor || "#e8934a";
  widgetShape = widgetShape || "pill";

  // 1. Root theme attribute & accent CSS vars
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.setProperty("--accent", accentColor);
  document.documentElement.style.setProperty("--accent-2", accentColor);
  document.documentElement.style.setProperty("--accent-glow", `${accentColor}33`);

  // 2. Local storage persistence for 0-flicker reload
  try {
    localStorage.setItem("mdm_theme", theme);
    localStorage.setItem("mdm_accent", accentColor);
    localStorage.setItem("mdm_shape", widgetShape);
  } catch (_) {}

  // 3. Hidden form inputs
  if ($("set-theme")) $("set-theme").value = theme;
  if ($("set-accent-color")) $("set-accent-color").value = accentColor;
  if ($("set-widget-shape")) $("set-widget-shape").value = widgetShape;

  // 4. Theme mode chips active state
  document.querySelectorAll("#theme-mode-chips .theme-chip").forEach((c) => {
    c.classList.toggle("on", c.dataset.themeVal === theme);
  });

  // 5. Color palette swatches and badge
  const lowerAccent = accentColor.toLowerCase();
  document.querySelectorAll("#color-palette .color-swatch").forEach((s) => {
    s.classList.toggle("on", s.dataset.color.toLowerCase() === lowerAccent);
  });
  if ($("custom-color-input")) $("custom-color-input").value = accentColor;
  if ($("color-name-badge")) {
    $("color-name-badge").textContent = `${getColorName(accentColor)} (${accentColor})`;
  }

  // 6. Widget shape chips
  document.querySelectorAll("#widget-shape-chips .shape-chip").forEach((c) => {
    c.classList.toggle("on", c.dataset.shape === widgetShape);
  });

  // 7. Widget preview display
  const pw = $("preview-widget");
  if (pw) {
    pw.classList.toggle("is-pill", widgetShape === "pill");
    pw.classList.toggle("is-card", widgetShape === "card");
    pw.classList.toggle("is-minimalistic", widgetShape === "minimalistic");
    const pillContent = pw.querySelector(".pw-pill-content");
    const cardContent = pw.querySelector(".pw-card-content");
    const miniContent = pw.querySelector(".pw-mini-content");
    const spark = $("pw-spark");
    if (pillContent) pillContent.classList.toggle("hidden", widgetShape !== "pill");
    if (cardContent) cardContent.classList.toggle("hidden", widgetShape !== "card");
    if (miniContent) miniContent.classList.toggle("hidden", widgetShape !== "minimalistic");
    if (spark) spark.classList.toggle("hidden", widgetShape !== "card");
  }

  if (widgetShape === "card") {
    renderPreviewSpark(accentColor);
    startPreviewSpark();
  } else {
    stopPreviewSpark();
  }
}

// Appearance UI event bindings
document.querySelectorAll("#theme-mode-chips .theme-chip").forEach((btn) => {
  btn.addEventListener("click", () => {
    const val = btn.dataset.themeVal;
    applyAppearance(val, $("set-accent-color")?.value, $("set-widget-shape")?.value);
    $("settings-dirty")?.classList.remove("hidden");
  });
});

document.querySelectorAll("#color-palette .color-swatch").forEach((btn) => {
  btn.addEventListener("click", () => {
    const color = btn.dataset.color;
    applyAppearance($("set-theme")?.value, color, $("set-widget-shape")?.value);
    $("settings-dirty")?.classList.remove("hidden");
  });
});

$("custom-color-input")?.addEventListener("input", (e) => {
  const color = e.target.value;
  applyAppearance($("set-theme")?.value, color, $("set-widget-shape")?.value);
  $("settings-dirty")?.classList.remove("hidden");
});

document.querySelectorAll("#widget-shape-chips .shape-chip").forEach((btn) => {
  btn.addEventListener("click", () => {
    const shape = btn.dataset.shape;
    applyAppearance($("set-theme")?.value, $("set-accent-color")?.value, shape);
    $("settings-dirty")?.classList.remove("hidden");
  });
});

// ---------- engine binary updates ----------

async function loadBinsStatus() {
  try {
    const res = await fetch("/api/bins/status");
    if (!res.ok) return;
    const bins = await res.json();
    for (const b of bins) {
      if (b.name === "aria2c") {
        const verEl = $("aria2-version-text");
        if (verEl) {
          verEl.textContent = b.version || "Installed";
        }
      } else if (b.name === "yt-dlp") {
        const verEl = $("ytdlp-version-text");
        if (verEl) {
          verEl.textContent = b.version || "Installed";
        }
      }
    }
  } catch (err) {
    console.warn("cannot load bins status:", err);
  }
}

async function triggerBinUpdate(name, btnId, statusId, verId) {
  const btn = $(btnId);
  const status = $(statusId);
  const ver = $(verId);
  if (!btn) return;

  btn.disabled = true;
  const originalText = btn.textContent;
  btn.textContent = "Updating...";
  if (status) {
    status.style.color = "var(--muted)";
    status.textContent = `Checking GitHub for ${name} updates...`;
  }

  try {
    const res = await fetch("/api/bins/update", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    const data = await res.json();
    if (data.ok) {
      if (status) {
        status.style.color = "var(--accent)";
        status.textContent = data.message || `Successfully updated ${name}!`;
      }
      if (ver && data.version) {
        ver.textContent = data.version;
      }
      notice(data.message || `${name} updated successfully!`);
    } else {
      if (status) {
        status.style.color = "#ef4444";
        status.textContent = data.message || `Update failed. Existing binary preserved.`;
      }
      notice(data.message || `Failed to update ${name}`, true);
    }
  } catch (err) {
    if (status) {
      status.style.color = "#ef4444";
      status.textContent = `Network error: ${err.message}. Offline binary preserved.`;
    }
    notice(`Network error updating ${name}: ${err.message}`, true);
  } finally {
    btn.disabled = false;
    btn.textContent = originalText;
  }
}

$("btn-update-aria2")?.addEventListener("click", () => {
  triggerBinUpdate("aria2c", "btn-update-aria2", "aria2-update-status", "aria2-version-text");
});

$("btn-update-ytdlp")?.addEventListener("click", () => {
  triggerBinUpdate("yt-dlp", "btn-update-ytdlp", "ytdlp-update-status", "ytdlp-version-text");
});

// ---------- browser extension management ----------

const BROWSER_ICONS = {
  chrome: `<svg viewBox="0 0 24 24" class="ext-browser-ico"><circle cx="12" cy="12" r="10" fill="none" stroke="currentColor" stroke-width="1.8"/><circle cx="12" cy="12" r="4" fill="none" stroke="currentColor" stroke-width="1.8"/><line x1="21.17" y1="8" x2="12" y2="8" stroke="currentColor" stroke-width="1.8"/><line x1="3.95" y1="6.06" x2="8.54" y2="14" stroke="currentColor" stroke-width="1.8"/><line x1="10.88" y1="21.94" x2="15.46" y2="14" stroke="currentColor" stroke-width="1.8"/></svg>`,
  edge: `<svg viewBox="0 0 24 24" class="ext-browser-ico"><path d="M12 2a10 10 0 0 0-9.8 12c.5 2.1 2 4.1 4.2 4.9 3.5 1.4 7.6-.3 9.4-3.5a5.5 5.5 0 0 0-4.8-8.4c-2.4 0-4.5 1.5-5.3 3.7.8-1.5 2.4-2.4 4.1-2.4 2.8 0 5 2.2 5 5 0 2.2-1.8 4-4 4-2.5 0-4.4-1.8-4.8-4.2C6 11 8.2 7 12 7c4.4 0 8 3.6 8 8 0 1.2-.3 2.3-.8 3.3A10 10 0 0 0 12 2Z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/></svg>`,
  brave: `<svg viewBox="0 0 24 24" class="ext-browser-ico"><path d="M12 2 4 5v6c0 5.55 3.84 10.74 8 12 4.16-1.26 8-6.45 8-12V5l-8-3Zm0 4a4 4 0 0 1 4 4c0 2.5-2 3.5-4 5.5-2-2-4-3-4-5.5a4 4 0 0 1 4-4Z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/></svg>`,
  opera: `<svg viewBox="0 0 24 24" class="ext-browser-ico"><ellipse cx="12" cy="12" rx="9" ry="10" fill="none" stroke="currentColor" stroke-width="1.8"/><ellipse cx="12" cy="12" rx="4.5" ry="7.5" fill="none" stroke="currentColor" stroke-width="1.8"/></svg>`,
  vivaldi: `<svg viewBox="0 0 24 24" class="ext-browser-ico"><rect x="3" y="3" width="18" height="18" rx="4" fill="none" stroke="currentColor" stroke-width="1.8"/><path d="m8 8 4 8 4-8" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  firefox: `<svg viewBox="0 0 24 24" class="ext-browser-ico"><circle cx="12" cy="12" r="10" fill="none" stroke="currentColor" stroke-width="1.8"/><path d="M12 2a10 10 0 0 1 10 10c0 4.5-3 8-7 9 2-2 3-5 1-7-1-1-3-1-4-2 0-2 1-3 2-4-2 .5-3.5 2-4 4-.5 2 0 4-1 5-1.5-1.5-2.5-3.5-2.5-6a8 8 0 0 1 8-8Z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/></svg>`,
};

async function loadExtensionStatus() {
  try {
    const res = await fetch("/api/extension/status");
    if (!res.ok) return;
    const data = await res.json();

    const pathInput = $("ext-folder-path");
    const badgeEl = $("ext-version-badge");
    const gridEl = $("ext-browser-grid");

    if (pathInput) {
      pathInput.value = data.folder_path || "Extension folder not found";
    }

    if (badgeEl) {
      if (data.folder_exists && data.manifest_exists) {
        badgeEl.textContent = `v${data.version || "0.1.0"} (Bundled & Ready)`;
        badgeEl.className = "ext-version-badge ready";
      } else {
        badgeEl.textContent = "Not Located";
        badgeEl.className = "ext-version-badge missing";
      }
    }

    if (gridEl && Array.isArray(data.browsers)) {
      gridEl.innerHTML = "";
      data.browsers.forEach((b) => {
        const card = mk("div", `ext-browser-card ${b.installed ? "is-installed" : "not-installed"}`);
        
        const topRow = mk("div", "ext-card-top");
        const icoWrap = mk("div", `ext-card-ico ${b.id}`);
        icoWrap.innerHTML = BROWSER_ICONS[b.id] || BROWSER_ICONS.chrome;
        topRow.appendChild(icoWrap);

        const badge = mk("span", `ext-card-badge ${b.installed ? "detected" : "missing"}`);
        badge.textContent = b.installed ? "Detected" : "Not Found";
        topRow.appendChild(badge);
        card.appendChild(topRow);

        const title = mk("div", "ext-card-title");
        title.textContent = b.name;
        card.appendChild(title);

        const desc = mk("div", "ext-card-url");
        desc.textContent = b.extensions_url;
        card.appendChild(desc);

        const actRow = mk("div", "ext-card-action");
        if (b.installed) {
          const btn = mk("button", "ext-action-btn primary");
          btn.type = "button";
          btn.innerHTML = `<span>Install / Add</span><svg viewBox="0 0 24 24" class="ext-btn-ico"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6m4-3h6v6m-11 5L21 3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
          btn.addEventListener("click", async () => {
            btn.disabled = true;
            try {
              const resp = await fetch("/api/extension/open-browser", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ browser: b.id }),
              });
              if (resp.ok) {
                showToast({
                  type: "success",
                  title: `${b.name} Opened`,
                  message: `Launched ${b.name} to its extensions page and revealed the extension folder in Explorer. Follow the 4-step guide below!`,
                  timeout: 7000
                });
              } else {
                notice(`Could not launch ${b.name}: ${await resp.text()}`, true);
              }
            } catch (err) {
              notice(`Launch failed: ${err.message}`, true);
            } finally {
              btn.disabled = false;
            }
          });
          actRow.appendChild(btn);
        } else {
          const btn = mk("button", "ext-action-btn disabled");
          btn.type = "button";
          btn.disabled = true;
          btn.textContent = "Not Installed";
          actRow.appendChild(btn);
        }
        card.appendChild(actRow);
        gridEl.appendChild(card);
      });
    }
  } catch (err) {
    console.warn("cannot load extension status:", err);
  }
}

$("ext-copy-path-btn")?.addEventListener("click", async () => {
  const path = $("ext-folder-path")?.value;
  if (!path) return;
  try {
    await navigator.clipboard.writeText(path);
    const btn = $("ext-copy-path-btn");
    const label = btn.querySelector(".ext-btn-label");
    const ico = btn.querySelector(".ext-btn-ico");

    btn.classList.add("copied");
    if (label) label.textContent = "Copied!";
    if (ico) {
      ico.innerHTML = `<path d="M20 6 9 17l-5-5" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>`;
    }
    showToast({
      type: "info",
      title: "Path Copied",
      message: "Extension folder path copied to clipboard",
      timeout: 3000
    });
    setTimeout(() => {
      btn.classList.remove("copied");
      if (label) label.textContent = "Copy Path";
      if (ico) {
        ico.innerHTML = `<rect x="9" y="9" width="13" height="13" rx="2" ry="2" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>`;
      }
    }, 2000);
  } catch (e) {
    notice(`Cannot copy path: ${e}`, true);
  }
});

$("ext-open-folder-btn")?.addEventListener("click", async () => {
  try {
    const res = await fetch("/api/extension/open-folder", { method: "POST" });
    if (res.ok) {
      showToast({
        type: "info",
        title: "Folder Opened",
        message: "Extension folder revealed in Windows Explorer",
        timeout: 4000
      });
    } else {
      notice(`Failed to open folder: ${await res.text()}`, true);
    }
  } catch (e) {
    notice(`Failed to open folder: ${e.message}`, true);
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
  } else if (view === "about") {
    target = "#/about";
  } else {
    view = "home";
  }

  const views = { home: "view-home", downloads: "view-downloads", settings: "view-settings", about: "view-about" };
  for (const [v, id] of Object.entries(views)) {
    const el = $(id);
    if (el) el.hidden = v !== view;
  }

  for (const btn of document.querySelectorAll("[data-nav], [data-kind]")) {
    const dest = btn.dataset.kind || btn.dataset.nav;
    btn.classList.toggle("active", dest === target);
  }

  if (view === "settings") loadSettings();
  sync();
  if (view === "downloads") {
    requestAnimationFrame(renderSpeedGraph);
  }
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

const lastKnownJobStates = new Map();

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
      lastKnownJobStates.clear();
      for (const job of ev.jobs || []) {
        jobs.set(job.id, job);
        lastKnownJobStates.set(job.id, job.state);
        renderCard(job);
      }
      sync();
    } else if (ev.type === "job_updated" && ev.job) {
      const prev = lastKnownJobStates.get(ev.job.id);
      lastKnownJobStates.set(ev.job.id, ev.job.state);

      if (prev && prev !== ev.job.state) {
        if (ev.job.state === "done") {
          playNotificationSound("complete");
          showToast({
            type: "success",
            title: "Download complete",
            message: ev.job.title || ev.job.filename || ev.job.url,
            timeout: 6500,
            actions: (ev.job.output_path || ev.job.output_dir) ? [
              {
                label: "Show in Folder",
                onClick: async () => {
                  try {
                    const res = await fetch(`/api/jobs/${ev.job.id}/open-folder`, { method: "POST" });
                    if (res.ok) notice("Opened in file manager");
                  } catch (_) {}
                }
              }
            ] : []
          });
        } else if (ev.job.state === "failed") {
          playNotificationSound("error");
          showToast({
            type: "error",
            title: "Download failed",
            message: ev.job.error || "An error occurred during download",
            timeout: 8000,
            actions: [
              {
                label: "Resume / Retry",
                onClick: async () => {
                  try {
                    const res = await fetch(`/api/jobs/${ev.job.id}/resume`, { method: "POST" });
                    if (res.ok) notice("Retrying download");
                  } catch (_) {}
                }
              }
            ]
          });
        }
      }

      jobs.set(ev.job.id, ev.job);
      renderCard(ev.job);
      sync();
      healthSoon();
    } else if (ev.type === "job_removed" && ev.id) {
      lastKnownJobStates.delete(ev.id);
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
const btnResumeAll = $("btn-resume-all");
if (btnResumeAll) {
  btnResumeAll.addEventListener("click", async () => {
    try {
      const res = await fetch("/api/jobs/resume-all", { method: "POST" });
      if (res.ok) {
        notice("Resumed paused and failed downloads");
      } else {
        notice("Failed to resume all", true);
      }
    } catch (e) {
      notice(`Error: ${e}`, true);
    }
  });
}
const btnPauseAll = $("btn-pause-all");
if (btnPauseAll) {
  btnPauseAll.addEventListener("click", async () => {
    try {
      const res = await fetch("/api/jobs/pause-all", { method: "POST" });
      if (res.ok) {
        notice("Paused all active downloads");
      } else {
        notice("Failed to pause all", true);
      }
    } catch (e) {
      notice(`Error: ${e}`, true);
    }
  });
}
$("search").addEventListener("input", (e) => {
  searchQuery = e.target.value.trim().toLowerCase();
  sync();
});
$("collapse").addEventListener("click", () => document.querySelector(".app").classList.toggle("collapsed"));
window.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.shiftKey && (e.key === "S" || e.key === "s")) {
    e.preventDefault();
    document.querySelector(".app").classList.toggle("collapsed");
  } else if ((e.ctrlKey || e.metaKey) && !e.shiftKey && (e.key === "s" || e.key === "S")) {
    const view = $("view-settings");
    if (view && !view.hidden) {
      e.preventDefault();
      $("settings-form").requestSubmit();
    }
  }
});
window.addEventListener("hashchange", applyRoute);

// ---------- live speed graph & transfer tracking ----------

const GRAPH_POINTS = 60;
const speedHistory = {
  down: new Array(GRAPH_POINTS).fill(0),
  up: new Array(GRAPH_POINTS).fill(0),
};
let maxPeakDown = 0;
let maxPeakUp = 0;

function tickSpeedGraph() {
  const running = [...jobs.values()].filter(j => j.state === "running");
  const curDown = running.reduce((a, j) => a + (j.speed_bps > 0 ? j.speed_bps : 0), 0);
  const curUp = running.reduce((a, j) => a + (j.upload_speed_bps > 0 ? j.upload_speed_bps : 0), 0);

  speedHistory.down.shift();
  speedHistory.down.push(curDown);
  speedHistory.up.shift();
  speedHistory.up.push(curUp);

  if (curDown > maxPeakDown) maxPeakDown = curDown;
  if (curUp > maxPeakUp) maxPeakUp = curUp;

  const downCurEl = $("graph-down-cur");
  const downPeakEl = $("graph-down-peak");
  const upCurEl = $("graph-up-cur");
  const upPeakEl = $("graph-up-peak");

  if (downCurEl) downCurEl.textContent = fmtSpeed(curDown);
  if (downPeakEl) downPeakEl.textContent = `peak ${fmtSpeed(maxPeakDown)}`;
  if (upCurEl) upCurEl.textContent = fmtSpeed(curUp);
  if (upPeakEl) upPeakEl.textContent = `peak ${fmtSpeed(maxPeakUp)}`;

  const dlView = $("view-downloads");
  if (dlView && !dlView.hidden) {
    renderSpeedGraph();
  }
}

function renderSpeedGraph() {
  const canvas = $("speed-canvas");
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  if (rect.width === 0 || rect.height === 0) return;

  const dpr = window.devicePixelRatio || 1;
  const targetW = Math.round(rect.width * dpr);
  const targetH = Math.round(rect.height * dpr);
  if (canvas.width !== targetW || canvas.height !== targetH) {
    canvas.width = targetW;
    canvas.height = targetH;
  }

  const ctx = canvas.getContext("2d");
  ctx.save();
  ctx.scale(dpr, dpr);

  const w = rect.width;
  const h = rect.height;

  ctx.clearRect(0, 0, w, h);

  // Vertical scale headroom (minimum 128 KiB/s so empty state has a reasonable scale)
  const allSpeeds = [...speedHistory.down, ...speedHistory.up];
  const maxVal = Math.max(...allSpeeds, 128 * 1024) * 1.15;

  // Horizontal grid lines & speed values
  ctx.strokeStyle = "rgba(255, 255, 255, 0.06)";
  ctx.lineWidth = 1;
  ctx.fillStyle = "rgba(148, 163, 184, 0.4)";
  ctx.font = "10px Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif";
  ctx.textBaseline = "middle";

  const gridSteps = [0.25, 0.5, 0.75];
  for (const step of gridSteps) {
    const y = Math.round(h * (1 - step)) + 0.5;
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(w, y);
    ctx.stroke();

    const speedLabel = fmtSpeed(maxVal * step);
    ctx.fillText(speedLabel, 8, y - 6);
  }

  function drawSeries(data, strokeColor, fillColor) {
    const stepX = w / (GRAPH_POINTS - 1);
    const points = data.map((val, i) => {
      const x = i * stepX;
      const norm = Math.min(1, Math.max(0, val / maxVal));
      const y = h - (norm * (h - 10)) - 4;
      return { x, y };
    });

    // Fill area under curve
    ctx.beginPath();
    ctx.moveTo(points[0].x, h);
    for (let i = 0; i < points.length; i++) {
      ctx.lineTo(points[i].x, points[i].y);
    }
    ctx.lineTo(points[points.length - 1].x, h);
    ctx.closePath();
    ctx.fillStyle = fillColor;
    ctx.fill();

    // Stroke line
    ctx.beginPath();
    for (let i = 0; i < points.length; i++) {
      if (i === 0) ctx.moveTo(points[i].x, points[i].y);
      else ctx.lineTo(points[i].x, points[i].y);
    }
    ctx.strokeStyle = strokeColor;
    ctx.lineWidth = 2;
    ctx.stroke();
  }

  // Upload (green)
  const gradUp = ctx.createLinearGradient(0, 0, 0, h);
  gradUp.addColorStop(0, "rgba(72, 199, 116, 0.32)");
  gradUp.addColorStop(1, "rgba(72, 199, 116, 0.02)");
  drawSeries(speedHistory.up, "#48c774", gradUp);

  // Download (orange)
  const gradDown = ctx.createLinearGradient(0, 0, 0, h);
  gradDown.addColorStop(0, "rgba(232, 147, 74, 0.38)");
  gradDown.addColorStop(1, "rgba(232, 147, 74, 0.02)");
  drawSeries(speedHistory.down, "#e8934a", gradDown);

  ctx.restore();
}

// ---------- active downloads sheet ----------
const tbTransferBtn = $("tb-transfer-btn");
const activeSheet = $("active-sheet");
const sheetCloseBtn = $("sheet-btn-close");
const sheetViewAll = $("sheet-btn-view-all");
const sheetList = $("sheet-list");

function toggleActiveSheet(show) {
  if (!activeSheet) return;
  const isHidden = activeSheet.classList.contains("hidden");
  const next = show !== undefined ? show : isHidden;
  activeSheet.classList.toggle("hidden", !next);
  tbTransferBtn?.classList.toggle("open", next);
}

tbTransferBtn?.addEventListener("click", (e) => {
  e.stopPropagation();
  toggleActiveSheet();
});

sheetCloseBtn?.addEventListener("click", () => toggleActiveSheet(false));
sheetViewAll?.addEventListener("click", () => {
  toggleActiveSheet(false);
  go("#/downloads");
});

sheetList?.addEventListener("click", (e) => {
  const item = e.target.closest(".sheet-item");
  if (!item) return;
  toggleActiveSheet(false);
  go("#/downloads");
  const c = cards.get(item.dataset.id);
  if (c) {
    c.el.scrollIntoView({ behavior: "smooth", block: "center" });
    c.el.classList.add("highlight");
    setTimeout(() => c.el.classList.remove("highlight"), 1200);
  }
});

document.addEventListener("click", (e) => {
  if (activeSheet && !activeSheet.classList.contains("hidden")) {
    if (!activeSheet.contains(e.target) && !tbTransferBtn?.contains(e.target)) {
      toggleActiveSheet(false);
    }
  }
});

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    toggleActiveSheet(false);
  }
});

window.addEventListener("resize", () => {
  const dlView = $("view-downloads");
  if (dlView && !dlView.hidden) {
    renderSpeedGraph();
  }
});

// ---------- start ----------

setInterval(() => {
  for (const [id, job] of jobs) {
    if (job.state !== "running") continue;
    const c = cards.get(id);
    if (c) c.refs.note.textContent = `· ${fmtElapsed(elapsedSec(job))} elapsed`;
  }
  tickSpeedGraph();
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

// ---------- desktop window controls ----------
async function triggerWindowAction(action) {
  try {
    await fetch(`/api/window/${action}`, { method: "POST" });
  } catch (_) {}
  if (window.__TAURI__?.core) {
    const cmdMap = {
      minimize: "window_minimize",
      maximize: "window_toggle_maximize",
      close: "window_close",
      "toggle-floating": "window_toggle_floating",
    };
    if (cmdMap[action]) {
      window.__TAURI__.core.invoke(cmdMap[action]).catch(() => {});
    }
  }
}

$("tb-min")?.addEventListener("click", () => triggerWindowAction("minimize"));
$("tb-max")?.addEventListener("click", () => triggerWindowAction("maximize"));
$("tb-close")?.addEventListener("click", () => triggerWindowAction("close"));
$("tb-widget-btn")?.addEventListener("click", () => triggerWindowAction("toggle-floating"));

document.addEventListener("click", (e) => {
  const link = e.target.closest('a[href^="http"]');
  if (link && (link.target === "_blank" || link.classList.contains("about-link-row"))) {
    if (window.__TAURI__?.shell?.open) {
      e.preventDefault();
      window.__TAURI__.shell.open(link.href);
    } else if (window.__TAURI__?.opener?.openUrl) {
      e.preventDefault();
      window.__TAURI__.opener.openUrl(link.href);
    }
  }
});

$("titlebar")?.addEventListener("dblclick", (e) => {
  if (!e.target.closest(".tb-btn") && !e.target.closest(".tb-transfer-btn")) {
    triggerWindowAction("maximize");
  }
});

fetch("/api/window/state")
  .then(r => r.ok ? r.json() : null)
  .then(st => {
    if (st && st.is_desktop === false && !window.__TAURI__) {
      $("titlebar-controls")?.classList.add("hidden");
    } else {
      $("titlebar-controls")?.classList.remove("hidden");
    }
  })
  .catch(() => {});

async function loadConfigInitial() {
  try {
    const res = await fetch("/api/config");
    if (res.ok) {
      const cfg = await res.json();
      cfgCache = cfg;
      applyAppearance(cfg.theme, cfg.accent_color, cfg.widget_shape);
    }
  } catch (_) {}
}

loadConfigInitial();
refreshInitial();
loadOutDir();
applyRoute();
connect();
pollHealth();
setInterval(pollHealth, 5000);
