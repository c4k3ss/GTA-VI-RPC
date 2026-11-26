//! What a layer tells us directly: `%LOCALAPPDATA%\vi-rpc\state.json`, rewritten by the layer whenever
//! something changes. Every field is optional; what is present wins over our own readings.

use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
pub struct State {
    /// `menu`, `loading` or `world`
    pub scene: Option<String>,
    pub mission: Option<String>,
    pub street: Option<String>,
    pub area: Option<String>,
    /// e.g. `on foot`, `driving`, `in a cutscene`
    pub activity: Option<String>,
}

pub fn path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(std::path::PathBuf::from(base).join("vi-rpc").join("state.json"))
}

/// The file's contents, or nothing when it is absent, stale (older than 10 minutes) or malformed.
pub fn read() -> Option<State> {
    let p = path()?;
    let meta = std::fs::metadata(&p).ok()?;
    if meta.modified().ok()?.elapsed().map(|d| d.as_secs() > 600).unwrap_or(true) {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

impl State {
    /// The second line: `<activity> near <street>, <area>` with whatever is known.
    pub fn place_line(&self) -> Option<String> {
        let mut where_ = Vec::new();
        if let Some(s) = &self.street {
            where_.push(s.clone());
        }
        if let Some(a) = &self.area {
            where_.push(a.clone());
        }
        match (self.activity.as_deref(), where_.is_empty()) {
            (Some(act), false) => Some(format!("{} near {}", capitalize(act), where_.join(", "))),
            (Some(act), true) => Some(capitalize(act)),
            (None, false) => Some(format!("Near {}", where_.join(", "))),
            (None, true) => None,
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_line_forms() {
        let s: State = serde_json::from_str(r#"{"street":"Sunset Dr","area":"Key Lento","activity":"driving"}"#).unwrap();
        assert_eq!(s.place_line().as_deref(), Some("Driving near Sunset Dr, Key Lento"));
        let s: State = serde_json::from_str(r#"{"area":"Key Lento"}"#).unwrap();
        assert_eq!(s.place_line().as_deref(), Some("Near Key Lento"));
        let s: State = serde_json::from_str(r#"{"mission":"Gone Fishing"}"#).unwrap();
        assert_eq!(s.place_line(), None);
        assert_eq!(s.mission.as_deref(), Some("Gone Fishing"));
    }
}
