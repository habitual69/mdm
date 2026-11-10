// mDm Download Confirmation Dialog — popup window bootstrap
"use strict";

// --- Fixed dialog size -------------------------------------------------------
// Chrome popup windows are user-resizable by the OS window manager. Snap the
// popup back to its designed size so the fallback dialog never squashes.
const DIALOG_WIDTH = 580;
const DIALOG_HEIGHT = 460;
let popupWindowId = null;
let resizeSnapTimer = 0;

function restorePopupSize() {
  if (popupWindowId === null) return;
  chrome.windows.get(popupWindowId, (win) => {
    if (chrome.runtime.lastError || !win) return;
    if (win.width === DIALOG_WIDTH && win.height === DIALOG_HEIGHT) return;
    chrome.windows.update(
      popupWindowId,
      { width: DIALOG_WIDTH, height: DIALOG_HEIGHT },
      () => void chrome.runtime.lastError
    );
  });
}

chrome.windows?.getCurrent((win) => {
  if (chrome.runtime.lastError || !win || win.type !== "popup") return;
  popupWindowId = win.id;
  if (win.width !== DIALOG_WIDTH || win.height !== DIALOG_HEIGHT) {
    chrome.windows.update(
      win.id,
      { width: DIALOG_WIDTH, height: DIALOG_HEIGHT },
      () => void chrome.runtime.lastError
    );
  }
});

window.addEventListener("resize", () => {
  if (popupWindowId === null) return;
  clearTimeout(resizeSnapTimer);
  resizeSnapTimer = setTimeout(restorePopupSize, 150);
});

// --- Dialog UI ---------------------------------------------------------------
const dlgParams = new URLSearchParams(location.search);

document.body.appendChild(MdmDialogCore.build(document));

MdmDialogCore.create({
  root: document,
  params: {
    url: dlgParams.get("url") || "",
    filename: dlgParams.get("filename") || "",
    size: parseInt(dlgParams.get("size") || "0", 10) || 0,
    kind: dlgParams.get("kind") || "",
    format: dlgParams.get("format") || "",
    referrer: dlgParams.get("referrer") || "",
  },
  onClose: () => window.close(),
});
