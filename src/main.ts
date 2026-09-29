import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { check } from "@tauri-apps/plugin-updater";
import { openUrl } from "@tauri-apps/plugin-opener";
import { parseColor, toHex, toRgb, toHsl, toCss, type Rgba } from "./color";
import { parseUrl, parseEmail, cleanUrl, domainOf } from "./url";
import "./styles.css";

interface ClipItem {
  id: number;
  type: "text" | "image" | "files";
  content: string | null;
  image_path: string | null;
  preview: string | null;
  mime_type: string | null;
  size: number | null;
  created_at: number;
  last_used_at: number | null;
  use_count: number;
  is_pinned: boolean;
  source_app: string | null;
  category: string | null;
  tags: string | null;
}
interface SettingsView {
  font_size: number;
  compact_mode: boolean;
  window_memory: boolean;
  has_master: boolean;
  auto_lock_min: number;
  auto_paste: boolean;
  hidden_tabs: string[];
}

const PAGE = 300;

// Инлайновые штриховые иконки (currentColor), стиль .ico задаётся в CSS.
const ICONS: Record<string, string> = {
  text: '<path d="M7 4h7l4 4v12H7z"/><path d="M14 4v4h4M9.5 13h5M9.5 16.5h5"/>',
  image:
    '<rect x="4" y="5" width="16" height="14" rx="2.2"/><circle cx="9" cy="10" r="1.6"/><path d="M4.5 16.5l4.2-4 3 2.6 3.4-3.8L19.5 15"/>',
  files: '<path d="M4 7.5a2 2 0 012-2h3.3l2 2H18a2 2 0 012 2v7a2 2 0 01-2 2H6a2 2 0 01-2-2z"/>',
  pin: '<path d="M9 4.5h6M10.2 4.5l-.5 5.2-2.2 2.3v1.2h9v-1.2l-2.2-2.3-.5-5.2M12 13.4V19.5"/>',
  trash:
    '<path d="M4 7h16M9.5 7V5.2A1.2 1.2 0 0110.7 4h2.6a1.2 1.2 0 011.2 1.2V7M6.5 7l.9 12.2a1.5 1.5 0 001.5 1.4h6.2a1.5 1.5 0 001.5-1.4L17.5 7M10 11v6M14 11v6"/>',
};
const svgIcon = (name: string) => `<svg class="ico" viewBox="0 0 24 24">${ICONS[name] ?? ""}</svg>`;

const el = {
  search: document.getElementById("search") as HTMLInputElement,
  clear: document.getElementById("clear") as HTMLButtonElement,
  support: document.getElementById("support") as HTMLButtonElement,
  settings: document.getElementById("settings") as HTMLButtonElement,
  tabs: document.getElementById("tabs") as HTMLElement,
  filterToggle: document.getElementById("filter-toggle") as HTMLButtonElement,
  filterBar: document.getElementById("filter-bar") as HTMLElement,
  fSource: document.getElementById("f-source") as HTMLSelectElement,
  fSince: document.getElementById("f-since") as HTMLSelectElement,
  fTag: document.getElementById("f-tag") as HTMLInputElement,
  fRegex: document.getElementById("f-regex") as HTMLInputElement,
  fClear: document.getElementById("f-clear") as HTMLButtonElement,
  list: document.getElementById("list") as HTMLElement,
  slots: document.getElementById("slots") as HTMLElement,
  empty: document.getElementById("empty") as HTMLElement,
  ctx: document.getElementById("ctxmenu") as HTMLElement,
  confirm: document.getElementById("confirm") as HTMLElement,
  confirmOk: document.getElementById("confirm-ok") as HTMLButtonElement,
  confirmCancel: document.getElementById("confirm-cancel") as HTMLButtonElement,
  lightbox: document.getElementById("lightbox") as HTMLElement,
  lbView: document.getElementById("lb-view") as HTMLElement,
  lbImg: document.getElementById("lb-img") as HTMLImageElement,
  lbDim: document.getElementById("lb-dim") as HTMLElement,
  lbZoom: document.getElementById("lb-zoom") as HTMLButtonElement,
  lbCopy: document.getElementById("lb-copy") as HTMLButtonElement,
  lbClose: document.getElementById("lb-close") as HTMLButtonElement,
  colorbox: document.getElementById("colorbox") as HTMLElement,
  cbSwatch: document.getElementById("cb-swatch") as HTMLElement,
  cbRows: document.getElementById("cb-rows") as HTMLElement,
  cbUpsell: document.getElementById("cb-upsell") as HTMLElement,
  cbBuy: document.getElementById("cb-buy") as HTMLButtonElement,
  cbClose: document.getElementById("cb-close") as HTMLButtonElement,
  lock: document.getElementById("lock") as HTMLElement,
  lockPw: document.getElementById("lock-pw") as HTMLInputElement,
  lockErr: document.getElementById("lock-err") as HTMLElement,
  lockUnlock: document.getElementById("lock-unlock") as HTMLButtonElement,
};

let filter = "all";
/** Закреплённые во «Все»: отдельная группа над лентой (null — группы нет). */
let pinnedGroup: ClipItem[] | null = null;
/** Группа развёрнута. По умолчанию свёрнута; запоминается в localStorage. */
let pinnedOpen = false;
try {
  pinnedOpen = localStorage.getItem("pinnedOpen") === "1";
} catch {
  /* нет хранилища — просто свёрнуто */
}
/** Умные списки (Pro): текстовые элементы, отобранные на клиенте. */
const SMART_TABS = ["colors", "urls", "emails"];
let query = "";
let items: ClipItem[] = [];
let selected = -1;
let isPro = false;
let windowMemory = false;
let autoPaste = true;
let hasMaster = false;
let autoLockMin = 0;
let unlockedAt = 0;
const win = getCurrentWindow();

// ── Загрузка данных ──────────────────────────────────────
function facetActive(): boolean {
  return (
    !el.filterBar.hidden &&
    (!!el.fSource.value ||
      el.fSince.value !== "0" ||
      !!el.fTag.value.trim() ||
      !!el.fRegex.value.trim())
  );
}

function buildFilters() {
  const days = Number(el.fSince.value);
  return {
    text: query.trim() || null,
    source: el.fSource.value || null,
    tag: el.fTag.value.trim() || null,
    regex: el.fRegex.value.trim() || null,
    since_ms: days > 0 ? Date.now() - days * 86_400_000 : null,
    kind: ["text", "image", "files"].includes(filter) ? filter : null,
    pinned_only: filter === "pinned",
  };
}

async function refresh() {
  try {
    if (SMART_TABS.includes(filter)) {
      const base = await invoke<ClipItem[]>("list_items", {
        filter: "text",
        limit: 1000,
        offset: 0,
      });
      items = base.filter((it) => {
        const c = it.content ?? it.preview ?? "";
        if (filter === "colors") return !!parseColor(c);
        if (filter === "urls") return !!parseUrl(c);
        return !!parseEmail(c);
      });
    } else if (facetActive()) {
      items = await invoke<ClipItem[]>("search_advanced", {
        filters: buildFilters(),
        limit: PAGE,
      });
    } else if (query.trim()) {
      items = await invoke<ClipItem[]>("search_items", { query, limit: PAGE });
    } else if (filter === "all") {
      // «Все»: закреплённые — сворачиваемой группой сверху, ниже лента по времени.
      const [pinned, rest] = await Promise.all([
        invoke<ClipItem[]>("list_items", { filter: "pinned", limit: 1000, offset: 0 }),
        invoke<ClipItem[]>("list_items", { filter: "unpinned", limit: PAGE, offset: 0 }),
      ]);
      pinnedGroup = pinned.length > 0 ? pinned : null;
      items = pinnedGroup && pinnedOpen ? [...pinned, ...rest] : rest;
      if (selected >= items.length) selected = items.length - 1;
      return render();
    } else {
      items = await invoke<ClipItem[]>("list_items", { filter, limit: PAGE, offset: 0 });
    }
  } catch (e) {
    console.error("refresh failed", e);
    items = [];
  }
  pinnedGroup = null;
  if (selected >= items.length) selected = items.length - 1;
  render();
}

// ── Рендер списка ────────────────────────────────────────
function togglePinnedGroup() {
  pinnedOpen = !pinnedOpen;
  try {
    localStorage.setItem("pinnedOpen", pinnedOpen ? "1" : "0");
  } catch {
    /* не запомним — не страшно */
  }
  selected = -1;
  refresh();
}

function renderGroupHead(count: number) {
  const head = document.createElement("div");
  head.className = "group-head" + (pinnedOpen ? " open" : "");
  head.title = pinnedOpen ? "Свернуть закреплённые" : "Показать закреплённые";
  head.innerHTML = `<span class="group-arrow">▸</span>${svgIcon("pin")}`;
  const label = document.createElement("span");
  label.textContent = `Закреплённые · ${count}`;
  head.appendChild(label);
  head.addEventListener("click", togglePinnedGroup);
  el.list.appendChild(head);
}

function render() {
  el.list.innerHTML = "";
  el.empty.hidden = items.length > 0 || !!pinnedGroup;

  const groupSize = pinnedGroup && pinnedOpen ? pinnedGroup.length : 0;
  if (pinnedGroup) renderGroupHead(pinnedGroup.length);

  items.forEach((it, i) => {
    if (pinnedGroup && i === groupSize && groupSize > 0) {
      const sep = document.createElement("div");
      sep.className = "group-sep";
      el.list.appendChild(sep);
    }
    const card = document.createElement("div");
    card.className = "item" + (i === selected ? " selected" : "");
    card.dataset.id = String(it.id);

    const top = document.createElement("div");
    top.className = "item-top";

    const icon = document.createElement("span");
    icon.className = "item-icon";
    icon.innerHTML = svgIcon(it.type);
    top.appendChild(icon);

    if (it.type === "image") {
      const img = document.createElement("img");
      img.className = "item-thumb";
      img.alt = it.preview ?? "изображение";
      loadImage(it.id, img);
      top.appendChild(img);
    } else {
      const color = it.type === "text" ? parseColor(it.content ?? it.preview ?? "") : null;
      if (color) {
        const sw = document.createElement("div");
        sw.className = "item-swatch";
        sw.style.setProperty("--swatch", toCss(color));
        top.appendChild(sw);
      }
      const text = document.createElement("div");
      text.className = "item-text";
      text.textContent = it.preview ?? it.content ?? "";
      top.appendChild(text);
    }
    card.appendChild(top);

    const meta = document.createElement("div");
    meta.className = "item-meta";
    if (it.category) {
      const dot = document.createElement("span");
      dot.className = "cat-dot";
      dot.style.background = it.category;
      meta.appendChild(dot);
    }
    const time = document.createElement("span");
    time.textContent = fmtTime(it.last_used_at ?? it.created_at);
    meta.appendChild(time);
    if (it.use_count > 1) {
      const cnt = document.createElement("span");
      cnt.className = "item-count";
      cnt.textContent = `×${it.use_count}`;
      meta.appendChild(cnt);
    }
    if (it.source_app) {
      const s = document.createElement("span");
      s.className = "src-chip";
      s.textContent = "· " + it.source_app;
      meta.appendChild(s);
    }
    const u = it.type === "text" ? parseUrl(it.content ?? it.preview ?? "") : null;
    if (u) {
      const c = document.createElement("span");
      c.className = "url-chip";
      c.textContent = "🔗 " + domainOf(u);
      meta.appendChild(c);
    }
    if (it.tags) {
      it.tags
        .split(",")
        .filter(Boolean)
        .slice(0, 3)
        .forEach((tg) => {
          const c = document.createElement("span");
          c.className = "tag-chip";
          c.textContent = "#" + tg;
          meta.appendChild(c);
        });
    }
    if (it.is_pinned) {
      const pin = document.createElement("span");
      pin.className = "pin-badge";
      pin.innerHTML = svgIcon("pin");
      meta.appendChild(pin);
    }
    card.appendChild(meta);

    // Удаление одним кликом (видно при наведении и на выбранном элементе).
    const del = document.createElement("button");
    del.type = "button";
    del.className = "item-del";
    del.title = "Удалить (Delete)";
    del.innerHTML = svgIcon("trash");
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      remove(it);
    });
    del.addEventListener("dblclick", (e) => e.stopPropagation());
    card.appendChild(del);

    card.addEventListener("click", () => {
      if (it.type === "image") openLightbox(it);
      else choose(it);
    });
    card.addEventListener("dblclick", () => choose(it));
    card.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      selected = i;
      updateSelection();
      openContext(e.clientX, e.clientY, it);
    });
    el.list.appendChild(card);
  });
}

const imgCache = new Map<number, string>();
async function loadImage(id: number, img: HTMLImageElement) {
  if (imgCache.has(id)) {
    img.src = imgCache.get(id)!;
    return;
  }
  try {
    const url = await invoke<string | null>("item_image_data_url", { id });
    if (url) {
      imgCache.set(id, url);
      img.src = url;
    }
  } catch (e) {
    console.error("image load failed", e);
  }
}

// ── Действия ─────────────────────────────────────────────
/** Прячет окно; при `paste` бэкенд ещё и вставит буфер (Ctrl+V) туда, где был
 *  пользователь, — если автовставка включена в настройках. */
async function finishPick(paste: boolean) {
  closeContext();
  try {
    await invoke("finish_pick", { paste });
  } catch (e) {
    console.error("finish_pick failed", e);
    await win.hide();
  }
}

/** Выбор элемента: Enter/клик — вставить, Ctrl+Enter (`paste = false`) — только в буфер. */
async function choose(it: ClipItem, paste = true) {
  try {
    await invoke("copy_item", { id: it.id });
  } catch (e) {
    console.error("copy failed", e);
    return finishPick(false);
  }
  await finishPick(paste);
}

/** Shift+Enter: вставить как обычный текст (Pro); в Free — обычная вставка. */
async function choosePlain(it: ClipItem) {
  if (!isPro || it.type !== "text") return choose(it);
  try {
    await invoke("copy_item_plain", { id: it.id });
  } catch (e) {
    console.error("copy plain failed", e);
    return finishPick(false);
  }
  await finishPick(true);
}

async function togglePin(it: ClipItem) {
  await invoke("set_pinned", { id: it.id, pinned: !it.is_pinned });
}

async function remove(it: ClipItem) {
  await invoke("delete_item", { id: it.id });
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (e) {
    console.error("clipboard write failed", e);
  }
}

async function cleanAndCopyUrl(u: URL) {
  await copyText(cleanUrl(u));
}

async function setCategory(it: ClipItem, category: string | null) {
  try {
    await invoke("set_category", { id: it.id, category });
  } catch (e) {
    console.error(e);
  }
}

async function editTags(it: ClipItem) {
  const val = await promptInput("Теги через запятую", it.tags ?? "");
  if (val === null) return;
  try {
    await invoke("set_tags", { id: it.id, tags: val.trim() || null });
  } catch (e) {
    console.error(e);
  }
}

async function saveImageFile(it: ClipItem) {
  const url = await invoke<string | null>("item_image_data_url", { id: it.id });
  if (!url) return;
  const a = document.createElement("a");
  a.href = url;
  a.download = `clipvault-${it.id}.png`;
  document.body.appendChild(a);
  a.click();
  a.remove();
}

async function copyImagePath(it: ClipItem) {
  const p = await invoke<string | null>("item_image_path", { id: it.id });
  if (p) await copyText(p);
}

function upsell() {
  invoke("open_settings").catch((e) => console.error(e));
}

// Очистить историю, оставив закреплённые элементы.
function openConfirmClear() {
  el.confirm.hidden = false;
}
function closeConfirmClear() {
  el.confirm.hidden = true;
}
async function clearHistory() {
  closeConfirmClear();
  try {
    await invoke("clear_history", { keepPinned: true });
  } catch (e) {
    console.error("clear failed", e);
  }
}

// ── Просмотр картинки ────────────────────────────────────
let lbItem: ClipItem | null = null;
async function openLightbox(it: ClipItem) {
  try {
    const url = await invoke<string | null>("item_image_data_url", { id: it.id });
    if (!url) return;
    lbItem = it;
    setLightboxActual(false);
    el.lbDim.textContent = "";
    el.lbImg.onload = () => {
      el.lbDim.textContent = `${el.lbImg.naturalWidth}×${el.lbImg.naturalHeight}`;
    };
    el.lbImg.src = url;
    el.lightbox.hidden = false;
  } catch (e) {
    console.error("preview failed", e);
  }
}
// Вписать в окно ↔ исходный размер 1:1 (с прокруткой).
function setLightboxActual(on: boolean) {
  el.lightbox.classList.toggle("actual", on);
  el.lbZoom.textContent = on ? "Вписать" : "Исходный размер";
  el.lbView.scrollTo(0, 0);
}
function toggleLightboxActual() {
  setLightboxActual(!el.lightbox.classList.contains("actual"));
}
function closeLightbox() {
  el.lightbox.hidden = true;
  el.lbImg.src = "";
  lbItem = null;
}

// ── Цвет: конвертация (Pro) ──────────────────────────────
function openColorPanel(c: Rgba) {
  el.cbSwatch.style.background = toCss(c);
  el.cbRows.innerHTML = "";
  if (isPro) {
    el.cbUpsell.hidden = true;
    el.cbRows.hidden = false;
    const rows: [string, string][] = [
      ["HEX", toHex(c)],
      ["RGB", toRgb(c)],
      ["HSL", toHsl(c)],
    ];
    for (const [label, val] of rows) {
      const row = document.createElement("div");
      row.className = "cb-row";
      const l = document.createElement("span");
      l.className = "cb-label";
      l.textContent = label;
      const v = document.createElement("span");
      v.className = "cb-val";
      v.textContent = val;
      v.title = val;
      const b = document.createElement("button");
      b.className = "cb-copy";
      b.textContent = "Копировать";
      b.addEventListener("click", async () => {
        await copyText(val);
        b.textContent = "✓";
        setTimeout(() => (b.textContent = "Копировать"), 1000);
      });
      row.append(l, v, b);
      el.cbRows.appendChild(row);
    }
  } else {
    el.cbRows.hidden = true;
    el.cbUpsell.hidden = false;
  }
  el.colorbox.hidden = false;
}
function closeColorPanel() {
  el.colorbox.hidden = true;
  el.cbRows.innerHTML = "";
}

// ── Мини-модал ввода (теги) ──────────────────────────────
function promptInput(title: string, initial = ""): Promise<string | null> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "confirm";
    overlay.innerHTML = `<div class="confirm-box">
        <p class="confirm-text"></p>
        <input class="lock-input" type="text" spellcheck="false" />
        <div class="confirm-bar" style="margin-top:12px">
          <button type="button" class="pi-cancel">Отмена</button>
          <button type="button" class="pi-ok danger">ОК</button>
        </div>
      </div>`;
    (overlay.querySelector(".confirm-text") as HTMLElement).textContent = title;
    document.body.appendChild(overlay);
    const input = overlay.querySelector("input") as HTMLInputElement;
    input.value = initial;
    const close = (val: string | null) => {
      overlay.remove();
      resolve(val);
    };
    overlay.querySelector(".pi-cancel")!.addEventListener("click", () => close(null));
    overlay.querySelector(".pi-ok")!.addEventListener("click", () => close(input.value));
    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) close(null);
    });
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") close(input.value);
      else if (e.key === "Escape") close(null);
    });
    requestAnimationFrame(() => input.focus());
  });
}

// ── Контекстное меню ─────────────────────────────────────
function openContext(x: number, y: number, it: ClipItem) {
  el.ctx.innerHTML = "";
  // "Текст	Клавиша" — клавиша выводится справа серым.
  const add = (label: string, fn: () => void, danger = false) => {
    const b = document.createElement("button");
    const [text, key] = label.split("	");
    b.textContent = text;
    if (key) {
      const k = document.createElement("span");
      k.className = "key";
      k.textContent = key;
      b.appendChild(k);
    }
    if (danger) b.className = "danger";
    b.addEventListener("click", async () => {
      closeContext();
      await fn();
    });
    el.ctx.appendChild(b);
  };
  const sep = () => {
    const s = document.createElement("div");
    s.className = "sep";
    el.ctx.appendChild(s);
  };

  if (autoPaste) {
    add("Вставить	Enter", () => choose(it));
    if (it.type === "text" && isPro) add("📄 Как обычный текст	Shift+Enter", () => choosePlain(it));
    add("Только копировать	Ctrl+Enter", () => choose(it, false));
  } else {
    add("Копировать", () => choose(it));
    if (it.type === "text" && isPro) add("📄 Как обычный текст", () => choosePlain(it));
  }
  if (it.type === "image") add("Предпросмотр", () => openLightbox(it));

  const color = it.type === "text" ? parseColor(it.content ?? it.preview ?? "") : null;
  if (color) add("🎨 Конвертировать цвет", () => openColorPanel(color));

  const u = it.type === "text" ? parseUrl(it.content ?? it.preview ?? "") : null;
  if (u)
    add(isPro ? "🔗 Очистить ссылку (UTM)" : "🔗 Очистить ссылку — Pro", () =>
      isPro ? cleanAndCopyUrl(u) : upsell(),
    );

  if (it.type === "image" && isPro) {
    add("💾 Сохранить как…", () => saveImageFile(it));
    add("📋 Копировать путь", () => copyImagePath(it));
  }

  add(it.is_pinned ? "Открепить" : "Закрепить", () => togglePin(it));

  if (isPro) {
    add("🔴 Метка", () => setCategory(it, "#ef4444"));
    add("🟢 Метка", () => setCategory(it, "#22c55e"));
    add("🔵 Метка", () => setCategory(it, "#3b82f6"));
    if (it.category) add("Убрать метку", () => setCategory(it, null));
    add("🏷 Теги…", () => editTags(it));
  }

  sep();
  add("Удалить	Delete", () => remove(it), true);

  el.ctx.hidden = false;
  const w = el.ctx.offsetWidth;
  const h = el.ctx.offsetHeight;
  el.ctx.style.left = Math.min(x, window.innerWidth - w - 6) + "px";
  el.ctx.style.top = Math.min(y, window.innerHeight - h - 6) + "px";
}

function closeContext() {
  el.ctx.hidden = true;
}

// ── Слоты (Pro, 5.3) ─────────────────────────────────────
async function renderSlots() {
  if (!isPro) {
    el.slots.hidden = true;
    return;
  }
  el.slots.hidden = false;
  let slots: (number | null)[] = [];
  try {
    slots = await invoke<(number | null)[]>("get_slots");
  } catch {
    slots = [];
  }
  el.slots.innerHTML = "";
  for (let i = 0; i < 10; i++) {
    const id = slots[i] ?? null;
    const b = document.createElement("button");
    b.type = "button";
    b.className = "slot" + (id != null ? " filled" : "");
    b.textContent = String(i + 1);
    b.title =
      id != null
        ? "ЛКМ — вставить · ПКМ — очистить"
        : "ЛКМ — сохранить выбранный элемент в слот";
    b.addEventListener("click", async () => {
      if (id != null) {
        await invoke("restore_slot", { index: i }).catch((e) => console.error(e));
        await finishPick(true);
      } else {
        const cur = items[selected] ?? items[0];
        if (cur) {
          await invoke("set_slot", { index: i, id: cur.id }).catch((e) => console.error(e));
          renderSlots();
        }
      }
    });
    b.addEventListener("contextmenu", async (e) => {
      e.preventDefault();
      await invoke("set_slot", { index: i, id: null }).catch((err) => console.error(err));
      renderSlots();
    });
    el.slots.appendChild(b);
  }
}

// ── Экран блокировки (Pro, 5.2) ──────────────────────────
function maybeLock() {
  if (!hasMaster) return;
  const expired = autoLockMin > 0 && Date.now() - unlockedAt >= autoLockMin * 60_000;
  if (unlockedAt === 0 || expired) showLock();
}
function showLock() {
  el.lock.hidden = false;
  el.lockErr.hidden = true;
  el.lockPw.value = "";
  requestAnimationFrame(() => el.lockPw.focus());
}
async function tryUnlock() {
  try {
    const ok = await invoke<boolean>("verify_master_password", { password: el.lockPw.value });
    if (ok) {
      unlockedAt = Date.now();
      el.lock.hidden = true;
      el.lockPw.value = "";
      el.search.focus();
    } else {
      el.lockErr.hidden = false;
    }
  } catch (e) {
    console.error(e);
  }
}

// ── Выбор/навигация ──────────────────────────────────────
function updateSelection() {
  el.list.querySelectorAll(".item").forEach((n, i) =>
    n.classList.toggle("selected", i === selected),
  );
  const node = el.list.querySelectorAll<HTMLElement>(".item")[selected];
  node?.scrollIntoView({ block: "nearest" });
}

function move(delta: number) {
  if (items.length === 0) return;
  selected = Math.max(0, Math.min(items.length - 1, selected + delta));
  if (selected < 0) selected = 0;
  updateSelection();
}

// ── Время ────────────────────────────────────────────────
function fmtTime(ms: number): string {
  const d = new Date(ms);
  const now = new Date();
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  if (d.toDateString() === now.toDateString()) return `Сегодня, ${hh}:${mm}`;
  const dd = String(d.getDate()).padStart(2, "0");
  const mo = String(d.getMonth() + 1).padStart(2, "0");
  return `${dd}.${mo}, ${hh}:${mm}`;
}

// ── Настройки/лицензия ───────────────────────────────────
async function loadSettings() {
  try {
    const s = await invoke<SettingsView>("get_settings");
    document.documentElement.style.setProperty("--fs", s.font_size + "px");
    document.body.classList.toggle("compact", s.compact_mode);
    windowMemory = s.window_memory;
    autoPaste = s.auto_paste;
    applyHiddenTabs(s.hidden_tabs);
    hasMaster = s.has_master;
    autoLockMin = s.auto_lock_min;
  } catch (e) {
    console.error("get_settings failed", e);
  }
}
/** Прячет вкладки, выключенные в настройках. Если активная скрыта — переходим на «Все». */
function applyHiddenTabs(hidden: string[]) {
  el.tabs.querySelectorAll<HTMLElement>(".tab[data-filter]").forEach((t) => {
    const f = t.dataset.filter || "all";
    t.hidden = f !== "all" && hidden.includes(f);
  });
  if (filter !== "all" && hidden.includes(filter)) {
    filter = "all";
    el.tabs.querySelectorAll(".tab").forEach((x) => x.classList.remove("active"));
    el.tabs.querySelector('.tab[data-filter="all"]')?.classList.add("active");
    refresh();
  }
}

async function loadLicense() {
  try {
    isPro = (await invoke<{ pro: boolean }>("get_license")).pro;
  } catch {
    isPro = false;
  }
}
async function loadSources() {
  try {
    const src = await invoke<string[]>("list_sources");
    el.fSource.innerHTML = '<option value="">Все источники</option>';
    for (const s of src) {
      const o = document.createElement("option");
      o.value = s;
      o.textContent = s;
      el.fSource.appendChild(o);
    }
  } catch (e) {
    console.error(e);
  }
}

// ── Сохранение геометрии окна (Pro, 3.4) ─────────────────
let geoTimer: number | undefined;
async function saveGeo() {
  if (!windowMemory) return;
  try {
    const pos = await win.outerPosition();
    const size = await win.outerSize();
    await invoke("save_window_geometry", {
      x: pos.x,
      y: pos.y,
      w: size.width,
      h: size.height,
    });
  } catch (e) {
    console.error(e);
  }
}
function scheduleSaveGeo() {
  clearTimeout(geoTimer);
  geoTimer = window.setTimeout(saveGeo, 500);
}

// ── События DOM ──────────────────────────────────────────
let searchTimer: number | undefined;
el.search.addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = window.setTimeout(() => {
    query = el.search.value;
    selected = -1;
    refresh();
  }, 120);
});

el.tabs.querySelectorAll(".tab[data-filter]").forEach((t) =>
  t.addEventListener("click", () => {
    const f = (t as HTMLElement).dataset.filter || "all";
    if (SMART_TABS.includes(f) && !isPro) {
      upsell();
      return;
    }
    el.tabs.querySelectorAll(".tab").forEach((x) => x.classList.remove("active"));
    t.classList.add("active");
    filter = f;
    selected = -1;
    refresh();
  }),
);

el.filterToggle.addEventListener("click", () => {
  if (!isPro) {
    upsell();
    return;
  }
  el.filterBar.hidden = !el.filterBar.hidden;
  if (!el.filterBar.hidden) loadSources();
  refresh();
});
let filterTimer: number | undefined;
const onFilterChange = () => {
  clearTimeout(filterTimer);
  filterTimer = window.setTimeout(() => refresh(), 150);
};
[el.fSource, el.fSince, el.fTag, el.fRegex].forEach((n) =>
  n.addEventListener("input", onFilterChange),
);
el.fClear.addEventListener("click", () => {
  el.fSource.value = "";
  el.fSince.value = "0";
  el.fTag.value = "";
  el.fRegex.value = "";
  refresh();
});

el.support.addEventListener("click", () => {
  openUrl("https://boosty.to/wufcorp/donate").catch((e) =>
    console.error("open donate failed", e),
  );
});

el.settings.addEventListener("click", () => {
  invoke("open_settings").catch((e) => console.error("open settings failed", e));
});

el.clear.addEventListener("click", openConfirmClear);
el.confirmOk.addEventListener("click", clearHistory);
el.confirmCancel.addEventListener("click", closeConfirmClear);
el.confirm.addEventListener("click", (e) => {
  if (e.target === el.confirm) closeConfirmClear();
});

el.lbClose.addEventListener("click", closeLightbox);
el.lbZoom.addEventListener("click", toggleLightboxActual);
el.lbImg.addEventListener("click", toggleLightboxActual);
el.lbCopy.addEventListener("click", () => {
  const it = lbItem;
  closeLightbox();
  if (it) choose(it);
});
el.lightbox.addEventListener("click", (e) => {
  if (e.target === el.lightbox || e.target === el.lbView) closeLightbox();
});

el.cbClose.addEventListener("click", closeColorPanel);
el.cbBuy.addEventListener("click", () => {
  closeColorPanel();
  upsell();
});
el.colorbox.addEventListener("click", (e) => {
  if (e.target === el.colorbox) closeColorPanel();
});

el.lockUnlock.addEventListener("click", tryUnlock);
el.lockPw.addEventListener("keydown", (e) => {
  if (e.key === "Enter") tryUnlock();
});

document.addEventListener("keydown", (e) => {
  if (!el.lock.hidden) return; // заблокировано — навигация недоступна
  if (e.key === "Escape") {
    if (!el.confirm.hidden) return closeConfirmClear();
    if (!el.colorbox.hidden) return closeColorPanel();
    if (!el.lightbox.hidden) return closeLightbox();
    if (!el.ctx.hidden) return closeContext();
    win.hide();
  } else if (e.key === "Delete") {
    // В непустом поле ввода Delete стирает символы, а не элемент истории.
    const t = e.target as HTMLInputElement;
    if ((t.tagName === "INPUT" || t.tagName === "TEXTAREA") && t.value) return;
    if (!el.lightbox.hidden || !el.colorbox.hidden || !el.confirm.hidden) return;
    const it = items[selected];
    if (it) {
      e.preventDefault();
      remove(it);
    }
  } else if (e.key === "ArrowDown") {
    e.preventDefault();
    move(1);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    move(-1);
  } else if (e.key === "Enter") {
    // Enter — вставить, Shift+Enter — как обычный текст, Ctrl+Enter — только в буфер.
    const it = items[selected];
    if (!it) return;
    e.preventDefault();
    if (e.ctrlKey) choose(it, false);
    else if (it.type === "image") openLightbox(it);
    else if (e.shiftKey) choosePlain(it);
    else choose(it);
  }
});

document.addEventListener("click", (e) => {
  if (!el.ctx.hidden && !el.ctx.contains(e.target as Node)) closeContext();
});
window.addEventListener("scroll", closeContext, true);

// ── События из бэкенда ───────────────────────────────────
listen("history-updated", () => refresh());
listen("focus-search", () => {
  loadSettings();
  el.search.value = "";
  query = "";
  selected = -1;
  maybeLock();
  refresh();
  requestAnimationFrame(() => el.search.focus());
});
listen("license-changed", (e) => {
  isPro = (e.payload as { pro: boolean } | null)?.pro ?? false;
  renderSlots();
  refresh();
});

win.onMoved(() => scheduleSaveGeo());
win.onResized(() => scheduleSaveGeo());

// Старт
(async () => {
  await loadLicense();
  await loadSettings();
  maybeLock();
  renderSlots();
  refresh();
  autoCheckUpdates();
})();

// ── Авто-проверка обновлений ─────────────────────────────
async function autoCheckUpdates() {
  try {
    const s = await invoke<{ auto_update: boolean }>("get_settings");
    if (!s.auto_update) return;
    const update = await check();
    if (update) {
      el.settings.classList.add("has-update");
      el.settings.title = `Доступно обновление ${update.version} — откройте настройки`;
    }
  } catch {
    /* офлайн — молчим */
  }
}
