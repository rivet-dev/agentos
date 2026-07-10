#!/usr/bin/env python3

import argparse
import ast
import io
import pathlib
import re
import shlex
import sys
import tokenize


ARCH_MAP = {
    ("linux", "x64"): "linux-x86_64",
    ("linux", "arm64"): "linux-aarch64",
    ("mac", "x64"): "darwin64-x86_64-cc",
    ("mac", "arm64"): "darwin64-arm64-cc",
    ("win", "x64"): "VC-WIN64A",
    ("win", "x86"): "VC-WIN32",
    ("win", "arm64"): "VC-WIN64-ARM",
}


def strip_comments(text: str) -> str:
    tokens = []
    for token in tokenize.generate_tokens(io.StringIO(text).readline):
        if token.type != tokenize.COMMENT:
            tokens.append(token)
    return tokenize.untokenize(tokens)


def load_gyp_literal(path: pathlib.Path):
    return ast.literal_eval(strip_comments(path.read_text(encoding="utf-8")))


def expand_gyp_value(value, variables):
    if isinstance(value, str):
        match = re.fullmatch(r"<@\(([^)]+)\)", value)
        if match:
            return expand_gyp_value(variables[match.group(1)], variables)
        return [value]
    if isinstance(value, list):
        items = []
        for entry in value:
            items.extend(expand_gyp_value(entry, variables))
        return items
    raise TypeError(f"Unsupported GYP value: {type(value)!r}")


def normalize_path(value: str, deps_root: pathlib.Path) -> str:
    if value == ".":
        return deps_root.resolve().as_posix()
    if value.startswith("./"):
        value = value[2:]
    return (deps_root / value).resolve().as_posix()


def normalize_include_dir(value: str, base_dir: pathlib.Path) -> str:
    if value == ".":
        return base_dir.resolve().as_posix()
    if value.startswith("./"):
        value = value[2:]
    return (base_dir / value).resolve().as_posix()


def dedupe(items):
    seen = set()
    ordered = []
    for item in items:
        if item in seen:
            continue
        seen.add(item)
        ordered.append(item)
    return ordered


def split_flag_groups(items):
    result = []
    for item in items:
        result.extend(shlex.split(item))
    return result


def emit_list(handle, name, items):
    handle.write(f"set({name})\n")
    for item in items:
        handle.write(f"list(APPEND {name} [=[{item}]=])\n")
    handle.write("\n")


def resolve_mode(arch_dir: pathlib.Path, target_os: str, target_arch: str) -> str:
    if target_os == "win" and target_arch == "arm64":
        preferred = ["no-asm"]
    else:
        preferred = ["asm", "asm_avx2", "no-asm"]
    for candidate in preferred:
        if (arch_dir / candidate).is_dir():
            return candidate
    raise SystemExit(f"Unsupported vendored OpenSSL mode under {arch_dir}")


def load_arch_block(path: pathlib.Path):
    data = load_gyp_literal(path)
    variables = data.get("variables", {})
    return {
        "include_dirs": expand_gyp_value(data.get("include_dirs", []), variables),
        "defines": expand_gyp_value(data.get("defines", []), variables),
        "cflags": expand_gyp_value(data.get("cflags", []), variables),
        "libraries": expand_gyp_value(data.get("libraries", []), variables),
        "sources": expand_gyp_value(data.get("sources", []), variables),
    }


def bundle_defines(openssl_gyp: pathlib.Path, modules_dir: str):
    data = load_gyp_literal(openssl_gyp)
    for target in data.get("targets", []):
        if target.get("target_name") != "openssl":
            continue
        defines = []
        for define in target.get("defines", []):
            if define.startswith("OPENSSL_API_COMPAT="):
                continue
            defines.append(define)
        defines.extend(
            [
                "OPENSSL_API_COMPAT=0x10100000L",
                "OPENSSL_THREADS",
                "OPENSSL_NO_PINSHARED",
                f'MODULESDIR="{modules_dir}"',
            ]
        )
        return defines
    raise SystemExit("Could not find vendored OpenSSL target metadata")


def common_block(common_gypi: pathlib.Path, target_os: str, compiler_family: str):
    data = load_gyp_literal(common_gypi)
    include_dirs = list(data.get("include_dirs", []))
    defines = []
    cflags = []
    libraries = []

    if target_os in ("aix", "os400"):
        defines.extend(
            [
                "__LITTLE_ENDIAN=1234",
                "__BIG_ENDIAN=4321",
                "__BYTE_ORDER=__BIG_ENDIAN",
                "__FLOAT_WORD_ORDER=__BIG_ENDIAN",
                'OPENSSLDIR="/etc/ssl"',
                'ENGINESDIR="/dev/null"',
            ]
        )
    elif target_os == "win":
        defines.extend(
            [
                'OPENSSLDIR="C:\\\\Program Files\\\\Common Files\\\\SSL"',
                'ENGINESDIR="NUL"',
                "OPENSSL_SYS_WIN32",
                "WIN32_LEAN_AND_MEAN",
                "L_ENDIAN",
                "_CRT_SECURE_NO_DEPRECATE",
                "UNICODE",
                "_UNICODE",
            ]
        )
        libraries.extend(
            [
                "ws2_32.lib",
                "gdi32.lib",
                "advapi32.lib",
                "crypt32.lib",
                "user32.lib",
            ]
        )
    elif target_os == "mac":
        defines.extend(
            [
                'OPENSSLDIR="/System/Library/OpenSSL/"',
                'ENGINESDIR="/dev/null"',
            ]
        )
        cflags.append("-Wno-missing-field-initializers")
    elif target_os == "solaris":
        defines.extend(
            [
                'OPENSSLDIR="/etc/ssl"',
                'ENGINESDIR="/dev/null"',
                "__EXTENSIONS__",
            ]
        )
    else:
        defines.extend(
            [
                'OPENSSLDIR="/etc/ssl"',
                'ENGINESDIR="/dev/null"',
                "TERMIOS",
            ]
        )
        cflags.append("-Wno-missing-field-initializers")
        if compiler_family != "clang":
            cflags.append("-Wno-old-style-declaration")

    return {
        "include_dirs": include_dirs,
        "defines": defines,
        "cflags": cflags,
        "libraries": libraries,
    }


def filter_compile_options(options, target_os: str):
    if target_os != "win":
        return options
    filtered = []
    for option in options:
        if option.startswith("-Wa,"):
            continue
        if option.startswith("-") and not option.startswith("/"):
            continue
        filtered.append(option)
    return filtered


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project-root", required=True)
    parser.add_argument("--target-os", required=True)
    parser.add_argument("--target-arch", required=True)
    parser.add_argument("--compiler-family", required=True)
    parser.add_argument("--modules-dir", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    project_root = pathlib.Path(args.project_root).resolve()
    deps_root = (project_root / "deps" / "openssl").resolve()

    arch_name = ARCH_MAP.get((args.target_os, args.target_arch))
    if arch_name is None:
        raise SystemExit(
            f"Unsupported vendored OpenSSL target: {args.target_os}/{args.target_arch}"
        )

    arch_root = deps_root / "config" / "archs" / arch_name
    mode = resolve_mode(arch_root, args.target_os, args.target_arch)
    arch_block = load_arch_block(arch_root / mode / "openssl.gypi")
    cli_block = load_arch_block(arch_root / mode / "openssl-cl.gypi")
    common = common_block(deps_root / "openssl_common.gypi", args.target_os, args.compiler_family)
    bundle_defs = bundle_defines(deps_root / "openssl.gyp", args.modules_dir)

    base_defines = dedupe(bundle_defs + common["defines"] + arch_block["defines"])
    base_cflags = dedupe(
        filter_compile_options(
            split_flag_groups(common["cflags"] + arch_block["cflags"]),
            args.target_os,
        )
    )
    base_link_libraries = dedupe(
        split_flag_groups(common["libraries"] + arch_block["libraries"])
    )
    arch_include_base = arch_root / mode
    base_include_dirs = dedupe(
        [normalize_path(entry, deps_root) for entry in common["include_dirs"]]
        + [normalize_include_dir(entry, arch_include_base) for entry in arch_block["include_dirs"]]
    )
    source_paths = dedupe(
        normalize_path(entry, deps_root) for entry in arch_block["sources"]
    )

    cli_defines = dedupe(base_defines + cli_block["defines"] + ["MONOLITH"])
    cli_cflags = dedupe(
        filter_compile_options(
            split_flag_groups(common["cflags"] + cli_block["cflags"]),
            args.target_os,
        )
    )
    cli_link_libraries = dedupe(
        split_flag_groups(common["libraries"] + arch_block["libraries"] + cli_block["libraries"])
    )
    cli_include_dirs = dedupe(
        [normalize_path(entry, deps_root) for entry in common["include_dirs"]]
        + [normalize_include_dir(entry, arch_include_base) for entry in cli_block["include_dirs"]]
        + [normalize_path("openssl/apps/include", deps_root)]
    )
    cli_sources = dedupe(
        normalize_path(entry, deps_root) for entry in cli_block["sources"]
    )

    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8") as handle:
        handle.write("# Generated by scripts/generate_vendored_openssl_cmake.py\n\n")
        emit_list(handle, "EDGE_VENDORED_OPENSSL_SOURCES", source_paths)
        emit_list(handle, "EDGE_VENDORED_OPENSSL_INCLUDE_DIRS", base_include_dirs)
        emit_list(handle, "EDGE_VENDORED_OPENSSL_DEFINES", base_defines)
        emit_list(handle, "EDGE_VENDORED_OPENSSL_COMPILE_OPTIONS", base_cflags)
        emit_list(handle, "EDGE_VENDORED_OPENSSL_LINK_LIBRARIES", base_link_libraries)
        emit_list(handle, "EDGE_VENDORED_OPENSSL_CLI_SOURCES", cli_sources)
        emit_list(handle, "EDGE_VENDORED_OPENSSL_CLI_INCLUDE_DIRS", cli_include_dirs)
        emit_list(handle, "EDGE_VENDORED_OPENSSL_CLI_DEFINES", cli_defines)
        emit_list(handle, "EDGE_VENDORED_OPENSSL_CLI_COMPILE_OPTIONS", cli_cflags)
        emit_list(handle, "EDGE_VENDORED_OPENSSL_CLI_LINK_LIBRARIES", cli_link_libraries)
        handle.write(f"set(EDGE_VENDORED_OPENSSL_ARCH [=[{arch_name}]=])\n")
        handle.write(f"set(EDGE_VENDORED_OPENSSL_MODE [=[{mode}]=])\n")


if __name__ == "__main__":
    main()
