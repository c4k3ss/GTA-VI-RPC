# GTA VI Discord Rich Presence (working name: vi-rpc)

A standalone Windows tool that shows what you are doing in the Xbox build of GTA VI running under a VIctory-lineage
layer (CGBJ, AvatarManz, victory-fork, ...) as Discord Rich Presence. It ships no game data and patches nothing: it
reads the running process's memory, the way an external trainer or a stats overlay does.

## What it shows

| Field | Source | Example |
|---|---|---|
| Large image / state | Scene kind from the layer: menu, loading, world | "In the world" |
| Details | Mission name (hash -> name table) and stage | "Gone Fishing" |
| Small text | In-game clock and district (camera position -> district polygon) | "13:25 · Vice City" |
| Party / extra | Frame rate from the layer's window title; time played | "22 fps" |

## Where the data is

1. **The layer's window title**: `Americas | VIctory 8 fps 129.9 ms/frame 7577 draws/frame`. Free; gives fps and
   draws, and "menu vs world" by draw count (menus ~90, world thousands).
2. **The game's own state**: its crash writer prints, from fixed places in memory, exactly the fields we want:
   `Game State`, `Current Mission Name: 0x98c85f2a`, `Current SRL Name: fbk_stry000_FBK_STRY000_INTRO`,
   `Camera Position: <x, y, z>`, `Game Time: (13:25)`, `Frame`. Finding where that writer reads them gives us
   stable addresses relative to the title image. The title is mapped by the layer as a module named
   `victory-image-<hash>.dll` (hash = blake3 of the exe), so `base = module base of victory-image-*.dll` and the
   offsets are the same for everyone on build 1.0.2.1.
3. **Mission hash -> name**: the hashes are Jenkins-style (the Rage engine's `joaat`); names come from the
   strings in the same image (mission script names like `fbk_stry000`) and from a table we grow while playing.

## Steps

1. **Locate the fields** (reverse engineering on the user's own copy, read-only): find the crash writer's format
   strings ("Current Mission Name", "Camera Position", "Game Time") in the image, follow the code that formats
   them to the globals it reads. Output: a small table `{field: image offset, type}`.
2. **Reader**: a Rust program (`windows-sys`: `CreateToolhelp32Snapshot`, `ReadProcessMemory`) that finds
   `victory.exe`, the `victory-image-*.dll` module base, reads the fields every 2 s, and the window title.
3. **Presence**: `discord-rich-presence` crate over Discord's IPC pipe; needs the user's own Discord application id
   (same as the FL Studio RPC). Rate: at most one update per 15 s, as Discord allows.
4. **Names**: `missions.toml` (hash -> title, stage names), `districts.toml` (polygons in game coordinates),
   both built up from play; unknown hash shows "Mission 0x98c85f2a".
5. **Packaging**: one exe, a tray icon, starts with the launcher (`run.bat` can start it); works with every fork.

## Reference look (the user's example, 2026-11-26)

    Grand Theft Auto VI
    Playing Story Mode
    Standing Near Port Gelhorn
    15:06 (elapsed)

So: details = "Playing Story Mode" (or the mission's name while on one), state = "<Standing|On foot|Driving|...> near
<place>", place from the camera position against the title's zone names (find the zone-name table in memory, step 1).

## Not in scope

- Anything inside the layer or the game. No injection, no patches, no game files in the repo.
- Online features; this is presence only.

## Open questions

- Does the user want the reader in Rust or C# (the FL Studio RPC's language)?
- Discord application id and art assets (a large image for the game; Discord needs them uploaded to the app).

## Street / area: what the 2026-11-28 session found

- The current street is not kept as a fixed string, nor a pointer to the text DB string or record, nor a joaat
  of any spelling (full-process `find`/`text` sweeps and `--again` narrowing on three streets all came back 0).
- Street names live in the text DB as arrays of 24-byte small strings (15 chars inline + length|0x80) at
  0x92fad... (alphabetical, in chunks) and 0x92fa6... (load order); `dump 0x92fa00000 0x200000 pool.bin`
  gives them all; "Palmetto Ave" also appears in 29 per-language copies around 0x92fb45000.
- The HUD street label is a TRANSIENT heap string: page-hash sweeps (`pages a1/a2`, move, `pages b`,
  `pdiff`, still `cand c`, move, `cand d`, still `cand e`) showed the 16-byte std::string "Palmetto Ave" at
  0x936b87500 in a small-object allocator page, turned into a free-list entry once the label faded. Two more
  blocks on the same page ({ptr 0x938b77540, 0x230022} at +0x6c0, 0xd7c91750 at +0x700) freed with it.
- Next: a live watcher. Poll the allocator region (0x936b00000 +- a few MB) for any of the 615 table names;
  on a hit, scan the UI heap (0x93...-0x94...) for the pointer to that block; `chase` that owner to a fixed
  image offset. Read the string while shown, keep the last value. The area label should behave the same.
