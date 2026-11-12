#!/usr/bin/env python3
"""Private fixture group anchor; never imports the SDK or starts a build."""

import argparse
import ctypes
import json
import os
import signal
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--status", required=True)
    parser.add_argument("--max-ms", type=int, required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or len(command) > 64 or sum(map(len, command)) > 65536:
        parser.error("ERR_FIXTURE_ARGV_LIMIT: require 1..64 arguments totaling at most 65536 characters")
    if not 1000 <= args.max_ms <= 900000:
        parser.error("ERR_FIXTURE_DEADLINE: --max-ms must be 1000..900000")
    if os.getpid() != os.getpgrp():
        raise RuntimeError("ERR_FIXTURE_GROUP: supervisor must own its detached process group")
    libc = ctypes.CDLL(None, use_errno=True)
    libc.prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong]
    libc.prctl.restype = ctypes.c_int
    if libc.prctl(36, 1, 0, 0, 0) != 0:  # Linux PR_SET_CHILD_SUBREAPER, local <linux/prctl.h>.
        raise OSError(ctypes.get_errno(), "PR_SET_CHILD_SUBREAPER")

    soft_stop = False
    hard_stop = False

    def request_soft(_number, _frame):
        nonlocal soft_stop
        soft_stop = True

    def request_hard(_number, _frame):
        nonlocal hard_stop
        hard_stop = True

    signal.signal(signal.SIGUSR1, request_soft)
    signal.signal(signal.SIGTERM, request_hard)
    signal.signal(signal.SIGINT, request_hard)
    signal.signal(signal.SIGHUP, request_hard)
    child = subprocess.Popen(command, stdin=subprocess.DEVNULL)
    started = time.monotonic()
    with open(f"/proc/{child.pid}/stat", encoding="ascii") as source:
        child_stat = source.read(65536)
    child_start_ticks = child_stat[child_stat.rindex(")") + 2:].split()[19]
    state = {
        "supervisorPid": os.getpid(), "pgid": os.getpgrp(), "childPid": child.pid,
        "childExit": None, "childStartTicks": child_start_ticks, "reaped": [], "deadlineReached": False,
        "softStopSent": False, "groupStopSent": False, "forcedGroupKill": False,
    }

    def publish():
        temporary = args.status + ".tmp"
        with open(temporary, "w", encoding="utf-8") as target:
            json.dump(state, target)
            target.write("\n")
        os.replace(temporary, args.status)

    publish()
    group_stop_at = None
    while True:
        no_children = False
        while True:
            try:
                pid, status = os.waitpid(-1, os.WNOHANG)
            except ChildProcessError:
                no_children = True
                break
            if pid == 0:
                break
            code = os.waitstatus_to_exitcode(status)
            if pid == child.pid:
                child.returncode = code
                state["childExit"] = code
            if len(state["reaped"]) >= 256:
                print("ERR_FIXTURE_CHILD_LIMIT: exceeded 256 owned child exits", file=sys.stderr)
                hard_stop = True
                state["childLimitReached"] = True
                publish()
                break
            else:
                state["reaped"].append({"pid": pid, "exit": code})
            publish()
        if time.monotonic() - started >= args.max_ms / 1000:
            state["deadlineReached"] = True
            hard_stop = True
        if soft_stop and not state["softStopSent"]:
            state["softStopSent"] = True
            # Only this supervisor reaps the child, so its PID cannot be reused
            # between this test and kill while childExit is still unknown.
            if state["childExit"] is None:
                try:
                    os.kill(child.pid, signal.SIGTERM)
                except ProcessLookupError as error:
                    # A just-exited child is still ours and is reaped above.
                    state["childSignalRace"] = str(error)
            publish()
        if hard_stop and not state["groupStopSent"]:
            state["groupStopSent"] = True
            group_stop_at = time.monotonic()
            publish()
            os.killpg(os.getpgrp(), signal.SIGTERM)
        if state["groupStopSent"] and no_children:
            publish()
            return 0
        if group_stop_at is not None and time.monotonic() - group_stop_at >= 5:
            state["forcedGroupKill"] = True
            publish()
            # The live group leader anchors this identity until the final kill.
            # Never scan or signal unrelated host processes.
            os.killpg(os.getpgrp(), signal.SIGKILL)
            raise RuntimeError("SIGKILL unexpectedly returned")
        time.sleep(0.02)


if __name__ == "__main__":
    sys.exit(main())
