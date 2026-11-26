//! The title's own state, read from the running process: fields at fixed offsets inside the title image
//! (`victory-image-<hash>.dll`, the game's executable as the layer maps it), found with `vi-rpc clock`
//! (see re/ and PLAN.md). Offsets are per game build; the image's name carries the build's hash.

use crate::scan::Target;

/// Where a build keeps things. Offsets are from the image's base.
pub struct Layout {
    /// The first 16 hex digits of the executable's blake3 hash, as the layer names the image module.
    pub image_hash: &'static str,
    /// The clock block: day, month, year, hour, minute, second as u32 at these offsets.
    pub clock: usize,
    /// The camera's view-projection matrix, 16 f32 row-major; the world position is `-t * inv(R)` with
    /// `R` the top-left 3x3 and `t` the first three of the last row (checked by a walk due north: +146 on Y).
    pub camera: usize,
}

/// Grand Theft Auto VI, Xbox Series build 1.0.2.1 (`game_scarlett_master.exe`). The clock block was found on
/// 2026-11-26 by narrowing a scan of the image's data with the in-game time (`vi-rpc clock --image`): at
/// `+0x74df88c` day, month, year; `+0x74df898` hour, minute, second; then ms of the day, minute of the day and
/// the hour as a float.
pub const LAYOUTS: &[Layout] = &[Layout { image_hash: "b99f876b74d4a77f", clock: 0x74df88c, camera: 0x74def10 }];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    pub day: u32,
    pub month: u32,
    pub year: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Clock {
    /// `5:17 PM`
    pub fn text_12h(&self) -> String {
        let (h, ampm) = match self.hour {
            0 => (12, "AM"),
            1..=11 => (self.hour, "AM"),
            12 => (12, "PM"),
            h => (h - 12, "PM"),
        };
        format!("{h}:{:02} {ampm}", self.minute)
    }

    pub fn plausible(&self) -> bool {
        self.hour < 24 && self.minute < 60 && self.second < 60 && (1..=31).contains(&self.day) && (1..=12).contains(&self.month)
    }
}

pub struct Title {
    target: Target,
    base: usize,
    layout: &'static Layout,
}

impl Title {
    /// Attaches to the running layer when its title image is one with a known layout.
    pub fn attach(pid: u32) -> Option<Title> {
        let target = if pid != 0 { Target::open_pid(pid)? } else { Target::open("victory.exe")? };
        let img = target.image()?;
        let hash = img.name.strip_prefix("victory-image-")?.strip_suffix(".dll")?;
        let layout = LAYOUTS.iter().find(|l| l.image_hash == hash)?;
        Some(Title { base: img.base, target, layout })
    }

    /// The camera's position in the world (Rage axes: X east, Y north, Z up), in metres.
    pub fn camera(&self) -> Option<[f32; 3]> {
        let mut buf = [0u8; 64];
        if self.target.read(self.base + self.layout.camera, &mut buf) != buf.len() {
            return None;
        }
        let m: Vec<f32> = buf.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        camera_position(&m)
    }

    pub fn clock(&self) -> Option<Clock> {
        let mut buf = [0u8; 24];
        if self.target.read(self.base + self.layout.clock, &mut buf) != buf.len() {
            return None;
        }
        let w = |i: usize| u32::from_le_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
        let c = Clock { day: w(0), month: w(1), year: w(2), hour: w(3), minute: w(4), second: w(5) };
        c.plausible().then_some(c)
    }
}

/// `-t * inv(R)` for a row-major 4x4 whose top-left 3x3 is `R` and whose last row starts with `t`.
pub fn camera_position(m: &[f32]) -> Option<[f32; 3]> {
    if m.len() < 16 {
        return None;
    }
    {
        let r = |i: usize, j: usize| m[i * 4 + j] as f64;
        let t = [m[12] as f64, m[13] as f64, m[14] as f64];
        let det = r(0, 0) * (r(1, 1) * r(2, 2) - r(1, 2) * r(2, 1)) - r(0, 1) * (r(1, 0) * r(2, 2) - r(1, 2) * r(2, 0))
            + r(0, 2) * (r(1, 0) * r(2, 1) - r(1, 1) * r(2, 0));
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let inv = [
            [(r(1, 1) * r(2, 2) - r(1, 2) * r(2, 1)) / det, (r(0, 2) * r(2, 1) - r(0, 1) * r(2, 2)) / det, (r(0, 1) * r(1, 2) - r(0, 2) * r(1, 1)) / det],
            [(r(1, 2) * r(2, 0) - r(1, 0) * r(2, 2)) / det, (r(0, 0) * r(2, 2) - r(0, 2) * r(2, 0)) / det, (r(0, 2) * r(1, 0) - r(0, 0) * r(1, 2)) / det],
            [(r(1, 0) * r(2, 1) - r(1, 1) * r(2, 0)) / det, (r(0, 1) * r(2, 0) - r(0, 0) * r(2, 1)) / det, (r(0, 0) * r(1, 1) - r(0, 1) * r(1, 0)) / det],
        ];
        let p: Vec<f32> = (0..3).map(|i| -(t[0] * inv[0][i] + t[1] * inv[1][i] + t[2] * inv[2][i]) as f32).collect();
        let pos = [p[0], p[1], p[2]];
        pos.iter().all(|v| v.is_finite() && v.abs() < 20_000.0).then_some(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_hour_text() {
        let c = |hour, minute| Clock { day: 21, month: 6, year: 2019, hour, minute, second: 0 };
        assert_eq!(c(17, 17).text_12h(), "5:17 PM");
        assert_eq!(c(0, 5).text_12h(), "12:05 AM");
        assert_eq!(c(12, 0).text_12h(), "12:00 PM");
        assert_eq!(c(9, 7).text_12h(), "9:07 AM");
        assert!(c(17, 17).plausible());
        assert!(!c(24, 0).plausible());
    }

    #[test]
    fn camera_position_from_the_two_measured_matrices() {
        // image+0x74def10 before and after a walk due north on 2026-11-26 (matrix.py): the move must be ~+146 on Y
        let north1 = [0.761, -0.785, -0.006, -0.010, 0.643, 0.613, 0.287, 0.542, -0.089, -0.089, -0.958, 0.840, 5773.881, 1628.352, 1697.745, 3192.504];
        let north2 = [0.806, -0.738, 0.017, 0.028, 0.587, 0.670, 0.306, 0.525, -0.077, -0.077, -0.952, 0.851, 5477.815, 1988.184, 1819.411, 3112.965];
        let (a, b) = (camera_position(&north1).unwrap(), camera_position(&north2).unwrap());
        assert!((a[0] + 2566.8).abs() < 2.0 && (a[1] + 5943.3).abs() < 2.0 && a[2].abs() < 10.0, "{a:?}");
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        assert!(dx.abs() < 15.0 && (140.0..155.0).contains(&dy), "move {dx} {dy}");
    }
}
