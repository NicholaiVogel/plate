#!/usr/bin/env bash
# Builds plate in release mode and installs it for the current user:
#   ~/.local/bin/plate, a desktop entry and an icon.
#
# Usage: scripts/install.sh [--set-default]
#   --set-default  also make plate the default app for video, EXR, DPX and MXF
#                  (folders keep their current default; plate only appears in
#                  "Open With" for them).
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release -p plate

bin="$HOME/.local/bin"
apps="$HOME/.local/share/applications"
icons="$HOME/.local/share/icons/hicolor/scalable/apps"
mkdir -p "$bin" "$apps" "$icons"

install -m 755 target/release/plate "$bin/plate"
install -m 644 packaging/plate.desktop "$apps/plate.desktop"
install -m 644 packaging/plate.svg "$icons/plate.svg"
update-desktop-database "$apps" 2>/dev/null || true
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
echo "installed $bin/plate"

if [[ "${1:-}" == "--set-default" ]]; then
  grep '^MimeType=' packaging/plate.desktop | cut -d= -f2 | tr ';' '\n' | grep -v '^$' | grep -v '^inode/' |
    while read -r mime; do
      xdg-mime default plate.desktop "$mime"
    done
  echo "plate is now the default for video, EXR, DPX and MXF"
fi
