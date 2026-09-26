//! Чтение текущего содержимого буфера обмена (текст или изображение) + хэш.

use arboard::Clipboard;
use sha2::{Digest, Sha256};

/// Что удалось прочитать из буфера обмена.
pub enum Captured {
    Text {
        content: String,
        hash: String,
    },
    Image {
        rgba: Vec<u8>,
        width: usize,
        height: usize,
        hash: String,
    },
    /// Скопированные из Проводника файлы/папки (CF_HDROP).
    Files {
        paths: Vec<String>,
        hash: String,
    },
    /// Пусто/неподдерживаемо/ошибка — сохранять нечего.
    None,
}

/// sha256(bytes) в hex с префиксом типа ("t:" для текста, "i:" для картинки).
/// Тот же алгоритм используется при записи обратно, чтобы сработала защита от цикла.
pub(crate) fn sha_hex(prefix: &str, bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(prefix.len() + 64);
    s.push_str(prefix);
    for b in digest.iter() {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Читает буфер: текст → файлы → изображение. Пустой текст игнорируется (ТЗ §8).
///
/// arboard и clipboard-win открывают буфер по очереди в отдельных областях,
/// чтобы не держать хэндл буфера захваченным одновременно.
pub fn read_clipboard() -> Captured {
    // 1) Текст.
    if let Ok(mut cb) = Clipboard::new() {
        if let Ok(text) = cb.get_text() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let hash = sha_hex("t:", text.as_bytes());
                return Captured::Text { content: text, hash };
            }
        }
    }

    // 2) Файлы/папки из Проводника (CF_HDROP).
    if let Ok(files) =
        clipboard_win::get_clipboard::<Vec<String>, _>(clipboard_win::formats::FileList)
    {
        if !files.is_empty() {
            let hash = sha_hex("f:", files.join("\n").as_bytes());
            return Captured::Files { paths: files, hash };
        }
    }

    // 3) Изображение (RGBA).
    if let Ok(mut cb) = Clipboard::new() {
        if let Ok(img) = cb.get_image() {
            if img.width > 0 && img.height > 0 && !img.bytes.is_empty() {
                let rgba = img.bytes.into_owned();
                let hash = sha_hex("i:", &rgba);
                return Captured::Image {
                    rgba,
                    width: img.width,
                    height: img.height,
                    hash,
                };
            }
        }
    }

    Captured::None
}
