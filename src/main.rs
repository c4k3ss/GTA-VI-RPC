//! vi-rpc: Discord Rich Presence for the Xbox build of GTA VI running under a VIctory-lineage layer.
//!
//! Stage 1 (this file): everything the layer gives away for free. Its window title reads
//! `Americas | VIctory 8 fps 129.9 ms/frame 7577 draws/frame`; the draw count tells menus (tens) from the
//! world (thousands) and loading (a handful). Stage 2 adds the title's own state (mission, scene, clock,
//! position) read from the process's memory once their places are known (see re/).
//!
//! Config: `vi-rpc.toml` next to the exe, `app_id = "<Discord application id>"`.

use std::time::{Duration, Instant};

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use serde::Deserialize;

mod layer;
mod scan;
mod title;

#[derive(Deserialize)]
struct Config {
    app_id: String,
    /// Seconds between presence updates (Discord allows one per 15 s).
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    15
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
    let exe = std::env::current_exe().unwrap_or_default();
    let cfg_path = exe.with_file_name("vi-rpc.toml");
    let cfg: Config = match std::fs::read_to_string(&cfg_path).ok().and_then(|t| toml::from_str(&t).ok()) {
        Some(c) => c,
        None => {
            eprintln!("vi-rpc: put `app_id = \"<Discord application id>\"` in {}", cfg_path.display());
            std::process::exit(2);
        }
    };
    let mut client: Option<DiscordIpcClient> = None;
    let mut started: Option<Instant> = None;
    let mut last_shown = String::new();
    loop {
        let frame = layer::find();
        match frame {
            None => {
                if client.is_some() {
                    println!("vi-rpc: the game is gone; clearing presence");
                    if let Some(c) = client.as_mut() {
                        let _ = c.clear_activity();
                        let _ = c.close();
                    }
                    client = None;
                    started = None;
                }
            }
            Some(f) => {
                if client.is_none() {
                    let mut c = DiscordIpcClient::new(&cfg.app_id).expect("client");
                    match c.connect() {
                        Ok(()) => {
                            println!("vi-rpc: connected to Discord");
                            client = Some(c);
                            started = Some(Instant::now());
                        }
                        Err(e) => eprintln!("vi-rpc: Discord not reachable ({e}); retrying"),
                    }
                }
                if let (Some(c), Some(t0)) = (client.as_mut(), started) {
                    // the title's own clock, when this build's layout is known and the world is up
                    let title = if f.scene() == layer::Scene::World { title::Title::attach() } else { None };
                    let clock = title.as_ref().and_then(|t| t.clock());
                    let camera = title.as_ref().and_then(|t| t.camera());
                    // the coordinates stay out of the presence (they are logged below for debugging) until the
                    // place name is read from the title
                    let state = match clock {
                        Some(k) => format!("Somewhere in Leonida · {}", k.text_12h()),
                        None => f.state(),
                    };
                    if let Some(p) = camera {
                        println!("vi-rpc: camera at ({:.1}, {:.1}, {:.1})", p[0], p[1], p[2]);
                    }
                    let details = f.details();
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
                                println!("vi-rpc: {shown}");
                                last_shown = shown;
                            }
                        }
                        Err(e) => {
                            eprintln!("vi-rpc: update failed ({e}); reconnecting");
                            client = None;
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(cfg.interval.max(5)));
    }
}
