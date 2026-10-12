#!/usr/bin/env python3
"""Offline shell failure controls, not curl protocol/compiler acceptance.

Usage: python3 toolchain/c/tests/curl-build-integrity.py --archive /path/to/curl-8.23.0.tar.xz
The real pinned archive is hashed. Explicit extraction/configure/make doubles
avoid compiling or running upstream code; sha256sum and publication are real.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def executable(path, body):
    path.write_text(body)
    path.chmod(0o755)


def tree(path):
    return {str(p.relative_to(path)): sha(p) for p in path.rglob("*") if p.is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    args = parser.parse_args()
    owner = Path(__file__).resolve().parents[1]
    builder = owner / "scripts/build-curl-protocols.sh"
    source = builder.read_text()
    digest = re.search(r"^digest=([0-9a-f]{64})$", source, re.M).group(1)
    archive_name = re.search(r'^archive="\$CACHE_DIR/([^"/]+)"$', source, re.M).group(1)
    assert sha(args.archive) == digest, "provide the real pinned upstream archive"
    inputs = {str(p): sha(p) for p in [builder, args.archive,
        owner / "scripts/check-generation-limit.sh", owner / "scripts/resolve-generation-prefix.sh"]}
    results = []
    with tempfile.TemporaryDirectory(prefix="curl-integrity-") as temporary:
        root = Path(temporary)
        tools = root / "tools"
        tools.mkdir()
        executable(tools / "forbidden-compiler", "#!/bin/sh\necho unexpected-compiler >&2\nexit 99\n")
        # No network: only the corrupt-download case admits this double.
        executable(tools / "curl", '''#!/usr/bin/env python3
import os,pathlib,sys
assert os.environ['CASE'] == 'corrupt-download', 'unexpected download'
pathlib.Path(os.environ['TRACE']).open('a').write('download\\n')
pathlib.Path(sys.argv[sys.argv.index('-o')+1]).write_bytes(b'corrupt download')
''')
        executable(tools / "tar", '''#!/usr/bin/env python3
import os,pathlib,sys
pathlib.Path(os.environ['TRACE']).open('a').write('extract\\n')
p=pathlib.Path(sys.argv[sys.argv.index('-C')+1])/os.environ['SOURCE_DIR'];p.mkdir()
(p/'configure').write_text("""#!/bin/sh
printf 'configure\\n' >> "$TRACE"
[ "$CASE" != failed-configure ] || exit 41
for arg in "$@"; do case "$arg" in --prefix=*) printf '%s' "${arg#--prefix=}" > fixture-prefix ;; esac; done
mkdir -p lib src include
for feature in USE_OPENSSL USE_NGHTTP2 USE_NGHTTP3 USE_NGTCP2 USE_LIBSSH2; do printf '#define %s 1\\n' "$feature"; done > lib/curl_config.h
""")
(p/'configure').chmod(0o755)
(p/'curl-config').write_text("#!/bin/sh\\nprintf '%s\\n' '-lcurl'\\n")
(p/'curl-config').chmod(0o755)
''')
        executable(tools / "patch", "#!/bin/sh\ncat >/dev/null\nprintf 'patch-double\\n' >> \"$TRACE\"\n")
        executable(tools / "make", '''#!/usr/bin/env python3
import os,pathlib,sys
pathlib.Path(os.environ['TRACE']).open('a').write('make '+ ' '.join(sys.argv[1:])+'\\n')
if os.environ['CASE']=='failed-build':sys.exit(42)
p=pathlib.Path('fixture-prefix').read_text();dest=pathlib.Path(p)
if 'install' in sys.argv and os.environ['CASE']=='failed-install':sys.exit(43)
(dest/'lib').mkdir(parents=True,exist_ok=True)
(dest/'include/curl').mkdir(parents=True,exist_ok=True)
(dest/'lib/libcurl.a').write_bytes(b'fixture archive, not compiled curl')
(dest/'include/curl/curl.h').write_bytes(b'fixture header')
pathlib.Path('src/curl').write_bytes(b'fixture executable, not WASM')
''')
        deps = {}
        for name in ["OPENSSL", "PROTOCOL", "COMPRESSION"]:
            path = root / (name.lower()+".generation.fixture")
            path.mkdir()
            (path / ".agentos-generation").write_text("agentos-immutable-prefix-v1\n")
            deps[name + "_PREFIX"] = str(path)
        for case in ["corrupt-cache", "corrupt-download", "failed-configure", "failed-build", "failed-install", "success"]:
            run = root / case
            run.mkdir()
            cache = run / "cache"
            cache.mkdir()
            old = run / "curl.generation.previous"
            old.mkdir()
            (old / ".agentos-generation").write_text("agentos-immutable-prefix-v1\n")
            (old / "sentinel").write_bytes(b'previous published content')
            for relative in ["bin/curl.wasm", "lib/libcurl.a", "include/curl/curl.h", "share/agentos/curl-static-libs"]:
                previous = old / relative
                previous.parent.mkdir(parents=True, exist_ok=True)
                previous.write_bytes(b'previous fixture artifact')
            prefix = run / "curl"
            prefix.symlink_to(old.name)
            before = tree(old)
            archive = cache / archive_name
            if case == "corrupt-cache":
                archive.write_bytes(b'corrupt cache')
            elif case != "corrupt-download":
                shutil.copyfile(args.archive, archive)
            cache_before = tree(cache)
            trace = run / "trace"
            env = dict(os.environ, **deps, PATH=str(tools)+os.pathsep+os.environ["PATH"],
                CC=str(tools/"forbidden-compiler"), AR=str(tools/"forbidden-compiler"),
                RANLIB=str(tools/"forbidden-compiler"), PREFIX=str(prefix), CACHE_DIR=str(cache),
                CA_BUNDLE="/etc/ssl/certs/ca-certificates.crt", JOBS="1", MAX_GENERATIONS="8",
                CASE=case, TRACE=str(trace), SOURCE_DIR=archive_name.removesuffix(".tar.xz"))
            result = subprocess.run(["bash", str(builder)], env=env, capture_output=True, text=True, timeout=15)
            calls = trace.read_text().splitlines() if trace.exists() else []
            assert tree(old) == before, (case, "old generation changed")
            assert tree(cache) == cache_before, (case, "cache changed")
            assert not list(run.glob("curl.build.*")), (case, "work leaked")
            if case == "success":
                assert result.returncode == 0, result.stderr
                assert prefix.resolve() != old and prefix.is_symlink()
                assert not os.readlink(prefix).startswith("/"), "publication alias must be relative"
                assert (prefix / "lib/libcurl.a").read_bytes() == b'fixture archive, not compiled curl'
                assert (prefix / "bin/curl.wasm").is_file()
                assert len(list(run.glob("curl.generation.*"))) == 2
            else:
                expected = {"corrupt-cache": 1, "corrupt-download": 1, "failed-configure": 41,
                    "failed-build": 42, "failed-install": 43}[case]
                assert result.returncode == expected, (case, result.returncode, result.stderr)
                assert prefix.resolve() == old and os.readlink(prefix) == old.name
                assert list(run.glob("curl.generation.*")) == [old], (case, "unpublished generation leaked")
                if case.startswith("corrupt"):
                    assert calls == (["download"] if case == "corrupt-download" else []), calls
                elif case == "failed-configure":
                    assert calls == ["extract", "patch-double", "configure"], calls
                else:
                    assert calls[:3] == ["extract", "patch-double", "configure"] and any(c.startswith("make ") for c in calls)
            results.append({"case": case, "exit": result.returncode, "calls": calls,
                "previousGenerationUnchanged": True})
    assert all(sha(Path(path)) == digest for path, digest in inputs.items()), "inputs mutated"
    print(json.dumps({"inputs": inputs, "scope": "actual shell/checksum/publication with explicit upstream tool doubles", "results": results}, indent=2))


if __name__ == "__main__":
    main()
