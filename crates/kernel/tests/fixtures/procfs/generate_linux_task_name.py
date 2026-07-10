#!/usr/bin/env python3
"""Regenerate linux-*-task-name.json on a real Linux host."""

import ctypes
import json
import os
import platform


TASK_NAME = "agent)os\nproc"
PR_SET_NAME = 15

libc = ctypes.CDLL(None)
if libc.prctl(PR_SET_NAME, TASK_NAME.encode(), 0, 0, 0) != 0:
    raise OSError(ctypes.get_errno(), "prctl(PR_SET_NAME) failed")

pid = os.getpid()
with open(f"/proc/{pid}/comm", encoding="utf-8") as source:
    comm = source.read()
with open(f"/proc/{pid}/stat", encoding="utf-8") as source:
    stat = source.read()
with open(f"/proc/{pid}/status", encoding="utf-8") as source:
    status = source.read()

right_paren = stat.rfind(") ")
status_name = next(
    line.split("\t", 1)[1] for line in status.splitlines() if line.startswith("Name:\t")
)
print(
    json.dumps(
        {
            "source": f"{platform.system()} {platform.release()} {platform.machine()}",
            "task_name_input": TASK_NAME,
            "comm": comm,
            "stat_comm": stat[stat.find(" (") + 2 : right_paren],
            "status_name": status_name,
        },
        indent=2,
    )
)
