#!/usr/bin/env python3
"""Drive a NATIVE vim over a real PTY and emit, as JSON, the cumulative raw
terminal output after each scripted step. The node differential test replays the
SAME key sequence against the wasm vim and compares the rendered screen grids.

Protocol: argv[1] is a JSON spec:
  {
    "vim": "/usr/bin/vim",
    "cols": 80, "rows": 24,
    "file": "/tmp/xyz.txt",
    "openWait": 1.2,
    "steps": [ {"label": "insert", "keys_b64": "aQ==", "wait": 0.4}, ... ]
  }
Output (stdout): {"snaps": [ {"label": "open", "raw_b64": "..."}, ... ]}
"""
import os, pty, select, time, fcntl, termios, struct, sys, json, base64


def drive(spec):
    cols = spec.get("cols", 80)
    rows = spec.get("rows", 24)
    vim = spec.get("vim", "vim")
    path = spec["file"]
    extra = spec.get("vimArgs", [])
    # Start from a clean slate so native vim shows "[New]" exactly like the fresh
    # in-VM file — otherwise a leftover host file from a prior run pre-fills the
    # buffer and doubles the typed content.
    try:
        if os.path.exists(path):
            os.unlink(path)
    except OSError:
        pass
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm"
        os.environ["LANG"] = "C.UTF-8"
        os.execvp(vim, [vim, "-N", "-u", "NONE", "-i", "NONE", "-n"] + extra + [path])
        os._exit(1)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    out = bytearray()

    def pump(dur):
        end = time.time() + dur
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.1)
            if r:
                try:
                    d = os.read(fd, 65536)
                except OSError:
                    return
                if not d:
                    return
                out.extend(d)

    snaps = []
    pump(spec.get("openWait", 1.2))
    snaps.append({"label": "open", "raw_b64": base64.b64encode(bytes(out)).decode()})
    for step in spec.get("steps", []):
        os.write(fd, base64.b64decode(step["keys_b64"]))
        pump(step.get("wait", 0.4))
        snaps.append({"label": step["label"], "raw_b64": base64.b64encode(bytes(out)).decode()})
    try:
        os.close(fd)
    except OSError:
        pass
    return snaps


def main():
    spec = json.loads(sys.argv[1])
    snaps = drive(spec)
    sys.stdout.write(json.dumps({"snaps": snaps}))


if __name__ == "__main__":
    main()
