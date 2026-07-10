#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
default_edgejs_root="$(cd "${script_dir}/.." && pwd)"

edgejs_root="${EDGEJS_ROOT:-${default_edgejs_root}}"
wasmer_bin="${WASMER_BIN:-wasmer}"
package_dir="${WASIX_EDGEJS_PACKAGE_DIR:-${edgejs_root}/quickjs-wasm}"
guest_root="${WASIX_EDGEJS_GUEST_ROOT:-/workspace}"
guest_test_tmp_root="${WASIX_EDGEJS_GUEST_TEST_TMP_ROOT:-/tmp/edgejs-node-test}"
workspace_dirs_csv="${WASIX_EDGEJS_WORKSPACE_DIRS:-test,lib,deps,assets,build-quickjs-wasix}"
guest_exec_path="${WASIX_EDGEJS_GUEST_EXEC_PATH:-${guest_root}/build-quickjs-wasix/edgejs.wasm}"
wasmer_stack_args=()
created_run_root=0

if [[ -n "${WASMER_STACK_SIZE:-}" ]]; then
  wasmer_stack_args+=(--stack-size "${WASMER_STACK_SIZE}")
fi

if [[ -n "${WASIX_EDGEJS_HOST_RUN_ROOT:-}" ]]; then
  host_run_root="${WASIX_EDGEJS_HOST_RUN_ROOT}"
else
  host_run_root="$(mktemp -d "${TMPDIR:-/tmp}/edgejs-wasix-node.XXXXXX")"
  created_run_root=1
fi

host_workspace_root="${host_run_root}/workspace"
host_test_tmp_root="${host_run_root}/node-test"
mkdir -p "${host_workspace_root}" "${host_test_tmp_root}"

cleanup() {
  local status=$?
  if [[ "${WASIX_EDGEJS_KEEP_TMP:-0}" == "1" ]]; then
    printf 'Preserving WASIX EdgeJS test run root: %s\n' "${host_run_root}" >&2
  elif [[ "${WASIX_EDGEJS_KEEP_TMP:-0}" == "failed" && "${status}" != "0" ]]; then
    printf 'Preserving failed WASIX EdgeJS test run root: %s\n' "${host_run_root}" >&2
  elif [[ "${created_run_root}" == "1" ]]; then
    rm -rf "${host_run_root}"
  fi
  return "${status}"
}
trap cleanup EXIT

test_serial_input="${EDGEJS_WASIX_TEST_ID:-$*}"
test_serial_hash="$(printf '%s' "${test_serial_input}" | cksum | awk '{print $1}')"
test_serial_id="${TEST_SERIAL_ID:-wasix-${test_serial_hash}-$$}"

rewrite_guest_path_arg() {
  local arg="$1"
  case "${arg}" in
    "${edgejs_root}"/*)
      printf '%s\n' "${guest_root}/${arg#"${edgejs_root}/"}"
      ;;
    "${edgejs_root}")
      printf '%s\n' "${guest_root}"
      ;;
    *=${edgejs_root}/*)
      local key="${arg%%=*}"
      local value="${arg#*=}"
      printf '%s\n' "${key}=${guest_root}/${value#"${edgejs_root}/"}"
      ;;
    *=${edgejs_root})
      local key="${arg%%=*}"
      printf '%s\n' "${key}=${guest_root}"
      ;;
    *)
      printf '%s\n' "${arg}"
      ;;
  esac
}

guest_args=()
for arg in "$@"; do
  guest_args+=("$(rewrite_guest_path_arg "${arg}")")
done

volume_args=(
  --volume "${host_workspace_root}:${guest_root}"
  --volume "${host_test_tmp_root}:${guest_test_tmp_root}"
  --volume "${edgejs_root}/ssl-certs:/usr/local/ssl"
)

if [[ -d "${package_dir}/etc" ]]; then
  volume_args+=(--volume "${package_dir}/etc:/etc")
fi

IFS=',' read -r -a workspace_dirs <<< "${workspace_dirs_csv}"
for workspace_dir in "${workspace_dirs[@]}"; do
  workspace_dir="${workspace_dir#"${workspace_dir%%[![:space:]]*}"}"
  workspace_dir="${workspace_dir%"${workspace_dir##*[![:space:]]}"}"
  [[ -z "${workspace_dir}" ]] && continue
  if [[ ! -d "${edgejs_root}/${workspace_dir}" ]]; then
    printf 'warning: skipping missing WASIX EdgeJS workspace dir: %s\n' "${edgejs_root}/${workspace_dir}" >&2
    continue
  fi
  volume_args+=(--volume "${edgejs_root}/${workspace_dir}:${guest_root}/${workspace_dir}")
done

if [[ "${WASIX_EDGEJS_TRACE:-0}" == "1" ]]; then
  {
    printf '%q ' "${wasmer_bin}" run \
      --llvm \
      "${wasmer_stack_args[@]+"${wasmer_stack_args[@]}"}" \
      --net \
      --env HOME=/tmp \
      --env "EDGE_EXEC_PATH=${guest_exec_path}" \
      --env "NODE_TEST_DIR=${guest_test_tmp_root}" \
      --env "TEST_SERIAL_ID=${test_serial_id}" \
      "${volume_args[@]}" \
      --cwd "${guest_root}" \
      "${package_dir}" \
      -- "${guest_args[@]}"
    printf '\n'
  } >&2
fi

"${wasmer_bin}" run \
  --llvm \
  "${wasmer_stack_args[@]+"${wasmer_stack_args[@]}"}" \
  --net \
  --env HOME=/tmp \
  --env "EDGE_EXEC_PATH=${guest_exec_path}" \
  --env "NODE_TEST_DIR=${guest_test_tmp_root}" \
  --env "TEST_SERIAL_ID=${test_serial_id}" \
  "${volume_args[@]}" \
  --cwd "${guest_root}" \
  "${package_dir}" \
  -- "${guest_args[@]}"
