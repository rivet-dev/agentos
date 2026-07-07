#!/usr/bin/env bash
set -euo pipefail

kind="${1:-}"
shift || true

if [ -z "${kind}" ] || [ "$#" -eq 0 ]; then
  echo "usage: $0 <binary|shared-library> <artifact>..." >&2
  exit 2
fi

case "${kind}" in
  binary | shared-library) ;;
  *)
    echo "unsupported artifact kind: ${kind}" >&2
    exit 2
    ;;
esac

tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT

if command -v docker >/dev/null 2>&1; then
  container_runtime=docker
elif command -v podman >/dev/null 2>&1; then
  container_runtime=podman
else
  if command -v sudo >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update
    sudo apt-get install -y podman
  fi

  if command -v podman >/dev/null 2>&1; then
    container_runtime=podman
  else
    echo "docker or podman is required for Linux compatibility smoke tests" >&2
    exit 127
  fi
fi

run_container() {
  "${container_runtime}" run --rm -v "${tmpdir}:/artifacts:ro" "$@"
}

for artifact in "$@"; do
  cp "${artifact}" "${tmpdir}/$(basename "${artifact}")"
  chmod +x "${tmpdir}/$(basename "${artifact}")"
done

for image in docker.io/library/debian:11 docker.io/library/ubuntu:20.04; do
  for artifact in "$@"; do
    name="$(basename "${artifact}")"
    echo "Smoke-checking ${name} in ${image}"
    if [ "${kind}" = "binary" ]; then
      run_container "${image}" /bin/sh -ceu '
        status=0
        timeout 5s "/artifacts/$1" --version > /tmp/agentos-smoke.out 2> /tmp/agentos-smoke.err || status=$?
        cat /tmp/agentos-smoke.out
        cat /tmp/agentos-smoke.err >&2
        if grep -E "GLIBC_2\\.[0-9]+.*not found|version .*GLIBC_2\\.[0-9]+" /tmp/agentos-smoke.err >&2; then
          exit 1
        fi
        if [ "${status}" -eq 126 ] || [ "${status}" -eq 127 ]; then
          exit "${status}"
        fi
        exit 0
      ' sh "${name}"
    else
      run_container "${image}" /bin/sh -ceu '
        ldd "/artifacts/$1"
      ' sh "${name}"
    fi
  done
done
