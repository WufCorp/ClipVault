//! Глобальная горячая клавиша открытия истории. Комбинация настраивается
//! пользователем (Free, 2.5) и хранится в настройках (`Settings::open_hotkey`).

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::Shared;

/// Комбинация открытия истории по умолчанию (ТЗ §9).
pub const DEFAULT_OPEN: &str = "CommandOrControl+Shift+V";

/// Плагин глобальных горячих клавиш с обработчиком.
///
/// Alt+1..Alt+9 (только Pro) — быстрая вставка N-го элемента без открытия окна (5.4).
/// Любая другая зарегистрированная комбинация = открыть/спрятать историю.
pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if app.state::<Shared>().is_pro() {
                for n in 1u8..=9 {
                    if let Ok(sc) = Shortcut::try_from(format!("Alt+{n}").as_str()) {
                        if &sc == shortcut {
                            crate::commands::quick_paste_nth(app, (n - 1) as usize);
                            return;
                        }
                    }
                }
            }
            crate::window::toggle_history(app);
        })
        .build()
}

/// Регистрирует Alt+1..Alt+9 (быстрая вставка, Pro).
pub fn register_quickpaste(app: &AppHandle) {
    let gs = app.global_shortcut();
    for n in 1..=9 {
        let _ = gs.register(format!("Alt+{n}").as_str());
    }
}

/// Снимает Alt+1..Alt+9 (при деактивации Pro).
pub fn unregister_quickpaste(app: &AppHandle) {
    let gs = app.global_shortcut();
    for n in 1..=9 {
        let _ = gs.unregister(format!("Alt+{n}").as_str());
    }
}

/// Регистрирует горячую клавишу из настроек. Вызывается один раз при старте.
pub fn register(app: &AppHandle) {
    let hk = {
        let state = app.state::<Shared>();
        let s = state.settings.lock().unwrap();
        s.open_hotkey.clone()
    };
    if let Err(e) = app.global_shortcut().register(hk.as_str()) {
        tracing::error!("failed to register global shortcut {hk}: {e}");
        // Фолбэк на дефолт, если сохранённая комбинация невалидна.
        if hk != DEFAULT_OPEN {
            let _ = app.global_shortcut().register(DEFAULT_OPEN);
        }
    }
}

/// Меняет и сохраняет хоткей вызова. Новый регистрируется до снятия старого,
/// чтобы при невалидной комбинации приложение не осталось без хоткея.
pub fn set_open_hotkey(app: &AppHandle, new_hotkey: &str) -> Result<(), String> {
    let new_hotkey = new_hotkey.trim();
    if new_hotkey.is_empty() {
        return Err("пустая комбинация".into());
    }
    let gs = app.global_shortcut();
    let state = app.state::<Shared>();

    let old = state.settings.lock().unwrap().open_hotkey.clone();
    if old == new_hotkey {
        return Ok(());
    }

    gs.register(new_hotkey)
        .map_err(|e| format!("не удалось назначить комбинацию: {e}"))?;
    let _ = gs.unregister(old.as_str());

    let mut s = state.settings.lock().unwrap();
    s.open_hotkey = new_hotkey.to_string();
    s.save()
}
