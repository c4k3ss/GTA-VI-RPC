//! Lines go to the console when there is one, and to a buffer of the last 300 so a console opened later
//! (the tray's "Show console") can show what happened before.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::Mutex;

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

pub fn say(line: impl Into<String>) {
    let line = line.into();
    let mut l = LINES.lock().unwrap_or_else(|e| e.into_inner());
    let _ = writeln!(std::io::stdout(), "{line}");
    if l.len() >= 300 {
        l.pop_front();
    }
    l.push_back(line);
}

/// Serialize console changes with writes; replay only when a new console was created.
pub fn change_console(change: impl FnOnce() -> bool) {
    let l = LINES.lock().unwrap_or_else(|e| e.into_inner());
    if change() {
        for line in l.iter() {
            let _ = writeln!(std::io::stdout(), "{line}");
        }
    }
}
