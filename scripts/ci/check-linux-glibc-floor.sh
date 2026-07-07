#!/usr/bin/env bash
set -euo pipefail

floor_minor=27
regression_minor=32

if [ "$#" -eq 0 ]; then
  echo "usage: $0 <elf>..." >&2
  exit 2
fi

for artifact in "$@"; do
  echo "Checking glibc symbol floor for ${artifact}"
  versions="$(objdump -T "${artifact}" | grep -oE 'GLIBC_2\.[0-9]+' | sort -Vu || true)"

  if [ -z "${versions}" ]; then
    echo "No GLIBC_2.x symbols found in ${artifact}."
    continue
  fi

  echo "${versions}"

  while IFS= read -r version; do
    minor="${version#GLIBC_2.}"
    if [ "${minor}" -ge "${regression_minor}" ]; then
      echo "${artifact} references ${version}; this is at or above the hard regression tripwire GLIBC_2.${regression_minor}." >&2
      exit 1
    fi
    if [ "${minor}" -gt "${floor_minor}" ]; then
      echo "${artifact} references ${version}; expected GLIBC_2.${floor_minor} or older." >&2
      exit 1
    fi
  done <<< "${versions}"
done
