//! What the layer's window title says: `<title> | VIctory <fps> fps <ms> ms/frame <draws> draws/frame`
//! (VIctory and every fork of it name the window this way; Flamingo adds nothing to it).

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible};

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub title: String,
    pub fps: f32,
    pub ms: f32,
    pub draws: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scene {
    Loading,
    Menus,
    World,
}

impl Frame {
    pub fn scene(&self) -> Scene {
        match self.draws {
            0..=9 => Scene::Loading,
            10..=199 => Scene::Menus,
            _ => Scene::World,
        }
    }

    pub fn details(&self) -> String {
        match self.scene() {
            Scene::Loading => "Loading".to_string(),
            Scene::Menus => "In the menus".to_string(),
            Scene::World => "Playing Story Mode".to_string(),
        }
    }

    pub fn state(&self) -> String {
        match self.scene() {
            // Stage 2 replaces this with "<activity> near <place>" from the title's own state.
            Scene::World => format!("Somewhere in Leonida · {:.0} fps", self.fps),
            _ => format!("{:.0} fps", self.fps),
        }
    }

    /// `Americas | VIctory 8 fps 129.9 ms/frame 7577 draws/frame`
    pub fn parse(title: &str) -> Option<Frame> {
        let (name, rest) = title.split_once(" | VIctory ")?;
        let mut words = rest.split_whitespace();
        let fps: f32 = words.next()?.parse().ok()?;
        words.next().filter(|w| *w == "fps")?;
        let ms: f32 = words.next()?.parse().ok()?;
        words.next().filter(|w| *w == "ms/frame")?;
        let draws: u32 = words.next()?.parse().ok()?;
        Some(Frame { title: name.to_string(), fps, ms, draws })
    }
}

/// The first visible window whose title the layer wrote.
pub fn find() -> Option<Frame> {
    let mut found: Option<Frame> = None;
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the pointer to `found` passed below, valid for the call.
        let found = unsafe { &mut *(lparam as *mut Option<Frame>) };
        // SAFETY: plain Win32 queries on a window handle the system gave us.
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return 1;
            }
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            let text = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            if let Some(f) = Frame::parse(&text) {
                *found = Some(f);
                return 0;
            }
        }
        1
    }
    // SAFETY: `found` outlives the enumeration, which is synchronous.
    unsafe {
        EnumWindows(Some(each), &mut found as *mut _ as LPARAM);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_layers_title() {
        let f = Frame::parse("Americas | VIctory 8 fps 129.9 ms/frame 7577 draws/frame").unwrap();
        assert_eq!((f.title.as_str(), f.fps, f.ms, f.draws), ("Americas", 8.0, 129.9, 7577));
        assert_eq!(f.scene(), Scene::World);
        assert_eq!(Frame::parse("Americas | VIctory 30 fps 33.3 ms/frame 87 draws/frame").unwrap().scene(), Scene::Menus);
        assert_eq!(Frame::parse("Americas | VIctory 30 fps 33.3 ms/frame 3 draws/frame").unwrap().scene(), Scene::Loading);
        assert!(Frame::parse("Visual Studio Code").is_none());
    }
}
