#!/usr/bin/env python3
"""Prepare the locked engine tree and package or consume one matched target bundle."""
import argparse
from contextlib import contextmanager
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile

HERE = Path(__file__).resolve().parent
SOURCE = json.loads((HERE / "source.json").read_text())
LINUX_X64_SYSROOT = json.loads((HERE / "linux-x64-sysroot.json").read_text())
TARGETS = ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
           "x86_64-apple-darwin", "aarch64-apple-darwin")
ICU = "third_party/icu/common/icudtl.dat"
NOTICE_PATTERNS = ("LICENSE", "v8/LICENSE*", "third_party/*/LICENSE*", "third_party/*/src/LICENSE*")


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def gitlinks(root):
    return {path: revision for attrs in git(root, "ls-tree", "-r", "HEAD").splitlines()
            for mode, kind, revision, path in [attrs.split(maxsplit=3)] if kind == "commit"}


def verify_source(root):
    source_patches()
    require(git(root, "rev-parse", "HEAD") == SOURCE["rustyV8"], "incorrect rusty_v8 revision")
    git(root, "diff", "--quiet", "--ignore-submodules=all", "HEAD")
    v8 = root / "v8"
    require(git(v8, "write-tree") == SOURCE["v8Tree"], "incorrect patched V8 tree")
    git(v8, "diff", "--quiet")
    links = gitlinks(root)
    require(links["third_party/icu"] == SOURCE["icu"], "incorrect ICU pin")
    for path, revision in links.items():
        if path in ("v8", "tools/win"):
            continue
        require(git(root / path, "rev-parse", "HEAD") == revision, "incorrect revision: " + path)
        git(root / path, "diff", "--quiet", "HEAD")


def source_patches():
    paths = []
    for patch in SOURCE["agentosPatches"]:
        path = HERE / patch["file"]
        require(digest(path) == patch["sha256"], f"agentOS V8 patch SHA-256 mismatch: {path.name}")
        paths.append(path)
    return paths


def install_rust_glue(checkout, dest, config):
    # Base and PR checks may have different pins, or a released registry crate.
    # Interpret the lock and pin from the checkout that Cargo will analyze.
    locked = [entry for entry in re.split(r"^\[\[package\]\]$", (checkout / "Cargo.lock").read_text(), flags=re.M)
              if re.search(r'^name = "v8"$', entry, re.M) and re.search(r'^source = "git\+', entry, re.M)]
    if not locked:
        return
    revision = json.loads((checkout / "scripts/ci/v8/source.json").read_text())["rustyV8"]
    require(re.fullmatch("[0-9a-f]{40}", revision), "invalid rusty_v8 revision")
    repo = "https://github.com/denoland/rusty_v8"
    identity = f'source = "git+{repo}?rev={revision}#{revision}"'
    require(len(locked) == 1 and identity in locked[0].splitlines(), "rusty_v8 lock does not match this checkout's source pin")
    dest.mkdir(parents=True, exist_ok=False)
    vendor = dest / "vendor/v8"
    vendor.mkdir(parents=True)
    with tempfile.TemporaryDirectory(dir=dest) as work:
        root = Path(work)
        git(root, "init", "-q")
        git(root, "fetch", "--no-recurse-submodules", "--no-tags", "--depth=1", repo, revision)
        git(root, "checkout", "--no-recurse-submodules", "--detach", "FETCH_HEAD")
        require(git(root, "rev-parse", "HEAD") == revision, "incorrect fetched Rust glue revision")
        inputs = rust_crate_inputs(root)
        for name, path in inputs.items():
            dst = vendor / name
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, dst)
        (vendor / ".cargo-checksum.json").write_text(json.dumps(
            {"package": None, "files": {name: digest(path) for name, path in inputs.items()}}, sort_keys=True))
    write_cargo_config(config, dest, revision)


def prepare(root):
    require(not root.exists(), "prepare requires a new checkout directory")
    root.mkdir(parents=True)
    git(root, "init", "-q")
    git(root, "remote", "add", "origin", "https://github.com/denoland/rusty_v8.git")
    git(root, "fetch", "--depth=1", "--no-tags", "origin", SOURCE["rustyV8"])
    git(root, "checkout", "--detach", "FETCH_HEAD")
    modules = sorted(gitlinks(root).keys() - {"v8", "tools/win"})
    git(root, "submodule", "update", "--init", "--depth=1", "--jobs=4", "--", *modules)
    v8 = root / "v8"
    git(v8, "init", "-q")
    git(v8, "fetch", "--no-tags", "--depth=1", "https://github.com/denoland/v8.git", SOURCE["denoV8"])
    git(v8, "checkout", "--detach", "FETCH_HEAD")
    patches = SOURCE["upstreamPatches"]
    git(v8, "fetch", "--no-tags", "--depth=" + str(len(patches) + 1),
        "https://github.com/v8/v8.git", patches[-1])
    git(v8, "cherry-pick", "--no-commit", *patches)
    for patch in source_patches():
        git(v8, "apply", "--index", str(patch))
    verify_source(root)


def filenames(target):
    return "librusty_v8_release_" + target + ".a", "src_binding_release_" + target + ".rs"


def rust_crate_inputs(source):
    # Only the pinned Rust repository's blobs; a gitlink represents an entire
    # native submodule and is intentionally absent from the binary consumer.
    inputs = {}
    for entry in git(source, "ls-tree", "-rz", "HEAD").split("\0"):
        if entry:
            attrs, name = entry.split("\t", 1)
            mode, kind, _ = attrs.split()
            if kind == "blob":
                require(mode in ("100644", "100755"), "unexpected non-file Rust crate input: " + name)
                inputs[name] = source / name
    return inputs


def pack(source, build, target, output):
    verify_source(source)
    gn = build / target / "release" / "gn_out"
    args = (gn / "args.gn").read_text()
    settings = dict(re.findall(r"^(\w+)\s*=\s*(\S+)\s*$", args, re.M))
    expected = {"is_debug": "false", "v8_enable_sandbox": "false", "use_custom_libcxx": "true",
                "v8_enable_pointer_compression": "false", "v8_enable_v8_checks": "false",
                "v8_generate_external_defines_header": "true",
                "target_cpu": '"arm64"' if target.startswith("aarch64") else '"x64"'}
    require(all(settings.get(k) == v for k, v in expected.items()), "wrong GN ABI or target")
    if target == "x86_64-unknown-linux-gnu":
        sysroot = os.environ.get("AGENTOS_V8_RELEASE_SYSROOT")
        require(sysroot and settings.get("sysroot") == json.dumps(sysroot),
                "Linux x64 archive must use the verified release sysroot")
        require(settings.get("use_glib") == "false", "standalone V8 must not use runner GLib headers")
    if "apple" in target:
        require(sys.platform == "darwin", "Darwin archive must be produced on macOS")
    else:
        require(sys.platform.startswith("linux"), "Linux archive must be produced on Linux")
    archive, binding = filenames(target)
    inputs = {archive: gn / "obj/librusty_v8.a", binding: gn / "src_binding.rs",
              "generated/v8-gn.h": gn / "gen/v8/include/v8-gn.h", ICU: source / ICU}
    for header in sorted((source / "v8/include").rglob("*.h")):
        inputs[header.relative_to(source).as_posix()] = header
    for pattern in NOTICE_PATTERNS:
        for notice in source.glob(pattern):
            if notice.is_file():
                inputs["licenses/" + notice.relative_to(source).as_posix()] = notice
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output.parent) as work:
        work = Path(work)
        crate = rust_crate_inputs(source)
        checksums = work / ".cargo-checksum.json"
        checksums.write_text(json.dumps({"package": None, "files": {name: digest(path) for name, path in crate.items()}}, sort_keys=True))
        inputs.update({"vendor/v8/" + name: path for name, path in crate.items()})
        inputs["vendor/v8/.cargo-checksum.json"] = checksums
        receipt = {"source": SOURCE, "target": target, "gn": expected,
                   "sha256": {name: digest(path) for name, path in inputs.items()}}
        if target == "x86_64-unknown-linux-gnu":
            receipt["releaseSysroot"] = LINUX_X64_SYSROOT
        manifest = work / "provenance.json"
        manifest.write_text(json.dumps(receipt, sort_keys=True, indent=2) + "\n")
        inputs[manifest.name] = manifest
        packed = work / "bundle.tar.gz"
        with packed.open("wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=1) as zipped:
            with tarfile.open(fileobj=zipped, mode="w|") as tar:
                for name, path in sorted(inputs.items()):
                    info = tarfile.TarInfo(name)
                    info.size, info.mode = path.stat().st_size, 0o644
                    with path.open("rb") as data:
                        tar.addfile(info, data)
        os.replace(packed, output)
    print(digest(output), output.name)


def verify_bundle(root, target):
    manifest = json.loads((root / "provenance.json").read_text())
    require(manifest["source"] == SOURCE and manifest["target"] == target, "V8 source or target mismatch")
    if target == "x86_64-unknown-linux-gnu":
        require(manifest.get("releaseSysroot") == LINUX_X64_SYSROOT,
                "Linux x64 bundle lacks matching release sysroot provenance")
    archive, binding = filenames(target)
    required = {archive, binding, "generated/v8-gn.h", "v8/include/v8.h", ICU,
                "licenses/LICENSE", "licenses/v8/LICENSE", "licenses/third_party/icu/LICENSE",
                "vendor/v8/Cargo.toml", "vendor/v8/build.rs", "vendor/v8/src/lib.rs", "vendor/v8/.cargo-checksum.json"}
    hashes = manifest["sha256"]
    require(required <= hashes.keys(), "incomplete V8 bundle")
    present = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()}
    require(present == hashes.keys() | {"provenance.json"}, "unexpected or missing V8 bundle files")
    for name, checksum in hashes.items():
        notice = name.startswith("licenses/") and Path(name).name.startswith("LICENSE")
        require(name in required or notice or name.startswith("vendor/v8/") or (name.startswith("v8/include/") and name.endswith(".h")), "unexpected bundle file " + name)
        require(digest(root / name) == checksum, "V8 bundle digest mismatch: " + name)


def unpack(bundle, root):
    with tarfile.open(bundle, "r:gz") as tar:
        for member in tar:
            path = Path(member.name)
            require(member.isfile() and not path.is_absolute() and ".." not in path.parts, "unexpected archive entry")
            dest = root / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            with tar.extractfile(member) as data, dest.open("xb") as out:
                shutil.copyfileobj(data, out)


def download_action_bundle(artifact_id, target, work):
    require(type(artifact_id) is int and artifact_id > 0, "invalid V8 Actions artifact ID")
    zip_path = work / "artifact.zip"
    with zip_path.open("wb") as out:
        subprocess.run(["gh", "api", f"repos/rivet-dev/agentos/actions/artifacts/{artifact_id}/zip"],
                       stdout=out, check=True)
    with zipfile.ZipFile(zip_path) as archive:
        # Read the one expected entry directly; never extract arbitrary ZIP paths.
        with archive.open(target + ".tar.gz") as data, (work / "bundle").open("wb") as out:
            shutil.copyfileobj(data, out)


def pin_for(target, allow_actions):
    pin = json.loads((HERE / "archives.json").read_text()).get(target)
    require(pin, "No reviewed V8 artifact SHA-256 for " + target + ". See scripts/ci/v8/README.md.")
    if isinstance(pin, dict):
        require(allow_actions, "Release builds require a durable V8 archive pin for " + target)
        require(pin.keys() == {"sha256", "artifactId"}, "invalid temporary V8 artifact pin")
        require(type(pin["artifactId"]) is int and pin["artifactId"] > 0, "invalid V8 Actions artifact ID")
        checksum, artifact_id = pin["sha256"], pin["artifactId"]
    else:
        checksum, artifact_id = pin, None
    require(re.fullmatch(r"[a-f0-9]{64}", checksum or ""), "invalid reviewed V8 checksum")
    return checksum, artifact_id


@contextmanager
def reviewed_archive(target, dest, allow_actions):
    checksum, artifact_id = pin_for(target, allow_actions)
    with tempfile.TemporaryDirectory(dir=dest) as work:
        work = Path(work)
        if artifact_id is not None:
            download_action_bundle(artifact_id, target, work)
        else:
            url = f"https://releases.rivet.dev/agent-os/v8/{SOURCE['v8Tree']}/{checksum}/{target}.tar.gz"
            with urllib.request.urlopen(url, timeout=120) as response, (work / "bundle").open("wb") as out:
                shutil.copyfileobj(response, out)
        require(digest(work / "bundle") == checksum, "V8 archive SHA-256 mismatch")
        yield work / "bundle", checksum


def install(target, dest, local_bundle=None, expected_digest=None, allow_actions=False):
    dest.mkdir(parents=True, exist_ok=True)
    if local_bundle is None:
        require(expected_digest is None, "--sha256 requires --bundle")
        with reviewed_archive(target, dest, allow_actions) as (bundle, checksum):
            return install_verified(bundle, checksum, target, dest)
    require(re.fullmatch(r"[a-f0-9]{64}", expected_digest or ""), "expected an independent bundle SHA-256")
    require(digest(local_bundle) == expected_digest, "V8 archive SHA-256 mismatch")
    return install_verified(local_bundle, expected_digest, target, dest)


def install_verified(local_bundle, expected_digest, target, dest):
    with tempfile.TemporaryDirectory(dir=dest) as work:
        work = Path(work)
        root = work / "verified"
        root.mkdir()
        unpack(local_bundle, root)
        verify_bundle(root, target)
        ready = dest / expected_digest
        try:
            root.rename(ready)
        except OSError:
            if not ready.is_dir():
                raise
            # Another consumer installed the same pinned bytes. Validate every
            # existing file against the newly authenticated manifest.
            require((ready / "provenance.json").read_bytes() == (root / "provenance.json").read_bytes(), "cached V8 provenance mismatch")
            verify_bundle(ready, target)
        return verified_env_paths(ready, target)


def verified_env_paths(ready, target):
    archive, binding = filenames(target)
    hashes = json.loads((ready / "provenance.json").read_text())["sha256"]
    return {"RUSTY_V8_ARCHIVE": str(ready / archive), "RUSTY_V8_ARCHIVE_SHA256": hashes[archive],
            "RUSTY_V8_SRC_BINDING_PATH": str(ready / binding), "AGENTOS_RUSTY_V8_SOURCE": str(ready),
            "AGENTOS_RUSTY_V8_GN_INCLUDE": str(ready / "generated"), "V8_FROM_SOURCE": "0",
            "RUSTY_V8_SKIP_DOWNLOAD": "0", "V8_FORCE_DEBUG": "0"}


def write_cargo_config(path, root, revision=None):
    revision = revision or SOURCE["rustyV8"]
    config = ('[source.agentos-rusty-v8]\ngit = "https://github.com/denoland/rusty_v8"\n'
              f'rev = "{revision}"\nreplace-with = "agentos-v8-vendor"\n'
              f'[source.agentos-v8-vendor]\ndirectory = {json.dumps(str(root / "vendor"))}\n')
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with path.open("x") as out:
            out.write(config)
    except FileExistsError:
        require(path.read_text() == config, f"Cargo config already exists at {path}; specify a new --cargo-config path")


def write_consumer_lock(source, output):
    # Cargo needs a locked Git identity before it can use a directory source.
    # The producer's lock already has the complete dependency graph; its root
    # package becomes a Git dependency only in the standalone version check.
    revision = SOURCE["rustyV8"]
    data = git(source, "show", revision + ":Cargo.lock")
    entry = r'^(\[\[package\]\]\nname = "v8"\nversion = "[^"\n]+"\n)(?=dependencies = \[)'
    identity = f'source = "git+https://github.com/denoland/rusty_v8?rev={revision}#{revision}"\n'
    locked, count = re.subn(entry, lambda m: m.group(1) + identity, data, flags=re.M)
    require(count == 1, "expected one local v8 package in the pinned rusty_v8 Cargo.lock")
    with output.open("x") as out:
        out.write(locked + "\n")


def temporal_dependencies(lock):
    names = {"temporal_capi", "temporal_rs", "timezone_provider", "zoneinfo64", "diplomat", "diplomat-runtime"}
    result = {}
    for package in re.split(r"^\[\[package\]\]$", lock.read_text(), flags=re.M):
        entry = dict(re.findall(r'^(name|version|checksum) = "([^"]+)"$', package, re.M))
        name = entry.get("name")
        if name in names:
            require(name not in result, f"ambiguous Temporal dependency {name}")
            result[name] = (entry["version"], entry["checksum"])
    require(result.keys() == names, "missing Temporal ABI dependency in Cargo.lock")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    cmds = parser.add_subparsers(dest="command", required=True)
    consumer_lock = cmds.add_parser("consumer-lock")
    consumer_lock.add_argument("source", type=Path)
    consumer_lock.add_argument("output", type=Path)
    temporal = cmds.add_parser("check-temporal-lock")
    temporal.add_argument("producer", type=Path)
    temporal.add_argument("consumer", type=Path)
    glue = cmds.add_parser("install-glue", help="prepare only Rust source for metadata tools")
    glue.add_argument("checkout", type=Path)
    glue.add_argument("dest", type=Path)
    glue.add_argument("--cargo-config", required=True, type=Path)
    fetch = cmds.add_parser("fetch", help="stage a reviewed bundle for a separate build context")
    fetch.add_argument("target", choices=TARGETS)
    fetch.add_argument("dest", type=Path)
    fetch.add_argument("--allow-actions", action="store_true")
    for name in ("prepare", "check-source", "pack"):
        cmd = cmds.add_parser(name)
        cmd.add_argument("source", type=Path)
        if name == "pack":
            cmd.add_argument("build", type=Path)
            cmd.add_argument("target", choices=TARGETS)
            cmd.add_argument("output", type=Path)
    cmd = cmds.add_parser("install")
    cmd.add_argument("target", choices=TARGETS)
    cmd.add_argument("dest", type=Path)
    cmd.add_argument("--bundle", type=Path)
    cmd.add_argument("--sha256")
    cmd.add_argument("--build-context", type=Path, help="use a staged target bundle if supplied")
    cmd.add_argument("--allow-actions", action="store_true", help="allow draft artifact pins, using gh with Actions read permission")
    cmd.add_argument("--cargo-config", type=Path, help="write this additional Cargo config for the pinned Git dependency")
    activation = cmd.add_mutually_exclusive_group()
    activation.add_argument("--github-env", type=Path)
    activation.add_argument("--shell-env", type=Path)
    args = parser.parse_args()
    if args.command == "install-glue":
        install_rust_glue(args.checkout, args.dest.resolve(), args.cargo_config)
    elif args.command == "fetch":
        args.dest.mkdir(parents=True, exist_ok=True)
        with reviewed_archive(args.target, args.dest, args.allow_actions) as (bundle, _):
            bundle.replace(args.dest / (args.target + ".tar.gz"))
    elif args.command == "check-temporal-lock":
        require(temporal_dependencies(args.producer) == temporal_dependencies(args.consumer), "Temporal ABI dependency mismatch")
    elif args.command == "consumer-lock":
        write_consumer_lock(args.source, args.output)
    elif args.command == "prepare":
        prepare(args.source)
    elif args.command == "check-source":
        verify_source(args.source)
    elif args.command == "pack":
        pack(args.source, args.build, args.target, args.output)
    else:
        if args.build_context:
            require(args.bundle is None and args.sha256 is None, "--build-context and --bundle cannot be combined")
            staged = args.build_context / (args.target + ".tar.gz")
            if staged.is_file():
                args.bundle = staged
                args.sha256, _ = pin_for(args.target, allow_actions=True)
        env = install(args.target, args.dest.resolve(), args.bundle, args.sha256, args.allow_actions)
        if args.cargo_config:
            write_cargo_config(args.cargo_config, Path(env["AGENTOS_RUSTY_V8_SOURCE"]))
        if args.github_env:
            require(all("\n" not in value and "\r" not in value for value in env.values()), "invalid environment path")
            with args.github_env.open("a") as out:
                out.writelines(f"{k}={v}\n" for k, v in env.items())
        elif args.shell_env:
            import shlex
            args.shell_env.write_text("".join(f"export {k}={shlex.quote(v)}\n" for k, v in env.items()))
        else:
            print(json.dumps(env, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, tarfile.TarError, subprocess.CalledProcessError) as exc:
        sys.exit(str(exc))
