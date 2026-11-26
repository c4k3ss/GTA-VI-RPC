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
