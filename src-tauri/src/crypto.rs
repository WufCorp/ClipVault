//! Криптопримитивы: DPAPI-защита локальных секретов (5.1) и хеш мастер-пароля (5.2).
//!
//! DPAPI шифрует данные ключом, привязанным к учётной записи Windows, — их нельзя
//! расшифровать под другим пользователем/на другом ПК. Используется для защиты
//! файла лицензии и хеша мастер-пароля «в покое».
//!
//! ⚠️ Полное шифрование истории (БД) отложено: конфликтует с FTS5 (индексирует
//! открытый текст) и требует SQLCipher (недоступен со связкой bundled-rusqlite).

use sha2::{Digest, Sha256};

// ── Мастер-пароль: солёный SHA-256 ───────────────────────
/// Возвращает строку "saltHex:hashHex".
pub fn hash_password(pw: &str) -> String {
    let mut salt = [0u8; 16];
    let _ = getrandom::getrandom(&mut salt);
    let mut h = Sha256::new();
    h.update(salt);
    h.update(pw.as_bytes());
    format!("{}:{}", hex::encode(salt), hex::encode(h.finalize()))
}

/// Проверяет пароль против сохранённого "saltHex:hashHex".
pub fn verify_password(pw: &str, stored: &str) -> bool {
    let Some((salt_hex, hash_hex)) = stored.split_once(':') else {
        return false;
    };
    let Ok(salt) = hex::decode(salt_hex) else {
        return false;
    };
    let mut h = Sha256::new();
    h.update(&salt);
    h.update(pw.as_bytes());
    hex::encode(h.finalize()) == hash_hex
}

// ── DPAPI (Windows) ──────────────────────────────────────
#[cfg(windows)]
pub fn dpapi_protect(data: &[u8]) -> Option<Vec<u8>> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };
        let ok = CryptProtectData(
            &in_blob, null(), null(), null(), null(), 0, &mut out_blob,
        );
        if ok == 0 || out_blob.pbData.is_null() {
            return None;
        }
        let out = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        LocalFree(out_blob.pbData as _);
        Some(out)
    }
}

#[cfg(windows)]
pub fn dpapi_unprotect(data: &[u8]) -> Option<Vec<u8>> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };
        let ok = CryptUnprotectData(
            &in_blob, null_mut(), null(), null(), null(), 0, &mut out_blob,
        );
        if ok == 0 || out_blob.pbData.is_null() {
            return None;
        }
        let out = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        LocalFree(out_blob.pbData as _);
        Some(out)
    }
}

#[cfg(not(windows))]
pub fn dpapi_protect(_data: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(not(windows))]
pub fn dpapi_unprotect(_data: &[u8]) -> Option<Vec<u8>> {
    None
}
