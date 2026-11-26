//! Lines go to the console when there is one, and to a buffer of the last 300 so a console opened later
//! (the tray's "Show console") can show what happened before.

use std::collections::VecDeque;
use std::sync::Mutex;

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

pub fn say(line: impl Into<String>) {
    let line = line.into();
    println!("{line}");
    let mut l = LINES.lock().unwrap_or_else(|e| e.into_inner());
    if l.len() >= 300 {
        l.pop_front();
    }
    l.push_back(line);
}

pub fn replay() {
    let l = LINES.lock().unwrap_or_else(|e| e.into_inner());
    for line in l.iter() {
        println!("{line}");
    }
}
