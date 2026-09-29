//! Автовставка: после выбора элемента вернуть фокус окну, в котором был
//! пользователь, и нажать за него Ctrl+V.
//!
//! Окно-цель — последнее активное окно чужого приложения. Его отслеживает
//! системный хук смены активного окна (`start_tracking`), поэтому цель верна
//! и при открытии истории из трея, и в режиме «Не прятать окно», когда между
//! вставками пользователь переходит в другое поле другой программы.
//! Если цели нет или она запущена от администратора (Windows не пускает туда
//! чужой ввод), элемент просто остаётся в буфере.

use std::sync::atomic::{AtomicIsize, Ordering};
use tauri::{AppHandle, Manager};

/// HWND окна-цели (0 = нет). Хранится числом: сам HWND — указатель, не `Send`.
static TARGET: AtomicIsize = AtomicIsize::new(0);

/// Запускает отслеживание активного окна. Вызывать один раз при старте.
pub fn start_tracking() {
    TARGET.store(imp::foreground_target(), Ordering::Relaxed);
    std::thread::spawn(imp::track_foreground);
}

fn hide_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(crate::window::MAIN) {
        let _ = win.hide();
    }
}

/// Завершает выбор: при `paste` вставляет буфер в окно-цель, при `hide`
/// прячет окно истории (в режиме «Не прятать окно» оно остаётся на экране).
///
/// Вставка идёт в отдельном потоке: сначала ждём, пока пользователь отпустит
/// Enter и модификаторы (иначе Shift+Enter превратится в Ctrl+Shift+V,
/// а автоповтор Enter допишет в документ перевод строки), затем возвращаем
/// фокус цели и нажимаем Ctrl+V.
pub fn finish(app: &AppHandle, paste: bool, hide: bool) {
    let hwnd = TARGET.load(Ordering::Relaxed);
    if !paste || hwnd == 0 {
        if hide {
            hide_main(app);
        }
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        imp::wait_keys_released();
        if hide {
            hide_main(&app);
        }
        if imp::activate(hwnd) {
            imp::send_ctrl_v();
        }
    });
}

/// Вставка в текущее активное окно (быстрая вставка Alt+1..9 — окно истории
/// не открывается, фокус и так у нужного приложения).
pub fn paste_to_foreground() {
    std::thread::spawn(|| {
        imp::wait_keys_released();
        if imp::foreground_target() != 0 {
            imp::send_ctrl_v();
        }
    });
}

#[cfg(windows)]
mod imp {
    use super::{Ordering, TARGET};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VK_CONTROL, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN,
        VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetClassNameW, GetForegroundWindow, GetMessageW,
        GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow, ShowWindow,
        TranslateMessage, EVENT_SYSTEM_FOREGROUND, MSG, SW_RESTORE, WINEVENT_OUTOFCONTEXT,
        WINEVENT_SKIPOWNPROCESS,
    };

    /// Окна оболочки, куда вставлять бессмысленно (панель задач, трей).
    const SHELL_CLASSES: &[&str] = &[
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
        "NotifyIconOverflowWindow",
        "TopLevelWindowForOverflowXamlIsland",
    ];

    /// `hwnd`, если это окно чужого приложения и не панель задач; иначе 0.
    fn as_target(hwnd: HWND) -> isize {
        if hwnd.is_null() {
            return 0;
        }
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 || pid == std::process::id() {
                return 0;
            }
            let mut buf = [0u16; 64];
            let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            if SHELL_CLASSES.contains(&class.as_str()) {
                return 0;
            }
        }
        hwnd as isize
    }

    pub fn foreground_target() -> isize {
        as_target(unsafe { GetForegroundWindow() })
    }

    unsafe extern "system" fn on_foreground(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        _id_object: i32,
        _id_child: i32,
        _thread: u32,
        _time: u32,
    ) {
        let t = as_target(hwnd);
        if t != 0 {
            TARGET.store(t, Ordering::Relaxed);
        }
    }

    /// Поток с хуком смены активного окна. Свои окна хук не видит
    /// (SKIPOWNPROCESS), панель задач отсекает `as_target`.
    pub fn track_foreground() {
        unsafe {
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            if hook.is_null() {
                tracing::error!("autopaste: SetWinEventHook failed");
                return;
            }
            // OUTOFCONTEXT-хуку нужен цикл сообщений в этом потоке.
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// Делает окно активным. Право на это у нас есть: наш процесс только что
    /// получил ввод (Enter/клик). Возвращает false, если окна уже нет или
    /// Windows не отдала ему фокус.
    pub fn activate(hwnd: isize) -> bool {
        let hwnd = hwnd as HWND;
        unsafe {
            if IsWindow(hwnd) == 0 {
                return false;
            }
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
            let start = Instant::now();
            while GetForegroundWindow() != hwnd {
                if start.elapsed() > Duration::from_millis(500) {
                    tracing::warn!("autopaste: target window did not get focus");
                    return false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        // Приложению нужно время обработать смену фокуса.
        std::thread::sleep(Duration::from_millis(40));
        true
    }

    fn is_down(vk: u16) -> bool {
        unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
    }

    /// Ждёт отпускания Enter и модификаторов (не дольше 2 с).
    pub fn wait_keys_released() {
        let held = [VK_RETURN, VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN];
        let start = Instant::now();
        while held.iter().any(|&vk| is_down(vk)) && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn key(vk: u16, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    pub fn send_ctrl_v() {
        const VK_V: u16 = 0x56;
        let inputs = [
            key(VK_CONTROL, false),
            key(VK_V, false),
            key(VK_V, true),
            key(VK_CONTROL, true),
        ];
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize != inputs.len() {
            // Чаще всего — окно администратора (UIPI). Элемент остаётся в буфере.
            tracing::warn!("autopaste: SendInput sent {sent}/{}", inputs.len());
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn foreground_target() -> isize {
        0
    }
    pub fn track_foreground() {}
    pub fn activate(_hwnd: isize) -> bool {
        false
    }
    pub fn wait_keys_released() {}
    pub fn send_ctrl_v() {}
}
