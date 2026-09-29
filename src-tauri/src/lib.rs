//! ClipVault — фоновый менеджер истории буфера обмена.

mod clipboard;
mod commands;
mod crypto;
mod db;
mod hotkey;
mod license;
mod models;
mod paste;
mod paths;
mod settings;
mod source;
mod state;
mod tray;
mod window;

use tauri::Manager;

use state::Shared;

/// Инициализирует файловое логирование в <папка данных>\logs\app.log.
fn init_logging() {
    let appender = tracing_appender::rolling::never(paths::logs_dir(), "app.log");
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(appender)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Каталоги данных и логи — до всего остального (ТЗ §31).
    // Без них работать нечем: показываем причину и выходим, а не падаем молча
    // (типичный случай — portable-копия в Program Files или на read-only диске).
    if let Err(e) = paths::ensure_dirs() {
        let hint = if paths::is_portable() {
            "\n\nPortable-версии нужна папка, куда можно писать: перенесите её, \
             например, в Документы или на флешку."
        } else {
            ""
        };
        fatal_error(&format!(
            "Не удалось создать папку данных:\n{}\n\n{e}{hint}",
            paths::data_dir().display()
        ));
        std::process::exit(1);
    }
    init_logging();
    tracing::info!(
        portable = paths::is_portable(),
        data_dir = %paths::data_dir().display(),
        "ClipVault starting"
    );
    ensure_webview2();

    tauri::Builder::default()
        // single-instance должен быть зарегистрирован первым.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show_history(app);
        }))
        .plugin(hotkey::plugin())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // Автообновление через S3 (+ process для перезапуска) и открытие внешних ссылок.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Окно истории создаём сами (в конфиге create: false), чтобы в portable
            // увести профиль WebView2 из %LOCALAPPDATA% в папку данных.
            window::create_main(&handle)?;

            // Настройки и лицензия (офлайн-проверка Ed25519) — до создания Shared.
            let user_settings = settings::Settings::load();
            let license = license::load();
            if license.pro {
                tracing::info!("Pro license active");
            }

            // База данных + разделяемое состояние.
            let conn = db::open(&paths::db_path())
                .map_err(|e| format!("не удалось открыть БД: {e}"))?;
            // Геометрия окна для восстановления (Pro, 3.4) — снимаем до move в Shared.
            let restore_geom = if user_settings.window_memory {
                user_settings.win_geometry.clone()
            } else {
                None
            };

            let shared = Shared::new(conn, paths::images_dir(), user_settings, license);
            let is_pro_startup = shared.is_pro();
            app.manage(shared.clone());

            // Восстановить позицию/размер окна истории.
            if let (Some(g), Some(win)) = (&restore_geom, app.get_webview_window("main")) {
                let _ = win.set_position(tauri::PhysicalPosition::new(g.x, g.y));
                let _ = win.set_size(tauri::PhysicalSize::new(g.w, g.h));
            }

            // Автоочистка по возрасту (Pro, 4.1), если настроена.
            commands::run_age_cleanup(&handle, &shared);

            // Слушатель буфера обмена (нативный, без polling).
            clipboard::listener::spawn(shared, handle.clone());

            // Автозапуск: включаем по умолчанию при самом первом запуске
            // (дальше выбор пользователя уважаем — тумблер в трее).
            // Portable — не трогаем: в реестр попал бы путь к флешке/папке, которой
            // завтра может не быть. Включить можно вручную тумблером в трее.
            let marker = paths::data_dir().join(".initialized");
            if !marker.exists() {
                if !paths::is_portable() {
                    use tauri_plugin_autostart::ManagerExt;
                    if let Err(e) = handle.autolaunch().enable() {
                        tracing::warn!("could not enable autostart on first run: {e}");
                    }
                }
                let _ = std::fs::write(&marker, b"1");
            }

            // Трей и глобальная горячая клавиша.
            tray::build(&handle)?;
            // Куда вставлять выбранное: последнее активное окно чужого приложения.
            paste::start_tracking();
            hotkey::register(&handle);
            if is_pro_startup {
                hotkey::register_quickpaste(&handle);
            }

            tracing::info!("ClipVault ready");
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Крестик на окне истории не закрывает приложение — прячем (живём в трее).
            // Окно настроек закрывается штатно.
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if window.label() == crate::window::MAIN {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
            // В релизе прячем ТОЛЬКО окно истории при потере фокуса (поведение поппера),
            // если не включено «Не прятать окно». Настройки при потере фокуса не прячем.
            #[cfg(not(debug_assertions))]
            tauri::WindowEvent::Focused(false) => {
                if window.label() == crate::window::MAIN {
                    crate::window::hide_on_blur(window);
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::search_items,
            commands::copy_item,
            commands::set_pinned,
            commands::delete_item,
            commands::clear_history,
            commands::toggle_pause,
            commands::get_pause,
            commands::item_image_data_url,
            commands::hide_window,
            commands::open_settings,
            commands::get_license,
            commands::activate_license,
            commands::deactivate_license,
            commands::export_license,
            commands::get_settings,
            commands::set_auto_update,
            commands::copy_item_plain,
            commands::set_auto_paste,
            commands::finish_pick,
            commands::set_hidden_tabs,
            commands::set_keep_open,
            commands::set_open_hotkey,
            commands::set_ignore_apps,
            commands::list_sources,
            commands::export_history,
            commands::import_history,
            commands::set_font_size,
            commands::set_window_memory,
            commands::set_compact_mode,
            commands::save_window_geometry,
            commands::set_max_age_days,
            commands::set_category,
            commands::set_tags,
            commands::search_advanced,
            commands::item_image_path,
            commands::has_master_password,
            commands::verify_master_password,
            commands::set_master_password,
            commands::set_auto_lock,
            commands::get_slots,
            commands::set_slot,
            commands::restore_slot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Официальная страница загрузки WebView2 Runtime (Evergreen).
const WEBVIEW2_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

/// Окна рисует WebView2. На Windows 11 он есть всегда, на Windows 10 приходит
/// с обновлениями, но в урезанных сборках (LTSC, «облегчённые» образы, давно
/// не обновлявшиеся офлайн-ПК) его может не быть. Проверяем до старта Tauri,
/// чтобы объяснить по-русски и сразу предложить загрузку. Если его нет, выходим.
fn ensure_webview2() {
    let err = match tauri::webview_version() {
        Ok(v) => {
            tracing::info!("WebView2 {v}");
            return;
        }
        Err(e) => e,
    };
    tracing::error!("WebView2 Runtime not found: {err}");
    let open = ask_yes_no(
        "Для работы ClipVault нужен компонент Microsoft WebView2 Runtime — \
         он рисует окна программы. На этом компьютере он не найден.\n\n\
         Установите его с сайта Microsoft (бесплатно) и запустите ClipVault снова.\n\n\
         Открыть страницу загрузки?",
    );
    if open {
        let _ = std::process::Command::new("explorer").arg(WEBVIEW2_URL).spawn();
    }
    std::process::exit(1);
}

/// Системное окно сообщения до старта Tauri: консоли у релизной сборки нет.
#[cfg(windows)]
fn message_box(text: &str, style: u32) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, title) = (wide(text), wide("ClipVault"));
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), style) }
}

#[cfg(windows)]
fn fatal_error(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK};
    message_box(text, MB_OK | MB_ICONERROR);
}

#[cfg(windows)]
fn ask_yes_no(text: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONWARNING, MB_YESNO};
    message_box(text, MB_YESNO | MB_ICONWARNING) == IDYES
}

#[cfg(not(windows))]
fn fatal_error(text: &str) {
    eprintln!("{text}");
}

#[cfg(not(windows))]
fn ask_yes_no(text: &str) -> bool {
    eprintln!("{text}");
    false
}
