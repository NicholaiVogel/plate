#!/usr/bin/env bash
# Build a local .app, including the native libraries it needs.
set -euo pipefail
cd "$(dirname "$0")/.."
profile=release
case "${1:-}" in
  --debug) profile=debug ;;
  "") ;;
  *) echo "Usage: scripts/build-macos.sh [--debug]" >&2; exit 1 ;;
esac
scripts/setup-macos.sh
export PKG_CONFIG_PATH="$PWD/target/macos-deps/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
if [[ -x target/macos-deps/bin/pkg-config ]]; then
  export PKG_CONFIG="$PWD/target/macos-deps/bin/pkg-config"
fi
if [[ "$profile" == debug ]]; then
  cargo build -p plate --locked
else
  cargo build --release -p plate --locked
fi
python3 scripts/bundle-macos.py "target/$profile/plate" target/Plate.app
