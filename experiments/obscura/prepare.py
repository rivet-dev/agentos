#!/usr/bin/env python3
"""Prepare checksum-pinned upstream sources; generated vendor/ is ignored."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parent
UPSTREAM = json.loads((ROOT / "UPSTREAM.json").read_text())
CRATES = ["obscura-dom", "obscura-ssrf", "obscura-net", "obscura-browser", "obscura-js"]
MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
MAX_SOURCE_BYTES = 256 * 1024 * 1024


def archive(url, checksum, cache):
    path = cache / (checksum + ".tar.gz")
    if not path.exists():
        with urllib.request.urlopen(url, timeout=60) as response, path.open("wb") as output:
            total = 0
            while data := response.read(1024 * 1024):
                total += len(data)
                if total > MAX_ARCHIVE_BYTES:
                    path.unlink()
                    raise RuntimeError("source archive exceeds 64 MiB")
                output.write(data)
    if path.stat().st_size > MAX_ARCHIVE_BYTES:
        raise RuntimeError("cached source archive exceeds 64 MiB")
    with path.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != checksum:
        raise RuntimeError(f"checksum mismatch for {path}: expected {checksum}, got {actual}")
    return path


def extract(source, destination, select):
    with tarfile.open(source, "r:gz") as bundle:
        members = []
        total = 0
        for member in bundle:
            name = Path(member.name)
            # Both official archives have one top-level source directory.
            relative = Path(*name.parts[1:])
            if name.is_absolute() or ".." in name.parts:
                raise RuntimeError(f"unsafe archive path: {name}")
            if not relative.parts or not select(relative):
                continue
            if not (member.isfile() or member.isdir()):
                raise RuntimeError(f"unsupported archive entry: {name}")
            total += member.size
            if total > MAX_SOURCE_BYTES:
                raise RuntimeError("extracted sources exceed 256 MiB")
            member.name = str(relative)
            members.append(member)
        bundle.extractall(destination, members=members, filter="data")


def patch(directory, name):
    with (ROOT / "patches" / name).open("rb") as source:
        subprocess.run(["patch", "--batch", "--forward", "-p1"], cwd=directory, stdin=source, check=True)


def prepare(cache):
    cache.mkdir(parents=True, exist_ok=True)
    revision = UPSTREAM["revision"]
    obscura = archive(
        f"https://codeload.github.com/h4ckf0r0day/obscura/tar.gz/{revision}",
        UPSTREAM["archive_sha256"], cache,
    )
    deno_version = UPSTREAM["deno_core"]["version"]
    deno = archive(
        f"https://static.crates.io/crates/deno_core/deno_core-{deno_version}.crate",
        UPSTREAM["deno_core"]["archive_sha256"], cache,
    )
    with tempfile.TemporaryDirectory(prefix=".prepare-", dir=ROOT) as temporary:
        vendor = Path(temporary) / "vendor"
        vendor.mkdir()
        extract(obscura, vendor, lambda path: path.name in ["Cargo.toml", "LICENSE"] if len(path.parts) == 1
                else len(path.parts) >= 2 and path.parts[0] == "crates" and path.parts[1] in CRATES)
        patch(vendor, "embedding.patch")
        shutil.copyfile(ROOT / "patches/host.rs", vendor / "crates/obscura-net/src/host.rs")
        manifest = vendor / "Cargo.toml"
        source = manifest.read_text()
        members = ",\n".join(f'    "crates/{name}"' for name in CRATES)
        source = re.sub(r"members = \[.*?\]", f"members = [\n{members}\n]", source, count=1, flags=re.S)
        source = re.sub(r"\[patch.crates-io\].*?(?=\[profile.release\])", "", source, flags=re.S)
        manifest.write_text(source)
        for name in CRATES:
            manifest = vendor / "crates" / name / "Cargo.toml"
            source = manifest.read_text().replace("[package]\n", '[package]\nworkspace = "../.."\n', 1)
            # Rendering is not part of this embedding experiment. Its upstream
            # optional path dependencies must not pull in unprepared crates.
            source = re.sub(r'^obscura-render = .*\n', '', source, flags=re.M)
            source = re.sub(r'^render = \[.*?\]', 'render = []', source, flags=re.M | re.S)
            manifest.write_text(source)
        deno_directory = vendor / "deno_core"
        extract(deno, deno_directory, lambda _: True)
        patch(deno_directory, "deno-local-ops.patch")
        (vendor / "UPSTREAM.json").write_text(json.dumps(UPSTREAM, indent=2) + "\n")
        target = ROOT / "vendor"
        if target.exists():
            shutil.rmtree(target)
        vendor.rename(target)
    print(f"Prepared Obscura {revision} and Deno {deno_version} in {ROOT / 'vendor'}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=ROOT / ".cache")
    prepare(parser.parse_args().cache)
