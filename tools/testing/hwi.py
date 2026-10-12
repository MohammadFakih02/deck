"""Reads HWiNFO's shared memory (Global\\HWiNFO_SENS_SM2).
usage:
  hwi.py list [filter]                 every reading (sensor / label / value unit)
  hwi.py watch SECONDS pat1 pat2 ...   samples once a second, prints min/avg/max per matching reading
"""
import ctypes, struct, sys, time, re
from ctypes import wintypes

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.OpenFileMappingW.restype = wintypes.HANDLE
k32.OpenFileMappingW.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.LPCWSTR]
k32.MapViewOfFile.restype = ctypes.c_void_p
k32.MapViewOfFile.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, ctypes.c_size_t]
FILE_MAP_READ = 4


def snapshot():
    h = None
    for name in ("Global\\HWiNFO_SENS_SM2", "HWiNFO_SENS_SM2"):
        h = k32.OpenFileMappingW(FILE_MAP_READ, False, name)
        if h:
            break
    if not h:
        sys.exit("HWiNFO shared memory not found: enable Settings > Shared Memory Support in HWiNFO")
    p = k32.MapViewOfFile(h, FILE_MAP_READ, 0, 0, 0)
    hdr = ctypes.string_at(p, 44)
    sig, ver, rev, poll, off_s, sz_s, n_s, off_r, sz_r, n_r = struct.unpack("<3Iq6I", hdr)
    total = off_r + sz_r * n_r
    raw = ctypes.string_at(p, total)
    k32.UnmapViewOfFile(ctypes.c_void_p(p))
    k32.CloseHandle(h)
    s = lambda b: b.split(b"\0", 1)[0].decode("latin-1")
    sensors = []
    for i in range(n_s):
        o = off_s + i * sz_s
        sensors.append(s(raw[o + 8 + 128:o + 8 + 256]) or s(raw[o + 8:o + 8 + 128]))
    out = []
    for i in range(n_r):
        o = off_r + i * sz_r
        typ, si, rid = struct.unpack_from("<3I", raw, o)
        label = s(raw[o + 12 + 128:o + 12 + 256]) or s(raw[o + 12:o + 12 + 128])
        unit = s(raw[o + 268:o + 284])
        val, vmin, vmax, vavg = struct.unpack_from("<4d", raw, o + 284)
        out.append((sensors[si] if si < len(sensors) else "?", label, unit, val))
    return out


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "list"
    if cmd == "list":
        f = re.compile(sys.argv[2], re.I) if len(sys.argv) > 2 else None
        for sen, lab, unit, v in snapshot():
            line = f"{sen} | {lab} = {v:.2f} {unit}"
            if not f or f.search(line):
                print(line)
    elif cmd == "watch":
        secs = float(sys.argv[2]); pats = [re.compile(x, re.I) for x in sys.argv[3:]]
        acc = {}
        end = time.time() + secs
        while time.time() < end:
            for sen, lab, unit, v in snapshot():
                key = f"{sen} | {lab}"
                if any(pt.search(key) for pt in pats):
                    acc.setdefault(key, [unit, []])[1].append(v)
            time.sleep(1)
        for k, (u, vals) in acc.items():
            print(f"{k}: min {min(vals):.1f} avg {sum(vals)/len(vals):.1f} max {max(vals):.1f} {u} (n={len(vals)})")
