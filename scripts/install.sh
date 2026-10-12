#!/usr/bin/env bash
# Builds plate in release mode and installs it for the current user:
#   ~/.local/bin/plate, a desktop entry and an icon.
#
# Usage: scripts/install.sh [--set-default] [--debug]
#   --set-default  also make plate the default app for video, EXR, DPX and MXF.
set -euo pipefail

cd "$(dirname "$0")/.."
debug=false
set_default=false
for arg in "$@"; do
  case "$arg" in
    --debug) debug=true ;;
    --set-default) set_default=true ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

if [[ "$(uname -s)" == Darwin ]]; then
  if $debug; then
    scripts/build-macos.sh --debug
  else
    scripts/build-macos.sh
  fi
  apps="$HOME/Applications"
  bin="$HOME/.local/bin"
  mkdir -p "$apps" "$bin"
  /usr/bin/ditto target/Plate.app "$apps/Plate.app"
  /usr/bin/codesign --verify --deep --strict "$apps/Plate.app"
  ln -sf "$apps/Plate.app/Contents/MacOS/plate" "$bin/plate"
  echo "Installed $apps/Plate.app and $bin/plate"
  if $set_default; then
    echo "In Finder: Get Info on a media file, choose Plate under Open with, then Change All."
  fi
  exit 0
fi

profile=release
if $debug; then
  profile=debug
  cargo build -p plate
else
  cargo build --release -p plate
fi

bin="$HOME/.local/bin"
apps="$HOME/.local/share/applications"
icons="$HOME/.local/share/icons/hicolor/scalable/apps"
mkdir -p "$bin" "$apps" "$icons"

install -m 755 "target/$profile/plate" "$bin/plate"
install -m 644 packaging/plate.desktop "$apps/plate.desktop"
install -m 644 packaging/plate.svg "$icons/plate.svg"
update-desktop-database "$apps" 2>/dev/null || true
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
echo "installed $bin/plate"

if $set_default; then
  grep '^MimeType=' packaging/plate.desktop | cut -d= -f2 | tr ';' '\n' | grep -v '^$' | grep -v '^inode/' |
    while read -r mime; do
      xdg-mime default plate.desktop "$mime"
    done
  echo "plate is now the default for video, EXR, DPX and MXF"
fi
