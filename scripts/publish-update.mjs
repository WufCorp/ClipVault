// @ts-check
/**
 * publish-update — один прогон релиза автообновления ClipVault.
 *
 *   npm run publish-update
 *
 * Что делает по шагам:
 *   1. читает версию из package.json;
 *   2. собирает и ПОДПИСЫВАЕТ бандл (`npm run tauri build`);
 *   3. находит `*_x64-setup.exe` и парный `.sig` в bundle/nsis;
 *   4. генерирует latest.json (signature = содержимое .sig, url на S3);
 *   5. собирает portable-архив (clipvault.exe + маркер `portable` + readme);
 *   6. заливает установщик, архив и latest.json в бакет (public-read) через AWS CLI.
 *
 * Секреты в репозиторий НЕ попадают:
 *   - приватный ключ подписи: env TAURI_SIGNING_PRIVATE_KEY, либо путь к файлу
 *     ключа в env CLIPVAULT_UPDATER_KEY_FILE (скрипт сам прочитает файл);
 *   - ключи S3: профиль AWS CLI (по умолчанию `timeweb`), не в коде.
 *
 * Полезные флаги / env:
 *   --skip-build            не пересобирать (если бандл уже готов и подписан);
 *                           ключ и пароль тогда не нужны;
 *   --dry-run               всё сделать, но НЕ заливать на S3;
 *   RELEASE_NOTES="..."     текст «что нового» (иначе — из release-notes.txt
 *                           в корне, иначе — дефолт по версии);
 *   AWS_PROFILE=timeweb     профиль AWS CLI (по умолчанию timeweb).
 */

import { execFileSync } from "node:child_process";
import {
  readFileSync,
  writeFileSync,
  readdirSync,
  existsSync,
  statSync,
  mkdirSync,
  rmSync,
  copyFileSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");

// ── Конфиг S3 (публичные значения — совпадают с tauri.conf.json / RELEASE.md) ──
const S3 = {
  bucket: "prisma-prava",
  prefix: "updates",
  endpoint: "https://s3.twcstorage.ru",
  region: "ru-1",
  profile: process.env.AWS_PROFILE || "timeweb",
  publicBase: "https://s3.twcstorage.ru/prisma-prava/updates",
};

const args = new Set(process.argv.slice(2));
const SKIP_BUILD = args.has("--skip-build");
const DRY_RUN = args.has("--dry-run");

const log = (m) => console.log(`\x1b[36m▸\x1b[0m ${m}`);
const die = (m) => {
  console.error(`\x1b[31m✗ ${m}\x1b[0m`);
  process.exit(1);
};

/** Запуск команды с наследованием stdio; кидает при ненулевом коде. */
function run(cmd, cmdArgs, opts = {}) {
  execFileSync(cmd, cmdArgs, { stdio: "inherit", cwd: ROOT, shell: true, ...opts });
}

// ── 1. Версия ────────────────────────────────────────────────────────────────
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
const version = pkg.version;
if (!/^\d+\.\d+\.\d+/.test(version)) die(`странная версия в package.json: ${version}`);

// Версия ДОЛЖНА совпадать в трёх местах: иначе установщик (версия из Cargo.toml/
// tauri.conf.json) и latest.json (версия из package.json) разойдутся, и клиент
// либо не увидит апдейт, либо словит несоответствие.
const tauriConf = JSON.parse(
  readFileSync(join(ROOT, "src-tauri", "tauri.conf.json"), "utf8"),
);
const cargoToml = readFileSync(join(ROOT, "src-tauri", "Cargo.toml"), "utf8");
const cargoVer = (cargoToml.match(/^\s*version\s*=\s*"([^"]+)"/m) || [])[1];
const mismatches = [];
if (tauriConf.version !== version)
  mismatches.push(`tauri.conf.json = ${tauriConf.version}`);
if (cargoVer !== version) mismatches.push(`Cargo.toml = ${cargoVer}`);
if (mismatches.length) {
  die(
    `версии разошлись (package.json = ${version}, но ${mismatches.join(", ")}). ` +
      `Приведи все три к одному значению перед публикацией.`,
  );
}
log(`Версия релиза: ${version} (совпадает в 3 местах)`);

// ── Проверки окружения перед долгой сборкой ───────────────────────────────────
// С --skip-build ключ и пароль не нужны: бандл уже подписан (в т.ч. вручную через
// `npx tauri signer sign`), скрипт только берёт готовый .sig.
if (!SKIP_BUILD && !process.env.TAURI_SIGNING_PRIVATE_KEY) {
  const keyFile = process.env.CLIPVAULT_UPDATER_KEY_FILE;
  if (keyFile && existsSync(keyFile)) {
    process.env.TAURI_SIGNING_PRIVATE_KEY = readFileSync(keyFile, "utf8");
    log(`Ключ подписи прочитан из ${keyFile}`);
  } else {
    die(
      "нет приватного ключа подписи. Задай env TAURI_SIGNING_PRIVATE_KEY " +
        "(содержимое clipvault-updater.key) или CLIPVAULT_UPDATER_KEY_FILE=путь\\к\\ключу.",
    );
  }
}
// Ключ обновлений защищён паролем: без него tauri build падает только в самом
// конце, после ~5 минут сборки. Проверяем заранее.
if (!SKIP_BUILD && !process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
  die(
    "не задан пароль ключа подписи: env TAURI_SIGNING_PRIVATE_KEY_PASSWORD " +
      "(cmd: set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=..., PowerShell: $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = \"...\").",
  );
}

// ── 2. Сборка + подпись ───────────────────────────────────────────────────────
if (SKIP_BUILD) {
  log("Сборка пропущена (--skip-build)");
} else {
  log("Сборка и подпись: npm run tauri build …");
  run("npm", ["run", "tauri", "build"]);
}

// ── 3. Поиск артефактов ───────────────────────────────────────────────────────
const nsisDir = join(ROOT, "src-tauri", "target", "release", "bundle", "nsis");
if (!existsSync(nsisDir)) die(`не найден каталог бандла: ${nsisDir}`);

const files = readdirSync(nsisDir);
// Только файлы текущей версии: иначе можно выложить старый установщик или
// подпись от другой сборки.
const pick = (suffix) => files.find((f) => f.includes(`_${version}_`) && f.endsWith(suffix));

const exeName = pick("_x64-setup.exe");
const sigName = pick("_x64-setup.exe.sig");
if (!exeName) die(`не найден *_x64-setup.exe в ${nsisDir}`);
if (!sigName) die(`не найден *_x64-setup.exe.sig в ${nsisDir} (createUpdaterArtifacts?)`);

const exePath = join(nsisDir, exeName);
const sigPath = join(nsisDir, sigName);
const signature = readFileSync(sigPath, "utf8").trim();
log(`Установщик: ${exeName}`);
log(`Подпись:    ${sigName}`);

// ── 3.5 Portable-архив ────────────────────────────────────────────────────────
// Тот же clipvault.exe, что внутри установщика, + файл-маркер `portable`:
// увидев его рядом с собой, программа хранит данные в <папка>\data (paths.rs).
// Файлы лежат в корне архива: «Извлечь всё» само создаст папку по имени zip,
// а обновление — это распаковка поверх с заменой.
const releaseDir = join(ROOT, "src-tauri", "target", "release");
const appExe = join(releaseDir, "clipvault.exe");
if (!existsSync(appExe)) die(`не найден ${appExe}`);
if (Math.abs(statSync(appExe).mtimeMs - statSync(exePath).mtimeMs) > 30 * 60 * 1000)
  die("clipvault.exe и установщик собраны в разное время — пересобери без --skip-build");

const zipName = `ClipVault_${version}_x64-portable.zip`;
const zipPath = join(releaseDir, "bundle", zipName);
const stage = join(releaseDir, "portable-stage");
rmSync(stage, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });
copyFileSync(appExe, join(stage, "ClipVault.exe"));
writeFileSync(
  join(stage, "portable"),
  "Маркер portable-режима ClipVault: пока этот файл лежит рядом с ClipVault.exe,\r\n" +
    "история, настройки и лицензия хранятся в папке data рядом с программой.\r\n",
  "utf8",
);
writeFileSync(
  join(stage, "README.txt"),
  [
    `ClipVault ${version} — portable-версия`,
    "",
    "Запуск: ClipVault.exe. Нужен Windows 10/11 x64 с WebView2 (есть в системе по умолчанию).",
    "Папку держите там, где можно писать файлы (не в Program Files).",
    "",
    "Все данные — в папке data рядом с программой: история, картинки, настройки,",
    "лицензия Pro, логи и кэш окон. В профиль Windows ничего не пишется.",
    "Автозапуск по умолчанию выключен — включается в меню значка в трее.",
    "",
    "Обновление: закройте ClipVault (трей → Выход), распакуйте новый архив поверх",
    "с заменой файлов. Папку data не удаляйте.",
    "",
    "Перенос всего из установленной версии: закройте её и скопируйте содержимое",
    "папки %APPDATA%\\ClipVault в папку data. Обе версии одновременно не запускаются.",
    "",
    "Сайт: https://wufcorp.github.io/ClipVault/",
    "",
  ].join("\r\n"),
  "utf8",
);
rmSync(zipPath, { force: true });
run("powershell", [
  "-NoProfile",
  "-Command",
  `"Compress-Archive -Path '${join(stage, "*")}' -DestinationPath '${zipPath}'"`,
]);
rmSync(stage, { recursive: true, force: true });
const zipUrl = `${S3.publicBase}/${zipName}`;
log(`Portable:   ${zipName}`);

// ── 4. latest.json ────────────────────────────────────────────────────────────
let notes = process.env.RELEASE_NOTES;
if (!notes) {
  const notesFile = join(ROOT, "release-notes.txt");
  notes = existsSync(notesFile)
    ? readFileSync(notesFile, "utf8").trim()
    : `Обновление ClipVault ${version}.`;
}

const latest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature,
      url: `${S3.publicBase}/${exeName}`,
    },
  },
};
const latestPath = join(ROOT, "latest.json");
writeFileSync(latestPath, JSON.stringify(latest, null, 2) + "\n", "utf8");
log(`latest.json собран → ${latestPath}`);

// ── 4.5 docs/release.json (лендинг на GitHub Pages) ───────────────────────────
// Сайт читает этот файл со своего же origin и подставляет версию, размер и
// ссылку на установщик. Файл попадёт на Pages только после git commit + push.
const sizeMb = (statSync(exePath).size / 1024 / 1024).toFixed(1).replace(".", ",");
const relPath = join(ROOT, "docs", "release.json");
if (existsSync(join(ROOT, "docs"))) {
  const prev = existsSync(relPath) ? JSON.parse(readFileSync(relPath, "utf8")) : {};
  writeFileSync(
    relPath,
    JSON.stringify(
      {
        version,
        pub_date: latest.pub_date.slice(0, 10),
        size_mb: sizeMb,
        url: `${S3.publicBase}/${exeName}`,
        portable_url: zipUrl,
        portable_size_mb: (statSync(zipPath).size / 1024 / 1024).toFixed(1).replace(".", ","),
        github: prev.github || "https://github.com/WufCorp/ClipVault/releases",
      },
      null,
      2,
    ) + "\n",
    "utf8",
  );
  log(`docs/release.json обновлён (${version}, ${sizeMb} МБ) — не забудь запушить docs/`);
}

// ── 6. Заливка на S3 ──────────────────────────────────────────────────────────
const s3 = (localPath, key, extra = []) =>
  run("aws", [
    "s3",
    "cp",
    `"${localPath}"`,
    `"s3://${S3.bucket}/${S3.prefix}/${key}"`,
    "--endpoint-url",
    S3.endpoint,
    "--region",
    S3.region,
    "--profile",
    S3.profile,
    "--acl",
    "public-read",
    ...extra,
  ]);

if (DRY_RUN) {
  log("--dry-run: на S3 НИЧЕГО не заливаю. Готово к ручной проверке.");
  process.exit(0);
}

log("Загрузка установщика на S3 …");
s3(exePath, exeName);

log("Загрузка portable-архива на S3 …");
s3(zipPath, zipName);

log("Загрузка latest.json на S3 (no-cache) …");
// latest.json без кэша — иначе клиенты могут видеть старую версию.
s3(latestPath, "latest.json", ["--cache-control", "no-cache"]);

console.log(
  `\n\x1b[32m✓ Готово.\x1b[0m ClipVault ${version} опубликован. ` +
    `Установленные копии увидят обновление при следующей проверке.`,
);
