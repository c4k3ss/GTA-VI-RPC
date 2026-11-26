"""Reads the camera block (image+0x74def10, 4x4 f32) from two image snapshots and prints the world position
under each hypothesis, so the one where a known walk (e.g. 100 m north = +Y in Rage) shows up as such wins."""
import struct, sys, os
SNAP = r"D:\Portfolio\gta6-rpc\target-scan\release"
BLOCK = 0x74def10

def mat(label):
    d = open(os.path.join(SNAP, f"snap-{label}.bin"), "rb").read()
    return [list(struct.unpack_from("<4f", d, BLOCK + 16 * r)) for r in range(4)]

def hyp(m):
    r = [m[i][:3] for i in range(3)]           # 3x3 rotation, rows
    t = m[3][:3]
    # A: last row is the world position itself
    a = t
    # B: view matrix, row-major: pos = -R^T t
    b = [-(r[0][i] * t[0] + r[1][i] * t[1] + r[2][i] * t[2]) for i in range(3)]
    # C: view matrix, column-major: pos = -R t
    c = [-(r[i][0] * t[0] + r[i][1] * t[1] + r[i][2] * t[2]) for i in range(3)]
    return {"row4 as is": a, "-R^T t": b, "-R t": c}

if __name__ == "__main__":
    labels = sys.argv[1:] or ["north1", "north2"]
    ms = [mat(l) for l in labels]
    for l, m in zip(labels, ms):
        print(f"== {l}")
        for row in m:
            print("   " + " ".join(f"{v:10.3f}" for v in row))
    h = [hyp(m) for m in ms]
    print("== world position under each hypothesis, and the move between the two snapshots")
    for name in h[0]:
        p0, p1 = h[0][name], h[-1][name]
        d = [p1[i] - p0[i] for i in range(3)]
        dist = (d[0] ** 2 + d[1] ** 2) ** 0.5
        print(f"  {name:12s}: {tuple(round(v, 1) for v in p0)} -> {tuple(round(v, 1) for v in p1)}  move dx={d[0]:.1f} dy={d[1]:.1f} dz={d[2]:.1f} |xy|={dist:.1f}")
