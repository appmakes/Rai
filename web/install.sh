#!/bin/sh
# Rai installer — downloads pre-built binary from GitHub Releases
# Usage: curl -sSL https://appmakes.github.io/Rai/install.sh | sh

set -e

REPO="appmakes/Rai"
INSTALL_DIR="${RAI_INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS
OS="$(uname -s)"
case "$OS" in
  Linux)  os="linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *)      echo "Error: unsupported OS: $OS"; exit 1 ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)  arch="x86_64" ;;
  aarch64|arm64)  arch="aarch64" ;;
  *)              echo "Error: unsupported architecture: $ARCH"; exit 1 ;;
esac

TARGET="${arch}-${os}"

# GitHub API auth header (optional, avoids rate limits)
AUTH_HEADER=""
if [ -n "$GITHUB_TOKEN" ]; then
  AUTH_HEADER="Authorization: token ${GITHUB_TOKEN}"
fi

# Get latest release with available binaries
echo "Fetching latest release..."
ASSET_NAME="rai-${TARGET}.tar.gz"
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

LATEST=""
TAGS=$(curl -sSL ${AUTH_HEADER:+-H "$AUTH_HEADER"} "https://api.github.com/repos/${REPO}/releases" | grep '"tag_name"' | head -3 | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')

for tag in $TAGS; do
  URL="https://github.com/${REPO}/releases/download/${tag}/${ASSET_NAME}"
  if curl -sSL --head --fail "$URL" >/dev/null 2>&1; then
    LATEST="$tag"
    break
  fi
done

if [ -z "$LATEST" ]; then
  echo "Error: no release with binaries found for ${TARGET}. Check https://github.com/${REPO}/releases"
  exit 1
fi

echo "Installing rai ${LATEST} for ${TARGET}..."

# Download and extract
URL="https://github.com/${REPO}/releases/download/${LATEST}/${ASSET_NAME}"
curl -sSL "$URL" -o "${TMPDIR}/rai.tar.gz"
tar xzf "${TMPDIR}/rai.tar.gz" -C "$TMPDIR"

# Install
mkdir -p "$INSTALL_DIR"
mv "${TMPDIR}/rai" "${INSTALL_DIR}/rai"
chmod +x "${INSTALL_DIR}/rai"

echo ""
echo "rai ${LATEST} installed to ${INSTALL_DIR}/rai"

# Check if install dir is in PATH
case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo ""
    echo "Add ${INSTALL_DIR} to your PATH:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac

echo ""
echo "Run 'rai start' to get started."
