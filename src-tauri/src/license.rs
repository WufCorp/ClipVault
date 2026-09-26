//! Проверка офлайн-лицензии (Ed25519). Публичный ключ зашит в бинарь;
//! подпись проверяется локально, без сети, навсегда (DECISIONS §4).
//!
//! Формат ключа: `<seg1>.<seg2>`, где
//!   seg1 = base64url_nopad(JSON payload)      — {"tier","email","issued"}
//!   seg2 = base64url_nopad(Ed25519-подпись seg1.as_bytes(), 64 байта)
//! Подпись ставит владелец приватным ключом (см. `examples/genkey.rs`).

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::crypto;
use crate::paths;

/// Префикс файла лицензии, защищённого DPAPI (5.1).
const DPAPI_MARKER: &str = "DPAPI:";

/// Публичный ключ владельца (Ed25519, 32 байта hex). Парный приватный ключ —
/// только у владельца.
/// ⚠️ Это НЕ ключ подписи обновлений (tauri.conf.json → plugins.updater.pubkey).
const LICENSE_PUBLIC_KEY_HEX: &str =
    "b934b8ae3d5c3f037e25b1b448d18f13323d54b96b76bf7385567f55cbd43886";

/// Полезная нагрузка ключа (то, что подписано).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Payload {
    /// Уровень: сейчас только "pro".
    tier: String,
    /// Email покупателя (мягкая привязка, показывается в UI).
    #[serde(default)]
    email: String,
    /// Момент выпуска (unix ms), информационно.
    #[serde(default)]
    #[allow(dead_code)]
    issued: i64,
}

/// Статус лицензии для фронтенда.
#[derive(Debug, Clone, Serialize)]
pub struct LicenseInfo {
    pub pro: bool,
    pub email: Option<String>,
}

impl LicenseInfo {
    pub fn free() -> Self {
        LicenseInfo {
            pro: false,
            email: None,
        }
    }
}

fn verifying_key() -> Option<VerifyingKey> {
    let bytes = hex::decode(LICENSE_PUBLIC_KEY_HEX).ok()?;
    let arr: [u8; 32] = bytes.try_into().ok()?;
    VerifyingKey::from_bytes(&arr).ok()
}

/// Проверяет подписанный ключ. Ok(Payload) при валидной подписи и tier == "pro".
fn verify(key: &str) -> Result<Payload, String> {
    let key = key.trim();
    let (seg1, seg2) = key.split_once('.').ok_or("неверный формат ключа")?;

    let sig_bytes = URL_SAFE_NO_PAD
        .decode(seg2.as_bytes())
        .map_err(|_| "неверная подпись")?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "неверная длина подписи")?;
    let signature = Signature::from_bytes(&sig_arr);

    let vk = verifying_key().ok_or("внутренняя ошибка: публичный ключ")?;
    vk.verify_strict(seg1.as_bytes(), &signature)
        .map_err(|_| "ключ не подходит (подпись неверна)")?;

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(seg1.as_bytes())
        .map_err(|_| "повреждённый ключ")?;
    let payload: Payload =
        serde_json::from_slice(&payload_bytes).map_err(|_| "повреждённые данные ключа")?;
    if payload.tier != "pro" {
        return Err("неизвестный уровень лицензии".into());
    }
    Ok(payload)
}

/// Проверяет ключ (без сохранения на диск).
pub fn validate(key: &str) -> Result<LicenseInfo, String> {
    let p = verify(key)?;
    Ok(LicenseInfo {
        pro: true,
        email: if p.email.is_empty() {
            None
        } else {
            Some(p.email)
        },
    })
}

/// Читает сохранённый ключ с диска, снимая DPAPI-защиту если она есть.
fn read_key_file() -> Option<String> {
    let raw = std::fs::read_to_string(paths::license_path()).ok()?;
    let raw = raw.trim();
    if let Some(b64) = raw.strip_prefix(DPAPI_MARKER) {
        let protected = STANDARD.decode(b64.trim()).ok()?;
        let plain = crypto::dpapi_unprotect(&protected)?;
        Some(String::from_utf8_lossy(&plain).trim().to_string())
    } else {
        Some(raw.to_string())
    }
}

/// Пишет ключ на диск, по возможности защищая DPAPI (привязка к учётке Windows).
/// В portable — открытым текстом: папку переносят на другие ПК, где DPAPI ключ
/// не расшифрует и Pro молча пропадёт. Ключ подписан, подделать его всё равно нельзя.
fn write_key_file(key: &str) -> Result<(), String> {
    let key = key.trim();
    let protected = if paths::is_portable() {
        None
    } else {
        crypto::dpapi_protect(key.as_bytes())
    };
    let payload = match protected {
        Some(protected) => format!("{DPAPI_MARKER}{}", STANDARD.encode(protected)),
        None => key.to_string(),
    };
    std::fs::write(paths::license_path(), payload).map_err(|e| e.to_string())
}

/// Загружает и проверяет лицензию с диска при старте.
pub fn load() -> LicenseInfo {
    let Some(key) = read_key_file() else {
        return LicenseInfo::free();
    };
    let Ok(info) = validate(&key) else {
        return LicenseInfo::free();
    };
    // Portable с license.key, скопированным из установленной версии (DPAPI):
    // пока он расшифровывается, переписываем открытым текстом — иначе на
    // другом ПК Pro пропадёт.
    if paths::is_portable() {
        let on_disk = std::fs::read_to_string(paths::license_path()).unwrap_or_default();
        if on_disk.trim_start().starts_with(DPAPI_MARKER) {
            if let Err(e) = write_key_file(&key) {
                tracing::warn!("portable: could not rewrite license without DPAPI: {e}");
            }
        }
    }
    info
}

/// Сохраняет валидный ключ на диск (активация).
pub fn activate(key: &str) -> Result<LicenseInfo, String> {
    let info = validate(key)?;
    write_key_file(key)?;
    Ok(info)
}

/// Удаляет сохранённую лицензию (деактивация).
pub fn deactivate() -> Result<(), String> {
    let path = paths::license_path();
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Читает сырой сохранённый ключ (для экспорта в файл/буфер).
pub fn stored_key() -> Option<String> {
    read_key_file()
}
