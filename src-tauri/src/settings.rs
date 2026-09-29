//! Пользовательские настройки приложения (persisted JSON в %APPDATA%\ClipVault\settings.json).
//!
//! Все поля имеют serde-`default`, поэтому добавление новых полей не ломает
//! чтение старых файлов настроек.

use serde::{Deserialize, Serialize};

use crate::paths;

fn default_true() -> bool {
    true
}
fn default_hotkey() -> String {
    "CommandOrControl+Shift+V".to_string()
}
fn default_font() -> u32 {
    14
}

/// Геометрия окна истории (для «запоминания позиции/размера», Pro).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WinGeom {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Проверять обновления автоматически при старте (вкл по умолчанию).
    /// Выключение = «режим паранойи»: ноль сетевых запросов (DECISIONS §6).
    #[serde(default = "default_true")]
    pub auto_update: bool,

    /// Хоткей вызова окна истории (Free-настройка, 2.5).
    #[serde(default = "default_hotkey")]
    pub open_hotkey: String,

    /// Приложения-исключения: не сохранять буфер, если источник — один из этих
    /// процессов (имя exe в нижнем регистре, напр. "keepass.exe"). Free/безопасность (2.3).
    #[serde(default)]
    pub ignore_apps: Vec<String>,

    /// Запоминать позицию/размер окна истории (Pro, 3.4).
    #[serde(default)]
    pub window_memory: bool,
    #[serde(default)]
    pub win_geometry: Option<WinGeom>,

    /// Размер шрифта списка, px (Pro, 3.4).
    #[serde(default = "default_font")]
    pub font_size: u32,

    /// Компактный режим окна (Pro, 5.5).
    #[serde(default)]
    pub compact_mode: bool,

    /// Автоочистка по возрасту: удалять незакреплённые старше N дней (Pro, 4.1). 0 = выкл.
    #[serde(default)]
    pub max_age_days: u32,

    /// Хэш мастер-пароля (Pro, 5.2). None = защита выключена.
    #[serde(default)]
    pub master_hash: Option<String>,
    /// Автоблокировка через N минут бездействия (Pro, 5.2). 0 = выкл.
    #[serde(default)]
    pub auto_lock_min: u32,

    /// DPAPI-шифрование содержимого при хранении (Free/безопасность, 5.1).
    #[serde(default)]
    pub encrypt: bool,

    /// Автовставка: после выбора элемента нажать Ctrl+V в окне, где был
    /// пользователь (вкл по умолчанию). Выкл = только копировать в буфер.
    #[serde(default = "default_true")]
    pub auto_paste: bool,

    /// «Не прятать окно»: окно истории остаётся на экране после вставки и при
    /// переходе в другое приложение; закрывается Esc/крестиком/хоткеем (выкл по умолчанию).
    #[serde(default)]
    pub keep_open: bool,

    /// Скрытые вкладки окна истории (ключи data-filter: "colors", "urls"…).
    #[serde(default)]
    pub hidden_tabs: Vec<String>,

    /// Слоты (Pro, 5.3): до 10 закреплённых ссылок на элементы (id или None).
    #[serde(default)]
    pub slots: Vec<Option<i64>>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            auto_update: true,
            open_hotkey: default_hotkey(),
            ignore_apps: Vec::new(),
            window_memory: false,
            win_geometry: None,
            font_size: default_font(),
            compact_mode: false,
            max_age_days: 0,
            master_hash: None,
            auto_lock_min: 0,
            encrypt: false,
            auto_paste: true,
            keep_open: false,
            hidden_tabs: Vec::new(),
            slots: Vec::new(),
        }
    }
}

impl Settings {
    /// Читает настройки с диска; при отсутствии/ошибке — значения по умолчанию.
    pub fn load() -> Self {
        match std::fs::read_to_string(paths::settings_path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Settings::default(),
        }
    }

    /// Атомарно (best-effort) сохраняет настройки на диск.
    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(paths::settings_path(), json).map_err(|e| e.to_string())
    }
}
