//! State shared between the tray (main thread) and the presence thread: flags and the status line.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;

pub const REPO_URL: &str = "https://github.com/c4k3ss/GTA-VI-RPC";

/// Set by Quit, by `--exit-with-game` or by an update restart: everyone winds down.
pub static STOP: AtomicBool = AtomicBool::new(false);
/// "Pause presence" in the tray: keep running, show nothing on Discord.
pub static PAUSED: AtomicBool = AtomicBool::new(false);
/// "Check for updates" in the tray: the presence thread checks on its next turn.
pub static CHECK_NOW: AtomicBool = AtomicBool::new(false);
/// Held by the updater while the exe is swapped and by Quit before it exits, so an exit never lands between
/// the two renames; the updater checks `STOP` once it holds it.
pub static SWAP: Mutex<()> = Mutex::new(());
/// The single-instance mutex handle, released by the updater before it starts the new exe.
pub static INSTANCE_MUTEX: AtomicIsize = AtomicIsize::new(0);

/// Takes the single-instance mutex; false when another copy holds it (or the call failed).
pub fn claim_instance() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "vi-rpc-single-instance\0".encode_utf16().collect();
    // SAFETY: a plain Win32 call with a static name; the handle is kept for the life of the process.
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if h.is_null() {
            return false;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(h);
            return false;
        }
        INSTANCE_MUTEX.store(h as isize, Ordering::SeqCst);
        true
    }
}

/// Lets go of the single-instance mutex (the updater, just before the new exe starts).
pub fn release_instance() {
    let h = INSTANCE_MUTEX.swap(0, Ordering::SeqCst);
    if h != 0 {
        // SAFETY: the handle claim_instance() kept; closed exactly once here.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(h as _) };
    }
}
/// One line for the tray tooltip: what the tool is doing right now.
pub static STATUS: Mutex<String> = Mutex::new(String::new());

pub fn set_status(s: impl Into<String>) {
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = s.into();
}

pub fn status() -> String {
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
