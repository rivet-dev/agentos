from pathlib import Path
import tempfile
import unittest

from surface_audit import load_operation_evidence, render


class SurfaceAuditTests(unittest.TestCase):
    def test_repository_operation_evidence_is_valid(self) -> None:
        evidence = load_operation_evidence(Path(__file__).parent.parent / "operation-evidence.toml")
        self.assertIn("access", evidence)
        self.assertTrue(all(value.strip() for value in evidence.values()))

    def test_extracts_quick_callers_and_classifies_built_helpers(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tests/generic").mkdir(parents=True)
            (root / "src").mkdir()
            (root / "ltp").mkdir()
            (root / "agentos-command-bin").mkdir()
            (root / "tests/generic/group.list").write_text("001 auto quick\n002 auto\n")
            (root / "tests/generic/001").write_text(
                "$here/src/probe\n_require_xfs_io_command pwrite\n_require_attrs\n_require_acls\n"
                '_require_command "$FLOCK_PROG" flock\n'
            )
            (root / "src/probe.c").write_text(
                "open(path, 0); syncfs(fd); write(fd, data, 1);\n"
            )
            helper = root / "src/probe"
            helper.write_bytes(b"wasm")
            helper.chmod(0o755)
            (root / "agentos-built-helpers").write_text("probe\n")

            report = render(root, "abc123")

            self.assertIn("selected tests: 1", report)
            self.assertIn("| src helper | `probe` | supported+built |", report)
            self.assertIn("| xfs_io | `pwrite` | missing-tool |", report)
            self.assertIn("| feature/tool | `xattrs` | missing-tool |", report)
            self.assertIn("| feature/tool | `acls` | missing-tool |", report)
            self.assertIn("| external command | `flock` | missing-tool |", report)
            self.assertIn("| `open` | unverified-kernel | none |", report)
            self.assertIn("| `syncfs` | unverified-kernel | none |", report)
            self.assertIn("| `write` | unverified-kernel | none |", report)

            report = render(root, "abc123", {"open": "focused open test"})
            self.assertIn("| `open` | supported+tested | focused open test |", report)
            self.assertIn("| `write` | unverified-kernel | none |", report)

            with self.assertRaisesRegex(ValueError, "absent from the pinned surface"):
                render(root, "abc123", {"mmap": "unrelated evidence"})

            (root / "agentos-built-features").write_text("xattrs\nacls\n")
            flock = root / "agentos-command-bin/flock"
            flock.write_bytes(b"\0asm")
            flock.chmod(0o755)
            report = render(root, "abc123")
            self.assertIn("| feature/tool | `xattrs` | supported+built |", report)
            self.assertIn("| feature/tool | `acls` | supported+built |", report)
            self.assertIn("| external command | `flock` | supported+built |", report)


if __name__ == "__main__":
    unittest.main()
