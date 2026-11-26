//! vi-rpc: Discord Rich Presence for the Xbox build of GTA VI running under a VIctory-lineage layer.
//!
//! Stage 1: everything the layer gives away for free. Its window title reads
//! `Americas | VIctory 8 fps 129.9 ms/frame 7577 draws/frame`; the draw count tells menus (tens) from the
//! world (thousands) and loading (a handful). Stage 2 adds the title's own state (clock, position, later the
//! place) read from the process's memory at offsets found with the scanner in src/scan.rs (see re/).
//!
//! Config: `vi-rpc.toml` next to the exe; every key is optional (see vi-rpc.example.toml).

#![windows_subsystem = "windows"]

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use serde::Deserialize;

mod app;
mod layer;
mod log;
mod scan;
mod state;
mod title;
mod tray;
mod update;

// The project's own Discord application (its name and art are what Discord shows); one application serves
// every user, so no setup is needed. `vi-rpc.toml` next to the exe overrides it for people with their own.
const DEFAULT_APP_ID: &str = "1575527746090565632";

#[derive(Deserialize)]
#[serde(default)]
struct Config {
    app_id: String,
    /// Seconds between presence updates (Discord allows one per 15 s).
    interval: u64,
    /// Fetch and install newer releases from GitHub (on start and every six hours).
    auto_update: bool,
    /// Start with the console hidden (the tray menu shows it); `--console` on the command line overrides.
    hide_console: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { app_id: DEFAULT_APP_ID.to_string(), interval: 15, auto_update: true, hide_console: true }
    }
}

/// Discord shows at most 128 characters per line; longer ones are rejected rather than cut.
fn fit(s: &str) -> String {
    if s.chars().count() <= 128 { s.to_string() } else { s.chars().take(127).collect::<String>() + "…" }
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
    let exit_with_game = args.iter().any(|a| a == "--exit-with-game");
    // one instance at a time: a named mutex held for the life of the process (the updater lets go of it
    // just before it starts the new exe)
    // SAFETY: a plain Win32 call with a static name.
    unsafe {
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let name: Vec<u16> = "vi-rpc-single-instance\0".encode_utf16().collect();
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if h.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
            use windows_sys::Win32::System::Console::{AttachConsole, GetConsoleWindow, ATTACH_PARENT_PROCESS};
            use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
            if GetConsoleWindow().is_null() && AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                let message: Vec<u16> = "GTA VI RPC is already running (look for its icon in the tray).\0".encode_utf16().collect();
                let caption: Vec<u16> = "GTA VI RPC\0".encode_utf16().collect();
                MessageBoxW(std::ptr::null_mut(), message.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONINFORMATION);
            }
            log::say("vi-rpc: already running");
            return;
        }
        app::INSTANCE_MUTEX.store(h as isize, Ordering::SeqCst);
    }
    log::open_file();
    let exe = std::env::current_exe().unwrap_or_default();
    let cfg_path = exe.with_file_name("vi-rpc.toml");
    let cfg: Config = match std::fs::read_to_string(&cfg_path) {
        Ok(text) => match toml::from_str(&text) {
            Ok(c) => c,
            Err(e) => {
                log::say(format!("vi-rpc: {} is not valid; using the defaults ({e})", cfg_path.display()));
                Config::default()
            }
        },
        Err(_) => Config::default(),
    };
    log::say(format!("vi-rpc {}", update::VERSION));
    if let Some(p) = log::path() {
        log::say(format!("vi-rpc: log file {}", p.display()));
    }
    let start_hidden = cfg.hide_console && !args.iter().any(|a| a == "--console");
    let worker = std::thread::spawn(move || {
        presence(cfg, exit_with_game);
        app::STOP.store(true, Ordering::Relaxed);
    });
    // the tray runs here; the console starts hidden unless --console was given
    let quit = tray::run(start_hidden);
    if quit {
        app::STOP.store(true, Ordering::Relaxed);
        // never cut an exe swap in half
        let t0 = Instant::now();
        while app::UPDATING.load(Ordering::SeqCst) && t0.elapsed() < Duration::from_secs(60) {
            std::thread::sleep(Duration::from_millis(100));
        }
        log::say("vi-rpc: quit from the tray");
        std::process::exit(0);
    }
    let _ = worker.join();
}

/// The presence loop: finds the game, talks to Discord, checks for updates. Returns when the game is gone
/// (`--exit-with-game`), an update restarted the program, or Quit was chosen.
fn presence(cfg: Config, exit_with_game: bool) {
    let t_start = Instant::now();
    let mut next_update_check = Instant::now();
    let mut client: Option<DiscordIpcClient> = None;
    let mut started: Option<Instant> = None;
    let mut last_shown = String::new();
    let mut seen_game = false;
    let mut cleaned_old = false;
    loop {
        if app::STOP.load(Ordering::Relaxed) {
            return;
        }
        let manual = app::CHECK_NOW.swap(false, Ordering::Relaxed);
        if manual || (cfg.auto_update && Instant::now() >= next_update_check) {
            next_update_check = Instant::now() + Duration::from_secs(6 * 3600);
            app::set_status("Checking for updates");
            if update::check_and_apply(manual) {
                if let Some(c) = client.as_mut() {
                    let _ = c.clear_activity();
                    let _ = c.close();
                }
                return;
            }
        }
        if !cleaned_old && t_start.elapsed() > Duration::from_secs(120) {
            // this version has run for two minutes: the previous exe is not needed as a fallback any more
            update::clean_old();
            cleaned_old = true;
        }
        let paused = app::PAUSED.load(Ordering::Relaxed);
        let frame = if paused { None } else { layer::find() };
        match frame {
            None => {
                if client.is_some() {
                    log::say(if paused { "vi-rpc: paused; clearing presence" } else { "vi-rpc: the game is gone; clearing presence" });
                    if let Some(c) = client.as_mut() {
                        let _ = c.clear_activity();
                        let _ = c.close();
                    }
                    client = None;
                    started = None;
                    last_shown.clear();
                }
                app::set_status(if paused { "Paused" } else { "Waiting for the game" });
                if exit_with_game && seen_game && !paused {
                    log::say("vi-rpc: exiting with the game (--exit-with-game)");
                    return;
                }
            }
            Some(f) => {
                seen_game = true;
                if client.is_none() {
                    let mut c = DiscordIpcClient::new(&cfg.app_id).expect("client");
                    match c.connect() {
                        Ok(()) => {
                            log::say("vi-rpc: connected to Discord");
                            client = Some(c);
                            started = Some(Instant::now());
                        }
                        Err(e) => {
                            log::say(format!("vi-rpc: Discord not reachable ({e}); retrying"));
                            app::set_status("Game found, Discord not reachable");
                        }
                    }
                }
                if let (Some(c), Some(t0)) = (client.as_mut(), started) {
                    let told = state::read().unwrap_or_default();
                    // the layer's word on the scene beats the draw-count guess
                    let scene = match told.scene.as_deref() {
                        Some("loading") => layer::Scene::Loading,
                        Some("menu") | Some("menus") => layer::Scene::Menus,
                        Some("world") => layer::Scene::World,
                        _ => f.scene(),
                    };
                    // the title's own clock, when this build's layout is known and the world is up
                    let title = if scene == layer::Scene::World { title::Title::attach(f.pid) } else { None };
                    let clock = title.as_ref().and_then(|t| t.clock());
                    let camera = title.as_ref().and_then(|t| t.camera());
                    // the coordinates stay out of the presence (they are logged below for debugging) until the
                    // place name is read from the title
                    let place = told.place_line().unwrap_or_else(|| "Somewhere in Leonida".to_string());
                    let state = match clock {
                        Some(k) => format!("{place} · {}", k.text_12h()),
                        None if told.place_line().is_some() => place,
                        None => f.state_for(scene),
                    };
                    if let Some(p) = camera {
                        log::say(format!("vi-rpc: camera at ({:.1}, {:.1}, {:.1})", p[0], p[1], p[2]));
                    }
                    let details = match (&told.mission, scene) {
                        (Some(m), layer::Scene::World) => m.clone(),
                        _ => f.details_for(scene),
                    };
                    let (details, state) = (fit(&details), fit(&state));
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
                    // Discord answers every SET_ACTIVITY; read the answer so errors show and the pipe does not
                    // fill with unread replies
                    let sent = c.set_activity(act).and_then(|_| c.recv());
                    match sent {
                        Ok((_, reply)) if reply["evt"].as_str() == Some("ERROR") => {
                            log::say(format!("vi-rpc: Discord rejected the presence: {}", reply["data"]["message"].as_str().unwrap_or("?")));
                            app::set_status("Discord rejected the presence");
                        }
                        Ok(_) => {
                            if shown != last_shown {
                                log::say(format!("vi-rpc: {shown}"));
                                last_shown = shown;
                            }
                            app::set_status(format!("{details}\n{state}"));
                        }
                        Err(e) => {
                            log::say(format!("vi-rpc: update failed ({e}); reconnecting"));
                            client = None;
                            last_shown.clear();
                        }
                    }
                }
            }
        }
        for _ in 0..cfg.interval.clamp(5, 3600) * 4 {
            if app::STOP.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_keys_are_all_optional() {
        let c: Config = toml::from_str("auto_update = false").unwrap();
        assert!(!c.auto_update);
        assert_eq!(c.app_id, DEFAULT_APP_ID);
        assert_eq!(c.interval, 15);
        assert!(c.hide_console);
        let c: Config = toml::from_str("").unwrap();
        assert!(c.auto_update);
    }

    #[test]
    fn long_lines_are_cut() {
        assert_eq!(fit("short"), "short");
        assert_eq!(fit(&"x".repeat(200)).chars().count(), 128);
    }
}
