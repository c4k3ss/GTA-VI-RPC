//! Lines go to the console when there is one, to `%LOCALAPPDATA%\vi-rpc\vi-rpc.log`, and to a buffer of the
//! last 300 so a console opened later (the tray's "Show console") can show what happened before.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::sync::Mutex;

struct Log {
    lines: VecDeque<String>,
    file: Option<File>,
}

static LOG: Mutex<Log> = Mutex::new(Log { lines: VecDeque::new(), file: None });

/// The log file's path, when `%LOCALAPPDATA%` is known.
pub fn path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(std::path::PathBuf::from(base).join("vi-rpc").join("vi-rpc.log"))
}

/// Opens the log file (kept across runs; rolled to `vi-rpc.log.1` past 1 MiB) and installs a panic hook that
/// writes the panic there, since a windowed app has nowhere else to show it.
pub fn open_file() {
    if let Some(p) = path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::metadata(&p).map(|m| m.len() > 1 << 20).unwrap_or(false) {
            let _ = std::fs::rename(&p, p.with_extension("log.1"));
        }
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&p).ok();
        LOG.lock().unwrap_or_else(|e| e.into_inner()).file = file;
    }
    std::panic::set_hook(Box::new(|info| {
        say(format!("vi-rpc: PANIC {info}"));
    }));
}

fn stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}Z")
}

pub fn say(line: impl Into<String>) {
    let line = line.into();
    let mut l = LOG.lock().unwrap_or_else(|e| e.into_inner());
    let _ = writeln!(std::io::stdout(), "{line}");
    if let Some(f) = l.file.as_mut() {
        let _ = writeln!(f, "{} {line}", stamp());
    }
    if l.lines.len() >= 300 {
        l.lines.pop_front();
    }
    l.lines.push_back(line);
}

/// Serialize console changes with writes; replay only when a new console was created.
pub fn change_console(change: impl FnOnce() -> bool) {
    let l = LOG.lock().unwrap_or_else(|e| e.into_inner());
    if change() {
        for line in l.lines.iter() {
            let _ = writeln!(std::io::stdout(), "{line}");
        }
    }
}
