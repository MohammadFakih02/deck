import mmap, struct
h = mmap.mmap(-1, 40, "RTSSSharedMemoryV2", access=mmap.ACCESS_READ)
_,_,a1,a2,a3,o1,o2,o3,_ = struct.unpack_from("<4sIIIIIIII", h, 0)
m = mmap.mmap(-1, max(a2+a1*a3, o2+o1*o3), "RTSSSharedMemoryV2", access=mmap.ACCESS_READ)
sig, ver, app_sz, app_off, app_n, osd_sz, osd_off, osd_n, frame = struct.unpack_from("<4sIIIIIIII", m, 0)
print(sig, hex(ver), "osd slots", osd_n)
for i in range(osd_n):
    b = osd_off + i*osd_sz
    owner = m[b+256:b+512].split(b"\0")[0].decode(errors="replace")
    if owner:
        ex = m[b+512:b+512+4096].split(b"\0")[0].decode(errors="replace")
        txt = ex or m[b:b+256].split(b"\0")[0].decode(errors="replace")
        print(i, repr(owner), len(txt), repr(txt[:300]))
