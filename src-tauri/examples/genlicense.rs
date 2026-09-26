//! Owner-side CLI: генерация НОВОЙ пары ключей лицензий (Ed25519).
//!
//! Нужен один раз, если приватный seed лицензий утерян. Выводит:
//!   - PRIVATE — 32-байтовый seed (hex). СЕКРЕТ. В `CLIPVAULT_LICENSE_PRIVATE_KEY`
//!     для `genkey`. Сохранить вне репозитория (менеджер паролей / защищённый файл).
//!   - PUBLIC  — публичный ключ (hex). НЕ секрет. Вписать в `src/license.rs`
//!     → `LICENSE_PUBLIC_KEY_HEX`, затем пересобрать приложение.
//!
//! Запуск:
//!   cd src-tauri
//!   cargo run --example genlicense
//!
//! ⚠️ Это пара ключей ЛИЦЕНЗИЙ, отдельная от ключей подписи ОБНОВЛЕНИЙ
//!    (`~/.tauri/clipvault.key`, minisign). Не путать.
//! ⚠️ После смены публичного ключа ранее выданные Pro-ключи (если были) перестанут
//!    проверяться. На старте продаж это безопасно — ключей ещё нет.

use ed25519_dalek::SigningKey;

fn main() {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).expect("OS RNG недоступен");

    let signing_key = SigningKey::from_bytes(&seed);
    let public = signing_key.verifying_key();

    println!("PRIVATE (secret, -> env CLIPVAULT_LICENSE_PRIVATE_KEY):");
    println!("  {}", hex::encode(seed));
    println!();
    println!("PUBLIC (-> src/license.rs LICENSE_PUBLIC_KEY_HEX):");
    println!("  {}", hex::encode(public.to_bytes()));
}
