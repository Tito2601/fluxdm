//! System tray integration for FluxDM.
//!
//! • Left-click the tray icon → show / focus the main window.
//! • Context menu → Show, ─, Quit.
//! • Closing the window hides it to the tray instead of quitting.

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tracing::{info, warn};

/// Create the tray icon + context menu and wire up the window-close handler.
/// Call once from `tauri::Builder::setup`.
pub fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    // ── Build the context menu ────────────────────────────────────────────────
    let show = MenuItem::with_id(app, "show", "Show FluxDM", true, None::<&str>)?;
    let sep  = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit FluxDM", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show, &sep, &quit])?;

    // ── Create the tray icon ──────────────────────────────────────────────────
    let icon = app.default_window_icon()
        .cloned()
        .expect("No default window icon set — check tauri.conf.json bundle.icon");

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("FluxDM — Download Manager")
        // Left-click → show window
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        // Menu selection handler
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                info!("Quit selected from tray");
                app.exit(0);
            }
            other => warn!("Unknown tray menu event: {}", other),
        })
        .build(app)?;

    info!("System tray initialized");

    // ── Intercept window close → hide to tray ─────────────────────────────────
    if let Some(window) = app.get_webview_window("main") {
        let win_clone = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(e) = win_clone.hide() {
                    warn!("Failed to hide window: {}", e);
                }
            }
        });
        info!("Window close-to-tray handler registered");
    } else {
        warn!("Could not find main window — close-to-tray will not work");
    }

    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
