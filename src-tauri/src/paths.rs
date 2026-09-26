//! Расположение пользовательских данных.
//!
//! Обычная (установленная) версия: %APPDATA%\ClipVault\ — данные никогда не
//! хранятся в каталоге установки (ТЗ §31).
//! Portable-версия: рядом с .exe лежит файл-маркер `portable` → все данные,
//! включая кэш WebView2, живут в `<папка exe>\data\` и ничего не пишется в профиль.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Имена файла-маркера portable-режима (с расширением — на случай, если
/// пользователь создаст его через «Создать → Текстовый документ»).
const PORTABLE_MARKERS: [&str; 2] = ["portable", "portable.txt"];

/// Каталог, где лежит сам clipvault.exe.
fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Запущена ли portable-версия (есть маркер рядом с exe). Вычисляется один раз.
pub fn is_portable() -> bool {
    static PORTABLE: OnceLock<bool> = OnceLock::new();
    *PORTABLE.get_or_init(|| {
        exe_dir().is_some_and(|d| PORTABLE_MARKERS.iter().any(|m| d.join(m).is_file()))
    })
}

/// Корень данных.
/// Установленная версия: C:\Users\<USER>\AppData\Roaming\ClipVault.
/// Специально НЕ Local — туда установщик (currentUser) ставит саму программу,
/// и совпадение путей грозило бы сносом истории при обновлении/удалении.
/// Portable: <папка exe>\data.
pub fn data_dir() -> PathBuf {
    if is_portable() {
        if let Some(dir) = exe_dir() {
            return dir.join("data");
        }
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("ClipVault")
}

pub fn db_path() -> PathBuf {
    data_dir().join("clipvault.db")
}

pub fn images_dir() -> PathBuf {
    data_dir().join("images")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

/// Профиль WebView2 (кэш окон). Нужен только в portable-режиме: в обычном
/// Tauri кладёт его в %LOCALAPPDATA%\com.clipvault.desktop.
pub fn webview_dir() -> PathBuf {
    data_dir().join("webview")
}

/// Файл пользовательских настроек (JSON).
pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// Файл с активированной офлайн-лицензией (подписанный ключ, текст).
pub fn license_path() -> PathBuf {
    data_dir().join("license.key")
}

/// Создаёт все каталоги данных, если их ещё нет, и проверяет, что в них можно
/// писать (portable-копию могут положить в Program Files или на read-only диск).
/// Вызывается один раз при старте.
pub fn ensure_dirs() -> std::io::Result<()> {
    fs::create_dir_all(data_dir())?;
    fs::create_dir_all(images_dir())?;
    fs::create_dir_all(logs_dir())?;
    let probe = data_dir().join(".write-test");
    fs::write(&probe, b"")?;
    let _ = fs::remove_file(probe);
    Ok(())
}
