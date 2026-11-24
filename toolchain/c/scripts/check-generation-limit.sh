#!/usr/bin/env bash
# Source while holding the prefix's exclusive build lock. Never remove readers' generations.
: "${MAX_GENERATIONS:=8}"
[[ "$MAX_GENERATIONS" =~ ^[1-9][0-9]*$ && "$MAX_GENERATIONS" -le 64 ]] || { echo 'Invalid MAX_GENERATIONS: expected 1..64' >&2; exit 2; }
generation_count=$(find "$(dirname "$PREFIX")" -maxdepth 1 -type d -name "$(basename "$PREFIX").generation.*" -printf '.\n' | wc -l)
if (( generation_count >= MAX_GENERATIONS )); then
  echo "GenerationLimitExceeded: $PREFIX has $generation_count generations (limit=$MAX_GENERATIONS). Remove unused generations only after readers stop, or raise MAX_GENERATIONS (maximum64)." >&2
  exit 2
fi
if (( (generation_count + 1) * 4 >= MAX_GENERATIONS * 3 )); then
  echo "GenerationLimitNearThreshold: $PREFIX will have $((generation_count + 1))/$MAX_GENERATIONS generations" >&2
fi
