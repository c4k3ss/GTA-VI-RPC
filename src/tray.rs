//! The tray icon and its menu: pause the presence, start with Windows, show or hide the console, check for
//! updates, open the project page, quit. The icon is the exe's own (resource 1, embedded by build.rs). Runs on
//! the main thread with a plain Win32 message pump; the tooltip shows the presence thread's status line.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows_sys::Win32::System::Console::{AllocConsole, AttachConsole, FreeConsole, GetConsoleMode, GetConsoleWindow, GetStdHandle, SetConsoleOutputCP, SetConsoleTitleW, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};

use crate::app;

/// The scanner commands print a lot: use the console they were started from, or make one.
pub fn attach_or_alloc_console() {
    // SAFETY: plain Win32 console calls.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            AllocConsole();
        }
        SetConsoleOutputCP(65001);
    }
}

/// A console exists only while it is shown: made on demand (with the lines so far), freed when hidden.
fn show_console(show: bool) {
    // SAFETY: plain Win32 console calls.
    crate::log::change_console(|| unsafe {
        if show {
            if GetConsoleWindow().is_null() && AllocConsole() != 0 {
                let title: Vec<u16> = format!("GTA VI RPC {}\0", crate::update::VERSION).encode_utf16().collect();
                SetConsoleTitleW(title.as_ptr());
                SetConsoleOutputCP(65001); // the lines are UTF-8 ("·")
                return true;
            }
        } else if !GetConsoleWindow().is_null() {
            let streams = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
            let console_handles = streams.map(|stream| {
                let mut mode = 0;
                GetConsoleMode(GetStdHandle(stream), &mut mode) != 0
            });
            if FreeConsole() != 0 {
                // Detached console handles can be reused by unrelated files. Preserve redirected streams.
                for (stream, was_console) in streams.into_iter().zip(console_handles) {
                    if was_console {
                        SetStdHandle(stream, std::ptr::null_mut());
                    }
                }
            }
        }
        false
    });
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Opens a web page in the default browser.
pub fn open_url(url: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let verb = wide("open");
    let target = wide(url);
    // SAFETY: NUL-terminated strings that outlive the call.
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_NAME: &str = "GTA VI RPC";

/// Whether this exe is registered to start with Windows (an HKCU Run value naming this exe).
pub fn starts_with_windows() -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let key = wide(RUN_KEY);
    let name = wide(RUN_NAME);
    // SAFETY: a sized buffer and NUL-terminated names.
    let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _, &mut len) } == 0;
    if !ok {
        return false;
    }
    let chars = (len as usize / 2).min(buf.len());
    let value = String::from_utf16_lossy(&buf[..chars]).trim_end_matches('\0').to_string();
    let me = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    !me.is_empty() && value.to_lowercase().contains(&me.to_lowercase())
}

/// Registers or removes the HKCU Run value for this exe.
pub fn set_start_with_windows(on: bool) -> bool {
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ};
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let key_name = wide(RUN_KEY);
    let name = wide(RUN_NAME);
    // SAFETY: NUL-terminated names and a value buffer whose byte length is passed.
    unsafe {
        let mut key = std::ptr::null_mut();
        if RegCreateKeyExW(HKEY_CURRENT_USER, key_name.as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, std::ptr::null(), &mut key, std::ptr::null_mut()) != 0 {
            return false;
        }
        let r = if on {
            let value = wide(&format!("\"{}\"", exe.to_string_lossy()));
            RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, value.as_ptr() as *const u8, (value.len() * 2) as u32)
        } else {
            RegDeleteValueW(key, name.as_ptr())
        };
        RegCloseKey(key);
        r == 0
    }
}

/// Runs until Quit is chosen or `STOP` is set by the presence thread (an update restart or
/// `--exit-with-game`). Returns true on Quit.
pub fn run(start_hidden: bool) -> bool {
    let mut visible = !start_hidden;
    show_console(visible);
    let pause = CheckMenuItem::new("Pause presence", true, app::PAUSED.load(Ordering::Relaxed), None);
    let autostart = CheckMenuItem::new("Start with Windows", true, starts_with_windows(), None);
    let toggle = MenuItem::new(if visible { "Hide console" } else { "Show console" }, true, None);
    let check = MenuItem::new("Check for updates", true, None);
    let about = MenuItem::new(format!("About GTA VI RPC {}", crate::update::VERSION), true, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    let _ = menu.append_items(&[&pause, &autostart, &PredefinedMenuItem::separator(), &toggle, &check, &about, &PredefinedMenuItem::separator(), &quit]);
    let icon = Icon::from_resource(1, None).ok();
    let mut builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(format!("GTA VI RPC {}", crate::update::VERSION));
    if let Some(i) = icon {
        builder = builder.with_icon(i);
    }
    let tray = match builder.build() {
        Ok(t) => t,
        Err(e) => {
            crate::log::say(format!("vi-rpc: no tray icon ({e}); running with the console"));
            show_console(true);
            while !app::STOP.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
            }
            return false;
        }
    };
    let events = MenuEvent::receiver();
    let mut tooltip = String::new();
    loop {
        // SAFETY: a message pump for this thread's windows (the tray icon's hidden window).
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        while let Ok(ev) = events.try_recv() {
            if ev.id == quit.id() {
                return true;
            } else if ev.id == toggle.id() {
                visible = !visible;
                show_console(visible);
                toggle.set_text(if visible { "Hide console" } else { "Show console" });
            } else if ev.id == pause.id() {
                let on = pause.is_checked();
                app::PAUSED.store(on, Ordering::Relaxed);
                crate::log::say(if on { "vi-rpc: presence paused" } else { "vi-rpc: presence resumed" });
            } else if ev.id == autostart.id() {
                let on = autostart.is_checked();
                if set_start_with_windows(on) {
                    crate::log::say(if on { "vi-rpc: will start with Windows" } else { "vi-rpc: will not start with Windows" });
                } else {
                    crate::log::say("vi-rpc: could not change the Start with Windows setting");
                    autostart.set_checked(!on);
                }
            } else if ev.id == check.id() {
                app::CHECK_NOW.store(true, Ordering::Relaxed);
            } else if ev.id == about.id() {
                open_url(app::REPO_URL);
            }
        }
        if app::STOP.load(Ordering::Relaxed) {
            return false;
        }
        let now = app::status();
        if now != tooltip {
            let text = if now.is_empty() { format!("GTA VI RPC {}", crate::update::VERSION) } else { format!("GTA VI RPC {}\n{now}", crate::update::VERSION) };
            let _ = tray.set_tooltip(Some(text));
            tooltip = now;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
