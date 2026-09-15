# 🚀 Релиз ClipVault (Фаза 6)

Пошаговый выпуск подписанной, автообновляемой сборки. Требуется машина с Rust-тулчейном
и сетью. Опирается на [DECISIONS.md](DECISIONS.md) §5 (S3) и [ROADMAP.md](ROADMAP.md) Фаза 6.

## 0. Секреты (один раз)

Приватные ключи хранятся ВНЕ репозитория (у владельца). Их публичные пары уже зашиты:
- ключ обновлений → `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`;
- ключ лицензий → `src-tauri/src/license.rs` → `LICENSE_PUBLIC_KEY_HEX`.

Переменные окружения для подписи обновлений (PowerShell):
```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content "путь\clipvault-updater.key" -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""   # ключ без пароля
```

S3 (Timeweb) — Access/Secret Key владельца в отдельном профиле/менеджере секретов
(в репозиторий не класть). Бакет `prisma-prava`, регион `ru-1`, endpoint `https://s3.twcstorage.ru`.

## 6.1 Версия

Синхронно поднять версию в трёх местах (уже 2.0.0):
`package.json`, `src-tauri/Cargo.toml` (`[package].version`), `src-tauri/tauri.conf.json` (`version`).

## 6.2 Сборка и подпись

```bash
npm install
npm run tauri build
```
Результат в `src-tauri/target/release/bundle/nsis/`:
- `ClipVault_2.0.0_x64-setup.exe` — установщик;
- `ClipVault_2.0.0_x64-setup.exe.sig` — подпись обновления (создаётся, т.к. в конфиге
  `bundle.createUpdaterArtifacts = true`).

## 6.3 latest.json и загрузка на S3

Создать `latest.json` (значение `signature` = СОДЕРЖИМОЕ файла `.sig`):
```json
{
  "version": "2.0.0",
  "notes": "Монетизация: Pro-лицензия, автообновление, цвета, слоты, фасеты, безопасность.",
  "pub_date": "2026-09-15T00:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<содержимое .sig одной строкой>",
      "url": "https://s3.twcstorage.ru/prisma-prava/updates/ClipVault_2.0.0_x64-setup.exe"
    }
  }
}
```

Загрузить в бакет (объекты — public-read):
```
prisma-prava/updates/latest.json
prisma-prava/updates/ClipVault_2.0.0_x64-setup.exe
```
Endpoint проверки (`plugins.updater.endpoints[0]`) уже указывает на
`https://s3.twcstorage.ru/prisma-prava/updates/latest.json`.

## 6.3 Сквозной тест автообновления

1. Установить СТАРУЮ версию (например, 1.2.0) на чистую ВМ.
2. Опубликовать новую (шаги выше).
3. Настройки → «Проверить сейчас» ИЛИ перезапуск → на шестерёнке появляется точка.
4. Убедиться: скачало → установило (passive NSIS) → перезапустилось → версия обновилась.
5. История в `%APPDATA%\ClipVault\` сохранилась (данные в Roaming, установщик currentUser).

## 6.4 Режим паранойи (готово)

Тумблер «Проверять обновления автоматически» в настройках. Выкл = ноль сетевых запросов.

## 6.5 Каналы дистрибуции

- Прямая загрузка с сайта (ссылка на S3-установщик).
- GitHub Releases (залить тот же `*-setup.exe`).
- WinGet (манифест на установщик).
- ⚠️ Microsoft Store — проверить: песочница может резать глобальный хоткей/автозапуск.
  Если режет — Store-канал не использовать или пометить ограничения.

## 6.6 Запуск

Habr / Product Hunt / Reddit. Упор: приватность (единственный сетевой запрос —
проверка обновлений, отключаема; никакой телеметрии/Яндекса) и «399 ₽ разово, на всех
своих ПК». Донат Boosty и контакт Telegram — в настройках.

## Выпуск Pro-ключа покупателю (после оплаты)

```powershell
$env:CLIPVAULT_LICENSE_PRIVATE_KEY = Get-Content "путь\clipvault-license-private.hex" -Raw
cd src-tauri
cargo run --example genkey -- buyer@example.com
```
Вывод (строка `seg1.seg2`) — ключ покупателю. Активация в приложении: Настройки → вставить ключ.
