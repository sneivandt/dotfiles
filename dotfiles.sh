#!/bin/sh
set -o errexit
set -o nounset

# dotfiles.sh — Thin entry point for the dotfiles management engine.
#
# Default: downloads the latest published binary from GitHub Releases if no
# binary is present, then runs it. The binary handles its own updates.
# --build: builds the Rust binary from source (requires cargo).
#
# The wrapper only handles bootstrap/build concerns and otherwise forwards
# arguments to the Rust binary unchanged.

DOTFILES_ROOT="$(dirname "$(readlink -f "$0")")"
export DOTFILES_ROOT
DOTFILES_WRAPPER="sh"
export DOTFILES_WRAPPER

REPO="sneivandt/dotfiles"
BIN_DIR="$DOTFILES_ROOT/bin"
BINARY="$BIN_DIR/dotfiles"
CONNECT_TIMEOUT=10   # seconds — TCP connect timeout
TRANSFER_TIMEOUT=120 # seconds — total transfer timeout

BUILD_MODE=false
END_OF_OPTIONS=false
argc=$#
i=0
while [ "$i" -lt "$argc" ]; do
  arg=$1
  shift
  i=$((i + 1))
  if [ "$END_OF_OPTIONS" = false ] && [ "$arg" = "--build" ]; then
    BUILD_MODE=true
  else
    if [ "$arg" = "--" ]; then
      END_OF_OPTIONS=true
    fi
    set -- "$@" "$arg"
  fi
done

# --------------------------------------------------------------------------- #
# Build mode: build from source and run
# --------------------------------------------------------------------------- #
if [ "$BUILD_MODE" = true ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: cargo not found. Install Rust to use --build mode." >&2
    exit 1
  fi
  build_output=$(cd "$DOTFILES_ROOT/cli" &&
    cargo build --profile dev-opt --bin dotfiles --message-format=json-render-diagnostics)
  # Cargo reports the actual executable, including configured target directories
  # and target triples. Decode its JSON string without adding a build dependency.
  if ! build_binary=$(printf '%s\n' "$build_output" | awk '
    function decode(value, result, i, c, hex, j, n, digit) {
      result = ""
      for (i = 2; i < length(value); i++) {
        c = substr(value, i, 1)
        if (c == "\\") {
          c = substr(value, ++i, 1)
          if (c == "n") c = "\n"
          else if (c == "r") c = "\r"
          else if (c == "t") c = "\t"
          else if (c == "b") c = sprintf("%c", 8)
          else if (c == "f") c = sprintf("%c", 12)
          else if (c == "u") {
            hex = tolower(substr(value, i + 1, 4))
            n = 0
            for (j = 1; j <= 4; j++) {
              digit = substr(hex, j, 1)
              if (digit ~ /^[a-f]$/) digit = index("abcdef", digit) + 9
              else if (digit !~ /^[0-9]$/) exit 1
              n = n * 16 + digit
            }
            # serde_json emits non-ASCII paths as UTF-8, not Unicode escapes.
            if (n == 0 || n > 127) exit 1
            c = sprintf("%c", n)
            i += 4
          } else if (c != "\\" && c != "\"" && c != "/") exit 1
        }
        result = result c
      }
      return result
    }
    /"reason"[[:space:]]*:[[:space:]]*"compiler-artifact"/ &&
    /"name"[[:space:]]*:[[:space:]]*"dotfiles"/ {
      if (match($0, /"executable"[[:space:]]*:[[:space:]]*"([^"\\]|\\.)*"/)) {
        value = substr($0, RSTART, RLENGTH)
        sub(/^"executable"[[:space:]]*:[[:space:]]*/, "", value)
        executable = decode(value)
        count++
      }
    }
    END {
      if (count != 1 || executable == "") exit 1
      print executable
    }
  '); then
    echo "ERROR: Cargo did not report a unique dotfiles executable." >&2
    exit 1
  fi
  exec "$build_binary" "$@"
fi

# --------------------------------------------------------------------------- #
# Production mode: ensure binary is present
# --------------------------------------------------------------------------- #

# Download a URL to a file.
# Usage: download_file <url> <output_file>
download_file() {
  _df_url="$1"
  _df_out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --connect-timeout "$CONNECT_TIMEOUT" --max-time "$TRANSFER_TIMEOUT" \
         -o "$_df_out" "$_df_url" 2>/dev/null
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$_df_out" --connect-timeout="$CONNECT_TIMEOUT" --timeout="$TRANSFER_TIMEOUT" \
         "$_df_url" 2>/dev/null
  else
    echo "ERROR: curl or wget required to download binary." >&2
    exit 1
  fi
}

# Resolve the latest release tag from the GitHub API.
# Prints the tag (e.g. "v0.2.0") on success, or an empty string on failure.
resolve_release_tag() {
  _api_url="https://api.github.com/repos/$REPO/releases/latest"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --connect-timeout "$CONNECT_TIMEOUT" --max-time "$TRANSFER_TIMEOUT" \
         "$_api_url" 2>/dev/null | \
      awk -F'"' '/"tag_name"/{print $4; exit}'
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- --connect-timeout="$CONNECT_TIMEOUT" --timeout="$TRANSFER_TIMEOUT" \
         "$_api_url" 2>/dev/null | \
      awk -F'"' '/"tag_name"/{print $4; exit}'
  fi
}

# Verify checksum in a subshell to scope the trap safely.
_verify_checksum() {
  _vc_tag="$1"
  _vc_asset="$2"
  _vc_binary="$3"
  tmpfile=$(mktemp)
  trap 'rm -f "$tmpfile"' EXIT
  if ! download_file \
    "https://github.com/$REPO/releases/download/$_vc_tag/checksums.sha256" \
    "$tmpfile"; then
    echo "ERROR: Failed to download checksum file." >&2
    return 1
  fi
  expected=$(awk -v fname="$_vc_asset" '{ name=$2; sub(/^\*/, "", name); if (name == fname) print $1 }' "$tmpfile")
  if [ -z "$expected" ]; then
    echo "ERROR: Checksum not found in checksum file for $_vc_asset." >&2
    return 1
  fi
  actual=$(sha256sum "$_vc_binary" | awk '{print $1}')
  if [ "$expected" != "$actual" ]; then
    echo "ERROR: Checksum verification failed!" >&2
    return 1
  fi
}

# Verify GitHub build provenance for a downloaded asset.
#
# Verification runs by default when gh is available. Set
# DOTFILES_SKIP_ATTESTATION=1 to skip the check explicitly.
_verify_attestation() {
  _va_binary="$1"

  if [ "${DOTFILES_SKIP_ATTESTATION:-0}" = "1" ]; then
    return 0
  fi

  if ! command -v gh >/dev/null 2>&1; then
    echo "WARNING: gh not found. Skipping build provenance verification." >&2
    return 0
  fi

  if gh attestation verify "$_va_binary" --repo "$REPO" >/dev/null 2>&1; then
    return 0
  fi

  echo "ERROR: Build provenance verification failed for $_va_binary." >&2
  return 1
}

# Download the bootstrap binary if needed.
download_binary() {
  _arch="$(uname -m)"
  case "$_arch" in
    x86_64|amd64)  asset="dotfiles-linux-x86_64" ;;
    aarch64|arm64) asset="dotfiles-linux-aarch64" ;;
    *)
      echo "ERROR: Unsupported architecture: $_arch" >&2
      echo "Supported architectures: x86_64, amd64, aarch64, arm64" >&2
      exit 1
      ;;
  esac

  tag=$(resolve_release_tag)
  if [ -z "$tag" ]; then
    echo "ERROR: Failed to resolve latest release tag." >&2
    echo "Check your internet connection or use --build to build from source." >&2
    exit 1
  fi

  url="https://github.com/$REPO/releases/download/$tag/$asset"

  mkdir -p "$BIN_DIR"

  echo "Bootstrap · dotfiles $tag · ${asset#dotfiles-}"
  if ! download_file "$url" "$BINARY"; then
    echo "ERROR: Failed to download dotfiles binary." >&2
    echo "Check your internet connection or use --build to build from source." >&2
    rm -f "$BINARY"
    exit 1
  fi

  if ! command -v sha256sum >/dev/null 2>&1; then
    echo "ERROR: sha256sum not found. Cannot verify download integrity." >&2
    rm -f "$BINARY"
    exit 1
  fi
  if ! ( _verify_checksum "$tag" "$asset" "$BINARY" ); then
    rm -f "$BINARY"
    exit 1
  fi

  if ! _verify_attestation "$BINARY"; then
    rm -f "$BINARY"
    exit 1
  fi

  chmod +x "$BINARY"

  echo "Downloaded · checksum verified · $BINARY"
}

# Bootstrap: download the latest binary only if no binary is present.
# Subsequent updates are handled by the binary itself.
if [ ! -x "$BINARY" ]; then
  download_binary
fi

exec "$BINARY" "$@"
