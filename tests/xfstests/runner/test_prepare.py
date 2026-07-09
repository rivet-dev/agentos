import unittest
from pathlib import Path
import tempfile

import prepare


class PrepareTests(unittest.TestCase):
    def test_excluded_test_ids_are_deduplicated_across_backends(self):
        records = [
            {"id": "generic/067", "backend": "memory", "disposition": "excluded"},
            {"id": "generic/067", "backend": "chunked_local", "disposition": "excluded"},
        ]

        self.assertEqual(
            prepare.rendered_excludes(records),
            "# Generated from exceptions.toml by runner/prepare.py. Do not edit.\n"
            "generic/067\n",
        )

    def test_reduced_generic_014_requires_exact_reviewed_shape(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "exceptions.toml"
            path.write_text(
                """schema = 1

[[exceptions]]
id = "generic/014"
backend = "object_s3"
disposition = "reduced"
reason = "Whole-object storage repeats full object rewrites."
tracking_issue = "ISSUES.md#f-014"
reduction = "truncfile-iterations"
full_iterations = 10000
reduced_iterations = 100
focused_coverage = "object sparse semantic regression"
""",
                encoding="utf-8",
            )

            records = prepare.load_exceptions(path)
            self.assertEqual(records[0]["reduced_iterations"], 100)

    def test_reduction_fields_fail_closed_on_other_dispositions(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "exceptions.toml"
            path.write_text(
                """schema = 1

[[exceptions]]
id = "generic/014"
backend = "object_s3"
disposition = "excluded"
reason = "This must not accept a hidden reduction field."
reduced_iterations = 1000
""",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "reduction fields require"):
                prepare.load_exceptions(path)


if __name__ == "__main__":
    unittest.main()
