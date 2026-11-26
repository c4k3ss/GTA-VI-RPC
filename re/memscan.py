"""Scans the running victory.exe (the title mapped inside it) for the crash-context strings and the live state
values, to find where they live when the strings are not in the exe on disk. Read-only: ReadProcessMemory.
Usage: python -I memscan.py [needle ...]   (default needles below). Prints module bases and every hit with the
module + offset it falls in, so a hit inside victory-image-*.dll becomes a stable image offset."""
import ctypes, ctypes.wintypes as w, sys, struct

k32 = ctypes.windll.kernel32
PROCESS_QUERY_INFORMATION, PROCESS_VM_READ = 0x400, 0x10
MEM_COMMIT, PAGE_NOACCESS, PAGE_GUARD = 0x1000, 0x01, 0x100

class MBI(ctypes.Structure):
    _fields_ = [("BaseAddress", ctypes.c_void_p), ("AllocationBase", ctypes.c_void_p), ("AllocationProtect", w.DWORD),
                ("PartitionId", w.WORD), ("RegionSize", ctypes.c_size_t), ("State", w.DWORD), ("Protect", w.DWORD),
                ("Type", w.DWORD)]

class ME32(ctypes.Structure):
    _fields_ = [("dwSize", w.DWORD), ("th32ModuleID", w.DWORD), ("th32ProcessID", w.DWORD), ("GlblcntUsage", w.DWORD),
                ("ProccntUsage", w.DWORD), ("modBaseAddr", ctypes.c_void_p), ("modBaseSize", w.DWORD),
                ("hModule", ctypes.c_void_p), ("szModule", ctypes.c_char * 256), ("szExePath", ctypes.c_char * 260)]

class PE32(ctypes.Structure):
    _fields_ = [("dwSize", w.DWORD), ("cntUsage", w.DWORD), ("th32ProcessID", w.DWORD), ("th32DefaultHeapID", ctypes.c_void_p),
                ("th32ModuleID", w.DWORD), ("cntThreads", w.DWORD), ("th32ParentProcessID", w.DWORD), ("pcPriClassBase", ctypes.c_long),
                ("dwFlags", w.DWORD), ("szExeFile", ctypes.c_char * 260)]

def find_pid(name=b"victory.exe"):
    snap = k32.CreateToolhelp32Snapshot(0x2, 0); e = PE32(); e.dwSize = ctypes.sizeof(PE32)
    ok = k32.Process32First(snap, ctypes.byref(e)); pid = None
    while ok:
        if e.szExeFile.lower() == name: pid = e.th32ProcessID; break
        ok = k32.Process32Next(snap, ctypes.byref(e))
    k32.CloseHandle(snap); return pid

def modules(pid):
    snap = k32.CreateToolhelp32Snapshot(0x8 | 0x10, pid); m = ME32(); m.dwSize = ctypes.sizeof(ME32); out = []
    ok = k32.Module32First(snap, ctypes.byref(m))
    while ok:
        out.append((m.szModule.decode(errors="replace"), m.modBaseAddr, m.modBaseSize)); ok = k32.Module32Next(snap, ctypes.byref(m))
    k32.CloseHandle(snap); return out

def regions(h):
    addr = 0; mbi = MBI()
    k32.VirtualQueryEx.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.POINTER(MBI), ctypes.c_size_t]
    while k32.VirtualQueryEx(h, addr, ctypes.byref(mbi), ctypes.sizeof(mbi)):
        if mbi.State == MEM_COMMIT and not (mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD)):
            yield (mbi.BaseAddress or 0), mbi.RegionSize
        addr = (mbi.BaseAddress or 0) + mbi.RegionSize
        if addr >= 0x7fffffffffff: break

def read(h, addr, size):
    buf = (ctypes.c_char * size)(); n = ctypes.c_size_t()
    k32.ReadProcessMemory.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
    if k32.ReadProcessMemory(h, addr, buf, size, ctypes.byref(n)): return buf.raw[:n.value]
    return b""

def main():
    needles = [a.encode() for a in sys.argv[1:]] or [b"Current Mission Name", b"Camera Position", b"Game Time: (", b"fbk_stry", b"SYSTEM INFO"]
    pid = find_pid()
    if not pid: print("victory.exe is not running"); return
    h = k32.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
    mods = modules(pid)
    for name, b, s in mods:
        if "victory" in name.lower(): print(f"module {name} base {b:#x} size {s:#x}")
    def owner(a):
        for name, b, s in mods:
            if b <= a < b + s: return f"{name}+{a-b:#x}"
        return "heap"
    total = 0
    for base, size in regions(h):
        for off in range(0, size, 1 << 24):
            chunk = read(h, base + off, min(1 << 24, size - off)); total += len(chunk)
            for nd in needles:
                i = chunk.find(nd)
                while i != -1:
                    a = base + off + i; ctx = chunk[i:i+80].split(b"\0")[0]
                    print(f"{nd.decode():22s} at {a:#x} ({owner(a)}): {ctx[:80]!r}")
                    i = chunk.find(nd, i + 1)
    print(f"scanned {total/2**30:.1f} GiB")

if __name__ == "__main__":
    main()
