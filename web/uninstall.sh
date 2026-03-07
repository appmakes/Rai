#!/bin/sh
# Rai uninstaller — removes binary installed by install.sh
# Usage: curl -sSL https://appmakes.github.io/Rai/uninstall.sh | sh

set -e

INSTALL_DIR="${RAI_INSTALL_DIR:-$HOME/.local/bin}"
RAI_BIN="${INSTALL_DIR}/rai"

if [ ! -f "$RAI_BIN" ]; then
  echo "rai is not installed at ${RAI_BIN}"
  echo "If you used a custom path, set RAI_INSTALL_DIR and run again."
  exit 1
fi

rm -f "$RAI_BIN"
echo "rai has been uninstalled from ${RAI_BIN}"

# Optional: full cleanup
if [ -d "$HOME/.config/rai" ] || [ -d "$HOME/.local/share/rai" ]; then
  echo ""
  echo "Config and data were left in place. To remove them:"
  [ -d "$HOME/.config/rai" ]    && echo "  rm -rf \$HOME/.config/rai"
  [ -d "$HOME/.local/share/rai" ] && echo "  rm -rf \$HOME/.local/share/rai"
fi
