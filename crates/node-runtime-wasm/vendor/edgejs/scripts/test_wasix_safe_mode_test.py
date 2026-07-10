#!/usr/bin/env python3

import importlib.util
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("test-wasix-safe-mode.py")


def load_script():
    spec = importlib.util.spec_from_file_location("test_wasix_safe_mode", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class WasixSafeModeTests(unittest.TestCase):
    def test_build_cases_can_skip_external_network_checks(self):
        module = load_script()

        cases = module.build_cases("example.com", include_network=False)

        names = [case[0] for case in cases]
        self.assertEqual(names, ["queueMicrotask", "blob.arrayBuffer"])

    def test_build_cases_includes_external_network_checks_when_requested(self):
        module = load_script()

        cases = module.build_cases("example.com", include_network=True)

        names = [case[0] for case in cases]
        self.assertIn("fetch http://example.com/", names)
        self.assertIn("https.get https://example.com/", names)
        self.assertIn("tls.connect verified example.com", names)


if __name__ == "__main__":
    unittest.main()
