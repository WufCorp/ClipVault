//! Команды, вызываемые из фронтенда через `invoke`.

use std::sync::atomic::Ordering;

use base64::{engine::general_purpose::STANDARD, Engine};
use image::ImageFormat;
use tauri::{AppHandle, Emitter, Manager, State};

use serde::{Deserialize, Serialize};

use crate::clipboard::{reader, writer};
use crate::crypto;
use crate::db::repo;
use crate::license::{self, LicenseInfo};
use crate::models::{ClipItem, NewItem, KIND_FILES, KIND_IMAGE, KIND_TEXT};
use crate::settings::{Settings, WinGeom};
use crate::state::Shared;

type CmdResult<T> = Result<T, String>;

fn emit_updated(app: &AppHandle) {
    let _ = app.emit("history-updated", ());
}

/// Список истории по вкладке: "all" | "pinned" | "text" | "image".
#[tauri::command]
pub fn list_items(
    state: State<'_, Shared>,
    filter: String,
    limit: i64,
    offset: i64,
) -> CmdResult<Vec<ClipItem>> {
    let conn = state.db.lock().unwrap();
    repo::list(&conn, &filter, limit, offset).map_err(|e| e.to_string())
}

/// Мгновенный поиск по тексту (FTS5).
#[tauri::command]
pub fn search_items(
    state: State<'_, Shared>,
    query: String,
    limit: i64,
) -> CmdResult<Vec<ClipItem>> {
    let conn = state.db.lock().unwrap();
    repo::search(&conn, &query, limit).map_err(|e| e.to_string())
}

/// Кладёт элемент по id обратно в буфер обмена (с защитой от цикла и touch).
/// Общий помощник для copy_item, слотов и быстрой вставки.
pub(crate) fn put_item_in_clipboard(state: &Shared, id: i64) -> CmdResult<()> {
    let item = {
        let conn = state.db.lock().unwrap();
        repo::get_by_id(&conn, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "элемент не найден".to_string())?
    };

    if item.kind == KIND_TEXT {
        let content = item.content.unwrap_or_default();
        let hash = reader::sha_hex("t:", content.as_bytes());
        *state.last_written_hash.lock().unwrap() = Some(hash);
        writer::write_text(&content)?;
    } else if item.kind == KIND_IMAGE {
        let file = item.image_path.ok_or_else(|| "нет файла картинки".to_string())?;
        let path = state.images_dir.join(&file);
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let img = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let rgba = img.into_raw();
        let hash = reader::sha_hex("i:", &rgba);
        *state.last_written_hash.lock().unwrap() = Some(hash);
        writer::write_image(rgba, w, h)?;
    } else if item.kind == KIND_FILES {
        let content = item.content.unwrap_or_default();
        let paths: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let hash = reader::sha_hex("f:", content.as_bytes());
        *state.last_written_hash.lock().unwrap() = Some(hash);
        writer::write_files(&paths)?;
    }

    let conn = state.db.lock().unwrap();
    let _ = repo::touch(&conn, id);
    Ok(())
}

/// Копирует выбранный элемент обратно в буфер обмена.
#[tauri::command]
pub fn copy_item(app: AppHandle, state: State<'_, Shared>, id: i64) -> CmdResult<()> {
    put_item_in_clipboard(state.inner(), id)?;
    emit_updated(&app);
    Ok(())
}

/// Копирует элемент как обычный текст (Pro, 3.2): нормализует пробелы/переводы строк.
/// Для не-текстовых элементов ведёт себя как обычное копирование.
#[tauri::command]
pub fn copy_item_plain(app: AppHandle, state: State<'_, Shared>, id: i64) -> CmdResult<()> {
    let item = {
        let conn = state.db.lock().unwrap();
        repo::get_by_id(&conn, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "элемент не найден".to_string())?
    };
    if item.kind == KIND_TEXT {
        let raw = item.content.unwrap_or_default();
        // Нормализация: CRLF→LF, убрать хвостовые пробелы строк, схлопнуть 3+ пустых строк.
        let normalized = normalize_plain(&raw);
        let hash = reader::sha_hex("t:", normalized.as_bytes());
        *state.last_written_hash.lock().unwrap() = Some(hash);
        writer::write_text(&normalized)?;
        let conn = state.db.lock().unwrap();
        let _ = repo::touch(&conn, item.id);
    } else {
        put_item_in_clipboard(state.inner(), id)?;
    }
    emit_updated(&app);
    Ok(())
}

fn normalize_plain(s: &str) -> String {
    let unix = s.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed: Vec<&str> = unix.lines().map(|l| l.trim_end()).collect();
    trimmed.join("\n").trim().to_string()
}

/// Закрепить/открепить элемент.
#[tauri::command]
pub fn set_pinned(
    app: AppHandle,
    state: State<'_, Shared>,
    id: i64,
    pinned: bool,
) -> CmdResult<()> {
    {
        let conn = state.db.lock().unwrap();
        repo::set_pinned(&conn, id, pinned).map_err(|e| e.to_string())?;
    }
    emit_updated(&app);
    Ok(())
}

/// Удалить элемент (и его файл-картинку, если есть).
#[tauri::command]
pub fn delete_item(app: AppHandle, state: State<'_, Shared>, id: i64) -> CmdResult<()> {
    let image = {
        let conn = state.db.lock().unwrap();
        repo::delete(&conn, id).map_err(|e| e.to_string())?
    };
    if let Some(file) = image {
        let _ = std::fs::remove_file(state.images_dir.join(file));
    }
    emit_updated(&app);
    Ok(())
}

/// Очистить историю. `keep_pinned` = true оставляет закреплённые.
#[tauri::command]
pub fn clear_history(
    app: AppHandle,
    state: State<'_, Shared>,
    keep_pinned: bool,
) -> CmdResult<()> {
    let removed = {
        let conn = state.db.lock().unwrap();
        repo::clear(&conn, keep_pinned).map_err(|e| e.to_string())?
    };
    for f in removed {
        let _ = std::fs::remove_file(state.images_dir.join(f));
    }
    emit_updated(&app);
    Ok(())
}

/// Переключить паузу записи. Возвращает новое состояние (true = на паузе).
#[tauri::command]
pub fn toggle_pause(state: State<'_, Shared>) -> bool {
    let new = !state.paused.load(Ordering::Relaxed);
    state.paused.store(new, Ordering::Relaxed);
    new
}

/// Текущее состояние паузы.
#[tauri::command]
pub fn get_pause(state: State<'_, Shared>) -> bool {
    state.paused.load(Ordering::Relaxed)
}

/// Отдаёт картинку элемента как data-URL (base64) для показа в webview.
#[tauri::command]
pub fn item_image_data_url(state: State<'_, Shared>, id: i64) -> CmdResult<Option<String>> {
    let file = {
        let conn = state.db.lock().unwrap();
        match repo::get_by_id(&conn, id).map_err(|e| e.to_string())? {
            Some(item) if item.kind == KIND_IMAGE => item.image_path,
            _ => None,
        }
    };
    let Some(file) = file else { return Ok(None) };
    let bytes = match std::fs::read(state.images_dir.join(file)) {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };
    Ok(Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes))))
}

/// Прячет окно истории (Esc / потеря фокуса / после выбора элемента).
#[tauri::command]
pub fn hide_window(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

/// Открывает (создаёт лениво) окно настроек.
///
/// Команда `async`, чтобы выполняться на воркер-потоке, а не в главном.
/// При первом обращении внутри вызывается `WebviewWindowBuilder::build()`,
/// который диспатчится в главный поток и блокирует вызывающий поток до
/// готовности окна. Синхронная команда исполняется как раз в главном потоке —
/// он ждёт сам себя, event loop встаёт, оба окна виснут («Не отвечает»).
/// Async снимает нагрузку с главного потока и даёт `build()` завершиться.
#[tauri::command]
pub async fn open_settings(app: AppHandle) {
    crate::window::show_settings(&app);
}

/// Текущий статус лицензии (Pro + email).
#[tauri::command]
pub fn get_license(state: State<'_, Shared>) -> LicenseInfo {
    LicenseInfo {
        pro: state.is_pro(),
        email: state.license_email.lock().unwrap().clone(),
    }
}

/// Активация лицензии: проверка подписи → сохранение → обновление Pro-гейта.
#[tauri::command]
pub fn activate_license(
    app: AppHandle,
    state: State<'_, Shared>,
    key: String,
) -> CmdResult<LicenseInfo> {
    let info = license::activate(&key)?;
    state.is_pro.store(info.pro, Ordering::Relaxed);
    *state.license_email.lock().unwrap() = info.email.clone();
    crate::hotkey::register_quickpaste(&app);
    let _ = app.emit("license-changed", info.clone());
    Ok(info)
}

/// Деактивация: удалить сохранённый ключ, сбросить Pro-гейт.
#[tauri::command]
pub fn deactivate_license(app: AppHandle, state: State<'_, Shared>) -> CmdResult<()> {
    license::deactivate()?;
    state.is_pro.store(false, Ordering::Relaxed);
    *state.license_email.lock().unwrap() = None;
    crate::hotkey::unregister_quickpaste(&app);
    let _ = app.emit("license-changed", LicenseInfo::free());
    Ok(())
}

/// Сырой сохранённый ключ (для «Экспорт лицензии»).
#[tauri::command]
pub fn export_license() -> Option<String> {
    license::stored_key()
}

/// Представление настроек для фронтенда (без хеша мастер-пароля).
#[derive(Serialize)]
pub struct SettingsView {
    pub auto_update: bool,
    pub open_hotkey: String,
    pub ignore_apps: Vec<String>,
    pub window_memory: bool,
    pub font_size: u32,
    pub compact_mode: bool,
    pub max_age_days: u32,
    pub auto_lock_min: u32,
    pub has_master: bool,
    pub slots: Vec<Option<i64>>,
    /// Portable-версия: обновление вручную (zip), а не через установщик.
    pub portable: bool,
    /// Где лежат данные (показываем в настройках).
    pub data_dir: String,
}

impl SettingsView {
    fn from(s: &Settings) -> Self {
        SettingsView {
            auto_update: s.auto_update,
            open_hotkey: s.open_hotkey.clone(),
            ignore_apps: s.ignore_apps.clone(),
            window_memory: s.window_memory,
            font_size: s.font_size,
            compact_mode: s.compact_mode,
            max_age_days: s.max_age_days,
            auto_lock_min: s.auto_lock_min,
            has_master: s.master_hash.is_some(),
            slots: s.slots.clone(),
            portable: crate::paths::is_portable(),
            data_dir: crate::paths::data_dir().display().to_string(),
        }
    }
}

/// Текущие настройки (для UI).
#[tauri::command]
pub fn get_settings(state: State<'_, Shared>) -> SettingsView {
    SettingsView::from(&state.settings.lock().unwrap())
}

/// Тумблер авто-проверки обновлений («режим паранойи» = false).
#[tauri::command]
pub fn set_auto_update(state: State<'_, Shared>, enabled: bool) -> CmdResult<()> {
    let mut s = state.settings.lock().unwrap();
    s.auto_update = enabled;
    s.save()
}

// ── Фаза 2: источник, игнор, экспорт, хоткей ─────────────

/// Смена хоткея вызова окна (Free, 2.5).
#[tauri::command]
pub fn set_open_hotkey(app: AppHandle, hotkey: String) -> CmdResult<()> {
    crate::hotkey::set_open_hotkey(&app, &hotkey)
}

/// Список приложений-исключений (Free/безопасность, 2.3). Имена exe, нижний регистр.
#[tauri::command]
pub fn set_ignore_apps(state: State<'_, Shared>, apps: Vec<String>) -> CmdResult<()> {
    let apps: Vec<String> = apps
        .iter()
        .map(|a| a.trim().to_lowercase())
        .filter(|a| !a.is_empty())
        .collect();
    let mut s = state.settings.lock().unwrap();
    s.ignore_apps = apps;
    s.save()
}

/// Список приложений-источников для фасета (4.2).
#[tauri::command]
pub fn list_sources(state: State<'_, Shared>) -> CmdResult<Vec<String>> {
    let conn = state.db.lock().unwrap();
    repo::distinct_sources(&conn).map_err(|e| e.to_string())
}

/// Экспорт истории (Free, 2.4). format: "json" | "txt".
#[tauri::command]
pub fn export_history(state: State<'_, Shared>, format: String) -> CmdResult<String> {
    let items = {
        let conn = state.db.lock().unwrap();
        repo::list(&conn, "all", 100_000, 0).map_err(|e| e.to_string())?
    };
    if format == "json" {
        serde_json::to_string_pretty(&items).map_err(|e| e.to_string())
    } else {
        let mut out = String::new();
        for it in &items {
            if it.kind == KIND_IMAGE {
                out.push_str(&format!("[image {}]\n", it.image_path.as_deref().unwrap_or("")));
            } else if let Some(c) = &it.content {
                out.push_str(c);
                out.push('\n');
            }
            out.push_str("----\n");
        }
        Ok(out)
    }
}

#[derive(Deserialize)]
pub struct ImportItem {
    #[serde(rename = "type")]
    kind: String,
    content: Option<String>,
}

/// Импорт истории из JSON (Pro, 4.1). Картинки пропускаются (нет файлов).
#[tauri::command]
pub fn import_history(app: AppHandle, state: State<'_, Shared>, json: String) -> CmdResult<usize> {
    let items: Vec<ImportItem> =
        serde_json::from_str(&json).map_err(|e| format!("неверный JSON: {e}"))?;
    let mut count = 0usize;
    {
        let conn = state.db.lock().unwrap();
        for it in items {
            if it.kind != KIND_TEXT && it.kind != KIND_FILES {
                continue;
            }
            let content = match it.content {
                Some(c) if !c.trim().is_empty() => c,
                _ => continue,
            };
            let prefix = if it.kind == KIND_FILES { "f:" } else { "t:" };
            let hash = reader::sha_hex(prefix, content.as_bytes());
            let preview: String = content.chars().take(200).collect();
            let ni = NewItem {
                kind: it.kind.clone(),
                content: Some(content),
                image_path: None,
                preview: Some(preview),
                mime_type: None,
                size: None,
                content_hash: hash,
                source_app: None,
            };
            if let Ok((_, inserted)) = repo::insert_or_bump(&conn, &ni) {
                if inserted {
                    count += 1;
                }
            }
        }
    }
    emit_updated(&app);
    Ok(count)
}

// ── Фаза 3.4: настройки окна/шрифта (Pro) ────────────────

#[tauri::command]
pub fn set_font_size(state: State<'_, Shared>, size: u32) -> CmdResult<()> {
    let mut s = state.settings.lock().unwrap();
    s.font_size = size.clamp(11, 22);
    s.save()
}

#[tauri::command]
pub fn set_window_memory(state: State<'_, Shared>, enabled: bool) -> CmdResult<()> {
    let mut s = state.settings.lock().unwrap();
    s.window_memory = enabled;
    s.save()
}

#[tauri::command]
pub fn set_compact_mode(state: State<'_, Shared>, enabled: bool) -> CmdResult<()> {
    let mut s = state.settings.lock().unwrap();
    s.compact_mode = enabled;
    s.save()
}

/// Сохранить геометрию окна (вызывается фронтендом при перемещении/ресайзе, если включено).
#[tauri::command]
pub fn save_window_geometry(
    state: State<'_, Shared>,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
) -> CmdResult<()> {
    let mut s = state.settings.lock().unwrap();
    if !s.window_memory {
        return Ok(());
    }
    s.win_geometry = Some(WinGeom { x, y, w, h });
    s.save()
}

// ── Фаза 4: автоочистка, категории/теги, фасеты ──────────

#[tauri::command]
pub fn set_max_age_days(app: AppHandle, state: State<'_, Shared>, days: u32) -> CmdResult<()> {
    {
        let mut s = state.settings.lock().unwrap();
        s.max_age_days = days;
        s.save()?;
    }
    run_age_cleanup(&app, state.inner());
    Ok(())
}

/// Немедленно применяет автоочистку по возрасту (если max_age_days > 0).
pub(crate) fn run_age_cleanup(app: &AppHandle, state: &Shared) {
    let days = state.settings.lock().unwrap().max_age_days;
    if days == 0 {
        return;
    }
    let cutoff = crate::models::now_ms() - (days as i64) * 86_400_000;
    let removed = {
        let conn = state.db.lock().unwrap();
        repo::cleanup_by_age(&conn, cutoff).unwrap_or_default()
    };
    for f in removed {
        let _ = std::fs::remove_file(state.images_dir.join(f));
    }
    emit_updated(app);
}

#[tauri::command]
pub fn set_category(
    app: AppHandle,
    state: State<'_, Shared>,
    id: i64,
    category: Option<String>,
) -> CmdResult<()> {
    let category = category.filter(|c| !c.trim().is_empty());
    {
        let conn = state.db.lock().unwrap();
        repo::set_category(&conn, id, category.as_deref()).map_err(|e| e.to_string())?;
    }
    emit_updated(&app);
    Ok(())
}

#[tauri::command]
pub fn set_tags(
    app: AppHandle,
    state: State<'_, Shared>,
    id: i64,
    tags: Option<String>,
) -> CmdResult<()> {
    let norm = tags.map(|t| normalize_tags(&t)).filter(|t| !t.is_empty());
    {
        let conn = state.db.lock().unwrap();
        repo::set_tags(&conn, id, norm.as_deref()).map_err(|e| e.to_string())?;
    }
    emit_updated(&app);
    Ok(())
}

fn normalize_tags(t: &str) -> String {
    let mut tags: Vec<String> = t
        .split([',', ' ', '\n', '\t'])
        .map(|x| x.trim().to_lowercase())
        .filter(|x| !x.is_empty())
        .collect();
    tags.dedup();
    tags.join(",")
}

#[derive(Deserialize, Default)]
pub struct SearchFilters {
    pub text: Option<String>,
    pub source: Option<String>,
    pub category: Option<String>,
    pub tag: Option<String>,
    pub since_ms: Option<i64>,
    pub kind: Option<String>,
    pub regex: Option<String>,
    pub pinned_only: Option<bool>,
}

/// Фасетный поиск (Pro, 4.2): источник/категория/тег/дата/тип + опц. regex по содержимому.
#[tauri::command]
pub fn search_advanced(
    state: State<'_, Shared>,
    filters: SearchFilters,
    limit: i64,
) -> CmdResult<Vec<ClipItem>> {
    let f = repo::Filters {
        text: filters.text,
        source: filters.source,
        category: filters.category,
        tag: filters.tag,
        since_ms: filters.since_ms,
        kind: filters.kind,
        pinned_only: filters.pinned_only.unwrap_or(false),
    };
    let items = {
        let conn = state.db.lock().unwrap();
        repo::query_items(&conn, &f, limit).map_err(|e| e.to_string())?
    };
    match filters.regex.filter(|r| !r.trim().is_empty()) {
        Some(rx) => {
            let re = regex::Regex::new(&rx).map_err(|e| format!("regex: {e}"))?;
            Ok(items
                .into_iter()
                .filter(|it| it.content.as_deref().map(|c| re.is_match(c)).unwrap_or(false))
                .collect())
        }
        None => Ok(items),
    }
}

// ── Фаза 4.5: картинки Pro ───────────────────────────────

/// Полный путь к файлу картинки на диске (для «Копировать путь»/сохранения).
#[tauri::command]
pub fn item_image_path(state: State<'_, Shared>, id: i64) -> CmdResult<Option<String>> {
    let conn = state.db.lock().unwrap();
    match repo::get_by_id(&conn, id).map_err(|e| e.to_string())? {
        Some(it) if it.kind == KIND_IMAGE => Ok(it
            .image_path
            .map(|f| state.images_dir.join(f).to_string_lossy().to_string())),
        _ => Ok(None),
    }
}

// ── Фаза 5.2: мастер-пароль ──────────────────────────────

#[tauri::command]
pub fn has_master_password(state: State<'_, Shared>) -> bool {
    state.settings.lock().unwrap().master_hash.is_some()
}

#[tauri::command]
pub fn verify_master_password(state: State<'_, Shared>, password: String) -> bool {
    let hash = state.settings.lock().unwrap().master_hash.clone();
    match hash {
        Some(h) => crypto::verify_password(&password, &h),
        None => true,
    }
}

/// Установить/сменить/снять мастер-пароль. new=None снимает защиту.
#[tauri::command]
pub fn set_master_password(
    state: State<'_, Shared>,
    current: Option<String>,
    new: Option<String>,
) -> CmdResult<()> {
    let mut s = state.settings.lock().unwrap();
    if let Some(existing) = s.master_hash.clone() {
        let ok = current
            .as_deref()
            .map(|c| crypto::verify_password(c, &existing))
            .unwrap_or(false);
        if !ok {
            return Err("неверный текущий пароль".into());
        }
    }
    match new {
        Some(pw) if !pw.is_empty() => s.master_hash = Some(crypto::hash_password(&pw)),
        _ => {
            s.master_hash = None;
            s.auto_lock_min = 0;
        }
    }
    s.save()
}

#[tauri::command]
pub fn set_auto_lock(state: State<'_, Shared>, minutes: u32) -> CmdResult<()> {
    let mut s = state.settings.lock().unwrap();
    s.auto_lock_min = minutes;
    s.save()
}

// ── Фаза 5.3: слоты ──────────────────────────────────────

fn ensure_slots(s: &mut Settings) {
    if s.slots.len() < 10 {
        s.slots.resize(10, None);
    }
}

#[tauri::command]
pub fn get_slots(state: State<'_, Shared>) -> Vec<Option<i64>> {
    let mut s = state.settings.lock().unwrap();
    ensure_slots(&mut s);
    s.slots.clone()
}

#[tauri::command]
pub fn set_slot(state: State<'_, Shared>, index: usize, id: Option<i64>) -> CmdResult<()> {
    if index >= 10 {
        return Err("неверный слот".into());
    }
    let mut s = state.settings.lock().unwrap();
    ensure_slots(&mut s);
    s.slots[index] = id;
    s.save()
}

/// Восстановить слот в буфер обмена.
#[tauri::command]
pub fn restore_slot(app: AppHandle, state: State<'_, Shared>, index: usize) -> CmdResult<()> {
    let id = {
        let mut s = state.settings.lock().unwrap();
        ensure_slots(&mut s);
        s.slots.get(index).copied().flatten()
    };
    let id = id.ok_or_else(|| "слот пуст".to_string())?;
    put_item_in_clipboard(state.inner(), id)?;
    emit_updated(&app);
    Ok(())
}

// ── Фаза 5.4: быстрая вставка N-го ───────────────────────

/// Кладёт N-й (0-based) самый свежий элемент в буфер. Вызывается из глоб. хоткея.
pub(crate) fn quick_paste_nth(app: &AppHandle, index: usize) {
    let state = app.state::<Shared>();
    let id = {
        let conn = state.db.lock().unwrap();
        match repo::list(&conn, "all", (index as i64) + 1, 0) {
            Ok(items) => items.get(index).map(|it| it.id),
            Err(_) => None,
        }
    };
    if let Some(id) = id {
        if put_item_in_clipboard(state.inner(), id).is_ok() {
            emit_updated(app);
        }
    }
}
