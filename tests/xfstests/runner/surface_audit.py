#!/usr/bin/env python3
"""Generate the pinned generic/quick helper and operation surface audit."""

from __future__ import annotations

import argparse
from collections import defaultdict
from pathlib import Path
import re
import tomllib


HELPER_PATH = re.compile(r"(?:\$here/|(?<![A-Za-z0-9_]))src/([A-Za-z0-9_.+/-]+)")
REQUIRE_HELPER = re.compile(r"_require_test_program\s+[\"']?([A-Za-z0-9_.+/-]+)")
REQUIRE_XFS_IO = re.compile(r"_require_xfs_io_command\s+[\"']?([A-Za-z][A-Za-z0-9_-]*)")
REQUIRE_COMMAND = re.compile(
    r"_require_command\s+(?:[\"']?\$[A-Z][A-Z0-9_]*[\"']?)\s+"
    r"[\"']?([A-Za-z0-9_.+-]+)"
)
C_CALL = re.compile(
    r"\b(access|chmod|chown|close|creat|fchmod|fchown|fcntl|fdatasync|fork|fstat|"
    r"flock|fsync|ftruncate|getdents|getgroups|getxattr|ioctl|link|listxattr|lseek|lstat|"
    r"mkdir|mmap|open|openat|pread|preadv|pwrite|pwritev|read|readdir|readlink|"
    r"removexattr|rename|renameat2|rmdir|setgid|setgroups|setuid|setxattr|stat|"
    r"statfs|symlink|sync|syncfs|truncate|unlink|utimensat|wait|waitpid|write|writev)\s*\("
)


def quick_tests(root: Path) -> list[str]:
    tests: list[str] = []
    group_list = root / "tests/generic/group.list"
    for line in group_list.read_text(encoding="utf-8").splitlines():
        fields = line.split()
        if fields and not fields[0].startswith("#") and "quick" in fields[1:]:
            tests.append(fields[0])
    if not tests:
        raise ValueError("pinned generic/quick group is empty")
    return tests


def add(mapping: dict[str, set[str]], name: str, test_id: str) -> None:
    name = name.rstrip(";,)|}")
    if name and "$" not in name:
        mapping[name].add(f"generic/{test_id}")


def extract(root: Path) -> dict[str, object]:
    helpers: dict[str, set[str]] = defaultdict(set)
    xfs_io: dict[str, set[str]] = defaultdict(set)
    features: dict[str, set[str]] = defaultdict(set)
    commands: dict[str, set[str]] = defaultdict(set)
    tests = quick_tests(root)
    for test_id in tests:
        path = root / "tests/generic" / test_id
        text = path.read_text(encoding="utf-8", errors="replace")
        active = "\n".join(
            line for line in text.splitlines() if not line.lstrip().startswith("#")
        )
        for match in HELPER_PATH.finditer(active):
            add(helpers, match.group(1), test_id)
        for match in REQUIRE_HELPER.finditer(active):
            add(helpers, match.group(1), test_id)
        for match in REQUIRE_XFS_IO.finditer(active):
            add(xfs_io, match.group(1), test_id)
        for match in REQUIRE_COMMAND.finditer(active):
            add(commands, match.group(1), test_id)
        if "_require_fssum" in active:
            add(helpers, "fssum", test_id)
        if "_require_runas" in active:
            add(helpers, "runas", test_id)
        if "$FSSTRESS_PROG" in active:
            features["fsstress"].add(f"generic/{test_id}")
        if "$FSX_PROG" in active:
            features["fsx"].add(f"generic/{test_id}")
        if "_require_attrs" in active:
            features["xattrs"].add(f"generic/{test_id}")
        if "_require_acls" in active:
            features["acls"].add(f"generic/{test_id}")

    operations: dict[str, set[str]] = defaultdict(set)
    for helper, callers in helpers.items():
        source = root / "src" / f"{helper}.c"
        if not source.is_file():
            continue
        text = source.read_text(encoding="utf-8", errors="replace")
        for operation in C_CALL.findall(text):
            operations[operation].update(callers)
    return {
        "tests": tests,
        "helpers": helpers,
        "xfs_io": xfs_io,
        "features": features,
        "commands": commands,
        "operations": operations,
    }


def evidence(callers: set[str], limit: int = 8) -> str:
    ordered = sorted(callers)
    shown = ", ".join(f"`{test}`" for test in ordered[:limit])
    if len(ordered) > limit:
        shown += f", +{len(ordered) - limit} more"
    return shown


def is_executable(path: Path) -> bool:
    return path.is_file() and path.stat().st_mode & 0o111 != 0


def is_wasm_command(path: Path) -> bool:
    return path.is_file() and path.read_bytes()[:4] == b"\0asm"


def load_operation_evidence(path: Path | None) -> dict[str, str]:
    if path is None:
        return {}
    document = tomllib.loads(path.read_text(encoding="utf-8"))
    if document.get("schema") != 1 or not isinstance(document.get("operations"), dict):
        raise ValueError("operation evidence must use schema 1 with an [operations] table")
    evidence_map = document["operations"]
    if not all(isinstance(operation, str) and isinstance(value, str) and value for operation, value in evidence_map.items()):
        raise ValueError("operation evidence entries must be non-empty strings")
    return evidence_map


def render(root: Path, pin: str, operation_evidence: dict[str, str] | None = None) -> str:
    audit = extract(root)
    helpers = audit["helpers"]
    xfs_io = audit["xfs_io"]
    features = audit["features"]
    commands = audit["commands"]
    operations = audit["operations"]
    operation_evidence = operation_evidence or {}
    unknown_evidence = sorted(set(operation_evidence) - set(operations))
    if unknown_evidence:
        raise ValueError(
            "operation evidence names operations absent from the pinned surface: "
            + ", ".join(unknown_evidence)
        )
    capability_path = root / "agentos-xfs-io-commands"
    xfs_capabilities = (
        set(capability_path.read_text(encoding="utf-8").split())
        if capability_path.is_file()
        else set()
    )
    built_helpers_path = root / "agentos-built-helpers"
    built_helpers = (
        set(built_helpers_path.read_text(encoding="utf-8").split())
        if built_helpers_path.is_file()
        else set()
    )
    built_features_path = root / "agentos-built-features"
    built_features = (
        set(built_features_path.read_text(encoding="utf-8").split())
        if built_features_path.is_file()
        else set()
    )

    lines = [
        "# Pinned filesystem surface audit",
        "",
        f"- xfstests SHA: `{pin}`",
        "- group: `generic/quick`",
        f"- selected tests: {len(audit['tests'])}",
        "- evidence: static pinned callsites plus staged executable/capability inspection",
        "- `unverified-kernel` remains a completion failure until runtime tracing or focused conformance evidence classifies it",
        "",
        "## Guest callers",
        "",
        "| Kind | Surface | Status | Pinned callers |",
        "|---|---|---|---|",
    ]
    for helper, callers in sorted(helpers.items()):
        status = (
            "supported+built"
            if helper in built_helpers and is_executable(root / "src" / helper)
            else "missing-tool"
        )
        lines.append(f"| src helper | `{helper}` | {status} | {evidence(callers)} |")
    for command, callers in sorted(xfs_io.items()):
        status = "supported+declared" if command in xfs_capabilities else "missing-tool"
        lines.append(f"| xfs_io | `{command}` | {status} | {evidence(callers)} |")
    for feature, callers in sorted(features.items()):
        if feature in {"fsstress", "fsx"}:
            status = "supported+built" if is_executable(root / "ltp" / feature) else "missing-tool"
        else:
            status = "supported+built" if feature in built_features else "missing-tool"
        lines.append(f"| feature/tool | `{feature}` | {status} | {evidence(callers)} |")
    for command, callers in sorted(commands.items()):
        status = (
            "supported+built"
            if is_wasm_command(root / "agentos-command-bin" / command)
            else "missing-tool"
        )
        lines.append(f"| external command | `{command}` | {status} | {evidence(callers)} |")

    lines.extend(
        [
            "",
            "## Helper operation callsites",
            "",
            "| Operation | Status | Runtime evidence | Pinned callers |",
            "|---|---|---|---|",
        ]
    )
    for operation, callers in sorted(operations.items()):
        runtime_evidence = operation_evidence.get(operation)
        status = "supported+tested" if runtime_evidence else "unverified-kernel"
        lines.append(
            f"| `{operation}` | {status} | {runtime_evidence or 'none'} | {evidence(callers)} |"
        )
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--pin", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--operation-evidence", type=Path)
    args = parser.parse_args()
    args.output.write_text(
        render(args.root, args.pin, load_operation_evidence(args.operation_evidence)),
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
