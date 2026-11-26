//! State shared between the tray (main thread) and the presence thread: flags and the status line.

use std::sync::atomic::{AtomicBool, AtomicIsize};
use std::sync::Mutex;

pub const REPO_URL: &str = "https://github.com/c4k3ss/GTA-VI-RPC";

/// Set by Quit, by `--exit-with-game` or by an update restart: everyone winds down.
pub static STOP: AtomicBool = AtomicBool::new(false);
/// "Pause presence" in the tray: keep running, show nothing on Discord.
pub static PAUSED: AtomicBool = AtomicBool::new(false);
/// "Check for updates" in the tray: the presence thread checks on its next turn.
pub static CHECK_NOW: AtomicBool = AtomicBool::new(false);
/// True while the exe is being swapped by the updater; Quit waits for it to clear.
pub static UPDATING: AtomicBool = AtomicBool::new(false);
/// The single-instance mutex handle, released by the updater before it starts the new exe.
pub static INSTANCE_MUTEX: AtomicIsize = AtomicIsize::new(0);
/// One line for the tray tooltip: what the tool is doing right now.
pub static STATUS: Mutex<String> = Mutex::new(String::new());

pub fn set_status(s: impl Into<String>) {
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = s.into();
}

pub fn status() -> String {
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
