#!/usr/bin/env bash
set -euo pipefail

REPO_SLUG="weekend-project-space/openwalk"
DEFAULT_BASE_URL="https://github.com/${REPO_SLUG}/releases"

VERSION="${OPENWALK_VERSION:-latest}"
INSTALL_DIR="${OPENWALK_INSTALL_DIR:-$HOME/.openwalk/bin}"
BASE_URL="${OPENWALK_RELEASE_BASE_URL:-$DEFAULT_BASE_URL}"
NO_PATH=0
DOWNLOAD_LAST_ERROR=''

usage() {
  cat <<'EOF'
Install openwalk from GitHub Releases.

Usage:
  install.sh [options]

Options:
  --version <tag>       Release tag to install, default: latest
  --install-dir <path>  Install directory, default: ~/.openwalk/bin
  --base-url <url>      Release base URL, default: https://github.com/weekend-project-space/openwalk/releases
  --no-path             Do not modify shell PATH config
  -h, --help            Show this help

Environment:
  OPENWALK_VERSION
  OPENWALK_INSTALL_DIR
  OPENWALK_RELEASE_BASE_URL
EOF
}

log() {
  printf '[openwalk-install] %s\n' "$*"
}

fail() {
  printf '[openwalk-install] error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

download() {
  local url="$1"
  local output="$2"
  local err_file
  err_file="$(mktemp)"
  DOWNLOAD_LAST_ERROR=''

  if command -v curl >/dev/null 2>&1; then
    if curl -fsSL "$url" -o "$output" 2>"$err_file"; then
      rm -f "$err_file"
      return 0
    fi
    DOWNLOAD_LAST_ERROR="$(cat "$err_file" 2>/dev/null || true)"
    rm -f "$err_file"
    return 1
  fi

  if command -v wget >/dev/null 2>&1; then
    if wget -qO "$output" "$url" 2>"$err_file"; then
      rm -f "$err_file"
      return 0
    fi
    DOWNLOAD_LAST_ERROR="$(cat "$err_file" 2>/dev/null || true)"
    rm -f "$err_file"
    return 1
  fi

  rm -f "$err_file"
  fail "either curl or wget is required"
}

download_error_is_404() {
  case "$DOWNLOAD_LAST_ERROR" in
    *"error: 404"*|*"error 404"*|*"404 Not Found"*|*"The requested URL returned error: 404"*|*"ERROR 404:"*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

fail_download() {
  local url="$1"

  if download_error_is_404; then
    cat >&2 <<EOF
[openwalk-install] error: no published release asset was found for:
[openwalk-install] error:   $url
[openwalk-install] hint: publish a GitHub Release first and upload:
[openwalk-install] hint:   $ASSET_NAME
[openwalk-install] hint:   $CHECKSUM_NAME
[openwalk-install] hint: or build locally with:
[openwalk-install] hint:   cargo build --release
EOF
    exit 1
  fi

  if [ -n "$DOWNLOAD_LAST_ERROR" ]; then
    fail "failed to download $url: $DOWNLOAD_LAST_ERROR"
  fi

  fail "failed to download $url"
}

checksum_sha256() {
  local file="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
    return
  fi

  fail "sha256sum or shasum is required for checksum verification"
}

release_path() {
  if [ "$VERSION" = "latest" ]; then
    printf 'latest/download'
  else
    printf 'download/%s' "$VERSION"
  fi
}

target_triple() {
  case "$(uname -m)" in
    x86_64|amd64)
      printf 'x86_64-unknown-linux-gnu'
      ;;
    aarch64|arm64)
      printf 'aarch64-unknown-linux-gnu'
      ;;
    *)
      fail "unsupported Linux architecture: $(uname -m)"
      ;;
  esac
}

path_contains_dir() {
  case ":${PATH:-}:" in
    *":$1:"*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

shell_rc_file() {
  case "${SHELL:-}" in
    */zsh)
      printf '%s/.zshrc' "$HOME"
      ;;
    */bash)
      printf '%s/.bashrc' "$HOME"
      ;;
    *)
      printf '%s/.profile' "$HOME"
      ;;
  esac
}

ensure_path_config() {
  local install_dir="$1"
  local rc_file path_line

  if path_contains_dir "$install_dir"; then
    log "PATH already contains $install_dir"
    return
  fi

  rc_file="$(shell_rc_file)"
  path_line="export PATH=\"$install_dir:\$PATH\""

  touch "$rc_file"

  if grep -Fqs "$path_line" "$rc_file"; then
    log "PATH entry already exists in $rc_file"
  else
    {
      printf '\n# Added by openwalk installer\n'
      printf '%s\n' "$path_line"
    } >>"$rc_file"
    log "added $install_dir to PATH in $rc_file"
  fi

  export PATH="$install_dir:$PATH"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || fail "missing value after --version"
      VERSION="$2"
      shift 2
      ;;
    --install-dir)
      [ "$#" -ge 2 ] || fail "missing value after --install-dir"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --base-url)
      [ "$#" -ge 2 ] || fail "missing value after --base-url"
      BASE_URL="$2"
      shift 2
      ;;
    --no-path)
      NO_PATH=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown option: $1"
      ;;
  esac
done

[ "$(uname -s)" = "Linux" ] || fail "this installer is intended for Linux"

need_cmd tar
need_cmd mktemp

TARGET_TRIPLE="$(target_triple)"
ASSET_NAME="openwalk-${TARGET_TRIPLE}.tar.gz"
CHECKSUM_NAME="openwalk-checksums.txt"
RELEASE_PATH="$(release_path)"
ASSET_URL="${BASE_URL}/${RELEASE_PATH}/${ASSET_NAME}"
CHECKSUM_URL="${BASE_URL}/${RELEASE_PATH}/${CHECKSUM_NAME}"

TMP_DIR="$(mktemp -d)"
ARCHIVE_PATH="${TMP_DIR}/${ASSET_NAME}"
CHECKSUM_PATH="${TMP_DIR}/${CHECKSUM_NAME}"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

log "downloading $ASSET_URL"
download "$ASSET_URL" "$ARCHIVE_PATH" || fail_download "$ASSET_URL"

log "downloading $CHECKSUM_URL"
download "$CHECKSUM_URL" "$CHECKSUM_PATH" || fail_download "$CHECKSUM_URL"

EXPECTED_SUM=''
while read -r sum file; do
  file="${file#\*}"
  if [ "$file" = "$ASSET_NAME" ]; then
    EXPECTED_SUM="$sum"
    break
  fi
done <"$CHECKSUM_PATH"

[ -n "$EXPECTED_SUM" ] || fail "could not find checksum for $ASSET_NAME in $CHECKSUM_NAME"

ACTUAL_SUM="$(checksum_sha256 "$ARCHIVE_PATH")"
[ "$ACTUAL_SUM" = "$EXPECTED_SUM" ] || fail "checksum mismatch for $ASSET_NAME"

log "extracting archive"
tar -xzf "$ARCHIVE_PATH" -C "$TMP_DIR"

BINARY_PATH="$(find "$TMP_DIR" -type f -name openwalk | head -n 1 || true)"
[ -n "$BINARY_PATH" ] || fail "failed to locate extracted openwalk binary"

mkdir -p "$INSTALL_DIR"

INSTALL_PATH="${INSTALL_DIR}/openwalk"
TMP_INSTALL_PATH="${INSTALL_DIR}/.openwalk.tmp.$$"

cp "$BINARY_PATH" "$TMP_INSTALL_PATH"
chmod 755 "$TMP_INSTALL_PATH"
mv -f "$TMP_INSTALL_PATH" "$INSTALL_PATH"

if [ "$NO_PATH" -eq 0 ]; then
  ensure_path_config "$INSTALL_DIR"
else
  log "skipped PATH update"
fi

VERSION_OUTPUT="$("$INSTALL_PATH" --version 2>/dev/null || true)"

log "installed to $INSTALL_PATH"
if [ -n "$VERSION_OUTPUT" ]; then
  log "$VERSION_OUTPUT"
fi

if [ "$NO_PATH" -eq 1 ] && ! path_contains_dir "$INSTALL_DIR"; then
  log "add $INSTALL_DIR to PATH to run \`openwalk\` directly"
fi
