//! Значок в системном трее и его меню (ТЗ §19–20).

use std::sync::atomic::Ordering;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::db::repo;
use crate::state::Shared;

/// Строит значок трея с меню: Открыть / Пауза / Очистить / Выход.
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Открыть историю", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "⚙ Настройки", true, None::<&str>)?;
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Запускать вместе с Windows",
        true,
        autostart_on,
        None::<&str>,
    )?;
    let pause = MenuItem::with_id(app, "pause", "⏸ Приостановить запись", true, None::<&str>)?;
    let clear = MenuItem::with_id(app, "clear", "Очистить историю", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &open, &settings, &sep1, &autostart, &pause, &clear, &sep2, &quit, &sep3,
        ],
    )?;
    let pause_item = pause.clone();
    let autostart_item = autostart.clone();

    let _tray = TrayIconBuilder::with_id("clipvault")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("ClipVault")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => crate::window::show_history(app),
            "settings" => crate::window::show_settings(app),
            "autostart" => toggle_autostart(app, &autostart_item),
            "pause" => toggle_pause(app, &pause_item),
            "clear" => clear_history(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::window::toggle_history(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn toggle_pause(app: &AppHandle, item: &MenuItem<tauri::Wry>) {
    let state = app.state::<Shared>();
    let now_paused = !state.paused.load(Ordering::Relaxed);
    state.paused.store(now_paused, Ordering::Relaxed);
    let label = if now_paused {
        "▶ Возобновить запись"
    } else {
        "⏸ Приостановить запись"
    };
    let _ = item.set_text(label);
    let _ = app.emit("pause-changed", now_paused);
}

fn toggle_autostart(app: &AppHandle, item: &CheckMenuItem<Wry>) {
    let al = app.autolaunch();
    let enable = !al.is_enabled().unwrap_or(false);
    let res = if enable { al.enable() } else { al.disable() };
    if let Err(e) = res {
        tracing::error!("autostart toggle failed: {e}");
    }
    let _ = item.set_checked(al.is_enabled().unwrap_or(enable));
}

fn clear_history(app: &AppHandle) {
    let state = app.state::<Shared>();
    let removed = {
        let conn = state.db.lock().unwrap();
        repo::clear(&conn, true).unwrap_or_default()
    };
    for f in removed {
        let _ = std::fs::remove_file(state.images_dir.join(f));
    }
    let _ = app.emit("history-updated", ());
}
