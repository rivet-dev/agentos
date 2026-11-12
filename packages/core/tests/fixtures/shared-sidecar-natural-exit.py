"""Linux owning-path regression; only this subprocess becomes a child subreaper."""
from pathlib import Path
import ctypes
import hashlib
import json
import os
import selectors
import signal
import subprocess
import sys
import tempfile
import time

LIMIT = 1024 * 1024


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def identity(pid):
    try:
        raw = Path(f"/proc/{pid}/stat").read_text()
        fields = raw[raw.rfind(")") + 2:].split()
        result = {"pid": pid, "state": fields[0], "ppid": int(fields[1]), "startTicks": int(fields[19])}
        try:
            result["exe"] = os.readlink(f"/proc/{pid}/exe")
        except FileNotFoundError:
            result["exe"] = None
        return result
    except FileNotFoundError:
        return None


def children(pid):
    try:
        return [int(value) for value in Path(f"/proc/{pid}/task/{pid}/children").read_text().split()]
    except FileNotFoundError:
        return []


def main():
    node, entry, binary, mode = sys.argv[1:]
    binary = Path(binary).resolve(strict=True)
    binary_hash = digest(binary)
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(36, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "PR_SET_CHILD_SUBREAPER failed")
    owned = {}
    events = []
    errors = []
    stderr = bytearray()
    adopted = []
    child = None
    whole_deadline = time.monotonic() + 60
    with tempfile.TemporaryDirectory(prefix="agentos-natural-exit-") as directory:
        try:
            child = subprocess.Popen(
                [node, str(Path(__file__).with_suffix(".mjs")), entry, directory, mode],
                env=os.environ | {"AGENTOS_SIDECAR_BIN": str(binary)},
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                start_new_session=True,
            )
            poll = selectors.DefaultSelector()
            poll.register(child.stdout, selectors.EVENT_READ, "stdout")
            poll.register(child.stderr, selectors.EVENT_READ, "stderr")
            buffer = bytearray()
            output_bytes = 0

            def observe():
                for pid in children(child.pid):
                    row = identity(pid)
                    if row and row["exe"] and Path(row["exe"]).resolve() == binary:
                        key = (pid, row["startTicks"])
                        if key not in owned:
                            row["exeSha256"] = digest(Path(f"/proc/{pid}/exe"))
                            assert row["exeSha256"] == binary_hash
                            owned[key] = row

            def pump():
                nonlocal output_bytes
                assert time.monotonic() < whole_deadline, "fixture exceeded 60s whole-process guard"
                observe()
                for key, _ in poll.select(.02):
                    chunk = os.read(key.fd, 65536)
                    if not chunk:
                        poll.unregister(key.fileobj)
                        continue
                    output_bytes += len(chunk)
                    assert output_bytes <= LIMIT, "fixture stdout+stderr exceeded 1MiB"
                    if key.data == "stderr":
                        stderr.extend(chunk)
                        del stderr[:-8192]
                    else:
                        buffer.extend(chunk)
                        while b"\n" in buffer:
                            line, _, rest = buffer.partition(b"\n")
                            buffer[:] = rest
                            try:
                                row = json.loads(line)
                            except (ValueError, UnicodeDecodeError):
                                continue
                            if isinstance(row, dict) and "phase" in row:
                                events.append(row)

            def phase(name):
                deadline = time.monotonic() + 15
                while not any(row["phase"] == name for row in events):
                    assert time.monotonic() < deadline, f"phase {name} timed out"
                    pump()
                    assert child.poll() is None or any(row["phase"] == name for row in events), f"host exited before {name}"
                observe()

            def release(name):
                Path(directory, name).touch(exist_ok=False)

            phase("ready")
            expected = 2 if mode == "two-pools" else 1
            assert len(owned) == expected, {"expectedLiveSidecars": expected, "observed": list(owned.values())}
            initial = set(owned)
            release("release-first")
            phase("idle")
            for pid, start in initial:
                row = identity(pid)
                assert row and row["startTicks"] == start and row["ppid"] == child.pid, "idle pool retired while ordinary host barrier was alive"
            release("release-idle")
            if mode == "reuse":
                phase("reused-ready")
                assert set(owned) == initial, "sequential VM did not reuse idle sidecar"
                for pid, start in initial:
                    row = identity(pid)
                    assert row and row["startTicks"] == start and row["ppid"] == child.pid
                release("release-reused")
            if mode in ("provider-success", "provider-failure"):
                phase("provider-entered")
                phase("provider-pending")
                assert set(owned) == initial, "provider setup must retain selected generation"
                for pid, start in initial:
                    row = identity(pid)
                    assert row and row["startTicks"] == start and row["ppid"] == child.pid and row["state"] != "Z", "selected sidecar retired during deferred provider setup"
                release("release-provider")
                if mode == "provider-success":
                    phase("provider-ready")
                    assert set(owned) == initial, "successful provider setup must use reserved generation"
                    release("release-provider-vm")
                    phase("provider-disposed")
                else:
                    phase("provider-failed")
            phase("done")
            if mode in ("before", "after", "during"):
                phase("revived-ready")
                if mode == "during":
                    assert len(set(owned) - initial) == 1, "revival during retirement must obtain one replacement generation"
                    for pid, start in initial:
                        row = identity(pid)
                        assert row is None or row["startTicks"] != start, "new generation became usable before original owner was reaped"
                assert [row["revived"] for row in events if row["phase"] == "revived-ready"] == [1]
                release("release-revived")
                phase("revived-done")
            deadline = time.monotonic() + 15
            while child.poll() is None:
                assert time.monotonic() < deadline, "host did not exit naturally"
                pump()
            pump()
            assert child.wait() == 0, "public SDK host failed"
            assert not [row for row in events if row["phase"] == "child-observation-error"], "live child observation failed"
            exits = [row for row in events if row["phase"] == "child-exit"]
            assert len(exits) == len(owned), {"missingOrDuplicateNativeExit": exits, "owned": list(owned.values())}
            assert {(row["pid"], row.get("startTicks")) for row in exits} == set(owned), "native exit records must match every live owned identity"
            for row in exits:
                assert row["code"] == 0 and row["signal"] is None, {"healthySidecarWasNotGracefullyReaped": row}
            assert len([row for row in events if row["phase"] == "revived-ready"]) == (1 if mode in ("before", "after", "during") else 0)
            for pid, start in owned:
                row = identity(pid)
                assert row is None or row["startTicks"] != start, {"ownedChildSurvivedHost": row}
            # Even an already-zombied child must have been reaped by Node itself.
            for pid in children(os.getpid()):
                if pid == child.pid:
                    continue
                row = identity(pid)
                adopted.append(row)
            assert not adopted, {"adoptedOwnedChildren": adopted}
            assert digest(binary) == binary_hash
        except BaseException as error:
            errors.append(repr(error))
        finally:
            # Dedicated Python fixture owns no other subprocesses: every child
            # here belongs to this launch. Always reap, including setup failures.
            if child and child.poll() is None:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                except OSError as error:
                    errors.append(f"host process-group cleanup failed: {error!r}")
                try:
                    child.wait(timeout=3)
                except (OSError, subprocess.TimeoutExpired) as error:
                    errors.append(f"host reap failed: {error!r}")
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                pending = children(os.getpid())
                if not pending:
                    break
                for pid in pending:
                    row = identity(pid)
                    if row and row["state"] != "Z":
                        try:
                            os.kill(pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        except OSError as error:
                            errors.append(f"owned child signal failed: {error!r}")
                    try:
                        os.waitpid(pid, os.WNOHANG)
                    except ChildProcessError:
                        pass
                    except OSError as error:
                        errors.append(f"owned child reap failed: {error!r}")
                time.sleep(.01)
            remaining = children(os.getpid())
            if remaining:
                errors.append(f"fixture cleanup did not reap children: {remaining}")
            if child:
                child.stdout.close()
                child.stderr.close()
            if "poll" in locals():
                poll.close()
    print(json.dumps({"mode": mode, "owned": list(owned.values()), "events": events, "adopted": adopted, "errors": errors, "stderr": stderr.decode(errors="replace"), "passed": not errors}))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
