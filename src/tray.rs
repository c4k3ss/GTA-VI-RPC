//! The tray icon: the console window is hidden at start and the menu shows or hides it, or quits. The icon is
//! the exe's own (resource 1, embedded by build.rs). Runs on the main thread with a plain Win32 message pump.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows_sys::Win32::System::Console::GetConsoleWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, ShowWindow, TranslateMessage, MSG, PM_REMOVE, SW_HIDE, SW_SHOW};

fn show_console(show: bool) {
    // SAFETY: plain Win32 calls on our own console window (null when there is none).
    unsafe {
        let w = GetConsoleWindow();
        if !w.is_null() {
            ShowWindow(w, if show { SW_SHOW } else { SW_HIDE });
        }
    }
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
            eprintln!("vi-rpc: no tray icon ({e}); the console stays");
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
