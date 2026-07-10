#!/bin/sh
set -u

# Edge.js installer

###############################################################################
# Styling
###############################################################################

reset="$(printf '\033[0m')"
bold="$(printf '\033[1m')"
green="$(printf '\033[32m')"
yellow="$(printf '\033[33m')"
red="$(printf '\033[31m')"
gray="$(printf '\033[37m')"
dark_gray="$(printf '\033[90m')"

###############################################################################
# Config
###############################################################################

EDGEJS_REPO="${EDGEJS_REPO:-wasmerio/edgejs}"
EDGEJS_NIGHTLY_REPO="${EDGEJS_NIGHTLY_REPO:-wasmerio/edgejs-nightlies}"

EDGEJS_HOME="${EDGEJS_HOME:-$HOME/.edgejs}"
EDGEJS_BIN_DIR="$EDGEJS_HOME/bin"
EDGEJS_BIN_PATH="$EDGEJS_BIN_DIR/edge"
EDGEJS_ENV_FILE="$EDGEJS_HOME/edgejs.sh"

EDGEJS_DEBUG="${EDGEJS_DEBUG:-0}"

REQUESTED_VERSION=""
USE_NIGHTLY="false"

RESOLVED_REPO=""
RESOLVED_TAG=""
RESOLVED_DOWNLOAD_URL=""
RESOLVED_ASSET_NAME=""

OS=""
ARCH=""
TARGET=""

TMP_DIR=""
DOWNLOADER=""
TTY_AVAILABLE="false"
JSON_PARSER=""

###############################################################################
# Output helpers
###############################################################################

say() {
  printf '%s\n' "$1"
}

status_downloading() {
  printf '%b\n' "${bold}${green}downloading${reset}${bold}${gray}: $1${reset}"
}

status_success() {
  printf '%b\n' "${bold}${green}success${reset}${bold}${gray}: $1${reset}"
}

status_info() {
  printf '%b\n' "${bold}${gray}info: $1${reset}"
}

status_warn() {
  printf '%b\n' "${bold}${yellow}warn${reset}${bold}${gray}: $1${reset}" >&2
}

status_error() {
  printf '%b\n' "${bold}${red}error${reset}${bold}${gray}: $1${reset}" >&2
}

debug() {
  if [ "$EDGEJS_DEBUG" = "1" ]; then
    printf '%b\n' "${bold}${dark_gray}debug${reset}${gray}: $1${reset}" >&2
  fi
}

###############################################################################
# Cleanup
###############################################################################

cleanup() {
  if [ -n "${TMP_DIR:-}" ] && [ -d "$TMP_DIR" ]; then
    debug "Cleaning up temp dir: $TMP_DIR"
    rm -rf "$TMP_DIR"
  fi
}

trap cleanup EXIT INT TERM

###############################################################################
# Utilities
###############################################################################

has_cmd() {
  command -v "$1" >/dev/null 2>&1
}

init_downloader() {
  if has_cmd curl; then
    DOWNLOADER="curl"
  elif has_cmd wget; then
    DOWNLOADER="wget"
  else
    status_error "Neither curl nor wget is installed."
    exit 1
  fi
  debug "Downloader: $DOWNLOADER"
}

init_json_parser() {
  if has_cmd jq; then
    JSON_PARSER="jq"
  elif has_cmd python3; then
    JSON_PARSER="python3"
  else
    status_error "This installer requires either 'jq' or 'python3' to parse GitHub release metadata."
    exit 1
  fi
  debug "JSON parser: $JSON_PARSER"
}

mktempdir() {
  if has_cmd mktemp; then
    mktemp -d 2>/dev/null || mktemp -d -t edgejs
  else
    d="${TMPDIR:-/tmp}/edgejs-install.$$"
    mkdir -p "$d" || exit 1
    printf '%s\n' "$d"
  fi
}

github_api_get() {
  url="$1"
  debug "GET $url"

  if [ "$DOWNLOADER" = "curl" ]; then
    if [ -n "${GITHUB_TOKEN:-}" ]; then
      curl -fsSL \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        -H "Authorization: Bearer $GITHUB_TOKEN" \
        "$url"
    else
      curl -fsSL \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "$url"
    fi
  else
    if [ -n "${GITHUB_TOKEN:-}" ]; then
      wget -qO- \
        --header="Accept: application/vnd.github+json" \
        --header="X-GitHub-Api-Version: 2022-11-28" \
        --header="Authorization: Bearer $GITHUB_TOKEN" \
        "$url"
    else
      wget -qO- \
        --header="Accept: application/vnd.github+json" \
        --header="X-GitHub-Api-Version: 2022-11-28" \
        "$url"
    fi
  fi
}

download_file() {
  url="$1"
  destination="$2"

  debug "Downloading file: $url -> $destination"

  if [ "$DOWNLOADER" = "curl" ]; then
    if [ -t 2 ]; then
      curl --fail --location --progress-bar "$url" --output "$destination"
    else
      curl --fail --location --silent --show-error "$url" --output "$destination"
    fi
  else
    if [ -t 2 ]; then
      if wget --help 2>/dev/null | grep -q -- '--show-progress'; then
        wget -q --show-progress --progress=bar:force:noscroll -O "$destination" "$url"
      else
        wget -nv --progress=bar:force:noscroll -O "$destination" "$url"
      fi
    else
      wget -nv -O "$destination" "$url"
    fi
  fi
}

###############################################################################
# JSON helpers
###############################################################################

json_release_field() {
  file="$1"
  field="$2"

  if [ "$JSON_PARSER" = "jq" ]; then
    jq -r --arg field "$field" '.[$field] // empty' "$file"
  else
    python3 - "$file" "$field" <<'PY'
import json, sys
path, field = sys.argv[1], sys.argv[2]
with open(path, 'r', encoding='utf-8') as f:
    data = json.load(f)
value = data.get(field, "")
if value is None:
    value = ""
print(value)
PY
  fi
}

json_latest_nightly_release_fields() {
  file="$1"

  if [ "$JSON_PARSER" = "jq" ]; then
    jq -r '
      map(select(.draft != true))
      | .[0]
      | [.tag_name // "", .assets_url // ""]
      | @tsv
    ' "$file"
  else
    python3 - "$file" <<'PY'
import json, sys
path = sys.argv[1]
with open(path, 'r', encoding='utf-8') as f:
    data = json.load(f)
for rel in data:
    if rel.get("draft") is True:
        continue
    print((rel.get("tag_name") or "") + "\t" + (rel.get("assets_url") or ""))
    break
PY
  fi
}

json_asset_download_url() {
  file="$1"
  asset_name="$2"

  if [ "$JSON_PARSER" = "jq" ]; then
    jq -r --arg name "$asset_name" '
      map(select(.name == $name))
      | .[0].browser_download_url // empty
    ' "$file"
  else
    python3 - "$file" "$asset_name" <<'PY'
import json, sys
path, asset_name = sys.argv[1], sys.argv[2]
with open(path, 'r', encoding='utf-8') as f:
    data = json.load(f)
for asset in data:
    if asset.get("name") == asset_name:
        print(asset.get("browser_download_url") or "")
        break
PY
  fi
}

###############################################################################
# TTY handling
###############################################################################

init_tty() {
  if [ -r /dev/tty ]; then
    TTY_AVAILABLE="true"
  else
    TTY_AVAILABLE="false"
  fi
  debug "TTY available: $TTY_AVAILABLE"
}

prompt_yes_no() {
  prompt="$1"
  default="${2:-Y}"

  if [ "$TTY_AVAILABLE" != "true" ]; then
    return 1
  fi

  while true; do
    if [ "$default" = "Y" ]; then
      printf '%s [Y/n] ' "$prompt" >/dev/tty
    else
      printf '%s [y/N] ' "$prompt" >/dev/tty
    fi

    if ! IFS= read -r answer </dev/tty; then
      return 1
    fi

    case "$answer" in
      "")
        [ "$default" = "Y" ] && return 0
        return 1
        ;;
      y|Y|yes|YES|Yes)
        return 0
        ;;
      n|N|no|NO|No)
        return 1
        ;;
      *)
        printf '%s\n' "Please answer yes or no." >/dev/tty
        ;;
    esac
  done
}

###############################################################################
# Argument parsing
###############################################################################

usage() {
  cat <<'EOF'
Edge.js installer

Usage:
  install.sh [version] [--nightly]
  install.sh --version <tag> [--nightly]
  install.sh --help
EOF
}

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --nightly)
        USE_NIGHTLY="true"
        ;;
      --version)
        shift
        if [ $# -eq 0 ]; then
          status_error "Missing value for --version"
          exit 1
        fi
        REQUESTED_VERSION="$1"
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      -*)
        status_error "Unknown argument: $1"
        exit 1
        ;;
      *)
        if [ -n "$REQUESTED_VERSION" ]; then
          status_error "Version was specified more than once."
          exit 1
        fi
        REQUESTED_VERSION="$1"
        ;;
    esac
    shift
  done

  debug "Requested version: ${REQUESTED_VERSION:-<none>}"
  debug "Use nightly: $USE_NIGHTLY"
}

###############################################################################
# Platform detection
###############################################################################

detect_os() {
  OS="$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]')"
  case "$OS" in
    darwin) OS="darwin" ;;
    linux) OS="linux" ;;
    *)
      status_error "Unsupported operating system: $OS"
      exit 1
      ;;
  esac
}

detect_arch() {
  ARCH="$(uname -m 2>/dev/null)"
  case "$ARCH" in
    x86_64|amd64) ARCH="amd64" ;;
    arm64|aarch64) ARCH="arm64" ;;
    *)
      status_error "Unsupported architecture: $ARCH"
      exit 1
      ;;
  esac
}

detect_target() {
  detect_os
  detect_arch
  TARGET="edgejs-${OS}-${ARCH}"
  debug "OS: $OS"
  debug "ARCH: $ARCH"
  debug "TARGET: $TARGET"
}

release_asset_candidates() {
  cat <<EOF
${TARGET}
${TARGET}.tar.gz
${TARGET}.tgz
${TARGET}.zip
edge-${OS}-${ARCH}
edge-${OS}-${ARCH}.tar.gz
edge-${OS}-${ARCH}.tgz
edge-${OS}-${ARCH}.zip
EOF
}

###############################################################################
# Release resolution
###############################################################################

release_api_url() {
  repo="$1"
  version="$2"

  if [ -n "$version" ]; then
    printf 'https://api.github.com/repos/%s/releases/tags/%s' "$repo" "$version"
  else
    printf 'https://api.github.com/repos/%s/releases/latest' "$repo"
  fi
}

releases_list_api_url() {
  repo="$1"
  printf 'https://api.github.com/repos/%s/releases?per_page=30' "$repo"
}

resolve_asset_from_assets_url() {
  repo="$1"
  tag="$2"
  assets_url="$3"

  debug "Resolving assets for tag: $tag"
  debug "Assets URL: $assets_url"

  assets_file="$TMP_DIR/assets.json"
  github_api_get "$assets_url" > "$assets_file" 2>/dev/null || true

  if [ ! -s "$assets_file" ]; then
    debug "Assets response was empty"
    return 1
  fi

  for asset in $(release_asset_candidates); do
    debug "Trying asset candidate: $asset"
    url="$(json_asset_download_url "$assets_file" "$asset")"
    if [ -n "$url" ]; then
      debug "Matched asset: $asset"
      debug "Download URL: $url"
      RESOLVED_REPO="$repo"
      RESOLVED_TAG="$tag"
      RESOLVED_ASSET_NAME="$asset"
      RESOLVED_DOWNLOAD_URL="$url"
      return 0
    fi
  done

  debug "No asset candidates matched for tag: $tag"
  return 1
}

try_resolve_release() {
  repo="$1"
  version="$2"

  debug "Trying release lookup in repo: $repo version: ${version:-<latest>}"

  release_file="$TMP_DIR/release.json"
  github_api_get "$(release_api_url "$repo" "$version")" > "$release_file" 2>/dev/null || true

  if [ ! -s "$release_file" ]; then
    debug "Release response was empty"
    return 1
  fi

  tag="$(json_release_field "$release_file" "tag_name")"
  assets_url="$(json_release_field "$release_file" "assets_url")"

  debug "Resolved tag_name: ${tag:-<none>}"
  debug "Resolved assets_url: ${assets_url:-<none>}"

  [ -n "$tag" ] || return 1
  [ -n "$assets_url" ] || return 1

  resolve_asset_from_assets_url "$repo" "$tag" "$assets_url"
}

try_resolve_latest_nightly() {
  repo="$1"

  debug "Trying latest nightly lookup in repo: $repo"

  releases_file="$TMP_DIR/nightly-releases.json"
  github_api_get "$(releases_list_api_url "$repo")" > "$releases_file" 2>/dev/null || true

  if [ ! -s "$releases_file" ]; then
    debug "Nightly releases response was empty"
    return 1
  fi

  fields="$(json_latest_nightly_release_fields "$releases_file")"
  tag="$(printf '%s' "$fields" | awk -F '\t' '{print $1}')"
  assets_url="$(printf '%s' "$fields" | awk -F '\t' '{print $2}')"

  debug "Nightly candidate tag: ${tag:-<none>}"
  debug "Nightly candidate assets_url: ${assets_url:-<none>}"

  [ -n "$tag" ] || return 1
  [ -n "$assets_url" ] || return 1

  resolve_asset_from_assets_url "$repo" "$tag" "$assets_url"
}

resolve_release() {
  if [ "$USE_NIGHTLY" = "true" ]; then
    if [ -n "$REQUESTED_VERSION" ]; then
      if try_resolve_release "$EDGEJS_NIGHTLY_REPO" "$REQUESTED_VERSION"; then
        return 0
      fi
    else
      if try_resolve_latest_nightly "$EDGEJS_NIGHTLY_REPO"; then
        return 0
      fi
    fi

    status_error "Could not resolve nightly release for ${TARGET}."
    exit 1
  fi

  if try_resolve_release "$EDGEJS_REPO" "$REQUESTED_VERSION"; then
    return 0
  fi

  status_warn "Stable release lookup failed or no matching asset was found for ${TARGET}."
  status_warn "Falling back to nightly releases."

  if [ -n "$REQUESTED_VERSION" ]; then
    if try_resolve_release "$EDGEJS_NIGHTLY_REPO" "$REQUESTED_VERSION"; then
      return 0
    fi
  else
    if try_resolve_latest_nightly "$EDGEJS_NIGHTLY_REPO"; then
      return 0
    fi
  fi

  status_error "Could not resolve any Edge.js release for ${TARGET}."
  exit 1
}

###############################################################################
# Install
###############################################################################

extract_if_needed() {
  archive="$1"
  outdir="$2"

  case "$archive" in
    *.tar.gz|*.tgz)
      tar -xzf "$archive" -C "$outdir"
      return 0
      ;;
    *.zip)
      if ! has_cmd unzip; then
        status_error "unzip is required to extract $archive"
        exit 1
      fi
      unzip -q "$archive" -d "$outdir"
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

find_bundle_root() {
  dir="$1"

  if [ -d "$dir/bin" ] || [ -d "$dir/lib" ] || [ -d "$dir/bin-compat" ]; then
    printf '%s\n' "$dir"
    return 0
  fi

  found="$(find "$dir" -mindepth 1 -maxdepth 2 -type d \( -name bin -o -name lib -o -name bin-compat \) 2>/dev/null | head -n 1)"
  if [ -n "$found" ]; then
    dirname "$found"
    return 0
  fi

  return 1
}

find_edge_binary() {
  dir="$1"

  for candidate in \
    "$dir/bin/edge" \
    "$dir/bin/edgejs"
  do
    if [ -f "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  found="$(find "$dir" -type f \( -path '*/bin/edge' -o -path '*/bin/edgejs' \) 2>/dev/null | head -n 1)"
  if [ -n "$found" ]; then
    printf '%s\n' "$found"
    return 0
  fi

  return 1
}

copy_dir_contents() {
  src="$1"
  dst="$2"

  mkdir -p "$dst"

  if has_cmd rsync; then
    rsync -a "$src"/ "$dst"/
  else
    cp -R "$src"/. "$dst"/
  fi
}

install_edge() {
  mkdir -p "$EDGEJS_HOME"
  mkdir -p "$EDGEJS_BIN_DIR"

  artifact="$TMP_DIR/$RESOLVED_ASSET_NAME"
  debug "Resolved repo: $RESOLVED_REPO"
  debug "Resolved tag: $RESOLVED_TAG"
  debug "Resolved asset: $RESOLVED_ASSET_NAME"
  debug "Artifact path: $artifact"

  status_downloading "$TARGET"
  if ! download_file "$RESOLVED_DOWNLOAD_URL" "$artifact"; then
    status_error "Failed to download $RESOLVED_ASSET_NAME"
    exit 1
  fi

  case "$RESOLVED_ASSET_NAME" in
    *.tar.gz|*.tgz|*.zip)
      extract_dir="$TMP_DIR/extract"
      mkdir -p "$extract_dir"
      debug "Extract dir: $extract_dir"

      if ! extract_if_needed "$artifact" "$extract_dir"; then
        status_error "Failed to extract $RESOLVED_ASSET_NAME"
        exit 1
      fi

      bundle_root="$(find_bundle_root "$extract_dir")"
      debug "Bundle root: ${bundle_root:-<none>}"

      if [ -z "${bundle_root:-}" ]; then
        status_error "Downloaded archive did not contain a valid Edge.js bundle."
        exit 1
      fi

      copy_dir_contents "$bundle_root" "$EDGEJS_HOME"

      src_bin="$(find_edge_binary "$EDGEJS_HOME")"
      debug "Installed binary path: ${src_bin:-<none>}"

      if [ -z "$src_bin" ]; then
        status_error "Installed bundle did not contain an 'edge' binary in bin/."
        exit 1
      fi

      chmod +x "$src_bin"

      if [ "$src_bin" != "$EDGEJS_BIN_PATH" ]; then
        cp "$src_bin" "$EDGEJS_BIN_PATH"
        chmod +x "$EDGEJS_BIN_PATH"
      fi
      ;;
    *)
      cp "$artifact" "$EDGEJS_BIN_PATH"
      chmod +x "$EDGEJS_BIN_PATH"
      ;;
  esac
}

###############################################################################
# Shell profile
###############################################################################

detect_profile() {
  if [ -n "${PROFILE:-}" ] && [ -f "$PROFILE" ]; then
    printf '%s\n' "$PROFILE"
    return 0
  fi

  shell_name="$(basename "${SHELL:-}")"

  case "$shell_name" in
    zsh)
      [ -f "$HOME/.zshrc" ] && { printf '%s\n' "$HOME/.zshrc"; return 0; }
      ;;
    bash)
      [ -f "$HOME/.bashrc" ] && { printf '%s\n' "$HOME/.bashrc"; return 0; }
      [ -f "$HOME/.bash_profile" ] && { printf '%s\n' "$HOME/.bash_profile"; return 0; }
      ;;
    fish)
      [ -f "$HOME/.config/fish/config.fish" ] && { printf '%s\n' "$HOME/.config/fish/config.fish"; return 0; }
      ;;
  esac

  for candidate in \
    "$HOME/.profile" \
    "$HOME/.bashrc" \
    "$HOME/.bash_profile" \
    "$HOME/.zshrc" \
    "$HOME/.config/fish/config.fish"
  do
    if [ -f "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  return 1
}

setup_shell_profile() {
  mkdir -p "$EDGEJS_HOME"

  cat > "$EDGEJS_ENV_FILE" <<EOF
# Edge.js
export EDGEJS_HOME="$EDGEJS_HOME"
export PATH="$EDGEJS_BIN_DIR:\$PATH"
EOF

  profile="$(detect_profile || true)"
  debug "Detected profile: ${profile:-<none>}"

  if [ -n "$profile" ]; then
    case "$profile" in
      *fish*)
        if ! grep -q 'EDGEJS_HOME' "$profile" 2>/dev/null; then
          {
            printf '\n# Edge.js\n'
            printf 'set -gx EDGEJS_HOME "%s"\n' "$EDGEJS_HOME"
            printf 'fish_add_path "%s"\n' "$EDGEJS_BIN_DIR"
          } >> "$profile"
        fi
        ;;
      *)
        if ! grep -q 'EDGEJS_HOME=' "$profile" 2>/dev/null; then
          {
            printf '\n# Edge.js\n'
            printf 'export EDGEJS_HOME="%s"\n' "$EDGEJS_HOME"
            printf 'export PATH="%s:$PATH"\n' "$EDGEJS_BIN_DIR"
          } >> "$profile"
        fi
        ;;
    esac
  else
    status_warn "Could not detect a shell profile automatically."
    status_warn "Add this manually to your shell config:"
    printf '\n'
    printf 'export EDGEJS_HOME="%s"\n' "$EDGEJS_HOME"
    printf 'export PATH="%s:$PATH"\n' "$EDGEJS_BIN_DIR"
  fi
}

###############################################################################
# Optional Wasmer install
###############################################################################

maybe_install_wasmer() {
  if has_cmd wasmer; then
    debug "Wasmer already installed"
    return 0
  fi

  printf '%b\n' "${bold}${gray}Wasmer is required to use Edge.js in \`--safe\` mode.${reset}"

  if prompt_yes_no "Would you like to install Wasmer now?" "Y"; then
    if has_cmd curl; then
      curl https://get.wasmer.io -sSfL | sh -s "v7.1.0-rc.2"
    else
      status_warn "Cannot install Wasmer automatically because curl is not available."
    fi
  else
    if [ "$TTY_AVAILABLE" != "true" ]; then
      status_info "Skipping Wasmer install prompt because no TTY is available."
    fi
  fi
}

###############################################################################
# Main
###############################################################################

main() {
  parse_args "$@"
  init_downloader
  init_json_parser
  init_tty
  detect_target

  say "Welcome to the Edge.js bash installer!"

  TMP_DIR="$(mktempdir)"
  debug "Temp dir: $TMP_DIR"

  resolve_release
  say "Latest release: $RESOLVED_TAG"
  say ""

  install_edge
  setup_shell_profile

  status_success "Edge.js has been installed in \`$EDGEJS_BIN_PATH\`"
  status_info "\`edge\` is now installed into your \$PATH"
  say "Run your first JS file with: \`edge myscript.js\`"
  say ""

  maybe_install_wasmer
}

main "$@"
