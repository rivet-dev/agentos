#!/usr/bin/env python3

import re
import sys
from pathlib import Path


def extract_function_window(source: str, signature: str, max_lines: int = 80) -> str:
  start = source.find(signature)
  if start < 0:
    raise ValueError(f"signature not found: {signature}")

  return "\n".join(source[start:].splitlines()[:max_lines])


def main() -> int:
  if len(sys.argv) != 2:
    print("usage: check_internal_binding_dispatch.py <edge_module_loader.cc>", file=sys.stderr)
    return 2

  path = Path(sys.argv[1])
  text = path.read_text(encoding="utf-8")
  body = extract_function_window(text, "static napi_value NativeGetInternalBindingCallback")

  if re.search(r"\bif\s*\(\s*name\s*==\s*\"[^\"]+\"", body):
    print("error: monolithic internalBinding name checks found in NativeGetInternalBindingCallback",
          file=sys.stderr)
    return 1

  if "binding_registry::Get(env, name" not in body:
    print("error: registry call missing in NativeGetInternalBindingCallback", file=sys.stderr)
    return 1

  if "binding_registry::Get(env, name," in body:
    print("error: NativeGetInternalBindingCallback must not pass a fallback resolver",
          file=sys.stderr)
    return 1

  if "internal_binding::Resolve(env, name" in body:
    print("error: NativeGetInternalBindingCallback should not call legacy dispatch directly",
          file=sys.stderr)
    return 1

  return 0


if __name__ == "__main__":
  raise SystemExit(main())
