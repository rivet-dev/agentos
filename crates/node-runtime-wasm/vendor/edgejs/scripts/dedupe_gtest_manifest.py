#!/usr/bin/env python3
import argparse
import re
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest")
    parser.add_argument(
        "--resource-lock-buckets",
        type=int,
        default=0,
        help="When > 0, assign each discovered test a RESOURCE_LOCK bucket to cap concurrency.",
    )
    parser.add_argument(
        "--resource-lock-prefix",
        default="edge_gtest_slot",
        help="Prefix used for generated RESOURCE_LOCK names.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    manifest = Path(args.manifest)
    if not manifest.exists():
        return 0

    add_re = re.compile(r"^add_test\(\[=\[(.*?)\]=\]")
    prop_re = re.compile(r"^set_tests_properties\(\[=\[(.*?)\]=\]")

    lines = manifest.read_text().splitlines()
    seen = set()
    kept = []

    for line in lines:
        add_match = add_re.match(line)
        if add_match:
            name = add_match.group(1)
            if name in seen:
                continue
            seen.add(name)
            kept.append(line)
            continue

        prop_match = prop_re.match(line)
        if prop_match:
            name = prop_match.group(1)
            if name not in seen:
                continue
            kept.append(line)
            continue

        kept.append(line)

    if args.resource_lock_buckets > 0:
        test_names = []
        for line in kept:
            add_match = add_re.match(line)
            if add_match:
                test_names.append(add_match.group(1))

        for index, name in enumerate(test_names):
            bucket = index % args.resource_lock_buckets
            kept.append(
                "set_tests_properties([=["
                + name
                + f"]=] PROPERTIES RESOURCE_LOCK {args.resource_lock_prefix}_{bucket})"
            )

    manifest.write_text("\n".join(kept) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
