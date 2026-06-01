/**
 * FluxDM Browser Extension — Background Service Worker (MV3)
 *
 * Responsibilities:
 *  1. Intercept browser downloads and redirect to FluxDM
 *  2. Right-click context menu ("Send to FluxDM") on links and pages
 *  3. Relay messages from popup.js → FluxDM
 *  4. Monitor clipboard for download URLs (via content script signals)
 *  5. Send through native messaging first; fall back to HTTP localhost:54321
 */

// ── Config ────────────────────────────────────────────────────────────────────

const NATIVE_HOST   = "com.fluxdm.host";
const HTTP_ENDPOINT = "http://127.0.0.1:54321/add";
const STATUS_URL    = "http://127.0.0.1:54321/status";

/** File extensions that should be captured automatically. */
const CAPTURE_RE = /\.(zip|exe|dmg|mp4|mkv|avi|mov|pdf|iso|7z|rar|tar\.gz|tar\.bz2|msi|deb|rpm|apk|pkg|jar|bin|img|xz|zst|epub|mobi)(\?.*)?$/i;

/** Headers map: url → {header: value} — collected by webRequest listener. */
const capturedHeaders = new Map();

// ── 1. Context menu setup ─────────────────────────────────────────────────────

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id:       "fluxdm-link",
    title:    "⬇ Download with FluxDM",
    contexts: ["link"],
  });
  chrome.contextMenus.create({
    id:       "fluxdm-page",
    title:    "⬇ Download this page with FluxDM",
    contexts: ["page"],
  });
  chrome.contextMenus.create({
    id:       "fluxdm-separator",
    type:     "separator",
    contexts: ["link", "page"],
  });
  chrome.contextMenus.create({
    id:       "fluxdm-clipboard",
    title:    "⬇ Download URL from clipboard",
    contexts: ["page"],
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const pageUrl = tab?.url || "";

  if (info.menuItemId === "fluxdm-link" && info.linkUrl) {
    const url      = info.linkUrl;
    const filename = urlFilename(url);
    const headers  = capturedHeaders.get(url) || {};
    const cookies  = await getCookieHeader(url);

    await sendToFluxDM({ url, filename, headers, cookies, referrer: pageUrl, pageUrl });
  }

  if (info.menuItemId === "fluxdm-page" && pageUrl) {
    const filename = urlFilename(pageUrl);
    const cookies  = await getCookieHeader(pageUrl);
    await sendToFluxDM({ url: pageUrl, filename, headers: {}, cookies, referrer: "", pageUrl });
  }

  if (info.menuItemId === "fluxdm-clipboard") {
    // Read from session storage (set by content.js clipboard monitor)
    const { clipboardUrl } = await chrome.storage.session.get("clipboardUrl");
    if (clipboardUrl) {
      const filename = urlFilename(clipboardUrl);
      const cookies  = await getCookieHeader(clipboardUrl);
      await sendToFluxDM({ url: clipboardUrl, filename, headers: {}, cookies, referrer: pageUrl, pageUrl });
      await chrome.storage.session.remove("clipboardUrl");
      chrome.action.setBadgeText({ text: "" });
    } else {
      showNotification("No download URL found in clipboard", true);
    }
  }
});

// ── 2. Intercept browser downloads ───────────────────────────────────────────

chrome.downloads.onCreated.addListener(async (item) => {
  const url      = item.url || item.finalUrl || "";
  const filename = item.filename?.split(/[/\\]/).pop() || urlFilename(url);

  // Only intercept if it looks like a large/binary file
  if (!CAPTURE_RE.test(filename) && !CAPTURE_RE.test(url)) return;

  chrome.downloads.cancel(item.id);
  chrome.downloads.erase({ id: item.id });

  const headers  = capturedHeaders.get(url) || {};
  const cookies  = await getCookieHeader(url);
  const [tab]    = await chrome.tabs.query({ active: true, currentWindow: true });
  const pageUrl  = tab?.url || "";
  const referrer = headers["referer"] || pageUrl;

  await sendToFluxDM({
    url,
    filename,
    headers,
    cookies,
    referrer,
    pageUrl,
    contentType: item.mime || headers["content-type"] || "",
    fileSize:    item.totalBytes || -1,
  });
});

// ── 3. Capture request headers ────────────────────────────────────────────────

chrome.webRequest.onBeforeSendHeaders.addListener(
  (details) => {
    const headers = {};
    for (const h of details.requestHeaders || []) {
      headers[h.name.toLowerCase()] = h.value;
    }
    capturedHeaders.set(details.url, headers);

    // Prevent unbounded growth
    if (capturedHeaders.size > 500) {
      capturedHeaders.delete(capturedHeaders.keys().next().value);
    }
  },
  { urls: ["<all_urls>"] },
  ["requestHeaders"]
);

// ── 4. Clipboard monitor (via content script) ─────────────────────────────────

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message.type === "clipboard_url") {
    const url = message.url;
    if (url && CAPTURE_RE.test(url)) {
      chrome.storage.session.set({ clipboardUrl: url });
      chrome.action.setBadgeText({ text: "1" });
      chrome.action.setBadgeBackgroundColor({ color: "#2563eb" });
    }
    sendResponse({ ok: true });
    return false;
  }

  // ── Relay from popup.js ───────────────────────────────────────────────────
  if (message.type === "add_download") {
    const { payload } = message;
    sendToFluxDM(payload)
      .then((result) => sendResponse({ ok: result.success, id: result.id, error: result.error }))
      .catch((err)   => sendResponse({ ok: false, error: String(err) }));
    return true; // keep channel open for async response
  }

  // ── Status check from popup ───────────────────────────────────────────────
  if (message.type === "check_status") {
    checkFluxDMStatus()
      .then((running) => sendResponse({ running }))
      .catch(()       => sendResponse({ running: false }));
    return true;
  }

  // ── Get clipboard URL for popup ───────────────────────────────────────────
  if (message.type === "get_clipboard_url") {
    chrome.storage.session.get("clipboardUrl", ({ clipboardUrl }) => {
      sendResponse({ url: clipboardUrl || null });
    });
    return true;
  }
});

// ── 5. Send to FluxDM ─────────────────────────────────────────────────────────

/**
 * Try native messaging first; fall back to HTTP.
 * Returns { success, id?, error? }
 */
async function sendToFluxDM(payload) {
  // Normalise the payload to the shape the server expects (camelCase)
  const normalized = {
    url:         payload.url,
    filename:    payload.filename    || urlFilename(payload.url),
    savePath:    payload.savePath    || null,
    headers:     payload.headers     || {},
    cookies:     payload.cookies     || "",
    referrer:    payload.referrer    || "",
    pageUrl:     payload.pageUrl     || "",
    contentType: payload.contentType || "",
    fileSize:    payload.fileSize    || -1,
  };

  // Try native messaging
  try {
    const result = await nativeMessage(normalized);
    if (result?.success) {
      showNotification(`Added to FluxDM: ${normalized.filename}`);
      return result;
    }
  } catch (_) {
    // Native host not installed or app not running — try HTTP
  }

  // HTTP fallback
  return sendViaHttp(normalized);
}

function nativeMessage(payload) {
  return new Promise((resolve, reject) => {
    let port;
    try {
      port = chrome.runtime.connectNative(NATIVE_HOST);
    } catch (e) {
      return reject(e);
    }

    const timer = setTimeout(() => {
      port.disconnect();
      reject(new Error("Native host timeout"));
    }, 5000);

    port.onMessage.addListener((msg) => {
      clearTimeout(timer);
      port.disconnect();
      resolve(msg);
    });

    port.onDisconnect.addListener(() => {
      clearTimeout(timer);
      if (chrome.runtime.lastError) {
        reject(new Error(chrome.runtime.lastError.message));
      }
    });

    port.postMessage(payload);
  });
}

async function sendViaHttp(payload) {
  try {
    const res = await fetch(HTTP_ENDPOINT, {
      method:  "POST",
      headers: { "Content-Type": "application/json" },
      body:    JSON.stringify(payload),
    });

    if (!res.ok) {
      const text = await res.text().catch(() => "");
      throw new Error(`HTTP ${res.status}: ${text}`);
    }

    const data = await res.json();
    if (data.success) {
      showNotification(`Added to FluxDM: ${payload.filename}`);
    }
    return data;
  } catch (e) {
    console.error("[FluxDM] HTTP fallback failed:", e);
    showNotification("FluxDM is not running. Please open the app.", true);
    return { success: false, error: String(e) };
  }
}

async function checkFluxDMStatus() {
  try {
    const res = await fetch(STATUS_URL, { signal: AbortSignal.timeout(2000) });
    return res.ok;
  } catch {
    return false;
  }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

function urlFilename(url) {
  try {
    const u = new URL(url);
    const parts = u.pathname.split("/").filter(Boolean);
    const last  = parts[parts.length - 1] || "";
    return decodeURIComponent(last.split("?")[0]) || "download";
  } catch {
    return url.split("/").pop()?.split("?")[0] || "download";
  }
}

async function getCookieHeader(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {
    return "";
  }
}

function showNotification(message, isError = false) {
  chrome.notifications.create({
    type:     "basic",
    iconUrl:  "icons/icon48.png",
    title:    "FluxDM",
    message,
    priority: isError ? 2 : 0,
  });
}
