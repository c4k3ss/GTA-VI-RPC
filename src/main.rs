//! vi-rpc: Discord Rich Presence for the Xbox build of GTA VI running under a VIctory-lineage layer.
//!
//! Stage 1 (this file): everything the layer gives away for free. Its window title reads
//! `Americas | VIctory 8 fps 129.9 ms/frame 7577 draws/frame`; the draw count tells menus (tens) from the
//! world (thousands) and loading (a handful). Stage 2 adds the title's own state (mission, scene, clock,
//! position) read from the process's memory once their places are known (see re/).
//!
//! Config: `vi-rpc.toml` next to the exe, `app_id = "<Discord application id>"`.

#![windows_subsystem = "windows"]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use serde::Deserialize;

mod layer;
mod log;
mod scan;
mod state;
mod title;
mod tray;
mod update;

#[derive(Deserialize)]
struct Config {
    app_id: String,
    /// Seconds between presence updates (Discord allows one per 15 s).
    #[serde(default = "default_interval")]
    interval: u64,
    /// Fetch and install newer releases from GitHub (on start and every six hours).
    #[serde(default = "default_true")]
    auto_update: bool,
    /// Start with the console hidden (the tray menu shows it); `--console` on the command line overrides.
    #[serde(default = "default_true")]
    hide_console: bool,
}

fn default_true() -> bool {
    true
}

fn default_interval() -> u64 {
    15
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| !a.starts_with("--")) {
        // a scanner command: write into the console this was started from (a .bat window), else make one
        tray::attach_or_alloc_console();
    }
    match args.first().map(String::as_str) {
        Some("scan") => return scan::run(&args[1..]),
        Some("find") | Some("clock") | Some("text") => return scan::find(&args[1..]),
        Some("chase") => return scan::chase(&args[1..]),
        Some("peek") => return scan::peek(&args[1..]),
        Some("snap") => return scan::snap(&args[1..]),
        Some("diff") => return scan::diff(&args[1..]),
        Some("boxes") => return scan::boxes(&args[1..]),
        Some("udiff") => return scan::udiff(&args[1..]),
        _ => {}
    }
    // The project's own Discord application (its name and art are what Discord shows); one application serves
    // every user, so no setup is needed. `vi-rpc.toml` next to the exe overrides it for people with their own.
    const DEFAULT_APP_ID: &str = "1575527746090565632";
    let exit_with_game = args.iter().any(|a| a == "--exit-with-game");
    // one instance at a time: a named mutex held for the life of the process
    // SAFETY: a plain Win32 call with a static name; the handle is never closed on purpose.
    unsafe {
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let name: Vec<u16> = "vi-rpc-single-instance\0".encode_utf16().collect();
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if h.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
            log::say(format!("vi-rpc: already running"));
            return;
        }
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let cfg_path = exe.with_file_name("vi-rpc.toml");
    let cfg: Config = match std::fs::read_to_string(&cfg_path).ok().and_then(|t| toml::from_str(&t).ok()) {
        Some(c) => c,
        None => Config { app_id: DEFAULT_APP_ID.to_string(), interval: default_interval(), auto_update: true, hide_console: true },
    };
    log::say(format!("vi-rpc {}", update::VERSION));
    update::clean_old();
    let start_hidden = cfg.hide_console && !args.iter().any(|a| a == "--console");
    let stop = Arc::new(AtomicBool::new(false));
    let worker = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            presence(cfg, exit_with_game, stop.clone());
            stop.store(true, Ordering::Relaxed);
        })
    };
    // the tray runs here; the console starts hidden unless --console was given
    tray::run(stop.clone(), start_hidden);
    if !stop.load(Ordering::Relaxed) {
        log::say(format!("vi-rpc: quit from the tray"));
        std::process::exit(0);
    }
    let _ = worker.join();
}

/// The presence loop: finds the game, talks to Discord, checks for updates. Returns when the game is gone
/// (`--exit-with-game`) or an update restarted the program.
fn presence(cfg: Config, exit_with_game: bool, stop: Arc<AtomicBool>) {
    let mut next_update_check = Instant::now();
    let mut client: Option<DiscordIpcClient> = None;
    let mut started: Option<Instant> = None;
    let mut last_shown = String::new();
    let mut seen_game = false;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if cfg.auto_update && Instant::now() >= next_update_check {
            next_update_check = Instant::now() + Duration::from_secs(6 * 3600);
            if update::check_and_apply() {
                if let Some(c) = client.as_mut() {
                    let _ = c.close();
                }
                return;
            }
        }
        let frame = layer::find();
        match frame {
            None => {
                if client.is_some() {
                    log::say(format!("vi-rpc: the game is gone; clearing presence"));
                    if let Some(c) = client.as_mut() {
                        let _ = c.clear_activity();
                        let _ = c.close();
                    }
                    client = None;
                    started = None;
                }
                if exit_with_game && seen_game {
                    log::say(format!("vi-rpc: exiting with the game (--exit-with-game)"));
                    return;
                }
            }
            Some(f) => {
                seen_game = true;
                if client.is_none() {
                    let mut c = DiscordIpcClient::new(&cfg.app_id).expect("client");
                    match c.connect() {
                        Ok(()) => {
                            log::say(format!("vi-rpc: connected to Discord"));
                            client = Some(c);
                            started = Some(Instant::now());
                        }
                        Err(e) => log::say(format!("vi-rpc: Discord not reachable ({e}); retrying")),
                    }
                }
                if let (Some(c), Some(t0)) = (client.as_mut(), started) {
                    // the title's own clock, when this build's layout is known and the world is up
                    let title = if f.scene() == layer::Scene::World { title::Title::attach() } else { None };
                    let clock = title.as_ref().and_then(|t| t.clock());
                    let camera = title.as_ref().and_then(|t| t.camera());
                    // the coordinates stay out of the presence (they are logged below for debugging) until the
                    // place name is read from the title
                    let told = state::read().unwrap_or_default();
                    let place = told.place_line().unwrap_or_else(|| "Somewhere in Leonida".to_string());
                    let state = match clock {
                        Some(k) => format!("{place} · {}", k.text_12h()),
                        None if told.place_line().is_some() => place,
                        None => f.state(),
                    };
                    if let Some(p) = camera {
                        log::say(format!("vi-rpc: camera at ({:.1}, {:.1}, {:.1})", p[0], p[1], p[2]));
                    }
                    let details = match (&told.mission, f.scene()) {
                        (Some(m), layer::Scene::World) => m.clone(),
                        _ => f.details(),
                    };
                    let shown = format!("{details} | {state}");
                    let start = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64 - t0.elapsed().as_secs() as i64)
                        .unwrap_or(0);
                    let act = activity::Activity::new()
                        .details(&details)
                        .state(&state)
                        .assets(activity::Assets::new().large_image("game").large_text("Grand Theft Auto VI"))
                        .timestamps(activity::Timestamps::new().start(start));
                    match c.set_activity(act) {
                        Ok(()) => {
                            if shown != last_shown {
                                log::say(format!("vi-rpc: {shown}"));
                                last_shown = shown;
                            }
                        }
                        Err(e) => {
                            log::say(format!("vi-rpc: update failed ({e}); reconnecting"));
                            client = None;
                        }
                    }
                }
            }
        }
        for _ in 0..cfg.interval.max(5) * 4 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}
