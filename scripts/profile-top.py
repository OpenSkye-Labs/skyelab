#!/usr/bin/env python3
"""Top functions of a samply profile, without the Firefox Profiler (docs/debugging.md, "Performance runs").

    scripts/profile-top.py <profile.json.gz> [thread name, default the main thread] [rows, default 40]

Needs the `.syms.json` sidecar that `samply record --unstable-presymbolicate` writes (scripts/perf.sh does).
Counts only samples where the thread was on CPU, and prints inclusive ("total") and self time per function.
`SPAN=50-57` keeps only samples from 50 s to 57 s after the thread's first sample (a slow stretch the trace showed).
"""
import os
import bisect, collections, gzip, json, re, sys

path = sys.argv[1]
want = sys.argv[2] if len(sys.argv) > 2 else None
rows = int(sys.argv[3]) if len(sys.argv) > 3 else 40
prof = json.load(gzip.open(path))
syms = json.load(open(path.removesuffix(".gz") + ".syms.json"))
names = syms["string_table"]
tables = {}
for lib in syms["data"]:
    st = sorted(lib["symbol_table"], key=lambda e: e["rva"])
    tables[lib["debug_name"]] = ([e["rva"] for e in st], st)

def symbol(lib, addr):
    rvas, st = tables.get(lib, ([], []))
    i = bisect.bisect_right(rvas, addr) - 1
    if i < 0 or addr >= rvas[i] + st[i]["size"]:
        return f"{lib}+{addr:#x}"
    return names[st[i]["symbol"]]

def short(name):
    name = re.sub(r"::h[0-9a-f]{16}$", "", name)
    return name if len(name) < 140 else name[:137] + "..."

threads = prof["threads"]
t = next(x for x in threads if (x["name"] == want if want else x.get("isMainThread")))
libs = prof["libs"]
ft, fn, rt, stk = t["frameTable"], t["funcTable"], t["resourceTable"], t["stackTable"]
frame_name = {}
def fname(f):
    if f not in frame_name:
        res = fn["resource"][ft["func"][f]]
        lib = libs[rt["lib"][res]]["debugName"] if res is not None and res >= 0 else "?"
        frame_name[f] = short(symbol(lib, ft["address"][f]))
    return frame_name[f]

total, self_ = collections.Counter(), collections.Counter()
s = t["samples"]
cpu = s.get("threadCPUDelta") or [1] * s["length"]
times = s.get("time") or [0] * s["length"]
lo, hi = (float(x) * 1000 for x in os.environ.get("SPAN", "0-1e12").split("-"))
n = 0
for st, d, at in zip(s["stack"], cpu, times):
    if st is None or not d or not lo <= at - times[0] <= hi:
        continue
    n += 1
    seen = set()
    first = True
    while st is not None:
        name = fname(stk["frame"][st])
        if first:
            self_[name] += 1
            first = False
        if name not in seen:
            total[name] += 1
            seen.add(name)
        st = stk["prefix"][st]
print(f"{t['name']}: {n} on-CPU samples")
print("-- inclusive")
for name, c in total.most_common(rows):
    print(f"{100 * c / n:5.1f}%  {name}")
print("-- self")
for name, c in self_.most_common(rows // 2):
    print(f"{100 * c / n:5.1f}%  {name}")
