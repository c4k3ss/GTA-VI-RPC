import struct, sys
EXE = r"D:\GTA VI Xbox\game_scarlett_master.exe"
def sections(data):
    pe = struct.unpack_from("<I", data, 0x3c)[0]
    nsec = struct.unpack_from("<H", data, pe + 6)[0]
    opt = struct.unpack_from("<H", data, pe + 20)[0]
    base = struct.unpack_from("<Q", data, pe + 24 + 24)[0]
    secs = []
    off = pe + 24 + opt
    for i in range(nsec):
        name = data[off:off+8].rstrip(b"\0").decode(errors="replace")
        vsize, va, rsize, raw = struct.unpack_from("<IIII", data, off + 8)
        secs.append((name, va, vsize, raw, rsize)); off += 40
    return base, secs
def off2rva(secs, o):
    for n, va, vs, raw, rs in secs:
        if raw <= o < raw + rs: return va + (o - raw)
def rva2off(secs, r):
    for n, va, vs, raw, rs in secs:
        if va <= r < va + max(vs, rs): return raw + (r - va)
if __name__ == "__main__":
    data = open(EXE, "rb").read()
    base, secs = sections(data)
    print(f"image base {base:#x}, size {len(data):#x}")
    for s in secs: print(f"  {s[0]:8s} rva {s[1]:#010x} vsize {s[2]:#010x} raw {s[3]:#010x} rsize {s[4]:#010x}")
    for needle in [b"Current Mission Name", b"Camera Position", b"Game Time", b"Current SRL Name", b"Game State", b"Game Running Time"]:
        i = data.find(needle); hits = []
        while i != -1 and len(hits) < 4:
            hits.append(i); i = data.find(needle, i + 1)
        print(needle.decode(), ["%#x (rva %#x)" % (h, off2rva(secs, h) or 0) for h in hits])
