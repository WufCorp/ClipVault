//! Поведение окон: история (показать/спрятать/фокус) и экран настроек.

use crate::state::Shared;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

pub const MAIN: &str = "main";
pub const SETTINGS: &str = "settings";

/// В portable-режиме уводит профиль WebView2 в папку данных программы
/// (иначе Tauri создаёт его в %LOCALAPPDATA%\com.clipvault.desktop).
/// Профиль один на все окна — WebView2 требует этого в пределах процесса.
fn portable_profile<'a, R: tauri::Runtime, M: Manager<R>>(
    b: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    if crate::paths::is_portable() {
        b.data_directory(crate::paths::webview_dir())
    } else {
        b
    }
}

/// Создаёт окно истории по описанию из tauri.conf.json (там `create: false`).
pub fn create_main(app: &AppHandle) -> tauri::Result<()> {
    let cfg = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN)
        .cloned()
        .expect("tauri.conf.json: нет окна main");
    portable_profile(WebviewWindowBuilder::from_config(app, &cfg)?).build()?;
    Ok(())
}

/// Показать окно истории поверх всех, сфокусировать и попросить фронтенд
/// обновить список и поставить курсор в поиск.
pub fn show_history(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(MAIN) {
        // Запомненная позиция (Pro) важнее; иначе — у курсора.
        let remembered = {
            let s = app.state::<Shared>();
            let s = s.settings.lock().unwrap();
            s.window_memory && s.win_geometry.is_some()
        };
        if !remembered {
            place_near_cursor(app, &win);
        }
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        let _ = app.emit("focus-search", ());
    }
}

/// Ставит окно левым верхним углом к курсору на том мониторе, где мышь.
/// Если окно не влезает вправо/вниз — сдвигает его внутрь рабочей области
/// (без панели задач).
fn place_near_cursor(app: &AppHandle, win: &WebviewWindow) {
    let Ok(cursor) = app.cursor_position() else { return };
    let Ok(Some(monitor)) = app.monitor_from_point(cursor.x, cursor.y) else { return };
    let (Ok(size), Ok(cur_scale)) = (win.outer_size(), win.scale_factor()) else { return };

    // Размер окна в пикселях целевого монитора (у мониторов бывает разный DPI).
    let k = monitor.scale_factor() / cur_scale;
    let w = (size.width as f64 * k).round() as i32;
    let h = (size.height as f64 * k).round() as i32;

    let area = monitor.work_area();
    let (left, top) = (area.position.x, area.position.y);
    let right = left + area.size.width as i32;
    let bottom = top + area.size.height as i32;

    let x = (cursor.x as i32).min(right - w).max(left);
    let y = (cursor.y as i32).min(bottom - h).max(top);
    let _ = win.set_position(PhysicalPosition::new(x, y));
}

/// Переключить видимость (для клика по иконке трея / повторного нажатия хоткея).
pub fn toggle_history(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(MAIN) {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            show_history(app);
        }
    }
}

/// Показать окно настроек (создаётся лениво при первом обращении).
pub fn show_settings(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(SETTINGS) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    match portable_profile(WebviewWindowBuilder::new(
        app,
        SETTINGS,
        WebviewUrl::App("settings.html".into()),
    ))
    .title("ClipVault — Настройки")
    .inner_size(580.0, 660.0)
    .min_inner_size(460.0, 480.0)
    .resizable(true)
    .center()
    .build()
    {
        Ok(win) => {
            let _ = win.set_focus();
        }
        Err(e) => tracing::error!("failed to open settings window: {e}"),
    }
}
