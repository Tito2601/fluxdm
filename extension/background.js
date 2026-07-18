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

/** Streaming manifests: HLS playlists and DASH descriptions. */
const MANIFEST_RE = /\.(m3u8|mpd)(\?|$)/i;

/** Content types that identify a manifest when the URL carries no extension. */
const MANIFEST_TYPES = [
  "application/vnd.apple.mpegurl",
  "application/x-mpegurl",
  "audio/mpegurl",
  "audio/x-mpegurl",
  "application/dash+xml",
];

/**
 * Individual media segments. Excluded deliberately: one playing video fetches
 * hundreds of these, and none of them are independently downloadable — only the
 * manifest that indexes them is.
 */
const SEGMENT_RE = /\.(ts|m4s|aac|m4a|mp4|cmf[vat])(\?|$)/i;

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

// ── 3b. Sniff HLS/DASH manifests ──────────────────────────────────────────────
//
// Streaming video is never a plain download: the page fetches a manifest that
// indexes hundreds of short segments, so there is no single URL for the browser
// download API to intercept. Watching request traffic for that manifest is what
// makes "download this video" possible at all.
//
// State lives in chrome.storage.session rather than a module variable because an
// MV3 service worker is evicted after ~30s idle — a Map would silently lose every
// stream found before the user got around to clicking.

/** Key for a tab's discovered streams. */
const streamKey = (tabId) => `streams_${tabId}`;

/** Cap per tab: a long session on a video site would otherwise grow forever. */
const MAX_STREAMS_PER_TAB = 20;

chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0) return; // Not attached to a tab (e.g. a worker fetch)

    const type = manifestType(details);
    if (!type) return;

    recordStream(details.tabId, {
      url:     details.url,
      type,                       // "hls" | "dash"
      headers: capturedHeaders.get(details.url) || {},
      foundAt: Date.now(),
    });
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"]
);

/**
 * Classify a response as an HLS or DASH manifest, or null if it is neither.
 *
 * Content-Type is checked before the URL because a signed or query-routed
 * manifest often has no recognisable extension, and because some CDNs serve
 * `.m3u8` paths that are really redirects.
 */
function manifestType(details) {
  const header = (details.responseHeaders || []).find(
    (h) => h.name.toLowerCase() === "content-type"
  );
  const mime = (header?.value || "").split(";")[0].trim().toLowerCase();

  if (MANIFEST_TYPES.includes(mime)) {
    return mime === "application/dash+xml" ? "dash" : "hls";
  }

  // Segments share their manifest's media types on some servers, so the segment
  // check has to run before falling back to the URL.
  if (SEGMENT_RE.test(details.url) && !MANIFEST_RE.test(details.url)) return null;

  const m = details.url.match(MANIFEST_RE);
  if (m) return m[1].toLowerCase() === "mpd" ? "dash" : "hls";

  return null;
}

async function recordStream(tabId, stream) {
  const key   = streamKey(tabId);
  const store = await chrome.storage.session.get(key);
  const found = store[key] || [];

  // A player re-requests its manifest constantly (live edge, quality switches).
  if (found.some((s) => s.url === stream.url)) return;

  found.push(stream);
  const trimmed = found.slice(-MAX_STREAMS_PER_TAB);
  await chrome.storage.session.set({ [key]: trimmed });

  chrome.action.setBadgeText({ tabId, text: String(trimmed.length) });
  chrome.action.setBadgeBackgroundColor({ color: "#2563eb" });

  // Tell the page to offer the panel. The tab may have no content script yet
  // (or be a restricted page), so a failure here is expected and ignored.
  chrome.tabs.sendMessage(tabId, {
    type:  "fluxdm_stream_found",
    count: trimmed.length,
  }).catch(() => {});
}

// Discovered streams belong to the page that was loaded, so a navigation clears
// them — otherwise the panel would offer a video the user has already left.
chrome.webNavigation?.onCommitted.addListener((details) => {
  if (details.frameId !== 0) return; // Ignore iframe navigation
  chrome.storage.session.remove(streamKey(details.tabId));
  chrome.action.setBadgeText({ tabId: details.tabId, text: "" });
});

chrome.tabs.onRemoved.addListener((tabId) => {
  chrome.storage.session.remove(streamKey(tabId));
});

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

  // ── Streams found in a tab ────────────────────────────────────────────────
  // The content script asks about its own tab; the popup has to name one.
  if (message.type === "get_streams") {
    const tabId = message.tabId ?? _sender.tab?.id;
    if (tabId == null) {
      sendResponse({ streams: [] });
      return false;
    }
    chrome.storage.session.get(streamKey(tabId), (store) => {
      sendResponse({ streams: store[streamKey(tabId)] || [] });
    });
    return true;
  }

  // ── Send a discovered stream to FluxDM ────────────────────────────────────
  if (message.type === "download_stream") {
    const { url, type, pageUrl, title } = message;

    (async () => {
      const stored = await chrome.storage.session.get(streamKey(_sender.tab?.id));
      const record = (stored[streamKey(_sender.tab?.id)] || []).find((s) => s.url === url);

      await sendToFluxDM({
        url,
        // The manifest's own name is meaningless ("index.m3u8"), so the page
        // title is what makes the finished file identifiable.
        filename:    streamFilename(title, type),
        headers:     record?.headers || capturedHeaders.get(url) || {},
        cookies:     await getCookieHeader(url),
        referrer:    pageUrl || "",
        pageUrl:     pageUrl || "",
        contentType: type === "dash" ? "application/dash+xml" : "application/x-mpegurl",
      });
    })()
      .then(()    => sendResponse({ ok: true }))
      .catch((e)  => sendResponse({ ok: false, error: String(e) }));

    return true;
  }
});

/** Build a filesystem-safe name from the page title. */
function streamFilename(title, type) {
  const base = (title || "video")
    .replace(/[<>:"/\\|?*\x00-\x1f]/g, "")  // Illegal on Windows and awkward elsewhere
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 120) || "video";

  // HLS segments are MPEG-TS and DASH segments are fragmented MP4. Naming them
  // accordingly keeps players from mis-detecting the container.
  return `${base}.${type === "dash" ? "mp4" : "ts"}`;
}

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
