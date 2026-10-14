#!/usr/bin/env python3
"""Measure one Linux process; CPU percentage is relative to one logical core."""
import argparse
import json
import os
from pathlib import Path
import time

p = argparse.ArgumentParser()
p.add_argument("pid", type=int)
p.add_argument("--seconds", type=float, default=60)
p.add_argument("--label", default="")
a = p.parse_args()
hz = os.sysconf("SC_CLK_TCK")
proc = Path("/proc") / str(a.pid)

def sample():
    fields = (proc / "stat").read_text().rsplit(")", 1)[1].split()
    status = (proc / "status").read_text().splitlines()
    rss = next(int(line.split()[1]) for line in status if line.startswith("VmRSS:"))
    return int(fields[11]) + int(fields[12]), rss

start_cpu, initial_rss = sample()
start = time.monotonic()
peak = initial_rss
while time.monotonic() - start < a.seconds:
    time.sleep(min(1, max(0.01, a.seconds - (time.monotonic() - start))))
    _, rss = sample()
    peak = max(peak, rss)
end_cpu, final_rss = sample()
elapsed = time.monotonic() - start
print(json.dumps({"label": a.label, "pid": a.pid, "seconds": elapsed, "cpu_percent_one_core": (end_cpu-start_cpu)/hz/elapsed*100, "initial_rss_mib": initial_rss/1024, "final_rss_mib": final_rss/1024, "peak_rss_mib": peak/1024}, indent=2))
