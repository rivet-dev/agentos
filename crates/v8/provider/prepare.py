"""Resolve and verify the fixed native ABI used by this binding package."""
from pathlib import Path
import os
import sys
import engine

target = sys.argv[1]
engine.require(target in engine.TARGETS, "unsupported agentOS V8 target: " + target)
out = Path(os.environ["OUT_DIR"])
local = os.environ.get("AGENTOS_RUSTY_V8_SOURCE")
if local:
    root = Path(local).resolve()
    engine.verify_bundle(root, target)
else:
    env = engine.install(target, out / "native")
    root = Path(env["AGENTOS_RUSTY_V8_SOURCE"])
(out / "bundle-root").write_text(str(root))
