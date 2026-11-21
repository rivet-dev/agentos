"""Small bundle controls; these do not clone V8 or run Cargo."""
import io
import json
from pathlib import Path
import os
import sys
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import engine


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "source"
        self.target = engine.TARGETS[0]
        self.bundle = self.root / "build.tar.gz"
        build = self.root / "build"
        gn = build / self.target / "release/gn_out"
        files = {
            self.source / "LICENSE": "rusty_v8 license",
            self.source / "v8/LICENSE": "V8 license",
            self.source / "third_party/icu/LICENSE": "ICU license",
            self.source / "v8/include/v8.h": "header",
            self.source / engine.ICU: "data",
            gn / "args.gn": '\n'.join(['is_debug = false', 'use_custom_libcxx = true',
                'v8_enable_sandbox = false', 'v8_enable_pointer_compression = false',
                'v8_enable_v8_checks = false', 'v8_generate_external_defines_header = true',
                'sysroot = "/fixture/sysroot"', 'use_glib = false',
                'target_cpu = "x64"']),
            gn / "obj/librusty_v8.a": "test archive",
            gn / "src_binding.rs": "test binding",
            gn / "gen/v8/include/v8-gn.h": "test GN header",
        }
        for path, value in files.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(value)
        crate = {}
        for name in ("Cargo.toml", "build.rs", "src/lib.rs"):
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("test crate " + name)
            crate[name] = path
        with mock.patch.object(engine, "verify_source"), mock.patch.object(engine, "rust_crate_inputs", return_value=crate), mock.patch.object(sys, "platform", "linux"), mock.patch.dict(os.environ, {"AGENTOS_V8_RELEASE_SYSROOT": "/fixture/sysroot"}):
            engine.pack(self.source, build, self.target, self.bundle)
        self.pin = engine.digest(self.bundle)

    def install(self):
        return engine.install(self.target, self.root / "installed", self.bundle, self.pin)

    def test_matched_local_bundle_can_be_installed_repeatedly(self):
        env = self.install()
        self.assertEqual(env, self.install())
        self.assertEqual(Path(env["RUSTY_V8_ARCHIVE"]).read_text(), "test archive")
        self.assertEqual(Path(env["RUSTY_V8_SRC_BINDING_PATH"]).read_text(), "test binding")
        self.assertEqual(engine.digest(Path(env["RUSTY_V8_ARCHIVE"])), env["RUSTY_V8_ARCHIVE_SHA256"])
        vendor = Path(env["AGENTOS_RUSTY_V8_SOURCE"]) / "vendor/v8"
        crate_hashes = json.loads((vendor / ".cargo-checksum.json").read_text())
        self.assertEqual(crate_hashes["files"]["build.rs"], engine.digest(vendor / "build.rs"))
        config = self.root / "extra.toml"
        engine.write_cargo_config(config, vendor.parent.parent)
        self.assertIn(engine.SOURCE["rustyV8"], config.read_text())
        config.write_text("[build]\njobs = 2\n")
        with self.assertRaisesRegex(ValueError, "already exists"):
            engine.write_cargo_config(config, vendor.parent.parent)
        self.assertEqual(config.read_text(), "[build]\njobs = 2\n")
        Path(env["AGENTOS_RUSTY_V8_SOURCE"], engine.ICU).write_text("wrong")
        with self.assertRaisesRegex(ValueError, "digest mismatch"):
            self.install()

    def test_rejects_wrong_target_and_independent_checksum(self):
        with self.assertRaisesRegex(ValueError, "target mismatch"):
            engine.install(engine.TARGETS[1], self.root / "installed", self.bundle, self.pin)
        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
            engine.install(self.target, self.root / "installed", self.bundle, "0" * 64)

    def test_rejects_pre_sysroot_linux_archive_even_with_valid_file_hashes(self):
        root = self.root / "old-bundle"
        engine.unpack(self.bundle, root)
        manifest = root / "provenance.json"
        data = json.loads(manifest.read_text())
        del data["releaseSysroot"]
        manifest.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "release sysroot provenance"):
            engine.verify_bundle(root, self.target)

    def test_draft_uses_only_the_reviewed_artifact_id_and_tar_digest(self):
        pin_file = self.root / "archives.json"
        pin = {"sha256": self.pin, "artifactId": 123}
        locked = {self.target: pin}
        pin_file.write_text(json.dumps(locked))
        installed = self.root / "artifacts"
        def download(command, stdout, check):
            self.assertEqual(command, ["gh", "api", "repos/rivet-dev/agentos/actions/artifacts/123/zip"])
            self.assertTrue(check)
            with zipfile.ZipFile(stdout, "w") as archive:
                archive.write(self.bundle, self.target + ".tar.gz")
        with mock.patch.object(engine, "HERE", self.root), mock.patch.object(engine.subprocess, "run", side_effect=download) as gh:
            with self.assertRaisesRegex(ValueError, "durable"):
                engine.install(self.target, installed)
            gh.assert_not_called()
            env = engine.install(self.target, installed, allow_actions=True)
            self.assertEqual(Path(env["RUSTY_V8_ARCHIVE"]).read_text(), "test archive")
            staged = self.root / "docker-input"
            with mock.patch.object(sys, "argv", ["engine.py", "fetch", self.target, str(staged), "--allow-actions"]):
                engine.main()
            docker_command = ["engine.py", "install", self.target, str(installed), "--build-context", str(staged),
                              "--shell-env", str(self.root / "docker.env")]
            gh.reset_mock()
            with mock.patch.object(sys, "argv", docker_command):
                engine.main()
                (staged / (self.target + ".tar.gz")).write_bytes(b"different")
                with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                    engine.main()
            gh.assert_not_called()
            pin["sha256"] = "0" * 64
            pin_file.write_text(json.dumps(locked))
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                engine.install(self.target, installed, allow_actions=True)

    def test_rejects_links_and_parent_paths(self):
        for name, kind in [("../outside", tarfile.REGTYPE), ("archive", tarfile.SYMTYPE)]:
            with tarfile.open(self.bundle, "w:gz") as tar:
                info = tarfile.TarInfo(name)
                info.type = kind
                tar.addfile(info, io.BytesIO())
            with self.assertRaisesRegex(ValueError, "unexpected archive entry"):
                engine.unpack(self.bundle, self.root / "unpacked")
        self.assertFalse((self.root / "outside").exists())

    def test_consumer_lock_roots_only_the_exact_upstream_crate(self):
        source = '[[package]]\nname = "v8"\nversion = "158.0.0"\ndependencies = [\n "bindgen",\n]\n'
        other = '[[package]]\nname = "bindgen"\nversion = "0.72.1"\nsource = "registry+fixture"\n'
        dest = self.root / "consumer.lock"
        with mock.patch.object(engine, "git", return_value=source + other) as git:
            engine.write_consumer_lock(self.source, dest)
        git.assert_called_once_with(self.source, "show", engine.SOURCE["rustyV8"] + ":Cargo.lock")
        self.assertIn(other, dest.read_text())
        self.assertEqual(dest.read_text().count(engine.SOURCE["rustyV8"]), 2)
        with mock.patch.object(engine, "git", return_value=other):
            with self.assertRaisesRegex(ValueError, "one local v8 package"):
                engine.write_consumer_lock(self.source, self.root / "unknown.lock")

    def test_hygiene_respects_each_checkouts_own_pin_and_registry_base(self):
        checkout = self.root / "checkout"
        checkout.mkdir()
        dest, config = self.root / "glue", self.root / "scoped.toml"
        lock = checkout / "Cargo.lock"
        lock.write_text('[[package]]\nname = "v8"\nsource = "registry+fixture"\n')
        engine.install_rust_glue(checkout, dest, config)
        self.assertFalse(config.exists())
        self.assertFalse(dest.exists())
        pin = checkout / "scripts/ci/v8/source.json"
        pin.parent.mkdir(parents=True)
        pin.write_text(json.dumps({"rustyV8": "1" * 40}))
        lock.write_text('[[package]]\nname = "v8"\nsource = "git+https://example.org/unreviewed"\n')
        with mock.patch.object(engine, "git") as git:
            with self.assertRaisesRegex(ValueError, "does not match"):
                engine.install_rust_glue(checkout, dest, config)
            git.assert_not_called()


if __name__ == "__main__":
    unittest.main()
