// mDm Chrome Extension — IDM-style Video Download Widget Content Script
"use strict";

const MASCOT_SVG = `
<svg viewBox="0 0 120 120" class="mdm-widget-mascot">
  <defs>
    <radialGradient id="mdm-g-mascot" cx="0.5" cy="0.5" r="0.75">
      <stop offset="0" stop-color="#f2a95c"/>
      <stop offset="1" stop-color="#c96a2e"/>
    </radialGradient>
  </defs>
  <circle cx="60" cy="62" r="44" fill="url(#mdm-g-mascot)"/>
  <circle cx="46" cy="44" r="6.5" fill="#141820"/>
  <circle cx="74" cy="44" r="6.5" fill="#141820"/>
  <circle cx="48" cy="42" r="2" fill="#fff"/>
  <circle cx="76" cy="42" r="2" fill="#fff"/>
  <path d="M47 57 Q60 66 73 57" stroke="#141820" stroke-width="4.5" stroke-linecap="round" fill="none"/>
  <rect x="54.5" y="70" width="11" height="16" rx="5.5" fill="#141820"/>
  <path d="M42 80 h36 L60 97 Z" fill="#141820"/>
</svg>
`;

const QUALITY_PRESETS = [
  {
    label: "Best Video Quality",
    badge: "AUTO",
    format: null,
    kind: "video",
    icon: `<svg viewBox="0 0 24 24" class="mdm-fmt-svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"/><line x1="7" y1="2" x2="7" y2="22"/><line x1="17" y1="2" x2="17" y2="22"/><line x1="2" y1="12" x2="22" y2="12"/><line x1="2" y1="7" x2="7" y2="7"/><line x1="2" y1="17" x2="7" y2="17"/><line x1="17" y1="17" x2="22" y2="17"/><line x1="17" y1="7" x2="22" y2="7"/></svg>`,
  },
  {
    label: "1080p Full HD",
    badge: "1080p",
    format: "bv*[height<=1080]+ba/b[height<=1080]/b",
    kind: "video",
    icon: `<svg viewBox="0 0 24 24" class="mdm-fmt-svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="20" height="14" rx="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>`,
  },
  {
    label: "720p HD",
    badge: "720p",
    format: "bv*[height<=720]+ba/b[height<=720]/b",
    kind: "video",
    icon: `<svg viewBox="0 0 24 24" class="mdm-fmt-svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="20" height="14" rx="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>`,
  },
  {
    label: "480p SD",
    badge: "480p",
    format: "bv*[height<=480]+ba/b[height<=480]/b",
    kind: "video",
    icon: `<svg viewBox="0 0 24 24" class="mdm-fmt-svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="20" height="14" rx="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>`,
  },
  {
    label: "Audio Only (MP3)",
    badge: "AUDIO",
    format: "bestaudio",
    kind: "audio",
    icon: `<svg viewBox="0 0 24 24" class="mdm-fmt-svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>`,
  },
];

let activeDropdown = null;
const activeWidgets = new Set();

// --- IDM Floating Widget Injection Outside Video Container -----------------

function getVisualVideoFrame(video) {
  const host = location.hostname.toLowerCase();

  if (host.includes("youtube.com")) {
    return video.closest("#movie_player") ||
           video.closest(".html5-video-player") ||
           video.closest("ytd-player") ||
           video.closest("ytd-shorts") ||
           video;
  }
  if (host.includes("twitter.com") || host.includes("x.com")) {
    return video.closest('[data-testid="videoPlayer"]') ||
           video.closest('[data-testid="videoComponent"]') ||
           video;
  }
  if (host.includes("reddit.com")) {
    return video.closest("shreddit-player") || video;
  }
  if (host.includes("tiktok.com")) {
    return video.closest('[data-e2e="browse-video"]') || video.closest(".video-player") || video;
  }
  if (host.includes("vimeo.com")) {
    return video.closest(".vp-video-wrapper") || video.closest(".player") || video;
  }
  if (host.includes("twitch.tv")) {
    return video.closest(".video-player__container") || video;
  }
  if (host.includes("instagram.com")) {
    const roleButton = video.closest('div[role="button"]') || video.closest('div[tabindex="0"]');
    if (roleButton && roleButton.parentElement) {
      return roleButton.parentElement;
    }
    return video;
  }

  return video.parentElement || video;
}

function updateWidgetPosition(widget, video) {
  if (!video || !video.isConnected) {
    widget.remove();
    activeWidgets.delete(widget);
    return;
  }

  const frame = getVisualVideoFrame(video);
  const target = frame || video;
  const rect = target.getBoundingClientRect();

  // If video is not visible or zero size or completely out of viewport:
  if (
    rect.width < 50 ||
    rect.height < 50 ||
    rect.bottom <= 0 ||
    rect.top >= window.innerHeight ||
    rect.right <= 0 ||
    rect.left >= window.innerWidth
  ) {
    widget.style.display = "none";
    return;
  }

  widget.style.display = "inline-flex";

  // If user has custom dragged position, keep offset relative to frame
  if (widget._customOffset) {
    widget.style.top = `${Math.round(rect.top + widget._customOffset.dy)}px`;
    widget.style.left = `${Math.round(rect.left + widget._customOffset.dx)}px`;
    return;
  }

  const widgetHeight = widget.offsetHeight || 28;
  const widgetWidth = widget.offsetWidth || 155;

  // OUTSIDE THE VIDEO FRAME:
  // Default: Place right above the top-right edge outside the video frame
  let top = rect.top - widgetHeight - 5;

  // If clipped at the top of the browser window (less than 4px space),
  // place it right below the bottom edge outside the video frame
  if (top < 4) {
    top = rect.bottom + 5;
  }

  // Align with the right edge of the video frame
  let left = rect.right - widgetWidth;
  if (left < 6) {
    left = Math.max(6, rect.left);
  }

  widget.style.top = `${Math.round(top)}px`;
  widget.style.left = `${Math.round(left)}px`;
}

function updateAllWidgetPositions() {
  for (const widget of activeWidgets) {
    if (widget._mdmVideo) {
      updateWidgetPosition(widget, widget._mdmVideo);
    }
  }
}

let scrollTicking = false;
function onScrollOrResize() {
  if (!scrollTicking) {
    scrollTicking = true;
    requestAnimationFrame(() => {
      updateAllWidgetPositions();
      scrollTicking = false;
    });
  }
}

window.addEventListener("scroll", onScrollOrResize, { passive: true, capture: true });
window.addEventListener("resize", onScrollOrResize, { passive: true });

function attachIdmWidget(videoEl) {
  if (videoEl.dataset.mdmAttached === "true" && videoEl._mdmWidget && videoEl._mdmWidget.isConnected) {
    return;
  }

  videoEl.dataset.mdmAttached = "true";

  if (videoEl._mdmWidget) {
    videoEl._mdmWidget.remove();
    activeWidgets.delete(videoEl._mdmWidget);
  }

  const widget = document.createElement("div");
  widget.className = "mdm-idm-widget";
  widget.title = "Download this video with mDm (Click to download, drag to move)";
  widget.innerHTML = `
    <div class="mdm-widget-main" title="Download this video with mDm">
      ${MASCOT_SVG}
      <span class="mdm-widget-label">Download this video</span>
    </div>
    <div class="mdm-widget-divider"></div>
    <div class="mdm-widget-arrow-btn" title="Select format & quality">
      <svg class="mdm-widget-arrow" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6"/></svg>
    </div>
  `;

  widget._mdmVideo = videoEl;
  videoEl._mdmWidget = widget;
  activeWidgets.add(widget);

  // Attach drag & click handling
  makeDraggable(widget, videoEl);

  document.body.appendChild(widget);
  updateWidgetPosition(widget, videoEl);
}

function makeDraggable(widget, videoEl) {
  let isDown = false;
  let hasDragged = false;
  let startX = 0;
  let startY = 0;
  let initLeft = 0;
  let initTop = 0;
  let clickedAction = "main"; // "main" or "arrow"

  widget.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || e.target.closest(".mdm-idm-dropdown")) return;
    isDown = true;
    hasDragged = false;
    startX = e.clientX;
    startY = e.clientY;

    if (e.target.closest(".mdm-widget-arrow-btn") || e.target.closest(".mdm-widget-arrow")) {
      clickedAction = "arrow";
    } else {
      clickedAction = "main";
    }

    const rect = widget.getBoundingClientRect();
    initLeft = rect.left;
    initTop = rect.top;

    e.stopPropagation();
    e.stopImmediatePropagation();

    const onPointerMove = (me) => {
      if (!isDown) return;
      const dx = me.clientX - startX;
      const dy = me.clientY - startY;
      if (Math.abs(dx) > 3 || Math.abs(dy) > 3) {
        hasDragged = true;
        me.stopPropagation();
        me.stopImmediatePropagation();
        me.preventDefault();
        widget.style.left = `${Math.max(0, initLeft + dx)}px`;
        widget.style.top = `${Math.max(0, initTop + dy)}px`;
      }
    };

    const onPointerUp = (ue) => {
      if (!isDown) return;
      isDown = false;
      window.removeEventListener("pointermove", onPointerMove, true);
      window.removeEventListener("pointerup", onPointerUp, true);

      if (hasDragged) {
        ue.stopPropagation();
        ue.stopImmediatePropagation();
        ue.preventDefault();

        // Save relative offset to frame so scrolling keeps dragged position
        if (widget._mdmVideo) {
          const frame = getVisualVideoFrame(widget._mdmVideo);
          const frameRect = (frame || widget._mdmVideo).getBoundingClientRect();
          const wRect = widget.getBoundingClientRect();
          widget._customOffset = {
            dx: wRect.left - frameRect.left,
            dy: wRect.top - frameRect.top,
          };
        }
      } else {
        ue.stopPropagation();
        ue.stopImmediatePropagation();
        ue.preventDefault();

        if (clickedAction === "arrow") {
          if (activeDropdown && activeDropdown.parentElement === widget) {
            closeDropdown();
          } else {
            closeDropdown();
            openDropdown(widget, videoEl);
          }
        } else {
          // User clicked "Download this video" or mascot:
          // Immediately trigger download dialog with default/best quality!
          closeDropdown();
          const currentUrl = getVideoUrl(videoEl);
          const pageTitle = document.title || "Video";
          console.log("[mDm] 'Download this video' clicked for URL:", currentUrl);
          triggerDownload(currentUrl, "video", null, pageTitle);
        }
      }
    };

    window.addEventListener("pointermove", onPointerMove, true);
    window.addEventListener("pointerup", onPointerUp, true);
  }, true);

  widget.addEventListener("click", (e) => {
    if (e.target.closest(".mdm-idm-dropdown")) return;
    e.stopPropagation();
    e.stopImmediatePropagation();
    e.preventDefault();
  }, true);

  widget.addEventListener("mousedown", (e) => {
    if (e.target.closest(".mdm-idm-dropdown")) return;
    e.stopPropagation();
    e.stopImmediatePropagation();
  }, true);

  widget.addEventListener("mouseup", (e) => {
    if (e.target.closest(".mdm-idm-dropdown")) return;
    e.stopPropagation();
    e.stopImmediatePropagation();
  }, true);
}

// --- Quality Dropdown Menu -------------------------------------------------

function openDropdown(widget, videoEl) {
  widget.classList.add("mdm-active");

  const dropdown = document.createElement("div");
  dropdown.className = "mdm-idm-dropdown";

  const rect = widget.getBoundingClientRect();
  if (window.innerHeight - rect.bottom < 260) {
    dropdown.classList.add("mdm-drop-up");
  }

  // Prevent dropdown clicks from bubbling out
  dropdown.addEventListener("pointerdown", (e) => e.stopPropagation());
  dropdown.addEventListener("mousedown", (e) => e.stopPropagation());
  dropdown.addEventListener("click", (e) => e.stopPropagation());

  const currentUrl = getVideoUrl(videoEl);
  const pageTitle = document.title || "Video";

  dropdown.innerHTML = `
    <div class="mdm-dropdown-header">
      <span class="mdm-dropdown-title">Download with mDm</span>
      <span class="mdm-dropdown-badge">Select Quality</span>
    </div>
    <div class="mdm-dropdown-list">
      ${QUALITY_PRESETS.map((p, i) => `
        <button type="button" class="mdm-format-item" data-index="${i}">
          <div class="mdm-fmt-left">
            <span class="mdm-fmt-icon">${p.icon}</span>
            <span class="mdm-fmt-name">${p.label}</span>
          </div>
          <span class="mdm-fmt-badge">${p.badge}</span>
        </button>
      `).join("")}
    </div>
    <div class="mdm-dropdown-footer">
      <span>mDm Desktop App</span>
      <span class="mdm-footer-link" id="mdm-btn-custom">Custom in mDm</span>
    </div>
  `;

  // Item click handlers
  dropdown.querySelectorAll(".mdm-format-item").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      e.preventDefault();
      const idx = Number(btn.dataset.index);
      const preset = QUALITY_PRESETS[idx];
      console.log("[mDm] Selected quality:", preset, "URL:", currentUrl);
      triggerDownload(currentUrl, preset.kind, preset.format, pageTitle);
      closeDropdown();
    });
  });

  dropdown.querySelector("#mdm-btn-custom")?.addEventListener("click", (e) => {
    e.stopPropagation();
    e.preventDefault();
    console.log("[mDm] Selected custom format for URL:", currentUrl);
    triggerDownload(currentUrl, "video", null, pageTitle);
    closeDropdown();
  });

  widget.appendChild(dropdown);
  activeDropdown = dropdown;
}

function closeDropdown() {
  if (activeDropdown) {
    const parentWidget = activeDropdown.closest(".mdm-idm-widget");
    if (parentWidget) parentWidget.classList.remove("mdm-active");
    activeDropdown.remove();
    activeDropdown = null;
  }
}

document.addEventListener("click", (e) => {
  if (activeDropdown && !activeDropdown.contains(e.target) && !e.target.closest(".mdm-idm-widget")) {
    closeDropdown();
  }
}, true);

// --- URL & Download Handling -----------------------------------------------

function getVideoUrl(videoEl) {
  const host = location.hostname.toLowerCase();

  // Instagram permalink resolution
  if (host.includes("instagram.com")) {
    if (location.pathname.startsWith("/p/") || location.pathname.startsWith("/reel/") || location.pathname.startsWith("/reels/")) {
      return location.href;
    }
    // Traverse upwards from videoEl looking for any anchor matching /p/, /reel/, /reels/
    let curr = videoEl;
    while (curr && curr !== document.body) {
      const link = curr.querySelector('a[href*="/p/"], a[href*="/reel/"], a[href*="/reels/"]');
      if (link && link.href) {
        return link.href;
      }
      curr = curr.parentElement;
    }
    if (location.href.includes("/p/") || location.href.includes("/reel/")) {
      return location.href;
    }
    if (videoEl.currentSrc && !videoEl.currentSrc.startsWith("blob:")) {
      return videoEl.currentSrc;
    }
    return location.href;
  }

  // Twitter / X permalink resolution (timeline, search, profile -> exact tweet URL)
  if (host.includes("twitter.com") || host.includes("x.com")) {
    if (location.pathname.includes("/status/")) {
      return location.href;
    }
    const tweet = videoEl.closest('article[data-testid="tweet"]') || videoEl.closest('article');
    if (tweet) {
      const link = tweet.querySelector('a[href*="/status/"]');
      if (link && link.href) {
        return link.href;
      }
    }
    let curr = videoEl;
    while (curr && curr !== document.body) {
      const link = curr.querySelector('a[href*="/status/"]');
      if (link && link.href) {
        return link.href;
      }
      curr = curr.parentElement;
    }
    return location.href;
  }

  // Reddit permalink resolution
  if (host.includes("reddit.com")) {
    if (location.pathname.includes("/comments/")) {
      return location.href;
    }
    const post = videoEl.closest('shreddit-post') || videoEl.closest('[data-test-id="post-content"]');
    if (post) {
      const permalink = post.getAttribute("permalink");
      if (permalink) {
        return "https://www.reddit.com" + permalink;
      }
      const link = post.querySelector('a[href*="/comments/"]');
      if (link && link.href) {
        return link.href;
      }
    }
    return location.href;
  }

  // TikTok video permalink resolution
  if (host.includes("tiktok.com")) {
    if (location.pathname.includes("/video/")) {
      return location.href;
    }
    let curr = videoEl;
    while (curr && curr !== document.body) {
      const link = curr.querySelector('a[href*="/video/"]');
      if (link && link.href) {
        return link.href;
      }
      curr = curr.parentElement;
    }
    return location.href;
  }

  // On platforms like YouTube, Vimeo, Twitch, the page URL is the true yt-dlp source
  if (host.includes("youtube.com") || host.includes("youtu.be") ||
      host.includes("vimeo.com") || host.includes("twitch.tv") ||
      host.includes("facebook.com")) {
    return location.href;
  }

  // Generic HTML5 video: inspect video src or child <source>
  if (videoEl.currentSrc && !videoEl.currentSrc.startsWith("blob:")) {
    return videoEl.currentSrc;
  }
  if (videoEl.src && !videoEl.src.startsWith("blob:")) {
    return videoEl.src;
  }
  const source = videoEl.querySelector("source[src]");
  if (source && source.src) {
    return source.src;
  }

  // Fallback to page URL for yt-dlp to inspect
  return location.href;
}

function triggerDownload(url, kind, format, title) {
  // Check settings to see if download confirmation dialog is enabled
  chrome.storage.sync.get({ showDownloadDialog: true }, (cfg) => {
    if (cfg.showDownloadDialog !== false) {
      showToast("Opening Download Dialog", title || url, "dialog");
      try {
        chrome.runtime.sendMessage({
          action: "TRIGGER_DOWNLOAD_DIALOG",
          payload: {
            url,
            kind,
            format,
            title,
            referrer: location.href,
          },
        });
      } catch (e) {
        console.warn("[mDm] Error sending TRIGGER_DOWNLOAD_DIALOG:", e);
      }
    } else {
      showToast("Sending to mDm…", title || url, "start");
      try {
        chrome.runtime.sendMessage(
          {
            action: "SEND_TO_MDM",
            payload: { url, kind, format, title },
          },
          (res) => {
            if (chrome.runtime.lastError || !res || !res.ok) {
              const err = res?.error || "Could not connect to mDm desktop app at 127.0.0.1:5757. Launch mDm to start.";
              showToast("mDm Download Failed", err, "error");
            } else {
              showToast("Download Started", `Sent to mDm: ${title || url}`, "complete");
            }
          }
        );
      } catch (e) {
        console.warn("[mDm] Error sending SEND_TO_MDM:", e);
      }
    }
  });
}

// --- Toast Notifications ---------------------------------------------------

function showToast(title, message, type = "info") {
  const existing = document.querySelector(".mdm-toast");
  if (existing) existing.remove();

  const isError = type === "error";
  const isComplete = type === "complete";
  const isStart = type === "start" || type === "downloading";
  const isDialog = type === "dialog";

  let statusBadge = "INFO";
  let statusClass = "mdm-status-info";
  let iconSvg = MASCOT_SVG;

  if (isError) {
    statusBadge = "FAILED";
    statusClass = "mdm-status-error";
    iconSvg = `
      <svg viewBox="0 0 24 24" fill="none" stroke="#ef4444" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="10" stroke-opacity="0.3"/>
        <line x1="15" y1="9" x2="9" y2="15"/>
        <line x1="9" y1="9" x2="15" y2="15"/>
      </svg>
    `;
  } else if (isComplete) {
    statusBadge = "COMPLETED";
    statusClass = "mdm-status-success";
    iconSvg = `
      <svg viewBox="0 0 24 24" fill="none" stroke="#10b981" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="10" stroke-opacity="0.3"/>
        <polyline points="8 12 11 15 16 9"/>
      </svg>
    `;
  } else if (isStart) {
    statusBadge = "DOWNLOADING";
    statusClass = "mdm-status-start";
    iconSvg = `
      <svg viewBox="0 0 24 24" fill="none" stroke="#e8934a" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="10" stroke-opacity="0.3"/>
        <path d="M12 7v8"/>
        <polyline points="9 12 12 15 15 12"/>
      </svg>
    `;
  } else if (isDialog) {
    statusBadge = "DIALOG";
    statusClass = "mdm-status-dialog";
    iconSvg = `
      <svg viewBox="0 0 24 24" fill="none" stroke="#38bdf8" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="10" stroke-opacity="0.3"/>
        <rect x="7" y="7" width="10" height="10" rx="1.5"/>
      </svg>
    `;
  }

  const toast = document.createElement("div");
  toast.className = `mdm-toast ${statusClass}`;

  toast.innerHTML = `
    <div class="mdm-toast-indicator-bar"></div>
    <div class="mdm-toast-icon">
      ${iconSvg}
    </div>
    <div class="mdm-toast-body">
      <div class="mdm-toast-header-row">
        <span class="mdm-toast-title">${title}</span>
        <span class="mdm-toast-badge">${statusBadge}</span>
      </div>
      <span class="mdm-toast-msg" title="${message}">${message}</span>
    </div>
  `;

  document.body.appendChild(toast);

  setTimeout(() => {
    toast.classList.add("mdm-toast-exit");
    setTimeout(() => toast.remove(), 250);
  }, 3600);
}

// --- DOM Video Scanner & Observer ------------------------------------------

function scanVideos() {
  const videos = document.querySelectorAll("video");
  videos.forEach((v) => {
    // Only attach to visible videos with some dimensions
    const rect = v.getBoundingClientRect();
    if (rect.width > 60 || rect.height > 60 || v.readyState > 0 || !v.paused) {
      attachIdmWidget(v);
    }
  });

  // Clean up detached widgets
  for (const widget of activeWidgets) {
    if (!widget._mdmVideo || !widget._mdmVideo.isConnected) {
      widget.remove();
      activeWidgets.delete(widget);
    }
  }

  updateAllWidgetPositions();
}

// Debounced MutationObserver for smooth performance
let scanTimer = null;
const observer = new MutationObserver(() => {
  if (scanTimer) clearTimeout(scanTimer);
  scanTimer = setTimeout(scanVideos, 300);
});

observer.observe(document.body || document.documentElement, {
  childList: true,
  subtree: true,
});

// Initial scan
scanVideos();
// Also trigger scan after window load and media events
window.addEventListener("load", scanVideos);
document.addEventListener("play", (e) => {
  if (e.target.tagName === "VIDEO") {
    attachIdmWidget(e.target);
    updateAllWidgetPositions();
  }
}, true);

// --- IDM-Style Direct Link Click Interception ------------------------------

const DEFAULT_EXTENSIONS = 
  "exe zip rar 7z iso tar gz bz2 xz mp4 mkv avi mov webm flv ts mp3 flac wav m4a aac ogg opus pdf doc docx xls xlsx ppt pptx apk deb rpm dmg msi appimage torrent".split(" ");

let contentCaptureSet = new Set(DEFAULT_EXTENSIONS);
let contentIntercept = true;

chrome.storage.sync.get({ interceptDownloads: true, captureExtensions: "" }, (cfg) => {
  if (cfg.interceptDownloads !== undefined) contentIntercept = cfg.interceptDownloads !== false;
  if (cfg.captureExtensions) {
    const arr = String(cfg.captureExtensions).toLowerCase().replace(/[.,;]/g, " ").trim().split(/\s+/);
    contentCaptureSet = new Set(arr.filter(Boolean));
  }
});

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "sync") {
    if (changes.interceptDownloads) contentIntercept = changes.interceptDownloads.newValue;
    if (changes.captureExtensions) {
      const arr = String(changes.captureExtensions.newValue).toLowerCase().replace(/[.,;]/g, " ").trim().split(/\s+/);
      contentCaptureSet = new Set(arr.filter(Boolean));
    }
  }
});

function getUrlExtension(rawUrl) {
  if (!rawUrl) return "";
  try {
    const pathname = new URL(rawUrl).pathname;
    const clean = pathname.split(/[?#]/)[0];
    const m = clean.match(/\.([a-z0-9]{1,8})$/i);
    return m ? m[1].toLowerCase() : "";
  } catch {
    const clean = String(rawUrl).split(/[?#]/)[0];
    const m = clean.match(/\.([a-z0-9]{1,8})$/i);
    return m ? m[1].toLowerCase() : "";
  }
}

document.addEventListener(
  "click",
  (e) => {
    if (!contentIntercept) return;
    const a = e.target.closest("a[href]");
    if (!a) return;

    const href = a.href;
    if (!href || href.startsWith("javascript:") || href.startsWith("#") || href.startsWith("mailto:")) return;

    // Skip internal hash jumps on same page
    if (href.startsWith(location.origin + location.pathname + "#")) return;

    // Check extension from href or download attribute
    const ext = getUrlExtension(a.download || href);
    if (ext && contentCaptureSet.has(ext)) {
      e.preventDefault();
      e.stopPropagation();

      let filename = a.download || "";
      if (!filename) {
        try {
          filename = new URL(href).pathname.split("/").pop() || "";
        } catch {
          filename = href.split("?")[0].split("/").pop() || "";
        }
      }
      filename = decodeURIComponent(filename).split("?")[0];

      chrome.runtime.sendMessage({
        action: "INTERCEPT_LINK_CLICK",
        payload: {
          url: href,
          filename,
          title: a.innerText?.trim() || filename,
        },
      });

      showToast("mDm Intercepted", `Sending to mDm: ${filename}`, "start");
    }
  },
  true
);
