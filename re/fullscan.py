import sys, time
sys.path.insert(0, r"D:\Portfolio\gta6-rpc\re")
import memscan as m
pid = m.find_pid(); h = m.k32.OpenProcess(0x410, False, pid); mods = m.modules(pid)
def owner(a):
    for name, b, s in mods:
        if b <= a < b + s: return f"{name}+{a-b:#x}"
    return "heap"
needles = [b"Current Mission Name", b"Camera Position", b"Game Time: (", b"fbk_stry", b"SYSTEM INFO", b"Current SRL", b"_INTRO"]
t0 = time.time(); total = 0; hits = 0
regs = [(b, s) for b, s in m.regions(h) if s <= (4 << 30)]
print(f"regions {len(regs)}, bytes {sum(s for _, s in regs)/2**30:.1f} GiB", flush=True)
for base, size in regs:
    if any(b <= base < b + s for _, b, s in mods if b): continue   # modules scanned separately
    for off in range(0, size, 1 << 24):
        chunk = m.read(h, base + off, min(1 << 24, size - off)); total += len(chunk)
        for nd in needles:
            i = chunk.find(nd)
            while i != -1 and hits < 200:
                a = base + off + i; ctx = chunk[max(0, i-24):i+90].split(b"\0")
                print(f"{nd.decode():22s} {a:#x} ({owner(a)}): {chunk[i:i+90].split(b'\0')[0]!r}", flush=True); hits += 1
                i = chunk.find(nd, i + 1)
    if total and int(total / 2**30) != int((total - size) / 2**30): print(f"... {total/2**30:.0f} GiB, {time.time()-t0:.0f} s", flush=True)
print(f"done: {total/2**30:.1f} GiB in {time.time()-t0:.0f} s, hits {hits}", flush=True)
