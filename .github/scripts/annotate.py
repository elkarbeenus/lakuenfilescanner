#!/usr/bin/env python3
"""Turn compiler errors from build logs into GitHub annotations, so they are
readable from the public check-run API without downloading logs."""
import re
import sys

count = 0
for name in sys.argv[1:]:
    try:
        lines = open(name, encoding="utf-8", errors="replace").read().splitlines()
    except OSError:
        continue
    lines = [re.sub(r"\x1b\[[0-9;]*m", "", l) for l in lines]
    blocks = []
    i = 0
    while i < len(lines):
        l = lines[i]
        if re.match(r"^(error(\[E\d+\])?:|error:|thread .* panicked|npm ERR!|Error )", l.strip()):
            block = lines[i:i + 18]
            blocks.append("\n".join(block))
            i += 18
        else:
            i += 1
    if not blocks:
        blocks = ["\n".join(lines[-60:])]
    for b in blocks[:9]:
        msg = b.replace("%", "%25").replace("\r", "").replace("\n", "%0A")
        print(f"::error title={name}::{msg[:6000]}")
        count += 1
print(f"annotated {count} block(s)")
