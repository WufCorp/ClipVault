//! Разделяемое состояние приложения.
//!
//! Один и тот же `Shared` живёт и в Tauri-`manage` (для команд из фронтенда),
//! и в потоке слушателя буфера обмена — поэтому все поля за `Arc`.

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::license::LicenseInfo;
use crate::settings::Settings;

#[derive(Clone)]
pub struct Shared {
    /// Единственное соединение с SQLite под мьютексом (запись из буфера редкая).
    pub db: Arc<Mutex<Connection>>,
    /// Пауза записи (тумблер в трее). Слушатель игнорирует события, пока true.
    pub paused: Arc<AtomicBool>,
    /// Хэш содержимого, которое ClipVault сам только что положил в буфер, —
    /// чтобы не сохранить своё же изменение (защита от цикла).
    pub last_written_hash: Arc<Mutex<Option<String>>>,
    /// Каталог с картинками (images/).
    pub images_dir: PathBuf,
    /// Единая точка Pro-гейта: фичи спрашивают `is_pro()`, а не проверяют лицензию сами.
    pub is_pro: Arc<AtomicBool>,
    /// Email из активной лицензии (для показа «Лицензия: …@…»).
    pub license_email: Arc<Mutex<Option<String>>>,
    /// Пользовательские настройки.
    pub settings: Arc<Mutex<Settings>>,
}

impl Shared {
    pub fn new(
        conn: Connection,
        images_dir: PathBuf,
        settings: Settings,
        license: LicenseInfo,
    ) -> Self {
        Shared {
            db: Arc::new(Mutex::new(conn)),
            paused: Arc::new(AtomicBool::new(false)),
            last_written_hash: Arc::new(Mutex::new(None)),
            images_dir,
            is_pro: Arc::new(AtomicBool::new(license.pro)),
            license_email: Arc::new(Mutex::new(license.email)),
            settings: Arc::new(Mutex::new(settings)),
        }
    }

    /// Активна ли Pro-лицензия. Единая точка гейта.
    pub fn is_pro(&self) -> bool {
        self.is_pro.load(std::sync::atomic::Ordering::Relaxed)
    }
}
