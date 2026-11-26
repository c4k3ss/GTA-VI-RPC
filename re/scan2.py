import sys, time, re, collections
sys.path.insert(0, r"D:\Portfolio\gta6-rpc\re")
import memscan as m
OUT = r"C:\Users\iusti\AppData\Local\Temp\claude\d49ce1bac6d242a4\41c8908d-fd2f-4c56-8ac3-43407ea804e8\scratchpad"
pid = m.find_pid(); h = m.k32.OpenProcess(0x410, False, pid); mods = m.modules(pid)
imgs = [x for x in mods if x[0].startswith("victory-image-b99f")]
if not imgs: print("the game is not running (no title image module)"); sys.exit(1)
ibase = imgs[0][1]
# 1. the image's .data (globals) and .rdata, saved for offline pointer searches (scratch only)
for name, rva, size in [(".data", 0x67e2000, 0x43ec398), (".rdata", 0x5f41000, 0x8a0c0c)]:
    buf = b"".join(m.read(h, ibase + rva + o, min(1 << 24, size - o)) for o in range(0, size, 1 << 24))
    open(f"{OUT}/image{name}.bin", "wb").write(buf); print(f"saved {name}: {len(buf):#x} bytes", flush=True)
# 2. every string that looks like a mission/scene/script name, with its address, deduped by text
pat = re.compile(rb"[A-Za-z0-9_]{0,40}(?:FBK_STRY|fbk_stry|_INTRO_|PROLOGUE|_STRY0|STRY00)[A-Za-z0-9_.]{0,60}")
seen = collections.defaultdict(list); t0 = time.time(); total = 0
regs = [(b, s) for b, s in m.regions(h) if s <= (4 << 30) and not any(mb <= b < mb + ms for _, mb, ms in mods if mb)]
for base, size in regs:
    for off in range(0, size, 1 << 24):
        chunk = m.read(h, base + off, min(1 << 24, size - off)); total += len(chunk)
        for mt in pat.finditer(chunk):
            s = mt.group()
            if len(seen[s]) < 6: seen[s].append(base + off + mt.start())
print(f"scanned {total/2**30:.1f} GiB in {time.time()-t0:.0f} s; distinct strings {len(seen)}", flush=True)
with open(f"{OUT}/scan2_strings.txt", "w", encoding="utf-8") as f:
    for s, addrs in sorted(seen.items(), key=lambda kv: -len(kv[1])):
        f.write(f"{len(addrs):3d} {s.decode(errors='replace')[:90]:92s} {' '.join(f'{a:#x}' for a in addrs[:4])}\n")
print("written scan2_strings.txt", flush=True)
