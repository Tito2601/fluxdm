/**
 * FluxDM Extension Popup Script
 *
 * • Shows whether FluxDM is running (status dot)
 * • Pre-fills URL from clipboard if a download URL was detected
 * • Sends download to background → native host / HTTP fallback
 */

const urlInput      = document.getElementById("urlInput");
const filenameInput = document.getElementById("filenameInput");
const downloadBtn   = document.getElementById("downloadBtn");
const statusEl      = document.getElementById("status");
const statusDot     = document.getElementById("statusDot");
const statusText    = document.getElementById("statusText");

// ── On load ───────────────────────────────────────────────────────────────────

(async () => {
  // Check whether FluxDM is running
  chrome.runtime.sendMessage({ type: "check_status" }, ({ running } = {}) => {
    if (statusDot && statusText) {
      if (running) {
        statusDot.className  = "dot dot-green";
        statusText.textContent = "FluxDM is running";
      } else {
        statusDot.className  = "dot dot-red";
        statusText.textContent = "FluxDM is not running";
        downloadBtn.disabled = true;
      }
    }
  });

  // Pre-fill from clipboard URL detected by content script
  chrome.runtime.sendMessage({ type: "get_clipboard_url" }, ({ url } = {}) => {
    if (url) {
      urlInput.value = url;
      filenameInput.placeholder = urlFilename(url);
      chrome.action.setBadgeText({ text: "" });
    }
  });

  // Also check current tab URL and pre-fill if it looks like a direct file link
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (tab?.url && !urlInput.value) {
    const tabUrl = tab.url;
    const CAPTURE_RE = /\.(zip|exe|dmg|mp4|mkv|avi|mov|pdf|iso|7z|rar|tar\.gz|msi|deb|rpm|apk|pkg|epub|mobi)(\?.*)?$/i;
    if (CAPTURE_RE.test(tabUrl)) {
      urlInput.value = tabUrl;
      filenameInput.placeholder = urlFilename(tabUrl);
    }
  }
})();

// ── Download button ───────────────────────────────────────────────────────────

downloadBtn.addEventListener("click", async () => {
  const url      = urlInput.value.trim();
  const filename = filenameInput.value.trim() || urlFilename(url);

  if (!url) {
    showStatus("Please enter a download URL", "error");
    return;
  }

  downloadBtn.disabled    = true;
  downloadBtn.textContent = "Sending…";

  try {
    const cookies = await new Promise((resolve) => {
      chrome.cookies.getAll({ url }, (list) => {
        resolve((list || []).map((c) => `${c.name}=${c.value}`).join("; "));
      });
    });

    const payload = {
      url,
      filename,
      headers:     {},
      cookies,
      referrer:    "",
      pageUrl:     "",
      contentType: "",
      fileSize:    -1,
    };

    chrome.runtime.sendMessage({ type: "add_download", payload }, (response) => {
      if (response?.ok) {
        showStatus("✓ Added to FluxDM queue!", "success");
        urlInput.value      = "";
        filenameInput.value = "";
      } else {
        const err = response?.error || "Unknown error";
        showStatus(
          err.includes("not running") || err.includes("ECONNREFUSED")
            ? "FluxDM is not running. Open the app first."
            : `Error: ${err}`,
          "error"
        );
      }
    });
  } catch (err) {
    showStatus(`Error: ${err.message}`, "error");
  } finally {
    downloadBtn.disabled    = false;
    downloadBtn.textContent = "⬇ Send to FluxDM";
  }
});

// ── Utilities ─────────────────────────────────────────────────────────────────

function urlFilename(url) {
  try {
    const u     = new URL(url);
    const parts = u.pathname.split("/").filter(Boolean);
    const last  = parts[parts.length - 1] || "";
    return decodeURIComponent(last.split("?")[0]) || "download";
  } catch {
    return url.split("/").pop()?.split("?")[0] || "download";
  }
}

function showStatus(message, type) {
  statusEl.textContent = message;
  statusEl.className   = `status ${type}`;
  setTimeout(() => { statusEl.className = "status"; }, 3500);
}
