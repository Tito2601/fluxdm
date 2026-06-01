import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { useDownloadStore } from "../store/downloadStore";
import { DownloadRequest, ProgressEvent, CompleteEvent, ErrorEvent } from "../types";

/** Request notification permission once, return whether it was granted. */
async function ensureNotificationPermission(): Promise<boolean> {
  let granted = await isPermissionGranted();
  if (!granted) {
    const result = await requestPermission();
    granted = result === "granted";
  }
  return granted;
}

/**
 * Sets up Tauri event listeners for download progress, completion, and errors.
 * Call this once at the App root level.
 */
export function useTauriEvents() {
  const {
    updateProgress,
    addDownloadFromEvent,
    markCompleted,
    markFailed,
    setPendingDownload,
  } = useDownloadStore();

  useEffect(() => {
    const unlisten: Array<() => void> = [];

    // Browser extension sent a URL → show the save dialog instead of auto-downloading.
    listen<DownloadRequest>("download_requested", (event) => {
      setPendingDownload(event.payload);
    }).then((fn) => unlisten.push(fn));

    // cmd_add_stream_download emits this so the download row appears immediately.
    listen<Record<string, unknown>>("download_added", (event) => {
      addDownloadFromEvent(event.payload);
    }).then((fn) => unlisten.push(fn));

    // Live progress ticks — camelCase fields thanks to #[serde(rename_all = "camelCase")]
    listen<ProgressEvent>("download_progress", (event) => {
      updateProgress(event.payload);
    }).then((fn) => unlisten.push(fn));

    // Listen for download completion — update store + fire desktop notification
    listen<CompleteEvent>("download_complete", async (event) => {
      markCompleted(event.payload.id);

      // Find the filename from the current store snapshot
      const dl = useDownloadStore
        .getState()
        .downloads.find((d) => d.id === event.payload.id);
      const filename = dl?.filename ?? "File";

      const granted = await ensureNotificationPermission();
      if (granted) {
        sendNotification({
          title: "Download Complete ✅",
          body: `${filename} saved successfully`,
        });
      }
    }).then((fn) => unlisten.push(fn));

    // Listen for download errors — update store + fire notification
    listen<ErrorEvent>("download_error", async (event) => {
      markFailed(event.payload.id, event.payload.error);

      const dl = useDownloadStore
        .getState()
        .downloads.find((d) => d.id === event.payload.id);
      const filename = dl?.filename ?? "File";

      const granted = await ensureNotificationPermission();
      if (granted) {
        sendNotification({
          title: "Download Failed ❌",
          body: `${filename}: ${event.payload.error}`,
        });
      }
    }).then((fn) => unlisten.push(fn));

    // Cleanup listeners on unmount
    return () => {
      unlisten.forEach((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [updateProgress, addDownloadFromEvent, markCompleted, markFailed, setPendingDownload]);
}
