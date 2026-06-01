/**
 * FluxDM Content Script
 * Monitors clipboard for download URLs and intercepts link clicks.
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
