//! Нативный слушатель буфера обмена (event-driven, без polling).
//!
//! На каждое изменение: проверяет паузу и защиту от цикла, читает содержимое,
//! сохраняет в БД (текст) или PNG-файл + запись (картинка), шлёт событие во фронтенд.

use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::thread;

use clipboard_master::{CallbackResult, ClipboardHandler, Master};
use image::{ImageFormat, RgbaImage};
use tauri::{AppHandle, Emitter};

use super::reader::{read_clipboard, Captured};
use crate::db::repo;
use crate::models::{NewItem, KIND_FILES, KIND_IMAGE, KIND_TEXT};
use crate::state::Shared;

/// Лимит истории: Free — 200 записей, Pro — до 10 000 (DECISIONS §2).
/// Закреплённые в лимит не входят и автоочисткой не трогаются.
const FREE_MAX_ITEMS: i64 = 200;
const PRO_MAX_ITEMS: i64 = 10_000;

struct Listener {
    shared: Shared,
    app: AppHandle,
}

impl Listener {
    fn handle(&mut self) {
        // Пауза записи (ТЗ §20).
        if self.shared.paused.load(Ordering::Relaxed) {
            return;
        }

        // Приложение-источник (для фильтра по источнику и игнор-списка, 2.2/2.3).
        let source = crate::source::foreground_process();
        if let Some(app) = &source {
            let ignored = {
                let s = self.shared.settings.lock().unwrap();
                s.ignore_apps.iter().any(|a| a == app)
            };
            if ignored {
                return; // не сохраняем из приложения-исключения (пароли/банки)
            }
        }

        let captured = read_clipboard();
        let hash = match &captured {
            Captured::Text { hash, .. } => hash.clone(),
            Captured::Image { hash, .. } => hash.clone(),
            Captured::Files { hash, .. } => hash.clone(),
            Captured::None => return,
        };

        // Защита от цикла: пропускаем изменение, которое ClipVault сам только что записал.
        {
            let mut guard = self.shared.last_written_hash.lock().unwrap();
            if guard.as_deref() == Some(hash.as_str()) {
                *guard = None;
                return;
            }
        }

        let new_item = match self.build_item(captured, &hash, source) {
            Some(it) => it,
            None => return,
        };

        let max_items = if self.shared.is_pro() {
            PRO_MAX_ITEMS
        } else {
            FREE_MAX_ITEMS
        };

        let conn = self.shared.db.lock().unwrap();
        match repo::insert_or_bump(&conn, &new_item) {
            Ok((_id, _inserted)) => {
                if let Ok(removed) = repo::cleanup(&conn, max_items) {
                    drop(conn);
                    self.remove_image_files(&removed);
                }
                let _ = self.app.emit("history-updated", ());
            }
            Err(e) => tracing::error!("insert failed: {e}"),
        }
    }

    /// Готовит запись для БД (и сохраняет PNG на диск для картинок).
    fn build_item(&self, captured: Captured, hash: &str, source: Option<String>) -> Option<NewItem> {
        match captured {
            Captured::Text { content, .. } => {
                let preview = make_preview(&content);
                let size = content.len() as i64;
                Some(NewItem {
                    kind: KIND_TEXT.into(),
                    content: Some(content),
                    image_path: None,
                    preview: Some(preview),
                    mime_type: Some("text/plain".into()),
                    size: Some(size),
                    content_hash: hash.to_string(),
                    source_app: source,
                })
            }
            Captured::Image {
                rgba,
                width,
                height,
                ..
            } => {
                let file_name = format!("{}.png", &hash.trim_start_matches("i:"));
                let path = self.shared.images_dir.join(&file_name);

                let mut png: Vec<u8> = Vec::new();
                let img = RgbaImage::from_raw(width as u32, height as u32, rgba)?;
                if image::DynamicImage::ImageRgba8(img)
                    .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
                    .is_err()
                {
                    tracing::error!("png encode failed");
                    return None;
                }
                if let Err(e) = std::fs::write(&path, &png) {
                    tracing::error!("png write failed: {e}");
                    return None;
                }
                Some(NewItem {
                    kind: KIND_IMAGE.into(),
                    content: None,
                    image_path: Some(file_name),
                    preview: Some(format!("{width} × {height}")),
                    mime_type: Some("image/png".into()),
                    size: Some(png.len() as i64),
                    content_hash: hash.to_string(),
                    source_app: source,
                })
            }
            Captured::Files { paths, .. } => {
                let names: Vec<String> = paths
                    .iter()
                    .map(|p| {
                        p.rsplit(['\\', '/'])
                            .next()
                            .filter(|s| !s.is_empty())
                            .unwrap_or(p)
                            .to_string()
                    })
                    .collect();
                let preview = if names.len() == 1 {
                    names[0].clone()
                } else {
                    format!("{} ({} файлов)", names.join(", "), names.len())
                };
                Some(NewItem {
                    kind: KIND_FILES.into(),
                    // Полные пути в content — по ним же работает поиск (FTS).
                    content: Some(paths.join("\n")),
                    image_path: None,
                    preview: Some(preview.chars().take(200).collect()),
                    mime_type: Some("text/uri-list".into()),
                    size: Some(paths.len() as i64),
                    content_hash: hash.to_string(),
                    source_app: source,
                })
            }
            Captured::None => None,
        }
    }

    fn remove_image_files(&self, files: &[String]) {
        for f in files {
            let _ = std::fs::remove_file(self.shared.images_dir.join(f));
        }
    }
}

impl ClipboardHandler for Listener {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        self.handle();
        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, error: std::io::Error) -> CallbackResult {
        tracing::warn!("clipboard listener error: {error}");
        CallbackResult::Next
    }
}

/// Однострочный предпросмотр, не длиннее 200 символов.
fn make_preview(s: &str) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(200).collect()
}

/// Запускает поток слушателя буфера обмена. Живёт всё время работы приложения.
pub fn spawn(shared: Shared, app: AppHandle) {
    thread::spawn(move || {
        let handler = Listener { shared, app };
        match Master::new(handler) {
            Ok(mut master) => {
                if let Err(e) = master.run() {
                    tracing::error!("clipboard master stopped: {e}");
                }
            }
            Err(e) => tracing::error!("clipboard master init failed: {e}"),
        }
    });
}
