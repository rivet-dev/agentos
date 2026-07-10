#!/usr/bin/env python3
# Embeds a binary file as a C byte array (optionally bz2-decompressing the input
# first). Used to embed the ICU common-data blob into the runtime for both the
# native and WASIX targets, so ICU has real locale data at runtime (activated via
# udata_setCommonData; see src/edge_icu_data.cc).
import argparse
import bz2
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--symbol", required=True)
    parser.add_argument("--compression", choices=("none", "bz2"), default="none")
    args = parser.parse_args()

    data = Path(args.input).read_bytes()
    if args.compression == "bz2":
        data = bz2.decompress(data)

    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)

    with out.open("w", encoding="utf-8") as f:
        f.write("#include <stddef.h>\n\n")
        f.write("#if defined(__GNUC__)\n")
        f.write("#define UBI_ALIGN16 __attribute__((aligned(16)))\n")
        f.write("#else\n")
        f.write("#define UBI_ALIGN16\n")
        f.write("#endif\n\n")
        f.write(f"const unsigned char {args.symbol}[] UBI_ALIGN16 = {{\n")
        for offset in range(0, len(data), 12):
            chunk = data[offset : offset + 12]
            hex_bytes = ", ".join(f"0x{byte:02x}" for byte in chunk)
            f.write(f"  {hex_bytes},\n")
        f.write("};\n")
        f.write(f"const size_t {args.symbol}_len = sizeof({args.symbol});\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
