//! The tray icon: the console window is hidden at start and the menu shows or hides it, or quits. The icon is
//! the exe's own (resource 1, embedded by build.rs). Runs on the main thread with a plain Win32 message pump.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows_sys::Win32::System::Console::{AllocConsole, AttachConsole, FreeConsole, GetConsoleMode, GetConsoleWindow, GetStdHandle, SetConsoleTitleW, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};

/// The scanner commands print a lot: use the console they were started from, or make one.
pub fn attach_or_alloc_console() {
    // SAFETY: plain Win32 console calls.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            AllocConsole();
        }
    }
}

/// A console exists only while it is shown: made on demand (with the lines so far), freed when hidden.
fn show_console(show: bool) {
    // SAFETY: plain Win32 console calls, serialized with log writes and replay.
    crate::log::change_console(|| unsafe {
        if show {
            if GetConsoleWindow().is_null() && AllocConsole() != 0 {
                let title: Vec<u16> = format!("GTA VI RPC {}\0", crate::update::VERSION).encode_utf16().collect();
                SetConsoleTitleW(title.as_ptr());
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

/// Runs until Quit is chosen or `stop` is set by the presence thread (an update restart). Returns on quit.
pub fn run(stop: Arc<AtomicBool>, start_hidden: bool) {
    let mut visible = !start_hidden;
    show_console(visible);
    let toggle = MenuItem::new(if visible { "Hide console" } else { "Show console" }, true, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    let _ = menu.append(&toggle);
    let _ = menu.append(&quit);
    let icon = Icon::from_resource(1, None).ok();
    let mut builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(format!("GTA VI RPC {}", crate::update::VERSION));
    if let Some(i) = icon {
        builder = builder.with_icon(i);
    }
    let _tray = match builder.build() {
        Ok(t) => t,
        Err(e) => {
            crate::log::say(format!("vi-rpc: no tray icon ({e})"));
            show_console(true);
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
            }
            return;
        }
    };
    let events = MenuEvent::receiver();
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
                return;
            }
            if ev.id == toggle.id() {
                visible = !visible;
                show_console(visible);
                toggle.set_text(if visible { "Hide console" } else { "Show console" });
            }
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
