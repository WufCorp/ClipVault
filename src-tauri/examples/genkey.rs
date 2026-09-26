//! Owner-side CLI: генерация подписанных офлайн-ключей Pro.
//!
//! Приватный ключ (32-байтовый seed в hex) берётся из переменной окружения
//! `CLIPVAULT_LICENSE_PRIVATE_KEY` — он НЕ хранится в репозитории/бинаре.
//! Парный публичный ключ зашит в `src/license.rs`.
//!
//! Режимы (PowerShell, из папки src-tauri):
//!   # один ключ на email покупателя
//!   cargo run --example genkey -- buyer@example.com
//!
//!   # склад: N ключей без email, каждый с уникальным id → CSV (UTF-8)
//!   cargo run --example genkey -- --count 100 --out keys.csv
//!
//! CSV: `id,key,sent_to,sent_at,payment` — последние три колонки пустые,
//! их заполняет владелец при выдаче ключа (DECISIONS §1, §4).
//! Приложение принимает ключи без email: поле `id` оно игнорирует, а email
//! просто не показывается в настройках.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_COUNT: u32 = 10_000;

fn die(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(2);
}

fn usage() -> ! {
    die("usage:\n  cargo run --example genkey -- <email>\n  cargo run --example genkey -- --count <N> --out <file.csv>")
}

fn signing_key() -> SigningKey {
    let seed_hex = std::env::var("CLIPVAULT_LICENSE_PRIVATE_KEY")
        .unwrap_or_else(|_| die("set CLIPVAULT_LICENSE_PRIVATE_KEY to the 32-byte private seed (hex)"));
    let seed = hex::decode(seed_hex.trim()).unwrap_or_else(|_| die("private key must be valid hex"));
    let seed: [u8; 32] = seed
        .as_slice()
        .try_into()
        .unwrap_or_else(|_| die("private key must be exactly 32 bytes"));
    SigningKey::from_bytes(&seed)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Подписывает payload → `base64url(JSON).base64url(sig)` (формат `license.rs`).
fn sign(sk: &SigningKey, payload: &serde_json::Value) -> String {
    let payload_json = serde_json::to_vec(payload).expect("serialize payload");
    let seg1 = URL_SAFE_NO_PAD.encode(&payload_json);
    let seg2 = URL_SAFE_NO_PAD.encode(sk.sign(seg1.as_bytes()).to_bytes());
    format!("{seg1}.{seg2}")
}

/// Уникальный id ключа: `CV-XXXX-XXXX` (8 случайных символов без 0/O/1/I).
fn random_id() -> String {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ"; // 32 символа
    let mut buf = [0u8; 8];
    getrandom::getrandom(&mut buf).expect("OS random");
    let s: String = buf
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect();
    format!("CV-{}-{}", &s[..4], &s[4..])
}

fn batch(count: u32, out: &str) {
    if count == 0 || count > MAX_COUNT {
        die(&format!("--count must be 1..={MAX_COUNT}"));
    }
    if std::path::Path::new(out).exists() {
        die(&format!("{out} already exists — не перезаписываю склад ключей"));
    }
    let sk = signing_key();
    let issued = now_ms();

    let mut ids = std::collections::HashSet::new();
    let mut csv = String::from("id,key,sent_to,sent_at,payment\n");
    while ids.len() < count as usize {
        let id = random_id();
        if !ids.insert(id.clone()) {
            continue; // коллизия в пределах пачки — берём другой
        }
        let payload = serde_json::json!({ "tier": "pro", "email": "", "issued": issued, "id": id });
        csv.push_str(&format!("{id},{},,,\n", sign(&sk, &payload)));
    }

    std::fs::write(out, csv).unwrap_or_else(|e| die(&format!("write {out}: {e}")));
    eprintln!("✓ {count} keys → {out}");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("--count") {
        let mut count = None;
        let mut out = None;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--count" => count = it.next().and_then(|v| v.parse::<u32>().ok()),
                "--out" => out = it.next().cloned(),
                _ => usage(),
            }
        }
        match (count, out) {
            (Some(c), Some(o)) => batch(c, &o),
            _ => usage(),
        }
        return;
    }

    let email = args.first().cloned().unwrap_or_default();
    if email.is_empty() || email.starts_with("--") {
        usage();
    }
    let payload = serde_json::json!({ "tier": "pro", "email": email, "issued": now_ms() });
    println!("{}", sign(&signing_key(), &payload));
}
