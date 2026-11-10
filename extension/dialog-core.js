// mDm Download Confirmation Dialog — shared UI (popup window + in-page modal)
"use strict";

(function () {
  const SVG_NS = "http://www.w3.org/2000/svg";

  // --- Category icons (built as real SVG nodes; no innerHTML, CSP/TT-safe) ---
  const CATEGORY_ICONS = {
    videos: [
      ["rect", { x: 2, y: 2, width: 20, height: 20, rx: 2.18, ry: 2.18 }],
      ["line", { x1: 7, y1: 2, x2: 7, y2: 22 }],
      ["line", { x1: 17, y1: 2, x2: 17, y2: 22 }],
      ["line", { x1: 2, y1: 12, x2: 22, y2: 12 }],
      ["line", { x1: 2, y1: 7, x2: 7, y2: 7 }],
      ["line", { x1: 2, y1: 17, x2: 7, y2: 17 }],
      ["line", { x1: 17, y1: 17, x2: 22, y2: 17 }],
      ["line", { x1: 17, y1: 7, x2: 22, y2: 7 }],
    ],
    audio: [
      ["path", { d: "M9 18V5l12-2v13" }],
      ["circle", { cx: 6, cy: 18, r: 3 }],
      ["circle", { cx: 18, cy: 16, r: 3 }],
    ],
    compressed: [
      ["polyline", { points: "21 8 21 21 3 21 3 8" }],
      ["rect", { x: 1, y: 3, width: 22, height: 5 }],
      ["line", { x1: 10, y1: 12, x2: 14, y2: 12 }],
    ],
    programs: [
      ["rect", { x: 4, y: 4, width: 16, height: 16, rx: 2 }],
      ["rect", { x: 9, y: 9, width: 6, height: 6 }],
      ["line", { x1: 9, y1: 1, x2: 9, y2: 4 }],
      ["line", { x1: 15, y1: 1, x2: 15, y2: 4 }],
      ["line", { x1: 9, y1: 20, x2: 9, y2: 23 }],
      ["line", { x1: 15, y1: 20, x2: 15, y2: 23 }],
      ["line", { x1: 20, y1: 9, x2: 23, y2: 9 }],
      ["line", { x1: 20, y1: 14, x2: 23, y2: 14 }],
      ["line", { x1: 1, y1: 9, x2: 4, y2: 9 }],
      ["line", { x1: 1, y1: 14, x2: 4, y2: 14 }],
    ],
    documents: [
      ["path", { d: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" }],
      ["polyline", { points: "14 2 14 8 20 8" }],
      ["line", { x1: 16, y1: 13, x2: 8, y2: 13 }],
      ["line", { x1: 16, y1: 17, x2: 8, y2: 17 }],
      ["polyline", { points: "10 9 9 9 8 9" }],
    ],
    general: [
      ["path", { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }],
      ["polyline", { points: "7 10 12 15 17 10" }],
      ["line", { x1: 12, y1: 15, x2: 12, y2: 3 }],
    ],
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

  function makeElement(doc, tag, attrs, children, namespace) {
    const node = namespace
      ? doc.createElementNS(namespace, tag)
      : doc.createElement(tag);
    if (attrs) {
      for (const key of Object.keys(attrs)) {
        const value = attrs[key];
        if (value === false || value == null) continue;
        node.setAttribute(key, value === true ? "" : String(value));
      }
    }
    if (children) {
      for (const child of children) {
        if (child == null) continue;
        node.appendChild(typeof child === "string" ? doc.createTextNode(child) : child);
      }
    }
    return node;
  }

  function buildIcon(doc, category) {
    const spec = CATEGORY_ICONS[category] || CATEGORY_ICONS.general;
    const svg = doc.createElementNS(SVG_NS, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "currentColor");
    svg.setAttribute("stroke-width", "2");
    svg.setAttribute("stroke-linecap", "round");
    svg.setAttribute("stroke-linejoin", "round");
    for (const [tag, attrs] of spec) {
      const node = doc.createElementNS(SVG_NS, tag);
      for (const key of Object.keys(attrs)) node.setAttribute(key, String(attrs[key]));
      svg.appendChild(node);
    }
    return svg;
  }

  // Builds the dialog markup without innerHTML/DOMParser so it works on pages
  // with Trusted Types enforcement (YouTube, GitHub, X, etc.).
  function build(doc) {
    const owner = doc || document;
    const h = (tag, attrs, ...children) => makeElement(owner, tag, attrs, children, null);
    const s = (tag, attrs, ...children) => makeElement(owner, tag, attrs, children, SVG_NS);

    const mascot = s(
      "svg", { class: "dlg-mascot", viewBox: "0 0 120 120", "aria-hidden": "true" },
      s("defs", null,
        s("radialGradient", { id: "g-dlg-mascot", cx: "0.5", cy: "0.5", r: "0.75" },
          s("stop", { offset: "0", "stop-color": "#f2a95c" }),
          s("stop", { offset: "1", "stop-color": "#c96a2e" })
        )
      ),
      s("circle", { cx: 60, cy: 62, r: 44, fill: "url(#g-dlg-mascot)" }),
      s("circle", { cx: 46, cy: 44, r: 6.5, fill: "#141820" }),
      s("circle", { cx: 74, cy: 44, r: 6.5, fill: "#141820" }),
      s("circle", { cx: 48, cy: 42, r: 2, fill: "#fff" }),
      s("circle", { cx: 76, cy: 42, r: 2, fill: "#fff" }),
      s("path", { d: "M47 57 Q60 66 73 57", stroke: "#141820", "stroke-width": 4.5, "stroke-linecap": "round", fill: "none" }),
      s("rect", { x: 54.5, y: 70, width: 11, height: 16, rx: 5.5, fill: "#141820" }),
      s("path", { d: "M42 80 h36 L60 97 Z", fill: "#141820" })
    );

    const closeIcon = s(
      "svg", { viewBox: "0 0 24 24", width: 14, height: 14, fill: "none", stroke: "currentColor", "stroke-width": 2.5, "stroke-linecap": "round" },
      s("line", { x1: 18, y1: 6, x2: 6, y2: 18 }),
      s("line", { x1: 6, y1: 6, x2: 18, y2: 18 })
    );

    const linkIcon = s(
      "svg", { class: "dlg-label-icon", viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", "stroke-width": 2, "stroke-linecap": "round", "stroke-linejoin": "round" },
      s("path", { d: "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" }),
      s("path", { d: "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" })
    );

    const copyIcon = s(
      "svg", { class: "btn-copy-icon", viewBox: "0 0 24 24", width: 13, height: 13, fill: "none", stroke: "currentColor", "stroke-width": 2, "stroke-linecap": "round", "stroke-linejoin": "round" },
      s("rect", { x: 9, y: 9, width: 13, height: 13, rx: 2, ry: 2 }),
      s("path", { d: "M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" })
    );

    const folderIcon = s(
      "svg", { class: "dlg-label-icon", viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", "stroke-width": 2, "stroke-linecap": "round", "stroke-linejoin": "round" },
      s("path", { d: "M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" })
    );

    const saveFolderIcon = s(
      "svg", { class: "dlg-label-icon", viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", "stroke-width": 2, "stroke-linecap": "round", "stroke-linejoin": "round" },
      s("path", { d: "M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2z" })
    );

    const clockIcon = s(
      "svg", { viewBox: "0 0 24 24", width: 13, height: 13, fill: "none", stroke: "currentColor", "stroke-width": 2.2, "stroke-linecap": "round", "stroke-linejoin": "round" },
      s("circle", { cx: 12, cy: 12, r: 10 }),
      s("polyline", { points: "12 6 12 12 16 14" })
    );

    const cancelIcon = s(
      "svg", { viewBox: "0 0 24 24", width: 13, height: 13, fill: "none", stroke: "currentColor", "stroke-width": 2.2, "stroke-linecap": "round", "stroke-linejoin": "round" },
      s("line", { x1: 18, y1: 6, x2: 6, y2: 18 }),
      s("line", { x1: 6, y1: 6, x2: 18, y2: 18 })
    );

    const startIcon = s(
      "svg", { viewBox: "0 0 24 24", width: 14, height: 14, fill: "none", stroke: "currentColor", "stroke-width": 2.5, "stroke-linecap": "round", "stroke-linejoin": "round" },
      s("path", { d: "M12 3v13" }),
      s("polyline", { points: "7 11 12 16 17 11" }),
      s("line", { x1: 5, y1: 21, x2: 19, y2: 21 })
    );

    const header = h("header", { class: "dlg-head" },
      h("div", { class: "dlg-title" },
        mascot,
        h("span", { class: "dlg-app-name" }, "mDm"),
        h("span", { class: "dlg-divider" }, "|"),
        h("span", { class: "dlg-header-label" }, "Download File Info")
      ),
      h("button", { type: "button", class: "dlg-btn-close", id: "btn-close", title: "Close", "aria-label": "Close dialog" }, closeIcon)
    );

    const fileCard = h("div", { class: "dlg-file-card" },
      h("div", { class: "dlg-file-icon-box", id: "dlg-file-icon", title: "Detected File Type" }),
      h("div", { class: "dlg-file-meta" },
        h("div", { class: "dlg-file-label-row" },
          h("label", { for: "dlg-filename", class: "dlg-meta-title" }, "Save As:"),
          h("span", { class: "dlg-ext-badge", id: "dlg-ext-badge" }, "FILE")
        ),
        h("input", {
          type: "text", id: "dlg-filename", class: "dlg-input dlg-filename-input",
          spellcheck: "false", autocomplete: "off", placeholder: "Filename...",
        })
      )
    );

    const urlRow = h("div", { class: "dlg-row" },
      h("label", { for: "dlg-url", class: "dlg-label" }, linkIcon, h("span", null, "URL:")),
      h("div", { class: "dlg-field-wrap dlg-has-button" },
        h("input", {
          type: "text", id: "dlg-url", class: "dlg-input dlg-readonly",
          readonly: true, spellcheck: "false", placeholder: "Download target URL...",
        }),
        h("button", { type: "button", class: "dlg-field-btn", id: "btn-copy-url", title: "Copy URL to clipboard" },
          copyIcon,
          h("span", { class: "dlg-copy-text", id: "dlg-copy-text" }, "Copy")
        )
      )
    );

    const categoryRow = h("div", { class: "dlg-row" },
      h("label", { for: "dlg-category", class: "dlg-label" }, folderIcon, h("span", null, "Category:")),
      h("div", { class: "dlg-field-wrap dlg-grid-cat-size" },
        h("select", { id: "dlg-category", class: "dlg-select" },
          h("option", { value: "general" }, "General"),
          h("option", { value: "programs" }, "Programs (.exe, .msi, .apk, .iso)"),
          h("option", { value: "compressed" }, "Compressed (.zip, .rar, .7z, .tar)"),
          h("option", { value: "videos" }, "Videos (.mp4, .mkv, .webm, .mov)"),
          h("option", { value: "audio" }, "Audio (.mp3, .flac, .wav, .m4a)"),
          h("option", { value: "documents" }, "Documents (.pdf, .docx, .txt)")
        ),
        h("div", { class: "dlg-size-badge", id: "dlg-size-badge" },
          h("span", { class: "dlg-size-dot", id: "dlg-size-dot" }),
          h("span", { id: "dlg-size" }, "Size: Probing…")
        )
      )
    );

    const saveRow = h("div", { class: "dlg-row" },
      h("label", { for: "dlg-output-dir", class: "dlg-label" }, saveFolderIcon, h("span", null, "Save To:")),
      h("div", { class: "dlg-field-wrap" },
        h("input", {
          type: "text", id: "dlg-output-dir", class: "dlg-input",
          spellcheck: "false", placeholder: "Destination directory...",
        })
      )
    );

    const optionsRow = h("div", { class: "dlg-options-row" },
      h("label", { class: "dlg-checkbox-label" },
        h("input", { type: "checkbox", id: "dlg-chk-show", checked: true }),
        h("span", null, "Always show this dialog before starting downloads")
      )
    );

    const body = h("main", { class: "dlg-body" }, fileCard, urlRow, categoryRow, saveRow, optionsRow);

    const footer = h("footer", { class: "dlg-footer" },
      h("div", { class: "dlg-footer-left" },
        h("div", { class: "dlg-status-indicator", id: "dlg-status" },
          h("span", { class: "dlg-status-dot is-connecting", id: "dlg-status-dot" }),
          h("span", { class: "dlg-status-label", id: "dlg-status-text" }, "Connecting to mDm…")
        )
      ),
      h("div", { class: "dlg-buttons" },
        h("button", { type: "button", id: "btn-later", class: "dlg-btn-secondary", title: "Add to queue in paused state" },
          clockIcon, h("span", null, "Download Later")
        ),
        h("button", { type: "button", id: "btn-cancel", class: "dlg-btn-secondary", title: "Cancel this download" },
          cancelIcon, h("span", null, "Cancel")
        ),
        h("button", { type: "button", id: "btn-start", class: "dlg-btn-primary", title: "Start downloading immediately" },
          startIcon, h("span", null, "Start Download")
        )
      )
    );

    return h("div", { class: "dlg-window" }, header, body, footer);
  }

  function createDialog(options) {
    const root = options.root || document;
    const params = options.params || {};
    const onClose = typeof options.onClose === "function" ? options.onClose : () => {};
    const byId = (id) => root.querySelector("#" + id);

    const el = {
      url: byId("dlg-url"),
      filename: byId("dlg-filename"),
      category: byId("dlg-category"),
      outputDir: byId("dlg-output-dir"),
      extBadge: byId("dlg-ext-badge"),
      fileIcon: byId("dlg-file-icon"),
      size: byId("dlg-size"),
      sizeDot: byId("dlg-size-dot"),
      copyBtn: byId("btn-copy-url"),
      copyText: byId("dlg-copy-text"),
      chkShow: byId("dlg-chk-show"),
      statusDot: byId("dlg-status-dot"),
      statusText: byId("dlg-status-text"),
      btnStart: byId("btn-start"),
      btnLater: byId("btn-later"),
      btnCancel: byId("btn-cancel"),
      btnClose: byId("btn-close"),
    };

    if (!el.url || !el.filename || !el.category || !el.btnStart) {
      console.error("[mDm] Dialog markup is missing required elements");
      return { destroy() {} };
    }

    const doc = root.ownerDocument || document;
    let destroyed = false;
    let copyTimer = 0;
    let rawSize = parseInt(params.size || 0, 10) || 0;
    const rawUrl = params.url || "";
    const rawKind = params.kind || "";
    const rawFormat = params.format || "";

    const startLabel = el.btnStart.querySelector("span");
    const laterLabel = el.btnLater ? el.btnLater.querySelector("span") : null;
    const startText = startLabel ? startLabel.textContent : "Start Download";
    const laterText = laterLabel ? laterLabel.textContent : "Download Later";

    let filename = params.filename || "";
    if (!filename && rawUrl) {
      try {
        filename = new URL(rawUrl).pathname.split("/").pop() || "download";
      } catch (_) {
        filename = rawUrl.split("?")[0].split("/").pop() || "download";
      }
    }
    try { filename = decodeURIComponent(filename); } catch (_) {}
    filename = (filename || "").split("?")[0].trim() || "download";

    function setFileIcon(category) {
      if (el.fileIcon) el.fileIcon.replaceChildren(buildIcon(doc, category));
    }

    function updateFilePresentation(name, category) {
      const parts = name.split(".");
      const ext = (parts.length > 1 ? parts.pop() : "").toLowerCase();
      const cat = category || EXT_CATEGORY[ext] || "general";
      if (el.extBadge) {
        el.extBadge.textContent = (ext ? ext.toUpperCase() : cat.toUpperCase()).slice(0, 6);
      }
      setFileIcon(cat);
    }

    function showSize(text, stateClass) {
      if (el.size) el.size.textContent = text;
      if (el.sizeDot) {
        el.sizeDot.className = "dlg-size-dot" + (stateClass ? " " + stateClass : "");
      }
    }

    function setStatus(kind, text) {
      if (el.statusDot) {
        el.statusDot.className = "dlg-status-dot" + (kind ? " is-" + kind : "");
      }
      if (el.statusText) {
        el.statusText.textContent = text;
        el.statusText.title = text;
      }
    }

    function setSubmitting(downloadLater) {
      if (el.btnStart) el.btnStart.disabled = true;
      if (el.btnLater) el.btnLater.disabled = true;
      if (downloadLater) {
        if (laterLabel) laterLabel.textContent = "Saving…";
      } else if (startLabel) {
        startLabel.textContent = "Starting…";
      }
    }

    function resetSubmitting() {
      if (el.btnStart) el.btnStart.disabled = false;
      if (el.btnLater) el.btnLater.disabled = false;
      if (startLabel) startLabel.textContent = startText;
      if (laterLabel) laterLabel.textContent = laterText;
    }

    function submitDownload(downloadLater) {
      if (destroyed) return;
      const url = el.url.value.trim();
      if (!url) {
        setStatus("error", "No download URL provided");
        return;
      }
      const title = el.filename.value.trim();
      const cat = el.category.value;
      const outputDir = el.outputDir ? el.outputDir.value.trim() : "";
      const showDialog = !!(el.chkShow && el.chkShow.checked);

      try { chrome.storage.sync.set({ showDownloadDialog: showDialog }); } catch (_) {}

      let kind = "direct";
      if (cat === "videos") kind = "video";
      else if (cat === "audio") kind = "audio";
      else if (rawKind === "video") kind = "video";
      else if (rawKind === "audio") kind = "audio";

      setSubmitting(downloadLater);

      chrome.runtime.sendMessage(
        {
          action: "SEND_TO_MDM",
          payload: {
            url,
            kind,
            format: rawFormat || undefined,
            title,
            output_dir: outputDir || undefined,
            downloadLater,
          },
        },
        (res) => {
          if (destroyed) return;
          if (chrome.runtime.lastError) {
            setStatus("error", "mDm Engine unavailable — reload the extension");
            resetSubmitting();
            return;
          }
          if (res && res.ok) {
            onClose();
            return;
          }
          setStatus("error", (res && res.error) || "Could not connect to mDm at 127.0.0.1:5757");
          resetSubmitting();
        }
      );
    }

    function legacyCopy(text) {
      const area = doc.createElement("textarea");
      area.value = text;
      area.setAttribute("readonly", "");
      area.style.position = "fixed";
      area.style.top = "-9999px";
      area.style.opacity = "0";
      (doc.body || doc.documentElement).appendChild(area);
      area.select();
      try { doc.execCommand("copy"); } catch (_) {}
      area.remove();
    }

    // --- Initial populate -----------------------------------------------------
    el.url.value = rawUrl;
    el.filename.value = filename;
    const initialExt = (filename.split(".").pop() || "").toLowerCase();
    const initialCategory =
      EXT_CATEGORY[initialExt] ||
      (rawKind === "video" ? "videos" : rawKind === "audio" ? "audio" : "general");
    el.category.value = initialCategory;
    updateFilePresentation(filename, initialCategory);

    if (rawSize > 0) {
      showSize(`Size: ${fmtBytes(rawSize)}`, "is-verified");
    } else {
      showSize("Size: Probing…", "is-probing");
      if (rawUrl) {
        try {
          chrome.runtime.sendMessage({ action: "PROBE_URL_SIZE", url: rawUrl }, (res) => {
            if (destroyed) return;
            if (chrome.runtime.lastError || !res || !res.ok || !(res.size > 0)) {
              showSize("Size: Unknown (Stream)", "");
              return;
            }
            rawSize = res.size;
            showSize(`Size: ${fmtBytes(rawSize)}`, "is-verified");
            if (res.filename && el.filename.value === "download") {
              el.filename.value = res.filename;
              updateFilePresentation(res.filename, el.category.value);
            }
          });
        } catch (_) {}
      } else {
        showSize("Size: Unknown (Stream)", "");
      }
    }

    // --- Engine status --------------------------------------------------------
    try {
      chrome.runtime.sendMessage({ action: "CHECK_STATUS" }, (res) => {
        if (destroyed) return;
        if (chrome.runtime.lastError) {
          setStatus("error", "mDm status unavailable");
        } else if (res && res.connected) {
          setStatus("connected", `mDm Ready (v${res.version || "0.1.0"})`);
        } else {
          setStatus("error", "mDm Offline — launch the app");
        }
      });
    } catch (_) {}

    // --- Default output directory --------------------------------------------
    try {
      chrome.runtime.sendMessage({ action: "GET_CONFIG" }, (res) => {
        if (destroyed || chrome.runtime.lastError) return;
        const dir = res && res.config && res.config.output_dir;
        if (dir && el.outputDir) el.outputDir.value = dir;
      });
    } catch (_) {}

    // --- Saved preference -----------------------------------------------------
    try {
      chrome.storage.sync.get({ showDownloadDialog: true }, (cfg) => {
        if (!destroyed && el.chkShow) el.chkShow.checked = cfg.showDownloadDialog !== false;
      });
    } catch (_) {}

    // --- Event handlers -------------------------------------------------------
    const onFilenameInput = (e) => updateFilePresentation(e.target.value, el.category.value);
    const onCategoryChange = (e) => updateFilePresentation(el.filename.value, e.target.value);

    const onCopy = () => {
      const url = el.url.value;
      if (!url) return;
      const done = () => {
        if (destroyed || !el.copyText) return;
        const original = el.copyText.textContent;
        el.copyText.textContent = "Copied!";
        clearTimeout(copyTimer);
        copyTimer = setTimeout(() => {
          if (!destroyed) el.copyText.textContent = original;
        }, 1500);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(url).then(done).catch(() => { legacyCopy(url); done(); });
      } else {
        legacyCopy(url);
        done();
      }
    };

    const onStart = () => submitDownload(false);
    const onLater = () => submitDownload(true);
    const onCancel = () => onClose();

    const onKeyDown = (e) => {
      if (destroyed) return;
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
        return;
      }
      if (e.key === "Enter") {
        if (e.target && typeof e.target.closest === "function" && e.target.closest("button")) return;
        e.preventDefault();
        submitDownload(false);
      }
    };

    el.filename.addEventListener("input", onFilenameInput);
    el.category.addEventListener("change", onCategoryChange);
    if (el.copyBtn) el.copyBtn.addEventListener("click", onCopy);
    el.btnStart.addEventListener("click", onStart);
    if (el.btnLater) el.btnLater.addEventListener("click", onLater);
    if (el.btnCancel) el.btnCancel.addEventListener("click", onCancel);
    if (el.btnClose) el.btnClose.addEventListener("click", onCancel);

    const isShadowRoot = !!(root && root.host && typeof root.querySelector === "function");
    const keyTarget = isShadowRoot ? root : window;
    keyTarget.addEventListener("keydown", onKeyDown, true);

    return {
      destroy() {
        if (destroyed) return;
        destroyed = true;
        el.filename.removeEventListener("input", onFilenameInput);
        el.category.removeEventListener("change", onCategoryChange);
        if (el.copyBtn) el.copyBtn.removeEventListener("click", onCopy);
        el.btnStart.removeEventListener("click", onStart);
        if (el.btnLater) el.btnLater.removeEventListener("click", onLater);
        if (el.btnCancel) el.btnCancel.removeEventListener("click", onCancel);
        if (el.btnClose) el.btnClose.removeEventListener("click", onCancel);
        keyTarget.removeEventListener("keydown", onKeyDown, true);
        clearTimeout(copyTimer);
      },
    };
  }

  window.MdmDialogCore = { build, create: createDialog };
})();
