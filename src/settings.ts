import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import "./settings.css";

interface LicenseInfo {
  pro: boolean;
  email: string | null;
}
interface SettingsView {
  auto_update: boolean;
  auto_paste: boolean;
  keep_open: boolean;
  hidden_tabs: string[];
  open_hotkey: string;
  ignore_apps: string[];
  window_memory: boolean;
  font_size: number;
  compact_mode: boolean;
  max_age_days: number;
  auto_lock_min: number;
  has_master: boolean;
  slots: (number | null)[];
  portable: boolean;
  data_dir: string;
}

const LINKS = {
  buy: "https://yookassa.ru/my/i/aqiEEXXqILcr/l",
  donate: "https://boosty.to/wufcorp/donate",
  telegram: "https://t.me/WufCorp",
};
/** Portable-архив версии на S3 (кладёт туда scripts/publish-update.mjs). */
const portableZipUrl = (v: string) =>
  `https://s3.twcstorage.ru/prisma-prava/updates/ClipVault_${v}_x64-portable.zip`;
let isPortable = false;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const el = {
  badge: $<HTMLSpanElement>("lic-badge"),
  status: $<HTMLParagraphElement>("lic-status"),
  activate: $<HTMLDivElement>("lic-activate"),
  active: $<HTMLDivElement>("lic-active"),
  key: $<HTMLTextAreaElement>("lic-key"),
  error: $<HTMLDivElement>("lic-error"),
  apply: $<HTMLButtonElement>("lic-apply"),
  import: $<HTMLButtonElement>("lic-import"),
  exportClip: $<HTMLButtonElement>("lic-export-clip"),
  exportFile: $<HTMLButtonElement>("lic-export-file"),
  deactivate: $<HTMLButtonElement>("lic-deactivate"),
  proCard: $<HTMLElement>("pro-card"),
  buy: $<HTMLButtonElement>("buy-pro"),
  buyDialog: $<HTMLDialogElement>("buy-dialog"),
  buyGo: $<HTMLButtonElement>("buy-go"),
  buyCancel: $<HTMLButtonElement>("buy-cancel"),
  // Общие
  hotkey: $<HTMLInputElement>("hotkey"),
  hotkeySave: $<HTMLButtonElement>("hotkey-save"),
  hotkeyMsg: $<HTMLDivElement>("hotkey-msg"),
  autoPaste: $<HTMLInputElement>("auto-paste"),
  keepOpen: $<HTMLInputElement>("keep-open"),
  tabsVisible: $<HTMLDivElement>("tabs-visible"),
  ignore: $<HTMLTextAreaElement>("ignore"),
  ignoreSave: $<HTMLButtonElement>("ignore-save"),
  // Данные
  exportTxt: $<HTMLButtonElement>("export-txt"),
  exportJson: $<HTMLButtonElement>("export-json"),
  importJson: $<HTMLButtonElement>("import-json"),
  dataMsg: $<HTMLParagraphElement>("data-msg"),
  // Pro-настройки
  proSettings: $<HTMLElement>("pro-settings"),
  proLock: $<HTMLSpanElement>("pro-lock"),
  font: $<HTMLInputElement>("font"),
  fontVal: $<HTMLSpanElement>("font-val"),
  compact: $<HTMLInputElement>("compact"),
  winMemory: $<HTMLInputElement>("win-memory"),
  maxAge: $<HTMLInputElement>("max-age"),
  maxAgeSave: $<HTMLButtonElement>("max-age-save"),
  // Безопасность
  masterOff: $<HTMLDivElement>("master-off"),
  masterOn: $<HTMLDivElement>("master-on"),
  masterNew: $<HTMLInputElement>("master-new"),
  masterSet: $<HTMLButtonElement>("master-set"),
  masterCur: $<HTMLInputElement>("master-cur"),
  masterRemove: $<HTMLButtonElement>("master-remove"),
  autoLock: $<HTMLInputElement>("auto-lock"),
  masterMsg: $<HTMLDivElement>("master-msg"),
  // Обновления / поддержка
  autoUpdate: $<HTMLInputElement>("auto-update"),
  checkUpdate: $<HTMLButtonElement>("check-update"),
  updateStatus: $<HTMLParagraphElement>("update-status"),
  downloadPortable: $<HTMLButtonElement>("download-portable"),
  portableInfo: $<HTMLParagraphElement>("portable-info"),
  licenseStorage: $<HTMLSpanElement>("license-storage"),
  dataDir: $<HTMLSpanElement>("data-dir"),
  donate: $<HTMLButtonElement>("donate"),
  contact: $<HTMLButtonElement>("contact"),
  version: $<HTMLSpanElement>("app-version"),
};

// Скрытые input для импорта файлов.
function makeFileInput(accept: string): HTMLInputElement {
  const i = document.createElement("input");
  i.type = "file";
  i.accept = accept;
  i.hidden = true;
  document.body.appendChild(i);
  return i;
}
const licFileInput = makeFileInput(".key,.txt,text/plain");
const importFileInput = makeFileInput(".json,application/json");

// ── Утилиты ──────────────────────────────────────────────
function download(name: string, content: string, mime = "text/plain") {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
function msg(elm: HTMLElement, text: string, kind: "" | "ok" | "err" = "") {
  elm.hidden = false;
  elm.textContent = text;
  elm.className = elm.className.replace(/\b(ok|err)\b/g, "").trim();
  if (kind) elm.classList.add(kind);
}

// ── Лицензия ─────────────────────────────────────────────
function renderLicense(info: LicenseInfo) {
  el.badge.textContent = info.pro ? "Pro" : "Free";
  el.badge.className = "badge " + (info.pro ? "badge-pro" : "badge-free");
  el.status.textContent = info.pro
    ? info.email
      ? `Pro активирована · ${info.email}`
      : "Pro активирована."
    : "Бесплатная версия.";
  el.activate.hidden = info.pro;
  el.active.hidden = !info.pro;
  el.proCard.hidden = info.pro;
  applyProGating(info.pro);
}

function applyProGating(pro: boolean) {
  el.proLock.hidden = pro;
  el.proSettings.classList.toggle("pro-disabled", !pro);
  document.querySelectorAll<HTMLElement>("[data-pro]").forEach((n) => {
    (n as HTMLInputElement | HTMLButtonElement).disabled = !pro;
  });
}

function showError(msgText: string | null) {
  el.error.textContent = msgText ?? "";
  el.error.hidden = !msgText;
}

async function applyKey() {
  const key = el.key.value.trim();
  if (!key) return showError("Вставьте ключ.");
  el.apply.disabled = true;
  showError(null);
  try {
    renderLicense(await invoke<LicenseInfo>("activate_license", { key }));
    el.key.value = "";
  } catch (e) {
    showError(String(e));
  } finally {
    el.apply.disabled = false;
  }
}
async function deactivate() {
  try {
    await invoke("deactivate_license");
    renderLicense({ pro: false, email: null });
  } catch (e) {
    showError(String(e));
  }
}
async function copyKey() {
  const key = await invoke<string | null>("export_license");
  if (!key) return;
  try {
    await navigator.clipboard.writeText(key);
    flash(el.exportClip, "Скопировано ✓");
  } catch {
    flash(el.exportClip, "Не удалось");
  }
}
async function exportFile() {
  const key = await invoke<string | null>("export_license");
  if (key) download("clipvault-license.key", key);
}
function flash(btn: HTMLButtonElement, text: string) {
  const original = btn.textContent;
  btn.textContent = text;
  setTimeout(() => (btn.textContent = original), 1400);
}

// ── Обновления ───────────────────────────────────────────
function updStatus(text: string, kind: "" | "ok" | "err" = "") {
  el.updateStatus.hidden = false;
  el.updateStatus.textContent = text;
  el.updateStatus.className =
    "muted small" + (kind === "ok" ? " update-ok" : kind === "err" ? " update-err" : "");
}
async function checkUpdate() {
  el.checkUpdate.disabled = true;
  updStatus("Проверяем…");
  try {
    const update = await check();
    if (update && isPortable) {
      // Установщик поставил бы рядом вторую, обычную копию — portable
      // обновляется вручную из архива.
      updStatus(`Доступна версия ${update.version}. Скачайте архив и замените файлы.`, "ok");
      const url = portableZipUrl(update.version);
      el.downloadPortable.onclick = () => open(url);
      el.downloadPortable.hidden = false;
    } else if (update) {
      updStatus(`Доступна версия ${update.version}. Загрузка…`, "ok");
      await update.downloadAndInstall();
      updStatus("Обновление установлено. Перезапуск…", "ok");
      await relaunch();
    } else {
      updStatus("У вас последняя версия.", "ok");
    }
  } catch (e) {
    updStatus("Не удалось проверить обновления: " + String(e), "err");
  } finally {
    el.checkUpdate.disabled = false;
  }
}
async function open(url: string) {
  try {
    await openUrl(url);
  } catch (e) {
    console.error("open url failed", e);
  }
}

// ── Общие / данные / Pro / безопасность ──────────────────
// Хоткей задаётся «как в нормальном софте»: клик по полю → зажать сочетание.
// В value показываем дружелюбный вид (Ctrl + Shift + V), а сырой акселератор
// Tauri (CommandOrControl+Shift+V) храним в dataset.accel — его и шлём в бэкенд.

/** event.code → токен клавиши для акселератора Tauri (или null для модификатора). */
function mainKeyFromCode(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  const named: Record<string, string> = {
    ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
    Space: "Space", Enter: "Enter", Tab: "Tab", Backspace: "Backspace",
    Delete: "Delete", Home: "Home", End: "End", PageUp: "PageUp",
    PageDown: "PageDown", Insert: "Insert", Minus: "-", Equal: "=",
    BracketLeft: "[", BracketRight: "]", Semicolon: ";", Quote: "'",
    Backquote: "`", Backslash: "\\", Comma: ",", Period: ".", Slash: "/",
  };
  return named[code] ?? null;
}

function modsOf(e: KeyboardEvent): string[] {
  const m: string[] = [];
  if (e.ctrlKey) m.push("CommandOrControl");
  if (e.altKey) m.push("Alt");
  if (e.shiftKey) m.push("Shift");
  if (e.metaKey) m.push("Super");
  return m;
}

/** Полный акселератор из события, либо null если сочетание ещё неполное/недопустимое. */
function accelFromEvent(e: KeyboardEvent): string | null {
  const main = mainKeyFromCode(e.code);
  if (!main) return null; // нажаты только модификаторы
  const isFn = /^F([1-9]|1[0-9]|2[0-4])$/.test(main);
  const mods = modsOf(e);
  if (mods.length === 0 && !isFn) return null; // одиночную клавишу не берём (кроме F-клавиш)
  return [...mods, main].join("+");
}

/** Акселератор Tauri → человекочитаемая подпись. */
function prettyAccel(accel: string): string {
  return accel
    .split("+")
    .map((p) =>
      p === "CommandOrControl" || p === "Control" || p === "Ctrl"
        ? "Ctrl"
        : p === "Super" || p === "Meta"
          ? "Win"
          : p,
    )
    .join(" + ");
}

/** Живой предпросмотр удерживаемых модификаторов до нажатия основной клавиши. */
function livePreview(e: KeyboardEvent): string {
  const mods = modsOf(e).map((m) => (m === "CommandOrControl" ? "Ctrl" : m === "Super" ? "Win" : m));
  return mods.length ? mods.join(" + ") + " + …" : "…";
}

let recording = false;
let prevAccel = "";

function startRecord() {
  recording = true;
  prevAccel = el.hotkey.dataset.accel || "";
  el.hotkey.value = "";
  el.hotkey.placeholder = "Нажмите сочетание…";
  el.hotkey.classList.add("recording");
}
function stopRecord() {
  recording = false;
  el.hotkey.classList.remove("recording");
  el.hotkey.blur();
}
function setHotkey(accel: string) {
  el.hotkey.dataset.accel = accel;
  el.hotkey.value = prettyAccel(accel);
}

function onHotkeyKeydown(e: KeyboardEvent) {
  if (!recording) return;
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "Escape") {
    if (prevAccel) setHotkey(prevAccel);
    stopRecord();
    return;
  }
  const accel = accelFromEvent(e);
  if (accel) {
    setHotkey(accel);
    stopRecord(); // готовое сочетание — осталось нажать «Применить»
  } else {
    el.hotkey.value = livePreview(e); // держим модификаторы — показываем их
  }
}
function onHotkeyBlur() {
  if (!recording) return;
  recording = false;
  el.hotkey.classList.remove("recording");
  if (prevAccel) setHotkey(prevAccel); // ушли, не завершив запись — вернём прежнее
  else el.hotkey.value = "";
}

async function saveHotkey() {
  const hk = (el.hotkey.dataset.accel || "").trim();
  if (!hk) {
    msg(el.hotkeyMsg, "Сначала задайте сочетание", "err");
    return;
  }
  try {
    await invoke("set_open_hotkey", { hotkey: hk });
    msg(el.hotkeyMsg, "Готово ✓", "ok");
  } catch (e) {
    msg(el.hotkeyMsg, String(e), "err");
  }
}
async function saveIgnore() {
  const apps = el.ignore.value
    .split(/[,\n]/)
    .map((s) => s.trim())
    .filter(Boolean);
  try {
    await invoke("set_ignore_apps", { apps });
    flash(el.ignoreSave, "Сохранено ✓");
  } catch (e) {
    console.error(e);
  }
}
async function exportHistory(format: "txt" | "json") {
  try {
    const data = await invoke<string>("export_history", { format });
    download(
      `clipvault-history.${format}`,
      data,
      format === "json" ? "application/json" : "text/plain",
    );
  } catch (e) {
    msg(el.dataMsg, String(e), "err");
  }
}
async function importHistory(json: string) {
  try {
    const count = await invoke<number>("import_history", { json });
    msg(el.dataMsg, `Импортировано записей: ${count}.`, "ok");
  } catch (e) {
    msg(el.dataMsg, String(e), "err");
  }
}
async function setMaster() {
  const pw = el.masterNew.value;
  if (!pw) return msg(el.masterMsg, "Введите пароль.", "err");
  try {
    await invoke("set_master_password", { current: null, new: pw });
    el.masterNew.value = "";
    await loadSettings();
    msg(el.masterMsg, "Мастер-пароль установлен ✓", "ok");
  } catch (e) {
    msg(el.masterMsg, String(e), "err");
  }
}
async function removeMaster() {
  const cur = el.masterCur.value;
  try {
    await invoke("set_master_password", { current: cur, new: null });
    el.masterCur.value = "";
    await loadSettings();
    msg(el.masterMsg, "Пароль снят.", "ok");
  } catch (e) {
    msg(el.masterMsg, String(e), "err");
  }
}

// ── Инициализация ────────────────────────────────────────
async function loadSettings() {
  try {
    const s = await invoke<SettingsView>("get_settings");
    el.autoUpdate.checked = s.auto_update;
    el.autoPaste.checked = s.auto_paste;
    el.keepOpen.checked = s.keep_open;
    tabChecks().forEach((c) => (c.checked = !s.hidden_tabs.includes(c.dataset.tab!)));
    isPortable = s.portable;
    el.portableInfo.hidden = !s.portable;
    if (s.portable) {
      el.licenseStorage.textContent =
        "Ключ лицензии лежит в папке data без привязки к ПК — чтобы Pro переезжал вместе с программой.";
    }
    el.dataDir.textContent = s.data_dir;
    setHotkey(s.open_hotkey);
    el.ignore.value = s.ignore_apps.join(", ");
    el.font.value = String(s.font_size);
    el.fontVal.textContent = String(s.font_size);
    el.compact.checked = s.compact_mode;
    el.winMemory.checked = s.window_memory;
    el.maxAge.value = String(s.max_age_days);
    el.autoLock.value = String(s.auto_lock_min);
    el.masterOff.hidden = s.has_master;
    el.masterOn.hidden = !s.has_master;
  } catch (e) {
    console.error("get_settings failed", e);
  }
}

async function init() {
  try {
    renderLicense(await invoke<LicenseInfo>("get_license"));
  } catch (e) {
    console.error("get_license failed", e);
  }
  await loadSettings();
  try {
    el.version.textContent = await getVersion();
  } catch {
    /* keep default */
  }
}

// ── События ──────────────────────────────────────────────
el.apply.addEventListener("click", applyKey);
el.import.addEventListener("click", () => licFileInput.click());
licFileInput.addEventListener("change", async () => {
  const f = licFileInput.files?.[0];
  if (!f) return;
  el.key.value = (await f.text()).trim();
  licFileInput.value = "";
  await applyKey();
});
el.exportClip.addEventListener("click", copyKey);
el.exportFile.addEventListener("click", exportFile);
el.deactivate.addEventListener("click", deactivate);
// Перед оплатой — обязательное окно: ключ придёт на email, указанный в ЮKassa.
el.buy.addEventListener("click", () => el.buyDialog.showModal());
el.buyCancel.addEventListener("click", () => el.buyDialog.close());
el.buyGo.addEventListener("click", () => {
  el.buyDialog.close();
  open(LINKS.buy);
});
el.buyDialog.addEventListener("click", (e) => {
  if (e.target === el.buyDialog) el.buyDialog.close();
});

el.hotkeySave.addEventListener("click", saveHotkey);
el.hotkey.addEventListener("focus", startRecord);
el.hotkey.addEventListener("keydown", onHotkeyKeydown);
el.hotkey.addEventListener("blur", onHotkeyBlur);
el.ignoreSave.addEventListener("click", saveIgnore);
el.exportTxt.addEventListener("click", () => exportHistory("txt"));
el.exportJson.addEventListener("click", () => exportHistory("json"));
el.importJson.addEventListener("click", () => importFileInput.click());
importFileInput.addEventListener("change", async () => {
  const f = importFileInput.files?.[0];
  if (!f) return;
  const text = await f.text();
  importFileInput.value = "";
  await importHistory(text);
});

el.font.addEventListener("input", () => {
  el.fontVal.textContent = el.font.value;
});
el.font.addEventListener("change", () => {
  invoke("set_font_size", { size: Number(el.font.value) }).catch(console.error);
});
el.compact.addEventListener("change", () => {
  invoke("set_compact_mode", { enabled: el.compact.checked }).catch(console.error);
});
el.winMemory.addEventListener("change", () => {
  invoke("set_window_memory", { enabled: el.winMemory.checked }).catch(console.error);
});
el.maxAgeSave.addEventListener("click", () => {
  invoke("set_max_age_days", { days: Number(el.maxAge.value) })
    .then(() => flash(el.maxAgeSave, "Готово ✓"))
    .catch(console.error);
});

el.masterSet.addEventListener("click", setMaster);
el.masterRemove.addEventListener("click", removeMaster);
el.autoLock.addEventListener("change", () => {
  invoke("set_auto_lock", { minutes: Number(el.autoLock.value) }).catch(console.error);
});

el.checkUpdate.addEventListener("click", checkUpdate);
el.keepOpen.addEventListener("change", () => {
  invoke("set_keep_open", { enabled: el.keepOpen.checked }).catch(console.error);
});
const tabChecks = () => el.tabsVisible.querySelectorAll<HTMLInputElement>("input[data-tab]");
el.tabsVisible.addEventListener("change", () => {
  const tabs = [...tabChecks()].filter((c) => !c.checked).map((c) => c.dataset.tab!);
  invoke("set_hidden_tabs", { tabs }).catch(console.error);
});
el.autoPaste.addEventListener("change", () => {
  invoke("set_auto_paste", { enabled: el.autoPaste.checked }).catch(console.error);
});
el.autoUpdate.addEventListener("change", () => {
  invoke("set_auto_update", { enabled: el.autoUpdate.checked }).catch(console.error);
});
el.donate.addEventListener("click", () => open(LINKS.donate));
el.contact.addEventListener("click", () => open(LINKS.telegram));

init();
