//! Модель элемента истории буфера обмена.

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

/// Тип содержимого. В БД хранится как строка в колонке `type`.
pub const KIND_TEXT: &str = "text";
pub const KIND_IMAGE: &str = "image";
pub const KIND_FILES: &str = "files";

/// Один элемент истории (то, что уходит во фронтенд).
#[derive(Debug, Clone, Serialize)]
pub struct ClipItem {
    pub id: i64,
    /// "text" | "image"
    #[serde(rename = "type")]
    pub kind: String,
    /// Полный текст (для image = None).
    pub content: Option<String>,
    /// Имя файла картинки в images/ (для text = None).
    pub image_path: Option<String>,
    /// Короткий предпросмотр для списка.
    pub preview: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub use_count: i64,
    pub is_pinned: bool,
    /// Приложение-источник (имя exe, напр. "chrome.exe"). Может быть None.
    pub source_app: Option<String>,
    /// Цветовая метка/категория (Pro). Может быть None.
    pub category: Option<String>,
    /// Теги через запятую (Pro). Может быть None.
    pub tags: Option<String>,
}

/// Данные для вставки новой записи (без id — его назначит БД).
#[derive(Debug, Clone)]
pub struct NewItem {
    pub kind: String,
    pub content: Option<String>,
    pub image_path: Option<String>,
    pub preview: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub content_hash: String,
    /// Приложение-источник в момент захвата.
    pub source_app: Option<String>,
}

/// Текущее время в миллисекундах Unix.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
