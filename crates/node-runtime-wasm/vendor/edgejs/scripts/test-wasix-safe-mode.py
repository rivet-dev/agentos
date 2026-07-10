#!/usr/bin/env python3

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path


IGNORED_STDERR_PATTERNS = (
    re.compile(r"Skipping duplicate additional import env\.memory"),
)


def sanitize_stderr(stderr: str) -> str:
    lines = []
    for line in stderr.splitlines():
        if any(pattern.search(line) for pattern in IGNORED_STDERR_PATTERNS):
            continue
        if line.strip():
            lines.append(line)
    return "\n".join(lines)


def run_case(wasmer_bin: str, package_dir: Path, timeout: int, name: str, script: str,
             expected_stdout: str) -> None:
    cmd = [
        wasmer_bin,
        "run",
        ".",
        "--experimental-napi",
        "--net",
        "--",
        "-e",
        script,
    ]

    try:
        completed = subprocess.run(
            cmd,
            cwd=package_dir,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        raise RuntimeError(
            f"{name} timed out after {timeout}s\n"
            f"stdout: {exc.stdout or ''}\n"
            f"stderr: {exc.stderr or ''}"
        ) from exc

    stdout = completed.stdout
    stderr = sanitize_stderr(completed.stderr)

    if completed.returncode != 0:
        raise RuntimeError(
            f"{name} exited with {completed.returncode}\n"
            f"stdout: {stdout}\n"
            f"stderr: {stderr}"
        )

    if stdout != expected_stdout:
        raise RuntimeError(
            f"{name} stdout mismatch\n"
            f"expected: {expected_stdout!r}\n"
            f"actual:   {stdout!r}\n"
            f"stderr: {stderr}"
        )

    if stderr:
        raise RuntimeError(
            f"{name} emitted unexpected stderr\n"
            f"stderr: {stderr}"
        )

    print(f"[ok] {name}: {stdout.strip()}")


def build_cases(host: str, include_network: bool) -> list[tuple[str, str, str]]:
    cases = [
        (
            "queueMicrotask",
            "console.log('A'); queueMicrotask(() => console.log('B')); console.log('C');",
            "A\nC\nB\n",
        ),
        (
            "blob.arrayBuffer",
            "new Blob([new Uint8Array([65,66,67])]).arrayBuffer().then((ab) => console.log('BLOB', ab.byteLength));",
            "BLOB 3\n",
        ),
    ]

    if include_network:
        cases.extend([
            (
                f"fetch http://{host}/",
                f"const keepAlive = setTimeout(() => {{}}, 30000); fetch('http://{host}/').then((r) => console.log('FETCH', r.status)).catch((e) => {{ console.error('FETCHERR', e && (e.stack || e.message || e)); process.exitCode = 1; }}).finally(() => clearTimeout(keepAlive));",
                "FETCH 200\n",
            ),
            (
                f"https.get https://{host}/",
                f"require('node:https').get({{ hostname: '{host}', port: 443, path: '/', servername: '{host}' }}, (r) => {{ console.log('HTTPS', r.statusCode); r.resume(); }}).on('error', (e) => {{ console.error('HTTPSERR', e && (e.stack || e.message || e)); process.exit(1); }});",
                "HTTPS 200\n",
            ),
            (
                f"tls.connect verified {host}",
                f"const tls=require('node:tls'); const s=tls.connect(443,'{host}',{{servername:'{host}'}},()=>{{ console.log('TLS CONNECTED', s.authorized); s.destroy(); }}); s.on('close',()=>console.log('TLS CLOSE')); process.on('exit',(code)=>console.log('TLS EXIT', code)); s.on('error',(e)=>{{ console.error('TLSERR', e && (e.stack || e.message || e)); process.exitCode = 1; }});",
                "TLS CONNECTED true\nTLS CLOSE\nTLS EXIT 0\n",
            ),
        ])

    return cases


def main() -> int:
    parser = argparse.ArgumentParser(description="Run WASIX safe-mode smoke tests through Wasmer.")
    parser.add_argument(
        "--wasmer-bin",
        default=os.environ.get("WASMER_BIN", "wasmer"),
        help="Path to the Wasmer CLI binary.",
    )
    parser.add_argument(
        "--package-dir",
        default=str(Path(__file__).resolve().parents[1]),
        help="Directory containing wasmer.toml and the built WASIX package.",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=30,
        help="Per-case timeout in seconds.",
    )
    parser.add_argument(
        "--https-host",
        default="example.com",
        help="Host used for verified HTTPS/TLS smoke coverage.",
    )
    parser.add_argument(
        "--include-network",
        action="store_true",
        help="Run outbound HTTP/TLS smoke checks in addition to deterministic local checks.",
    )
    args = parser.parse_args()

    package_dir = Path(args.package_dir).resolve()
    if not (package_dir / "wasmer.toml").is_file():
        raise RuntimeError(f"missing wasmer.toml in {package_dir}")

    cases = build_cases(args.https_host, include_network=args.include_network)

    for name, script, expected_stdout in cases:
        run_case(args.wasmer_bin, package_dir, args.timeout, name, script, expected_stdout)

    if not args.include_network:
        print("Skipped outbound network smoke tests; pass --include-network to run them.")
    print("All WASIX safe-mode smoke tests passed.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as exc:
        print(str(exc), file=sys.stderr)
        raise SystemExit(1)
