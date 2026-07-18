/**
 * FluxDM Content Script
 * Monitors clipboard for download URLs and offers discovered video streams.
 */

// ── Clipboard monitoring ──────────────────────────────────────────────────────
const DOWNLOAD_EXTS = /\.(zip|exe|dmg|mp4|mkv|pdf|iso|7z|rar|tar\.gz|msi|deb|rpm|apk|pkg)(\?.*)?$/i;

document.addEventListener("focus", checkClipboard);
document.addEventListener("click", checkClipboard);

async function checkClipboard() {
  try {
    const text = await navigator.clipboard.readText();
    if (text && DOWNLOAD_EXTS.test(text)) {
      chrome.runtime.sendMessage({ type: "clipboard_url", url: text });
    }
  } catch {
    // User hasn't granted clipboard access — silent fail
  }
}

// ── Floating "Download this video" panel ──────────────────────────────────────
//
// Rendered into a shadow root so the host page's CSS cannot restyle or hide it,
// and so our own styles cannot leak out and disturb the page. The host element
// is positioned fixed at a very high z-index because video sites routinely stack
// their own overlays well above normal content.

const PANEL_ID = "fluxdm-stream-panel";
let panelHost = null;
let dismissed = false;

chrome.runtime.onMessage.addListener((message) => {
  if (message.type === "fluxdm_stream_found" && !dismissed) {
    showPanel();
  }
});

async function showPanel() {
  const { streams } = await chrome.runtime.sendMessage({ type: "get_streams" });
  if (!streams?.length) return;

  // Rebuilt rather than patched: a second stream appearing should re-render the
  // list, and the panel is small enough that diffing would be wasted effort.
  removePanel();

  panelHost = document.createElement("div");
  panelHost.id = PANEL_ID;
  Object.assign(panelHost.style, {
    position: "fixed",
    top:      "16px",
    right:    "16px",
    zIndex:   "2147483647", // Max — video sites stack overlays aggressively
  });

  const root = panelHost.attachShadow({ mode: "closed" });
  root.appendChild(buildPanel(streams));

  // documentElement, not body: some players replace body content wholesale.
  document.documentElement.appendChild(panelHost);
}

function buildPanel(streams) {
  const wrap = document.createElement("div");

  const style = document.createElement("style");
  style.textContent = `
    .card {
      font: 13px/1.4 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
      background: #0f172a;
      color: #e2e8f0;
      border: 1px solid #1e293b;
      border-radius: 10px;
      box-shadow: 0 8px 24px rgba(0,0,0,.45);
      width: 300px;
      overflow: hidden;
    }
    .head {
      display: flex; align-items: center; gap: 8px;
      padding: 10px 12px;
      background: #1e293b;
      font-weight: 600;
    }
    .head .dot {
      width: 8px; height: 8px; border-radius: 50%;
      background: #3b82f6; flex: none;
    }
    .head .spacer { flex: 1; }
    .x {
      background: none; border: none; color: #94a3b8;
      cursor: pointer; font-size: 16px; line-height: 1; padding: 0 2px;
    }
    .x:hover { color: #e2e8f0; }
    .list { max-height: 260px; overflow-y: auto; }
    .row {
      display: flex; align-items: center; gap: 10px;
      padding: 10px 12px;
      border-top: 1px solid #1e293b;
    }
    .meta { flex: 1; min-width: 0; }
    .kind {
      display: inline-block;
      font-size: 10px; font-weight: 700; letter-spacing: .04em;
      text-transform: uppercase;
      color: #93c5fd;
    }
    .url {
      display: block;
      font-size: 11px; color: #64748b;
      white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }
    .get {
      background: #2563eb; color: #fff;
      border: none; border-radius: 6px;
      padding: 6px 12px; font-size: 12px; font-weight: 600;
      cursor: pointer; flex: none;
    }
    .get:hover:not(:disabled) { background: #1d4ed8; }
    .get:disabled { opacity: .6; cursor: default; }
  `;
  wrap.appendChild(style);

  const card = document.createElement("div");
  card.className = "card";

  const head = document.createElement("div");
  head.className = "head";

  const dot = document.createElement("span");
  dot.className = "dot";

  const label = document.createElement("span");
  label.textContent = streams.length === 1
    ? "Video found on this page"
    : `${streams.length} videos found`;

  const spacer = document.createElement("span");
  spacer.className = "spacer";

  const close = document.createElement("button");
  close.className = "x";
  close.textContent = "×";
  close.title = "Dismiss";
  close.addEventListener("click", () => {
    // Sticky for this page view: re-showing on every manifest re-request would
    // make the panel impossible to get rid of on a live stream.
    dismissed = true;
    removePanel();
  });

  head.append(dot, label, spacer, close);
  card.appendChild(head);

  const list = document.createElement("div");
  list.className = "list";
  for (const s of streams) list.appendChild(buildRow(s));
  card.appendChild(list);

  wrap.appendChild(card);
  return wrap;
}

function buildRow(stream) {
  const row = document.createElement("div");
  row.className = "row";

  const meta = document.createElement("div");
  meta.className = "meta";

  const kind = document.createElement("span");
  kind.className = "kind";
  kind.textContent = stream.type;

  const url = document.createElement("span");
  url.className = "url";
  // textContent, never innerHTML: this string comes off the network.
  url.textContent = shortUrl(stream.url);
  url.title = stream.url;

  meta.append(kind, url);

  const btn = document.createElement("button");
  btn.className = "get";
  btn.textContent = "Download";
  btn.addEventListener("click", async () => {
    btn.disabled = true;
    btn.textContent = "Sending…";
    try {
      const res = await chrome.runtime.sendMessage({
        type:    "download_stream",
        url:     stream.url,
        kind:    stream.type,
        pageUrl: location.href,
        title:   document.title,
      });
      btn.textContent = res?.ok ? "Sent ✓" : "Failed";
    } catch {
      btn.textContent = "Failed";
    }
  });

  row.append(meta, btn);
  return row;
}

/** Filename-ish tail of a URL, for identifying one stream among several. */
function shortUrl(raw) {
  try {
    const u = new URL(raw);
    const last = u.pathname.split("/").filter(Boolean).pop() || u.pathname;
    return `${u.hostname}/…/${last}`;
  } catch {
    return raw.slice(0, 60);
  }
}

function removePanel() {
  panelHost?.remove();
  panelHost = null;
}
