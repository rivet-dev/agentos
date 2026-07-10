#!/usr/bin/env python3
"""Capture a real-Linux ps/pgrep fixture for the procps-ng VM e2e."""

import json
import os
import platform
import signal
import shutil
import subprocess
import tempfile
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo


def version_line(*command: str) -> str:
    result = subprocess.run(command, check=True, capture_output=True, text=True)
    return (result.stdout or result.stderr).splitlines()[0]


def main() -> None:
    with tempfile.TemporaryDirectory() as temp_dir:
        name = "aos-procfx"
        executable = Path(temp_dir) / name
        executable.symlink_to(shutil.which("sleep"))
        child = subprocess.Popen([executable, "30"])
        try:
            ps = subprocess.run(
                ["ps", "-o", "pid,ppid,stat,comm,args", "-p", str(child.pid)],
                check=True,
                capture_output=True,
                text=True,
            )
            pgrep = subprocess.run(
                ["pgrep", "-x", name],
                check=True,
                capture_output=True,
                text=True,
            )
            pstree = subprocess.run(
                ["pstree", "-p", str(child.pid)],
                check=True,
                capture_output=True,
                text=True,
            )
            prtstat = subprocess.run(
                ["prtstat", str(child.pid)],
                check=True,
                capture_output=True,
                text=True,
            )
            lines = ps.stdout.rstrip("\n").splitlines()
            fixture = {
                "captured_at_pst": datetime.now(ZoneInfo("America/Los_Angeles")).isoformat(),
                "linux": platform.uname()._asdict(),
                "procps_version": version_line("ps", "--version"),
                "psmisc_version": version_line("pstree", "--version"),
                "ps_command": "ps -o pid,ppid,stat,comm,args -p <pid>",
                "ps_header": lines[0],
                "ps_row": lines[1],
                "pgrep_command": f"pgrep -x {name}",
                "pgrep_pids": [int(value) for value in pgrep.stdout.split()],
                "target_pid": child.pid,
                "target_found": child.pid in {int(value) for value in pgrep.stdout.split()},
                "pstree_command": "pstree -p <pid>",
                "pstree_output": pstree.stdout.rstrip("\n"),
                "prtstat_command": "prtstat <pid>",
                "prtstat_output": prtstat.stdout.rstrip("\n"),
            }
            output = Path(__file__).with_name("linux-procps-ng-4.0.6.json")
            output.write_text(json.dumps(fixture, indent=2) + "\n")
        finally:
            os.kill(child.pid, signal.SIGTERM)
            child.wait()


if __name__ == "__main__":
    main()
