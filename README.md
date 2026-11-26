# vi-rpc

Discord Rich Presence for the Xbox Series build of Grand Theft Auto VI running on a PC through a
[VIctory](https://github.com/Marvisak/VIctory)-lineage layer (VIctory itself, victory-fork, Belobug, DexNom,
AvatarManz, CGBJ, Flamingo, VItoriaBr and their forks).

```
Grand Theft Auto VI
Playing Story Mode
Somewhere in Leonida · 5:17 PM
15:06 elapsed
```

It is a separate program. It patches nothing, injects nothing, and ships no game data: it reads the layer's
window title (frame rate and draw count, which tell menus from the world) and, for a known game build, a few
values from the running process's memory (the in-game clock; the camera position, kept for later). Everything
in this repository is offsets and code.

## Use

1. Build it (Rust stable, Windows): `cargo build --release`. The exe is `target\release\vi-rpc.exe`.
2. Make a Discord application at <https://discord.com/developers/applications>: its name is what Discord shows
   as "Playing ...", and under Rich Presence > Art Assets upload one image named `game`.
3. Put `vi-rpc.toml` next to the exe:
   ```toml
   app_id = "<your application id>"
   interval = 15          # seconds between updates; Discord allows one per 15 s
   ```
4. Run `vi-rpc.exe` while the game runs (or before: it waits for the game and clears the presence when the
   game exits). A layer's launcher can start it, e.g. `start "" vi-rpc.exe` before the `victory.exe run ...`
   line.

## What it reads

| Shown | Source |
|---|---|
| menus / loading / world, fps | the layer's window title, `<title> \| VIctory <fps> fps <ms> ms/frame <draws> draws/frame` |
| the in-game clock | the title image, build `1.0.2.1` (`victory-image-b99f876b74d4a77f.dll` as the layer maps it): six `u32` at `+0x74df88c` (day, month, year, hour, minute, second) |
| the camera position (console only for now) | the view-projection matrix at `+0x74def10`: world position = `-t * inv(R)` |

Other game builds need their own offsets: the image's name carries the build's hash, and `src/title.rs` maps
hashes to layouts. Other layers need the window title pattern and the image's name added in `src/layer.rs` and
`src/title.rs`.

## Finding offsets (`vi-rpc scan`, `clock`, `text`, `find`, `chase`, `peek`, `snap`, `diff`, `udiff`, `boxes`)

The same exe holds the scanner that found the offsets: value scans narrowed over time (`clock --image 17 10`,
then `--again` as the clock moves), string scans, pointer chases and snapshot diffs of the title image. The
`*.bat` files in the repository root drive them; `PLAN.md` has the notes. They read the process only.

## Licence

MIT. VIctory and its forks are GPL-3.0; this tool contains none of their code.
