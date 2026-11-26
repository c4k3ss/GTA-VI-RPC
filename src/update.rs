//! Self-update from the project's GitHub releases: the latest release's `vi-rpc.exe` replaces this one when
//! its version is newer. On start and every few hours; `auto_update = false` in `vi-rpc.toml` turns it off.
//! The running exe is renamed aside (Windows allows that), the new one takes its place and is started, and
//! this process exits; the old copy is removed on the next start.

use std::path::{Path, PathBuf};
use std::time::Duration;

const REPO: &str = "c4k3ss/GTA-VI-RPC";
const ASSET: &str = "vi-rpc.exe";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `(major, minor, patch)` of a `1.2.3` or `v1.2.3` string.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let mut it = s.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

pub fn newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Removes the previous exe left by an update, if any.
pub fn clean_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_file_name("vi-rpc.old.exe"));
    }
}

struct Release {
    tag: String,
    url: String,
}

fn latest_release() -> Result<Option<Release>, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("vi-rpc").build();
    let body: serde_json::Value = agent
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let tag = body["tag_name"].as_str().unwrap_or_default().to_string();
    let url = body["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(ASSET)))
        .and_then(|x| x["browser_download_url"].as_str())
        .map(str::to_string);
    Ok(url.map(|url| Release { tag, url }))
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(120)).user_agent("vi-rpc").build();
    let resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(to).map_err(|e| e.to_string())?;
    std::io::copy(&mut resp.into_reader(), &mut file).map_err(|e| e.to_string())?;
    Ok(())
}

/// Checks once; on a newer release, replaces this exe and restarts. Returns whether a restart was started.
pub fn check_and_apply() -> bool {
    let rel = match latest_release() {
        Ok(Some(r)) => r,
        Ok(None) => return false,
        Err(e) => {
            eprintln!("vi-rpc: update check failed ({e})");
            return false;
        }
    };
    if !newer(&rel.tag, VERSION) {
        return false;
    }
    println!("vi-rpc: version {} is out (this is {VERSION}); updating", rel.tag);
    let exe: PathBuf = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let fresh = exe.with_file_name("vi-rpc.new.exe");
    let old = exe.with_file_name("vi-rpc.old.exe");
    if let Err(e) = download(&rel.url, &fresh) {
        eprintln!("vi-rpc: download failed ({e})");
        let _ = std::fs::remove_file(&fresh);
        return false;
    }
    // a sane download: a PE file of a plausible size
    let ok = std::fs::read(&fresh).map(|d| d.len() > 500_000 && d.starts_with(b"MZ")).unwrap_or(false);
    if !ok {
        eprintln!("vi-rpc: the download does not look like the program; keeping this version");
        let _ = std::fs::remove_file(&fresh);
        return false;
    }
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(&exe, &old).and_then(|_| std::fs::rename(&fresh, &exe)) {
        eprintln!("vi-rpc: could not swap the files ({e})");
        let _ = std::fs::rename(&old, &exe);
        return false;
    }
    match std::process::Command::new(&exe).args(std::env::args().skip(1)).spawn() {
        Ok(_) => {
            println!("vi-rpc: updated to {}; restarting", rel.tag);
            true
        }
        Err(e) => {
            eprintln!("vi-rpc: the new version did not start ({e}); restoring");
            let _ = std::fs::rename(&exe, &fresh);
            let _ = std::fs::rename(&old, &exe);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
        assert!(!newer("latest", "0.1.0"));
    }
}
