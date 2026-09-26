//! Запись содержимого обратно в буфер обмена (когда пользователь выбрал элемент).

use arboard::{Clipboard, ImageData};
use std::borrow::Cow;

/// Кладёт текст в буфер обмена.
pub fn write_text(text: &str) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(text.to_owned()).map_err(|e| e.to_string())
}

/// Кладёт изображение (RGBA) в буфер обмена.
pub fn write_image(rgba: Vec<u8>, width: usize, height: usize) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| e.to_string())?;
    let data = ImageData {
        width,
        height,
        bytes: Cow::from(rgba),
    };
    cb.set_image(data).map_err(|e| e.to_string())
}

/// Кладёт список файлов/папок обратно в буфер (CF_HDROP), чтобы вставить в Проводник.
pub fn write_files(paths: &[String]) -> Result<(), String> {
    use clipboard_win::{formats::FileList, Clipboard as WinClipboard, Setter};
    let _clip = WinClipboard::new_attempts(10).map_err(|e| format!("clipboard open: {e:?}"))?;
    FileList
        .write_clipboard(paths)
        .map_err(|e| format!("set files: {e:?}"))
}
