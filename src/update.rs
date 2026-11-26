//! Self-update from the project's GitHub releases: the latest release's `vi-rpc.exe` replaces this one when
//! its version is newer. On start, every few hours and from the tray; `auto_update = false` in `vi-rpc.toml`
//! turns the automatic part off. The download is checked against the size and SHA-256 digest GitHub reports
//! for the asset. The running exe is renamed aside (Windows allows that), the new one takes its place and is
//! started, and this process exits; the old copy is removed by the new version once it has run for a while.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::app;

const REPO: &str = "c4k3ss/GTA-VI-RPC";
const ASSET: &str = "vi-rpc.exe";
/// No release of this tool is anywhere near this; a bigger asset is a mistake, not an update.
const MAX_ASSET_BYTES: u64 = 64 << 20;
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

/// Removes the previous exe left by an update, if any. Called once this version has run for a while, so a
/// broken update that dies at start leaves the old exe next to it for the user.
pub fn clean_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_file_name("vi-rpc.old.exe"));
    }
}

struct Release {
    tag: String,
    url: String,
    size: Option<u64>,
    /// `sha256:<hex>` as GitHub reports it, when it does.
    digest: Option<String>,
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
    let asset = body["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(ASSET)));
    Ok(asset.and_then(|x| {
        let url = x["browser_download_url"].as_str()?.to_string();
        Some(Release { tag: tag.clone(), url, size: x["size"].as_u64(), digest: x["digest"].as_str().map(str::to_string) })
    }))
}

/// Downloads to `to`, at most `MAX_ASSET_BYTES`; returns the byte count and the SHA-256 hex digest.
fn download(url: &str, to: &Path) -> Result<(u64, String), String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(120)).user_agent("vi-rpc").build();
    let resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut reader = std::io::Read::take(resp.into_reader(), MAX_ASSET_BYTES + 1);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 << 10];
    let mut total = 0u64;
    loop {
        let n = std::io::Read::read(&mut reader, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_ASSET_BYTES {
            return Err(format!("asset larger than {} MiB", MAX_ASSET_BYTES >> 20));
        }
        hasher.update(&buf[..n]);
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| e.to_string())?;
    }
    Ok((total, format!("{:x}", hasher.finalize())))
}

/// The download must match what GitHub says about the asset, and look like a Windows program.
fn verify(rel: &Release, path: &Path, got_size: u64, got_sha: &str) -> Result<(), String> {
    if let Some(size) = rel.size {
        if size != got_size {
            return Err(format!("size {got_size} differs from the release's {size}"));
        }
    }
    if let Some(d) = rel.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
        if !d.eq_ignore_ascii_case(got_sha) {
            return Err("SHA-256 digest differs from the release's".to_string());
        }
    }
    let mut head = [0u8; 2];
    let ok = std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok();
    if !ok || &head != b"MZ" || got_size < 500_000 {
        return Err("the download does not look like the program".to_string());
    }
    Ok(())
}

/// Checks once; on a newer release, replaces this exe and restarts. Returns whether a restart was started.
/// `manual` reports "up to date" too (the tray's Check for updates).
pub fn check_and_apply(manual: bool) -> bool {
    let rel = match latest_release() {
        Ok(Some(r)) => r,
        Ok(None) => {
            if manual {
                crate::log::say("vi-rpc: no release with vi-rpc.exe found");
            }
            return false;
        }
        Err(e) => {
            crate::log::say(format!("vi-rpc: update check failed ({e})"));
            return false;
        }
    };
    if !newer(&rel.tag, VERSION) {
        if manual {
            crate::log::say(format!("vi-rpc: up to date ({VERSION}; latest release {})", rel.tag));
        }
        return false;
    }
    crate::log::say(format!("vi-rpc: version {} is out (this is {VERSION}); updating", rel.tag));
    let exe: PathBuf = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let fresh = exe.with_file_name("vi-rpc.new.exe");
    let old = exe.with_file_name("vi-rpc.old.exe");
    let downloaded = download(&rel.url, &fresh).and_then(|(n, sha)| verify(&rel, &fresh, n, &sha));
    if let Err(e) = downloaded {
        crate::log::say(format!("vi-rpc: update not applied ({e}); keeping this version"));
        let _ = std::fs::remove_file(&fresh);
        return false;
    }
    // Quit takes the same lock before it exits, so the swap is never left half done; and if Quit got there
    // first, there is nothing to update any more.
    let _swap = app::SWAP.lock().unwrap_or_else(|e| e.into_inner());
    if app::STOP.load(Ordering::SeqCst) {
        let _ = std::fs::remove_file(&fresh);
        return false;
    }
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(&exe, &old).and_then(|_| std::fs::rename(&fresh, &exe)) {
        crate::log::say(format!("vi-rpc: could not swap the files ({e})"));
        let _ = std::fs::rename(&old, &exe);
        let _ = std::fs::remove_file(&fresh);
        return false;
    }
    // Let go of the single-instance mutex before the new exe starts, or it would see us and quit.
    app::release_instance();
    match std::process::Command::new(&exe).args(std::env::args().skip(1)).spawn() {
        Ok(_) => {
            crate::log::say(format!("vi-rpc: updated to {}; restarting", rel.tag));
            true
        }
        Err(e) => {
            crate::log::say(format!("vi-rpc: the new version did not start ({e}); restoring"));
            let _ = std::fs::rename(&exe, &fresh);
            let _ = std::fs::rename(&old, &exe);
            let _ = std::fs::remove_file(&fresh);
            if !app::claim_instance() {
                crate::log::say("vi-rpc: another copy took over meanwhile; stopping this one");
                app::STOP.store(true, Ordering::SeqCst);
            }
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

    #[test]
    fn verify_checks_size_and_digest() {
        let dir = std::env::temp_dir().join(format!("vi-rpc-verify-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("x.exe");
        let data = vec![b'M', b'Z'].into_iter().chain(std::iter::repeat(0u8).take(600_000)).collect::<Vec<_>>();
        std::fs::write(&p, &data).unwrap();
        let sha = format!("{:x}", Sha256::digest(&data));
        let rel = |size, digest: Option<&str>| Release { tag: "v9.9.9".into(), url: String::new(), size, digest: digest.map(str::to_string) };
        assert!(verify(&rel(Some(data.len() as u64), Some(&format!("sha256:{sha}"))), &p, data.len() as u64, &sha).is_ok());
        assert!(verify(&rel(Some(1), None), &p, data.len() as u64, &sha).is_err());
        assert!(verify(&rel(None, Some("sha256:00")), &p, data.len() as u64, &sha).is_err());
        assert!(verify(&rel(None, None), &p, data.len() as u64, &sha).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
