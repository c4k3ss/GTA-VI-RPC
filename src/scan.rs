//! `vi-rpc scan [needle ...]`: a read-only look through the running game for the strings that name its scene,
//! then for the fixed places (the title image's globals) that point at them, directly or through one hop.
//! The result is what stage 2 of the reader needs: `image+<offset>` chains that reach the current scene name.
//!
//! Memory is read in 16 MiB pieces with a time budget per sweep, smallest regions first, so a process whose
//! memory is largely paged out still gives an answer in a few minutes instead of hanging.

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32First, Module32Next, Process32First, Process32Next, MODULEENTRY32,
    PROCESSENTRY32, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

const CHUNK: usize = 32 << 20;

/// Seconds per sweep (`VI_SCAN_BUDGET`); the game maps about 40 GB, much of it paged out.
fn budget() -> Duration {
    Duration::from_secs(std::env::var("VI_SCAN_BUDGET").ok().and_then(|v| v.parse().ok()).unwrap_or(600))
}

pub struct Module {
    pub name: String,
    pub base: usize,
    pub size: usize,
}

pub struct Target {
    pub handle: HANDLE,
    pub modules: Vec<Module>,
}

impl Drop for Target {
    fn drop(&mut self) {
        // SAFETY: a handle OpenProcess gave us.
        unsafe { CloseHandle(self.handle) };
    }
}

fn pid_of(name: &str) -> Option<u32> {
    // SAFETY: Toolhelp snapshot walk with properly sized entries.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        let mut e: PROCESSENTRY32 = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;
        let mut ok = Process32First(snap, &mut e);
        let mut pid = None;
        while ok != 0 {
            let exe = CStr::from_ptr(e.szExeFile.as_ptr() as *const _).to_string_lossy();
            if exe.eq_ignore_ascii_case(name) {
                pid = Some(e.th32ProcessID);
                break;
            }
            ok = Process32Next(snap, &mut e);
        }
        CloseHandle(snap);
        pid
    }
}

impl Target {
    pub fn open(process: &str) -> Option<Target> {
        let pid = pid_of(process)?;
        // SAFETY: Toolhelp module walk; OpenProcess for reading only.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
            if handle.is_null() {
                return None;
            }
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
            let mut m: MODULEENTRY32 = std::mem::zeroed();
            m.dwSize = std::mem::size_of::<MODULEENTRY32>() as u32;
            let mut ok = Module32First(snap, &mut m);
            let mut modules = Vec::new();
            while ok != 0 {
                modules.push(Module {
                    name: CStr::from_ptr(m.szModule.as_ptr() as *const _).to_string_lossy().into_owned(),
                    base: m.modBaseAddr as usize,
                    size: m.modBaseSize as usize,
                });
                ok = Module32Next(snap, &mut m);
            }
            CloseHandle(snap);
            Some(Target { handle, modules })
        }
    }

    pub fn image(&self) -> Option<&Module> {
        self.modules.iter().find(|m| m.name.starts_with("victory-image-"))
    }

    pub fn owner(&self, addr: usize) -> String {
        for m in &self.modules {
            if m.base <= addr && addr < m.base + m.size {
                return format!("{}+{:#x}", m.name, addr - m.base);
            }
        }
        "heap".to_string()
    }

    pub fn read(&self, addr: usize, buf: &mut [u8]) -> usize {
        let mut n = 0usize;
        // SAFETY: reading another process's memory into our own buffer; failures return 0.
        let ok = unsafe { ReadProcessMemory(self.handle, addr as *const _, buf.as_mut_ptr() as *mut _, buf.len(), &mut n) };
        if ok == 0 { 0 } else { n }
    }

    /// Committed, readable regions outside the modules, largest first (the title's heaps hold what we look for).
    pub fn regions(&self, max_size: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut addr = 0usize;
        loop {
            let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
            // SAFETY: query of another process's address space into a local structure.
            let n = unsafe { VirtualQueryEx(self.handle, addr as *const _, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
            if n == 0 {
                break;
            }
            let base = mbi.BaseAddress as usize;
            let size = mbi.RegionSize;
            if mbi.State == MEM_COMMIT && mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD) == 0 && size <= max_size
                && !self.modules.iter().any(|m| m.base <= base && base < m.base + m.size)
            {
                out.push((base, size));
            }
            addr = match base.checked_add(size) {
                Some(a) if a < 0x7fff_ffff_ffff => a,
                _ => break,
            };
        }
        out.sort_by_key(|r| std::cmp::Reverse(r.1));
        out
    }

    /// Every chunk of every region, until the budget runs out. `f` gets (address, bytes).
    pub fn sweep(&self, budget: Duration, mut f: impl FnMut(usize, &[u8])) -> (usize, bool) {
        let t0 = Instant::now();
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0usize;
        for (base, size) in self.regions(4 << 30) {
            let mut off = 0;
            while off < size {
                if t0.elapsed() > budget {
                    return (total, false);
                }
                let want = CHUNK.min(size - off);
                let n = self.read(base + off, &mut buf[..want]);
                if n > 0 {
                    f(base + off, &buf[..n]);
                    if (total >> 32) != ((total + n) >> 32) {
                        println!("  ... {} GiB, {} s", (total + n) >> 30, t0.elapsed().as_secs());
                    }
                    total += n;
                }
                off += want;
            }
        }
        (total, true)
    }
}

fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    memchr::memmem::find_iter(hay, needle).collect()
}

fn c_string(hay: &[u8], at: usize) -> String {
    let mut start = at;
    while start > 0 && at - start < 96 && (0x20..0x7f).contains(&hay[start - 1]) {
        start -= 1;
    }
    let end = hay[at..].iter().position(|&b| b == 0 || !(0x20..0x7f).contains(&b)).map(|e| at + e).unwrap_or(hay.len());
    String::from_utf8_lossy(&hay[start..end.min(start + 120)]).into_owned()
}

/// Pointers in `hay` (8-byte aligned) whose target lies in [t - before, t + after) for a target t.
fn pointers_into(hay: &[u8], base: usize, targets: &[usize], before: usize, after: usize) -> Vec<(usize, usize, isize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 8 <= hay.len() {
        let v = u64::from_le_bytes(hay[i..i + 8].try_into().unwrap()) as usize;
        if v > 0x10000 && v < 0x7fff_ffff_ffff {
            let idx = targets.partition_point(|&t| t + after <= v);
            if let Some(&t) = targets.get(idx) {
                if v + before >= t && v < t + after {
                    out.push((base + i, t, v as isize - t as isize));
                }
            }
        }
        i += 8;
    }
    out
}

pub fn run(args: &[String]) {
    let needles: Vec<Vec<u8>> = if args.is_empty() {
        ["_INTRO_PROLOGUE", "FBK_STRY0", "fbk_stry0", "_PROLOGUE_"].iter().map(|s| s.as_bytes().to_vec()).collect()
    } else {
        args.iter().map(|s| s.as_bytes().to_vec()).collect()
    };
    let Some(t) = Target::open("victory.exe") else {
        eprintln!("vi-rpc scan: victory.exe is not running");
        std::process::exit(1);
    };
    let Some(img) = t.image() else {
        eprintln!("vi-rpc scan: no victory-image-* module: the title is not loaded yet");
        std::process::exit(1);
    };
    let (ibase, isize_) = (img.base, img.size);
    println!("image {} base {:#x} size {:#x}", img.name, ibase, isize_);

    // 1. the strings, with their addresses
    let mut found: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let (bytes, complete) = t.sweep(budget(), |addr, chunk| {
        for nd in &needles {
            for p in find_all(chunk, nd) {
                let s = c_string(chunk, p);
                let e = found.entry(s).or_default();
                if e.len() < 8 {
                    e.push(addr + p);
                }
            }
        }
    });
    println!("sweep 1: {:.1} GiB{}, {} distinct strings", bytes as f64 / (1u64 << 30) as f64, if complete { "" } else { " (budget hit)" }, found.len());
    let mut targets: Vec<usize> = Vec::new();
    for (s, addrs) in &found {
        println!("  {:<64} {}", s.chars().take(64).collect::<String>(), addrs.iter().map(|a| format!("{a:#x}")).collect::<Vec<_>>().join(" "));
        targets.extend(addrs);
    }
    if targets.is_empty() {
        println!("nothing to chase");
        return;
    }
    targets.sort_unstable();
    targets.dedup();

    // 2. fixed pointers in the image's data straight at a string (or a little before it: the struct holding it)
    let mut data = vec![0u8; isize_];
    let n = t.read(ibase, &mut data);
    data.truncate(n);
    let direct = pointers_into(&data, ibase, &targets, 0x200, 0x8);
    println!("sweep 2: {} fixed pointers in the image at a string (or up to 0x200 before it)", direct.len());
    for (at, tg, d) in direct.iter().take(40) {
        println!("  image+{:#x} -> {:#x}{:+#x}", at - ibase, tg, d);
    }

    // 3. heap pointers at the strings, then fixed pointers at those (a one-hop chain); skipped when a
    // direct pointer was found, since another full sweep costs minutes
    let mut hops: Vec<(usize, usize, isize)> = Vec::new();
    let (bytes, complete) = if direct.is_empty() { t.sweep(budget(), |addr, chunk| {
        if hops.len() < 20000 {
            hops.extend(pointers_into(chunk, addr, &targets, 0x200, 0x8));
        }
    }) } else { (0, true) };
    println!("sweep 3: {:.1} GiB{}, {} heap pointers at the strings", bytes as f64 / (1u64 << 30) as f64, if complete { "" } else { " (budget hit)" }, hops.len());
    let mut hop_addrs: Vec<usize> = hops.iter().map(|h| h.0).collect();
    hop_addrs.sort_unstable();
    hop_addrs.dedup();
    let two = pointers_into(&data, ibase, &hop_addrs, 0x400, 0x8);
    println!("  {} fixed pointers in the image at those (chains of one hop)", two.len());
    for (at, hp, d) in two.iter().take(40) {
        let h = hops.iter().find(|h| h.0 == *hp).unwrap();
        println!("  image+{:#x} -> {:#x}{:+#x} ; [{:#x}] -> {:#x}{:+#x} ({})", at - ibase, hp, d, hp, h.1, h.2, t.owner(h.1));
    }
    let out = std::env::current_exe().unwrap_or_default().with_file_name("scan.txt");
    let mut text = String::new();
    for (s, addrs) in &found {
        text += &format!("string {:?} {}\n", s, addrs.iter().map(|a| format!("{a:#x}")).collect::<Vec<_>>().join(" "));
    }
    for (at, tg, d) in &direct {
        text += &format!("direct image+{:#x} -> {:#x}{:+#x}\n", at - ibase, tg, d);
    }
    for (at, hp, d) in &two {
        let h = hops.iter().find(|h| h.0 == *hp).unwrap();
        text += &format!("hop image+{:#x} -> {:#x}{:+#x} ; {:#x} -> {:#x}{:+#x}\n", at - ibase, hp, d, hp, h.1, h.2);
    }
    let _ = std::fs::write(&out, text);
    println!("written {}", out.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_and_pointers_are_found() {
        let mut hay = vec![0u8; 64];
        hay[10..30].copy_from_slice(b"X_INTRO_PROLOGUE_JAI");
        hay[30] = b'L';
        assert_eq!(find_all(&hay, b"_INTRO_PROLOGUE"), vec![11]);
        assert_eq!(c_string(&hay, 11), "X_INTRO_PROLOGUE_JAIL");
        // a pointer at offset 40 to the string (base B): target B+0xa, pointer value B+0xb is +1
        const B: usize = 0x2000_0000_0000;
        hay[40..48].copy_from_slice(&((B + 0xb) as u64).to_le_bytes());
        let hits = pointers_into(&hay, B, &[B + 0xa], 0x10, 0x8);
        assert_eq!(hits, vec![(B + 40, B + 0xa, 1)]);
        // a pointer 0x1a before the string is within `before` 0x20 but not 0x10
        hay[48..56].copy_from_slice(&((B - 0x10) as u64).to_le_bytes());
        assert_eq!(pointers_into(&hay, B, &[B + 0xa], 0x20, 0x8).len(), 2);
        assert_eq!(pointers_into(&hay, B, &[B + 0xa], 0x10, 0x8).len(), 1);
    }
}

/// `vi-rpc find <u32 hex> [...]`: every 4-byte aligned place holding one of the values. `vi-rpc clock <hour>
/// <minute>`: every place that could hold the in-game clock under one of several forms: `hm32` two adjacent u32
/// (hour, minute), `mh32` the other way round, `hm8` two u8, `hm16` two u16, `mod` minute of the day as u32,
/// `sod` seconds of the day as u32 (any second of that minute). `--again <hour> <minute>` re-reads the places
/// of the last full scan (clock.txt, kept) and writes the survivors to clock-again.txt, with the fixed pointers
/// the image holds at them. Repeat `--again` as the clock moves: survivors only ever shrink.
pub fn find(args: &[String]) {
    let Some(t) = Target::open("victory.exe") else {
        eprintln!("vi-rpc: victory.exe is not running");
        std::process::exit(1);
    };
    let img = t.image().expect("the title image is not loaded");
    let (ibase, isize_) = (img.base, img.size);
    let out_dir = std::env::current_exe().unwrap_or_default().with_file_name("");
    let again = args.iter().any(|a| a == "--again");
    let image_only = args.iter().any(|a| a == "--image");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--again" && *a != "--image").collect();
    let mode = std::env::args().nth(1).unwrap_or_default();
    let is_clock = mode == "clock";
    let is_text = mode == "text";
    if is_clock && rest.len() != 2 {
        eprintln!("usage: vi-rpc clock [--again] <hour> <minute>");
        std::process::exit(2);
    }
    if is_text && rest.len() != 1 {
        eprintln!("usage: vi-rpc text [--again] <string>   (ASCII and UTF-16 forms are both looked for)");
        std::process::exit(2);
    }
    // (kind, bytes, range length): a place matches a kind when its bytes equal `bytes`, or for `sod` when the u32
    // there lies in [value, value + range)
    let mut kinds: Vec<(&str, Vec<u8>, u32)> = Vec::new();
    if is_clock {
        let (h, m) = (rest[0].parse::<u32>().expect("hour"), rest[1].parse::<u32>().expect("minute"));
        kinds.push(("hm32", [h.to_le_bytes(), m.to_le_bytes()].concat(), 0));
        kinds.push(("mh32", [m.to_le_bytes(), h.to_le_bytes()].concat(), 0));
        kinds.push(("hm8", vec![h as u8, m as u8], 0));
        kinds.push(("hm16", [(h as u16).to_le_bytes(), (m as u16).to_le_bytes()].concat(), 0));
        kinds.push(("mod", (h * 60 + m).to_le_bytes().to_vec(), 0));
        kinds.push(("sod", (h * 3600 + m * 60).to_le_bytes().to_vec(), 60));
        // the hour and the minute kept apart (the game's clock is a few separate globals in the Rage engine):
        // every u32 equal to the hour, every u32 equal to the minute; the narrowing passes sort them out
        kinds.push(("h32", h.to_le_bytes().to_vec(), 0));
        kinds.push(("m32", m.to_le_bytes().to_vec(), 0));
        kinds.push(("msod", ((h * 3600 + m * 60) * 1000).to_le_bytes().to_vec(), 60_000));
    } else if is_text {
        let t8 = rest[0].as_bytes().to_vec();
        let t16: Vec<u8> = rest[0].encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        kinds.push(("a8", t8, 0));
        kinds.push(("u16", t16, 0));
    } else {
        for a in &rest {
            let v = u32::from_str_radix(a.trim_start_matches("0x"), 16).expect("a hex u32");
            kinds.push(("u32", v.to_le_bytes().to_vec(), 0));
        }
    }
    let matches = |kind: &str, bytes: &[u8]| -> bool {
        kinds.iter().any(|(k, pat, range)| {
            *k == kind
                && if *range == 0 {
                    bytes.len() >= pat.len() && bytes[..pat.len()] == pat[..]
                } else {
                    bytes.len() >= 4 && {
                        let v = u32::from_le_bytes(bytes[..4].try_into().unwrap());
                        let lo = u32::from_le_bytes(pat[..4].try_into().unwrap());
                        v >= lo && v < lo + range
                    }
                }
        })
    };
    let tag = if image_only { "img-" } else { "" };
    let full = out_dir.join(format!("{tag}{}", if is_clock { "clock.txt" } else if is_text { "text.txt" } else { "find.txt" }));
    let mut places: Vec<(usize, String)> = Vec::new();
    if again {
        if !full.exists() {
            eprintln!("vi-rpc: no full scan saved yet ({}); run without --again first", full.display());
            std::process::exit(2);
        }
        // narrow the previous narrowing when there is one, so the passes accumulate
        let prev = out_dir.join(format!("{tag}{}", if is_clock { "clock-again.txt" } else if is_text { "text-again.txt" } else { "find-again.txt" }));
        let text = std::fs::read_to_string(if prev.exists() { &prev } else { &full }).unwrap_or_default();
        let mut buf = [0u8; 64];
        let mut seen = 0;
        for line in text.lines() {
            let mut it = line.split_whitespace();
            let (Some(a), Some(kind)) = (it.next(), it.next()) else { continue };
            let Ok(a) = usize::from_str_radix(a.trim_start_matches("0x"), 16) else { continue };
            seen += 1;
            let n = t.read(a, &mut buf);
            if n > 0 && matches(kind, &buf[..n]) {
                places.push((a, kind.to_string()));
            }
        }
        println!("again: {} of {} saved places still match", places.len(), seen);
    } else {
        let mut scan_chunk = |addr: usize, chunk: &[u8], places: &mut Vec<(usize, String)>| {
            for (kind, pat, range) in &kinds {
                if *range == 0 {
                    for i in find_all(chunk, pat) {
                        if pat.len() >= 4 && i % 4 != 0 && !is_text {
                            continue;
                        }
                        if places.len() < 8_000_000 {
                            places.push((addr + i, kind.to_string()));
                        }
                    }
                } else {
                    let lo = u32::from_le_bytes(pat[..4].try_into().unwrap());
                    let mut i = 0;
                    while i + 4 <= chunk.len() {
                        let v = u32::from_le_bytes(chunk[i..i + 4].try_into().unwrap());
                        if v >= lo && v < lo + range && places.len() < 8_000_000 {
                            places.push((addr + i, kind.to_string()));
                        }
                        i += 4;
                    }
                }
            }
        };
        if image_only {
            // the title image alone: its data sections hold the engine's globals (a Rage clock is a few of them)
            let mut buf = vec![0u8; CHUNK];
            let mut off = 0;
            while off < isize_ {
                let want = CHUNK.min(isize_ - off);
                let n = t.read(ibase + off, &mut buf[..want]);
                if n > 0 {
                    scan_chunk(ibase + off, &buf[..n], &mut places);
                }
                off += want;
            }
            println!("image sweep: {:.0} MiB, {} places", isize_ as f64 / (1u64 << 20) as f64, places.len());
        } else {
            let (bytes, complete) = t.sweep(budget(), |addr, chunk| scan_chunk(addr, chunk, &mut places));
            println!("sweep: {:.1} GiB{}, {} places", bytes as f64 / (1u64 << 30) as f64, if complete { "" } else { " (budget hit)" }, places.len());
        }
    }
    places.sort();
    places.dedup();
    let save = if again { out_dir.join(format!("{tag}{}", if is_clock { "clock-again.txt" } else if is_text { "text-again.txt" } else { "find-again.txt" })) } else { full };
    let _ = std::fs::write(&save, places.iter().map(|(a, k)| format!("{a:#x} {k}
")).collect::<String>());
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, k) in &places {
        *by_kind.entry(k.as_str()).or_default() += 1;
    }
    println!("  by kind: {}", by_kind.iter().map(|(k, n)| format!("{k}={n}")).collect::<Vec<_>>().join(" "));
    for (a, k) in places.iter().take(30) {
        println!("  {:#x} {} ({})", a, k, t.owner(*a));
    }
    if places.len() <= 5000 && !places.is_empty() {
        let mut data = vec![0u8; isize_];
        let n = t.read(ibase, &mut data);
        data.truncate(n);
        let addrs: Vec<usize> = places.iter().map(|p| p.0).collect();
        let direct = pointers_into(&data, ibase, &addrs, 0x400, 0x8);
        println!("{} fixed pointers in the image at a surviving place (or up to 0x400 before it)", direct.len());
        for (at, tg, d) in direct.iter().take(40) {
            println!("  image+{:#x} -> {:#x}{:+#x}", at - ibase, tg, d);
        }
        // places inside the image's own data are fixed by themselves
        for (a, k) in places.iter().filter(|(a, _)| *a >= ibase && *a < ibase + isize_) {
            println!("  FIXED: image+{:#x} {}", a - ibase, k);
        }
    }
    println!("saved {}", save.display());
}

/// `vi-rpc chase [file]`: for the places listed in `clock-again.txt` (or the file given: `<hex addr> ...` per
/// line), finds the heap pointers at them (up to 0x400 before, so a struct's start counts), then the image's
/// fixed pointers at those (one hop), then one more hop. Prints every chain `image+off -> [+a] -> [+b] -> place`
/// and saves them to chains.txt. One full sweep per hop, so a few minutes each; pause the game for it.
pub fn chase(args: &[String]) {
    let Some(t) = Target::open("victory.exe") else {
        eprintln!("vi-rpc: victory.exe is not running");
        std::process::exit(1);
    };
    let img = t.image().expect("the title image is not loaded");
    let (ibase, isize_) = (img.base, img.size);
    let out_dir = std::env::current_exe().unwrap_or_default().with_file_name("");
    let file = args.first().map(std::path::PathBuf::from).unwrap_or_else(|| out_dir.join("clock-again.txt"));
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let mut places: Vec<usize> = text.lines().filter_map(|l| usize::from_str_radix(l.split_whitespace().next()?.trim_start_matches("0x"), 16).ok()).collect();
    places.sort_unstable();
    places.dedup();
    println!("chasing {} places from {}", places.len(), file.display());
    if places.is_empty() || places.len() > 5000 {
        eprintln!("need 1..5000 places; narrow first");
        std::process::exit(2);
    }
    let mut data = vec![0u8; isize_];
    let n = t.read(ibase, &mut data);
    data.truncate(n);
    let mut chains: Vec<String> = Vec::new();
    // level 0: fixed pointers straight at a place
    for (at, tg, d) in pointers_into(&data, ibase, &places, 0x400, 0x8) {
        chains.push(format!("image+{:#x} -> {:#x}{:+#x}", at - ibase, tg, d));
    }
    println!("direct: {}", chains.len());
    let mut level: Vec<usize> = places.clone();
    let mut names: BTreeMap<usize, String> = places.iter().map(|&p| (p, format!("{p:#x}"))).collect();
    for hop in 1..=2 {
        let mut found: Vec<(usize, usize, isize)> = Vec::new();
        let (bytes, complete) = t.sweep(budget(), |addr, chunk| {
            if found.len() < 200_000 {
                found.extend(pointers_into(chunk, addr, &level, 0x400, 0x8));
            }
        });
        println!("hop {hop}: {:.1} GiB{}, {} heap pointers at the current level", bytes as f64 / (1u64 << 30) as f64, if complete { "" } else { " (budget hit)" }, found.len());
        let mut next: Vec<usize> = Vec::new();
        let mut next_names: BTreeMap<usize, String> = BTreeMap::new();
        for (at, tg, d) in &found {
            let desc = format!("[{:#x}] -> {}{:+#x}", at, names[tg], d);
            next_names.entry(*at).or_insert(desc);
            next.push(*at);
        }
        next.sort_unstable();
        next.dedup();
        let fixed = pointers_into(&data, ibase, &next, 0x400, 0x8);
        println!("  {} fixed pointers in the image at those", fixed.len());
        for (at, tg, d) in fixed.iter().take(60) {
            let line = format!("image+{:#x} -> {}{:+#x}", at - ibase, next_names[tg], d);
            println!("  {line}");
            chains.push(line);
        }
        if !fixed.is_empty() || next.is_empty() {
            break;
        }
        level = next;
        names = next_names;
    }
    let out = out_dir.join("chains.txt");
    let _ = std::fs::write(&out, chains.join("\n") + "\n");
    println!("saved {} ({} chains)", out.display(), chains.len());
}

/// `vi-rpc peek <hex addr> [...]` (or no address: every place in clock-again.txt): 64 bytes before and after each,
/// as u32 and f32 columns, to see what the game keeps next to a clock field.
pub fn peek(args: &[String]) {
    let Some(t) = Target::open("victory.exe") else {
        eprintln!("vi-rpc: victory.exe is not running");
        std::process::exit(1);
    };
    let out_dir = std::env::current_exe().unwrap_or_default().with_file_name("");
    let mut addrs: Vec<usize> = args.iter().filter_map(|a| usize::from_str_radix(a.trim_start_matches("0x"), 16).ok()).collect();
    if addrs.is_empty() {
        let text = std::fs::read_to_string(out_dir.join("clock-again.txt")).unwrap_or_default();
        addrs = text.lines().filter_map(|l| usize::from_str_radix(l.split_whitespace().next()?.trim_start_matches("0x"), 16).ok()).collect();
    }
    for a in addrs.iter().take(40) {
        let start = a.saturating_sub(64);
        let mut buf = [0u8; 128];
        let n = t.read(start, &mut buf);
        println!("== {:#x} ({})", a, t.owner(*a));
        let mut off = 0;
        while off + 16 <= n {
            let words: Vec<String> = (0..4).map(|k| {
                let b: [u8; 4] = buf[off + k * 4..off + k * 4 + 4].try_into().unwrap();
                let u = u32::from_le_bytes(b);
                let f = f32::from_le_bytes(b);
                if f.is_finite() && f.abs() > 1e-3 && f.abs() < 1e6 { format!("{u:>10} {f:>10.3}") } else { format!("{u:>10} {:>10}", "") }
            }).collect();
            let mark = if start + off <= *a && *a < start + off + 16 { " <-" } else { "" };
            println!("  {:#x}: {}{}", start + off, words.join(" |"), mark);
            off += 16;
        }
    }
}

/// `vi-rpc snap <label>`: saves the title image's bytes to `snap-<label>.bin` next to the exe (170 MB).
/// `vi-rpc diff <still1> <still2> <moved>`: offsets where the two still snapshots agree, and the moved one
/// differs by a position-like amount: three adjacent f32 (x, y, z) each within +-5000, the moved one 0.5..2000 away
/// from the still one in the plane. Prints them with the values, saves to diff.txt.
pub fn snap(args: &[String]) {
    let out_dir = std::env::current_exe().unwrap_or_default().with_file_name("");
    match args.first().map(String::as_str) {
        Some(label) if args.len() == 1 => {
            let Some(t) = Target::open("victory.exe") else {
                eprintln!("vi-rpc: victory.exe is not running");
                std::process::exit(1);
            };
            let img = t.image().expect("the title image is not loaded");
            let mut data = vec![0u8; img.size];
            let mut off = 0;
            while off < img.size {
                let want = CHUNK.min(img.size - off);
                let n = t.read(img.base + off, &mut data[off..off + want]);
                if n == 0 {
                    data[off..off + want].fill(0);
                }
                off += want;
            }
            let path = out_dir.join(format!("snap-{label}.bin"));
            std::fs::write(&path, &data).expect("write snapshot");
            println!("saved {} ({} MiB, image base {:#x})", path.display(), data.len() >> 20, img.base);
        }
        _ => {
            eprintln!("usage: vi-rpc snap <label>");
            std::process::exit(2);
        }
    }
}

pub fn diff(args: &[String]) {
    let out_dir = std::env::current_exe().unwrap_or_default().with_file_name("");
    if args.len() != 3 {
        eprintln!("usage: vi-rpc diff <still1> <still2> <moved>   (labels of snapshots made with `snap`)");
        std::process::exit(2);
    }
    let load = |l: &str| std::fs::read(out_dir.join(format!("snap-{l}.bin"))).unwrap_or_else(|e| { eprintln!("snap-{l}.bin: {e}"); std::process::exit(1) });
    let (a, b, c) = (load(&args[0]), load(&args[1]), load(&args[2]));
    let n = a.len().min(b.len()).min(c.len());
    let f = |d: &[u8], i: usize| f32::from_le_bytes(d[i..i + 4].try_into().unwrap());
    let mut hits = Vec::new();
    let mut i = 0;
    while i + 12 <= n {
        let (ax, ay, az) = (f(&a, i), f(&a, i + 4), f(&a, i + 8));
        if ax.is_finite() && ay.is_finite() && az.is_finite() && ax.abs() < 5000.0 && ay.abs() < 5000.0 && az.abs() < 5000.0
            && (ax != 0.0 || ay != 0.0) && a[i..i + 12] == b[i..i + 12]
        {
            let (cx, cy, cz) = (f(&c, i), f(&c, i + 4), f(&c, i + 8));
            if cx.is_finite() && cy.is_finite() && cz.is_finite() {
                let d = ((cx - ax).powi(2) + (cy - ay).powi(2)).sqrt();
                if d > 0.5 && d < 2000.0 && (cz - az).abs() < 200.0 {
                    hits.push((i, ax, ay, az, cx, cy, cz, d));
                }
            }
        }
        i += 4;
    }
    println!("{} position-like triples: still in both, moved {:.1}..2000 units in the third", hits.len(), 0.5);
    let mut text = String::new();
    for (i, ax, ay, az, cx, cy, cz, d) in hits.iter().take(80) {
        let line = format!("image+{i:#x}: still ({ax:.2}, {ay:.2}, {az:.2}) -> moved ({cx:.2}, {cy:.2}, {cz:.2}) d={d:.1}");
        println!("  {line}");
        text += &line;
        text += "\n";
    }
    let _ = std::fs::write(out_dir.join("diff.txt"), text);
    println!("saved {}", out_dir.join("diff.txt").display());
}

/// `vi-rpc boxes [x y]`: every place in memory where two float triples (min, max; adjacent, or with a 4th float
/// between as a padded vec4) form a box of 10..8000 m on a side that contains the point (the camera's position
/// read from the title when none is given), with the surrounding 16 u32 so a name hash can be spotted. Saves
/// boxes.txt. A zone record array shows up as many hits with one stride.
pub fn boxes(args: &[String]) {
    let Some(t) = Target::open("victory.exe") else {
        eprintln!("vi-rpc: victory.exe is not running");
        std::process::exit(1);
    };
    let out_dir = std::env::current_exe().unwrap_or_default().with_file_name("");
    let (px, py) = if args.len() >= 2 {
        (args[0].parse::<f32>().expect("x"), args[1].parse::<f32>().expect("y"))
    } else {
        let title = crate::title::Title::attach().expect("a known title layout");
        let p = title.camera().expect("the camera position");
        (p[0], p[1])
    };
    println!("looking for boxes around ({px:.1}, {py:.1})");
    let mut hits: Vec<(usize, usize, [f32; 6], Vec<u32>)> = Vec::new();   // (addr, layout 3|4, box, context)
    let f = |c: &[u8], i: usize| f32::from_le_bytes(c[i..i + 4].try_into().unwrap());
    let (bytes, complete) = t.sweep(budget(), |addr, chunk| {
        let mut i = 0;
        while i + 32 <= chunk.len() {
            let (ax, ay) = (f(chunk, i), f(chunk, i + 4));
            if ax.is_finite() && ay.is_finite() && ax <= px && ay <= py && ax > -20000.0 && ay > -20000.0 {
                for (stride, tag) in [(12usize, 3usize), (16, 4)] {
                    if i + stride + 12 > chunk.len() {
                        continue;
                    }
                    let (bx, by) = (f(chunk, i + stride), f(chunk, i + stride + 4));
                    if bx.is_finite() && by.is_finite() && bx >= px && by >= py {
                        let (w, h) = (bx - ax, by - ay);
                        // a real box: both corners inside the world and away from the origin (a zero corner is an
                        // entity position next to zeros), and the point not just inside but within the box's own
                        // extent of its centre, so huge boxes that cover everything are left out
                        let corners_ok = bx != 0.0 && by != 0.0 && ax != 0.0 && ay != 0.0 && bx < 20000.0 && by < 20000.0;
                        if corners_ok && (10.0..6000.0).contains(&w) && (10.0..6000.0).contains(&h) && hits.len() < 50_000 {
                            let az = f(chunk, i + 8);
                            let bz = f(chunk, i + stride + 8);
                            let ctx: Vec<u32> = (0..16).filter_map(|k| {
                                let o = i as isize - 32 + 4 * k as isize;
                                (o >= 0 && (o as usize) + 4 <= chunk.len()).then(|| u32::from_le_bytes(chunk[o as usize..o as usize + 4].try_into().unwrap()))
                            }).collect();
                            hits.push((addr + i, tag, [ax, ay, az, bx, by, bz], ctx));
                        }
                    }
                }
            }
            i += 4;
        }
    });
    println!("sweep: {:.1} GiB{}, {} boxes contain the point", bytes as f64 / (1u64 << 30) as f64, if complete { "" } else { " (budget hit)" }, hits.len());
    let mut text = String::new();
    for (a, tag, b, ctx) in &hits {
        let line = format!("{a:#x} ({}) min ({:.1}, {:.1}, {:.1}) max ({:.1}, {:.1}, {:.1}) size {:.0}x{:.0}  ctx {}",
            t.owner(*a), b[0], b[1], b[2], b[3], b[4], b[5], b[3] - b[0], b[4] - b[1],
            ctx.iter().map(|v| format!("{v:08x}")).collect::<Vec<_>>().join(" "));
        text += &line;
        text += "\n";
    }
    let _ = text;
    let _ = tag_unused(&hits);
    let path = out_dir.join("boxes.txt");
    let _ = std::fs::write(&path, &text);
    for (a, tag, b, _) in hits.iter().take(40) {
        println!("  {a:#x} ({}) vec{tag} min ({:.1}, {:.1}, {:.1}) max ({:.1}, {:.1}, {:.1}) size {:.0}x{:.0}", t.owner(*a), b[0], b[1], b[2], b[3], b[4], b[5], b[3] - b[0], b[4] - b[1]);
    }
    println!("saved {}", path.display());
}

fn tag_unused(_: &[(usize, usize, [f32; 6], Vec<u32>)]) {}

/// `vi-rpc udiff <same1> <same2> <changed>`: u32 fields of the image that are equal in the first two snapshots
/// and different in the third, which is how a "current area" field shows up when the third snapshot was taken
/// in another area. Values of 0 in any snapshot and float-looking values are left out. Saves udiff.txt.
pub fn udiff(args: &[String]) {
    let out_dir = std::env::current_exe().unwrap_or_default().with_file_name("");
    if args.len() != 3 {
        eprintln!("usage: vi-rpc udiff <same1> <same2> <changed>");
        std::process::exit(2);
    }
    let load = |l: &str| std::fs::read(out_dir.join(format!("snap-{l}.bin"))).unwrap_or_else(|e| { eprintln!("snap-{l}.bin: {e}"); std::process::exit(1) });
    let (a, b, c) = (load(&args[0]), load(&args[1]), load(&args[2]));
    let n = a.len().min(b.len()).min(c.len());
    let u = |d: &[u8], i: usize| u32::from_le_bytes(d[i..i + 4].try_into().unwrap());
    let floaty = |v: u32| { let f = f32::from_bits(v); f.is_finite() && f.abs() > 1e-6 && f.abs() < 1e7 && (v >> 23) & 0xff >= 0x60 };
    let mut hits = Vec::new();
    let mut i = 0;
    while i + 4 <= n {
        let (x, y, z) = (u(&a, i), u(&b, i), u(&c, i));
        if x == y && x != z && x != 0 && z != 0 && !floaty(x) && !floaty(z) {
            hits.push((i, x, z));
        }
        i += 4;
    }
    println!("{} u32 fields same in {} and {}, different in {}", hits.len(), args[0], args[1], args[2]);
    let mut text = String::new();
    for (i, x, z) in &hits {
        let line = format!("image+{i:#x}: {x:#x} ({x}) -> {z:#x} ({z})");
        text += &line;
        text += "\n";
    }
    for (i, x, z) in hits.iter().take(60) {
        println!("  image+{i:#x}: {x:#x} ({x}) -> {z:#x} ({z})");
    }
    let _ = std::fs::write(out_dir.join("udiff.txt"), text);
    println!("saved {}", out_dir.join("udiff.txt").display());
}
